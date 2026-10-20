//! Finding the Turris and exchanging messages with it. The kernel's usblp driver binds to the
//! screen's printer-class interface, so the screen is a character device (/dev/usb/lp*) that
//! reads and writes its bulk pipe; no USB library is needed.

use super::messages::{Response, TRANSFER_END, TRANSFER_END_STATUS, TRANSFER_TRACK_ID, request, restart_playback};
use super::wire::{FrameReader, Message, encode_frame};
use crate::link::{LinkError, Port};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const USB_VENDOR_ID: u16 = 0x391A;
pub const USB_PRODUCT_ID: u16 = 0x2011;
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
/// How long the screen gets to say whether a playback restart worked; it acts either way.
const RESTART_ANSWER_TIMEOUT: Duration = Duration::from_secs(2);

/// The device node of a connected Turris, e.g. /dev/usb/lp0.
pub fn find_device() -> Option<String> {
    find_in_sysfs(Path::new("/"))
}

/// /sys/class/usbmisc/lp*/device is the USB interface; its parent holds idVendor/idProduct.
fn find_in_sysfs(root: &Path) -> Option<String> {
    let class = root.join("sys/class/usbmisc");
    let mut names: Vec<String> = std::fs::read_dir(&class).ok()?.flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).filter(|name| name.starts_with("lp")).collect();
    names.sort();
    names.into_iter().find(|name| is_turris(&class.join(name))).map(|name| format!("/dev/usb/{name}"))
}

fn is_turris(class_device: &Path) -> bool {
    let usb_device = class_device.join("device").join("..");
    read_hex(&usb_device.join("idVendor")) == Some(USB_VENDOR_ID) && read_hex(&usb_device.join("idProduct")) == Some(USB_PRODUCT_ID)
}

fn read_hex(path: &Path) -> Option<u16> {
    u16::from_str_radix(std::fs::read_to_string(path).ok()?.trim(), 16).ok()
}

/// The usblp device, opened non-blocking so that reading never waits for the screen.
pub struct UsbPrinterPort(File);

impl UsbPrinterPort {
    pub fn open(path: &str) -> io::Result<Self> {
        OpenOptions::new().read(true).write(true).custom_flags(libc::O_NONBLOCK).open(path).map(Self)
    }
}

