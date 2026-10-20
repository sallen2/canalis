//! Request/response exchange with the Panorama screen over its serial port.
//! The screen answers every request with "1 200" and AckNumber = SeqNumber + 1.

use crate::protocol::{FrameReader, Message, encode_frame};
use serde_json::Value;
use std::io;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const USB_VENDOR_ID: u16 = 0x18D1;
pub const USB_PRODUCT_ID: u16 = 0x2D03;
const POLL_INTERVAL: Duration = Duration::from_millis(10);
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug)]
pub enum LinkError {
    Io(io::Error),
    Timeout { acknowledgement: u64 },
    NotFound,
    /// The Turris didn't answer a query (its command number). Only the Turris, which isn't
    /// looked for on Windows, gives these two.
    #[cfg_attr(windows, allow(dead_code))]
    NoAnswer { command: u32 },
    /// The Turris answered with an error code.
    #[cfg_attr(windows, allow(dead_code))]
    Refused { code: u64, why: String },
}

impl std::fmt::Display for LinkError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
            Self::Timeout { acknowledgement } => write!(formatter, "screen did not answer (waiting for AckNumber={acknowledgement})"),
            Self::NotFound if cfg!(windows) => write!(formatter, "no TRYX Panorama found (USB 18d1:2d03)"),
            Self::NotFound => write!(formatter, "no TRYX Panorama (USB 18d1:2d03) or Turris (USB 391a:2011) found"),
            Self::NoAnswer { command } => write!(formatter, "screen did not answer request {command}"),
            Self::Refused { code, why } => write!(formatter, "screen refused the request (code {code}: {why})"),
        }
    }
}

impl From<io::Error> for LinkError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub trait Port {
    fn write_bytes(&mut self, data: &[u8]) -> io::Result<()>;
    fn read_available(&mut self) -> io::Result<Vec<u8>>;
}

pub struct PanoramaLink<P: Port> {
    port: P,
    reader: FrameReader,
    next_sequence: u64,
    latest_status: Option<Value>,
}

impl<P: Port> PanoramaLink<P> {
    pub fn new(port: P) -> Self {
        Self { port, reader: FrameReader::default(), next_sequence: 0, latest_status: None }
    }

    pub fn latest_status(&self) -> Option<&Value> {
        self.latest_status.as_ref()
    }

    pub fn request(&mut self, method: &str, command: &str, body: Option<&Value>, timeout: Duration) -> Result<Message, LinkError> {
        let sequence = self.send(method, command, body)?;
        self.await_acknowledgement(sequence + 1, timeout)
    }

    fn send(&mut self, method: &str, command: &str, body: Option<&Value>) -> Result<u64, LinkError> {
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        let message = Message::request(method, command, sequence, unix_millis(), body);
        self.port.write_bytes(&encode_frame(&message.to_bytes()))?;
        Ok(sequence)
    }

    fn await_acknowledgement(&mut self, acknowledgement: u64, timeout: Duration) -> Result<Message, LinkError> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Some(response) = self.read_responses()?.into_iter().find(|response| response.acknowledged_sequence() == Some(acknowledgement)) {
                return Ok(response);
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        Err(LinkError::Timeout { acknowledgement })
    }

    fn read_responses(&mut self) -> Result<Vec<Message>, LinkError> {
        let data = self.port.read_available()?;
        let responses: Vec<Message> = self.reader.feed(&data).iter().map(|payload| Message::parse(payload)).collect();
        for response in &responses {
            self.remember_status(response);
        }
        Ok(responses)
    }

    fn remember_status(&mut self, response: &Message) {
        if let Some(body) = response.json().filter(|body| body.get("status").is_some()) {
            self.latest_status = Some(body);
        }
    }
}

pub fn unix_millis() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|elapsed| elapsed.as_millis() as u64).unwrap_or(0)
}

pub struct SerialDevice(Box<dyn serialport::SerialPort>);

impl Port for SerialDevice {
    fn write_bytes(&mut self, data: &[u8]) -> io::Result<()> {
        self.0.write_all(data)?;
        self.0.flush()
    }

    fn read_available(&mut self) -> io::Result<Vec<u8>> {
        let waiting = self.0.bytes_to_read().map_err(io::Error::other)? as usize;
        let mut buffer = vec![0; waiting];
        if waiting > 0 {
            self.0.read_exact(&mut buffer)?;
        }
        Ok(buffer)
    }
}

pub fn open_serial_device(name: &str) -> Result<SerialDevice, LinkError> {
    let port = serialport::new(name, 115_200).timeout(Duration::from_millis(200)).open().map_err(io::Error::other)?;
    Ok(SerialDevice(port))
}

pub fn find_serial_port() -> Result<String, LinkError> {
    if cfg!(target_os = "linux") {
        find_in_sysfs(Path::new("/")).ok_or(LinkError::NotFound)
    } else {
        find_with_serialport().ok_or(LinkError::NotFound)
    }
}

/// Linux: /sys/class/tty/ttyACM*/device is the USB interface; its parent holds idVendor/idProduct.
fn find_in_sysfs(root: &Path) -> Option<String> {
    let entries = std::fs::read_dir(root.join("sys/class/tty")).ok()?;
    let mut names: Vec<String> = entries.flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names.into_iter().find(|name| is_panorama_tty(root, name)).map(|name| format!("/dev/{name}"))
}

fn is_panorama_tty(root: &Path, name: &str) -> bool {
    let interface = root.join("sys/class/tty").join(name).join("device");
    let usb_device = interface.join("..");
    read_hex(&usb_device.join("idVendor")) == Some(USB_VENDOR_ID) && read_hex(&usb_device.join("idProduct")) == Some(USB_PRODUCT_ID)
}

fn read_hex(path: &Path) -> Option<u16> {
    u16::from_str_radix(std::fs::read_to_string(path).ok()?.trim(), 16).ok()
}

fn find_with_serialport() -> Option<String> {
    serialport::available_ports().ok()?.into_iter().find_map(|port| match port.port_type {
        serialport::SerialPortType::UsbPort(usb) if (usb.vid, usb.pid) == (USB_VENDOR_ID, USB_PRODUCT_ID) => Some(port.port_name),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::encode_frame;

    /// Answers every request like the real screen: AckNumber = SeqNumber + 1.
    #[derive(Default)]
    struct FakePanorama {
        silent: bool,
        received: Vec<Message>,
        reader: FrameReader,
        pending: Vec<u8>,
    }

    impl Port for &mut FakePanorama {
        fn write_bytes(&mut self, data: &[u8]) -> io::Result<()> {
            for payload in self.reader.feed(data) {
                let request = Message::parse(&payload);
                if !self.silent {
                    self.pending.extend(encode_frame(&response_to(&request)));
                }
                self.received.push(request);
            }
            Ok(())
        }

        fn read_available(&mut self) -> io::Result<Vec<u8>> {
            Ok(std::mem::take(&mut self.pending))
        }
    }

    fn response_to(request: &Message) -> Vec<u8> {
        let acknowledgement: u64 = request.header("SeqNumber").unwrap().parse::<u64>().unwrap() + 1;
        let body = r#"{"status":{"fanLCD":"2010","turboPump":"2850"},"availableStorage":3393744896}"#;
        format!("1 200\r\nAckNumber={acknowledgement}\r\nContentLength={}\r\nContentType=json\r\n\r\n{body}", body.len()).into_bytes()
    }

    #[test]
    fn request_returns_matching_response_and_numbers_increase() {
        let mut device = FakePanorama::default();
        let mut link = PanoramaLink::new(&mut device);
        let response = link.request("POST", "conn", None, DEFAULT_TIMEOUT).unwrap();
        assert_eq!(response.start_line, "1 200");
        link.request("STATE", "all", Some(&serde_json::json!({})), DEFAULT_TIMEOUT).unwrap();
        assert_eq!(link.latest_status().unwrap()["status"]["turboPump"], "2850");
        drop(link);
        let sequences: Vec<&str> = device.received.iter().map(|message| message.header("SeqNumber").unwrap()).collect();
        assert_eq!(sequences, ["0", "1"]);
    }

    #[test]
    fn times_out_when_screen_is_silent() {
        let mut device = FakePanorama { silent: true, ..FakePanorama::default() };
        let mut link = PanoramaLink::new(&mut device);
        let result = link.request("POST", "conn", None, Duration::from_millis(30));
        assert!(matches!(result, Err(LinkError::Timeout { acknowledgement: 1 })));
    }

    #[cfg(unix)]
    #[test]
    fn finds_panorama_tty_in_sysfs() {
        let root = std::env::temp_dir().join(format!("canalis-sysfs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let usb_device = root.join("sys/devices/usb1/1-2");
        let interface = usb_device.join("1-2:1.0");
        std::fs::create_dir_all(&interface).unwrap();
        std::fs::write(usb_device.join("idVendor"), "18d1\n").unwrap();
        std::fs::write(usb_device.join("idProduct"), "2d03\n").unwrap();
        let tty = root.join("sys/class/tty/ttyACM0");
        std::fs::create_dir_all(&tty).unwrap();
        std::os::unix::fs::symlink(&interface, tty.join("device")).unwrap();
        assert_eq!(find_in_sysfs(&root), Some("/dev/ttyACM0".to_string()));
    }
}