impl Port for UsbPrinterPort {
    /// usblp has one transfer in flight at a time and, non-blocking, reports WouldBlock until it
    /// is done, or accepts part of the data.
    fn write_bytes(&mut self, data: &[u8]) -> io::Result<()> {
        let deadline = Instant::now() + WRITE_TIMEOUT;
        let mut written = 0;
        while written < data.len() {
            match self.0.write(&data[written..]) {
                Ok(0) => return Err(io::Error::new(io::ErrorKind::WriteZero, "the screen took no data")),
                Ok(count) => written += count,
                Err(error) if matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted) => {
                    if Instant::now() > deadline {
                        return Err(io::Error::new(io::ErrorKind::TimedOut, "the screen stopped taking data"));
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    fn read_available(&mut self) -> io::Result<Vec<u8>> {
        let mut data = Vec::new();
        let mut buffer = [0u8; 16 * 1024];
        loop {
            match self.0.read(&mut buffer) {
                Ok(0) => return Ok(data),
                Ok(count) => data.extend_from_slice(&buffer[..count]),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(data),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
    }
}

/// Requests and their responses, matched by track id.
pub struct TurrisLink<P: Port> {
    port: P,
    reader: FrameReader,
    next_track_id: u64,
}

impl<P: Port> TurrisLink<P> {
    pub fn new(port: P) -> Self {
        // Start somewhere new each session, so a late answer to an old request can't match.
        let start = SystemTime::now().duration_since(UNIX_EPOCH).map(|elapsed| elapsed.as_micros() as u64).unwrap_or(1);
        Self { port, reader: FrameReader::default(), next_track_id: start & 0xFFFF_FFFF_FFFF }
    }

    /// Sends `command` and returns the body of the answer, which must be an `expected` message.
    pub fn query(&mut self, command: u32, body: &Message, expected: u32, timeout: Duration) -> Result<Vec<u8>, LinkError> {
        let track_id = self.allocate_track_id();
        self.write(1, track_id, command, body)?;
        self.await_response(track_id, expected, timeout)?.ok_or(LinkError::NoAnswer { command })
    }

    /// Sends a message the screen doesn't answer: settings, layouts and readings.
    pub fn send(&mut self, command: u32, body: &Message) -> Result<(), LinkError> {
        let track_id = self.allocate_track_id();
        self.write(1, track_id, command, body)?;
        self.discard_responses()
    }

    /// Restarts playback, which makes newly chosen media show.
    pub fn restart_playback(&mut self) -> Result<(), LinkError> {
        self.write(0, TRANSFER_TRACK_ID, TRANSFER_END, &restart_playback())?;
        match self.await_response(TRANSFER_TRACK_ID, TRANSFER_END_STATUS, RESTART_ANSWER_TIMEOUT) {
            Ok(_) | Err(LinkError::Refused { .. }) => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Throws away anything already waiting, such as answers meant for a previous session.
    pub fn discard_responses(&mut self) -> Result<(), LinkError> {
        let data = self.port.read_available()?;
        for payload in self.reader.feed(&data) {
            if let Some((code, why)) = Response::parse(&payload).and_then(|response| response.error) {
                eprintln!("canalis: the Turris reported error {code}: {why}");
            }
        }
        Ok(())
    }

    fn allocate_track_id(&mut self) -> u64 {
        let mut track_id = self.next_track_id;
        if track_id == 0 || track_id == TRANSFER_TRACK_ID {
            track_id += 1;
        }
        self.next_track_id = track_id + 1;
        track_id
    }

    fn write(&mut self, version: u32, track_id: u64, command: u32, body: &Message) -> Result<(), LinkError> {
        self.port.write_bytes(&encode_frame(&request(version, track_id, command, body)))?;
        Ok(())
    }

    /// The body of the answer to `track_id`, or None when none came in time. An answer whose body
    /// isn't `expected`, or that carries an error code, means the screen refused the request.
    fn await_response(&mut self, track_id: u64, expected: u32, timeout: Duration) -> Result<Option<Vec<u8>>, LinkError> {
        let deadline = Instant::now() + timeout;
        loop {
            let data = self.port.read_available()?;
            for payload in self.reader.feed(&data) {
                let Some(response) = Response::parse(&payload).filter(|response| response.track_id == track_id) else { continue };
                return match (response.error, response.body) {
                    (Some((code, why)), _) => Err(LinkError::Refused { code, why }),
                    (None, Some((number, body))) if number == expected => Ok(Some(body)),
                    (None, Some((number, _))) => Err(LinkError::Refused { code: u64::from(number), why: "unexpected answer".into() }),
                    (None, None) => Err(LinkError::Refused { code: 0, why: "empty answer".into() }),
                };
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::turris::messages::tests::{PONG_FRAME, hex};
    use crate::turris::messages::{PING, PONG};
    use crate::turris::wire::{bytes_field, varint_field};

    /// Answers each request with `answer(request) -> payload`, like the screen, and records them.
    pub struct FakeTurris {
        pub received: Vec<Vec<u8>>,
        pending: Vec<u8>,
        reader: FrameReader,
        answer: Box<dyn FnMut(&[u8]) -> Option<Vec<u8>>>,
    }

    impl FakeTurris {
        pub fn new(answer: impl FnMut(&[u8]) -> Option<Vec<u8>> + 'static) -> Self {
            Self { received: Vec::new(), pending: Vec::new(), reader: FrameReader::default(), answer: Box::new(answer) }
        }
    }

    impl Port for &mut FakeTurris {
        fn write_bytes(&mut self, data: &[u8]) -> io::Result<()> {
            for payload in self.reader.feed(data) {
                if let Some(answer) = (self.answer)(&payload) {
                    self.pending.extend(encode_frame(&answer));
                }
                self.received.push(payload);
            }
            Ok(())
        }

        fn read_available(&mut self) -> io::Result<Vec<u8>> {
            Ok(std::mem::take(&mut self.pending))
        }
    }

    /// `answer_payload` with its header's track id replaced by the request's.
    pub fn answering(request: &[u8], answer_payload: &[u8]) -> Vec<u8> {
        let track_id = varint_field(bytes_field(request, 1).unwrap(), 2).unwrap();
        let header = Message::new().varint(1, 1).varint(2, track_id);
        Message::new().message(1, &header).raw(&crate::turris::wire::fields_except(answer_payload, &[1])).into_bytes()
    }

    #[test]
    fn finds_the_turris_in_sysfs() {
        let root = std::env::temp_dir().join(format!("canalis-usbmisc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let usb_device = root.join("sys/devices/usb1/1-8");
        let interface = usb_device.join("1-8:1.0");
        std::fs::create_dir_all(&interface).unwrap();
        std::fs::write(usb_device.join("idVendor"), "391a\n").unwrap();
        std::fs::write(usb_device.join("idProduct"), "2011\n").unwrap();
        let class_device = root.join("sys/class/usbmisc/lp0");
        std::fs::create_dir_all(&class_device).unwrap();
        std::os::unix::fs::symlink(&interface, class_device.join("device")).unwrap();
        std::fs::create_dir_all(root.join("sys/class/usbmisc/hiddev0")).unwrap();
        assert_eq!(find_in_sysfs(&root), Some("/dev/usb/lp0".to_string()));
        std::fs::write(usb_device.join("idProduct"), "1011\n").unwrap();
        assert_eq!(find_in_sysfs(&root), None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn query_matches_the_answer_by_track_id() {
        let mut turris = FakeTurris::new(|request| Some(answering(request, &hex(PONG_FRAME))));
        let mut link = TurrisLink::new(&mut turris);
        let body = link.query(PING, &Message::new(), PONG, Duration::from_millis(50)).unwrap();
        assert_eq!(body, Message::new().string(1, "Hey!").into_bytes());
    }

    #[test]
    fn a_silent_screen_is_no_answer() {
        let mut turris = FakeTurris::new(|_| None);
        let mut link = TurrisLink::new(&mut turris);
        assert!(matches!(link.query(PING, &Message::new(), PONG, Duration::from_millis(30)), Err(LinkError::NoAnswer { command: PING })));
    }

    #[test]
    fn an_error_code_is_a_refusal() {
        let refusal = Message::new().message(2, &Message::new().varint(1, 2).string(2, "unsupported")).into_bytes();
        let mut turris = FakeTurris::new(move |request| Some(answering(request, &Message::new().message(1, &Message::new()).raw(&refusal).into_bytes())));
        let mut link = TurrisLink::new(&mut turris);
        let result = link.query(PING, &Message::new(), PONG, Duration::from_millis(50));
        assert!(matches!(result, Err(LinkError::Refused { code: 2, ref why }) if why == "unsupported"));
    }

    #[test]
    fn answers_to_other_requests_are_ignored() {
        let mut turris = FakeTurris::new(|_| Some(hex(PONG_FRAME)));
        let mut link = TurrisLink::new(&mut turris);
        assert!(matches!(link.query(PING, &Message::new(), PONG, Duration::from_millis(30)), Err(LinkError::NoAnswer { .. })));
    }

    #[test]
    fn track_ids_increase_and_avoid_the_transfer_id() {
        let mut turris = FakeTurris::new(|_| None);
        let mut link = TurrisLink::new(&mut turris);
        link.next_track_id = TRANSFER_TRACK_ID;
        assert_eq!(link.allocate_track_id(), TRANSFER_TRACK_ID + 1);
        assert_eq!(link.allocate_track_id(), TRANSFER_TRACK_ID + 2);
    }
}
