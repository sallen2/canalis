//! The Turris's messages. Each frame holds a Request or a Response: a header (version, track id),
//! and in responses an error (code 0 means success), then one body field whose number says what
//! the message is. The numbers follow DXVSI/Tryx-Linux-GUI's protocol notes (protocol/wire-v1,
//! MIT licence); the decoders are tested against frames captured from a TURRIS 620.

use super::wire::{Field, Message, bytes_field, fields, fields_except, repeated_bytes, string_field, varint_field};

pub const PING: u32 = 10;
pub const DEVICE_INFORMATION_QUERY: u32 = 100;
pub const MEDIA_CATALOG_QUERY: u32 = 103;
pub const USER_CONFIGURATION_QUERY: u32 = 104;
/// The screen stores these without answering them.
pub const USER_CONFIGURATION: u32 = 200;
pub const OVERLAY_LAYOUT: u32 = 201;
pub const METRIC_BATCH: u32 = 300;
pub const TRANSFER_END: u32 = 402;

pub const PONG: u32 = 10;
pub const DEVICE_INFORMATION: u32 = 500;
pub const MEDIA_CATALOG: u32 = 503;
pub const USER_CONFIGURATION_REPLY: u32 = 504;
pub const TRANSFER_END_STATUS: u32 = 802;

/// File transfers, and the playback restart that reuses their end message, carry this fixed
/// track id and header version 0, as KANALI's do.
pub const TRANSFER_TRACK_ID: u64 = 981_521;

const HEADER: u32 = 1;
const ERROR: u32 = 2;

/// The body of every query: a placeholder string, as KANALI sends it.
pub fn query_body() -> Message {
    Message::new().string(1, "NA")
}

pub fn request(version: u32, track_id: u64, command: u32, body: &Message) -> Vec<u8> {
    let mut header = Message::new();
    if version != 0 {
        header = header.varint(1, u64::from(version));
    }
    header = header.varint(2, track_id);
    Message::new().message(HEADER, &header).message(command, body).into_bytes()
}

/// Restarts playback after the media changes: the end of a file transfer with no file, which
/// the screen answers with status 2 (no file) but acts on.
pub fn restart_playback() -> Message {
    Message::new().string(1, "media")
}

#[derive(Debug, PartialEq)]
pub struct Response {
    pub track_id: u64,
    /// The screen's error code and reason, when the code isn't 0.
    pub error: Option<(u64, String)>,
    /// The body's field number and encoded message.
    pub body: Option<(u32, Vec<u8>)>,
}

impl Response {
    pub fn parse(payload: &[u8]) -> Option<Self> {
        let header = bytes_field(payload, HEADER)?;
        let track_id = varint_field(header, 2).unwrap_or(0);
        let error = bytes_field(payload, ERROR).and_then(|error| {
            let code = varint_field(error, 1).unwrap_or(0);
            (code != 0).then(|| (code, string_field(error, 2).unwrap_or_default()))
        });
        let body = fields(payload).find(|(number, _, _)| *number != HEADER && *number != ERROR).and_then(|(number, field, _)| match field {
            Field::Bytes(value) => Some((number, value.to_vec())),
            _ => None,
        });
        Some(Self { track_id, error, body })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeviceInformation {
    pub os_name: String,
    pub os_version: String,
    pub firmware_version: String,
    pub product_name: String,
    pub app_version: String,
    pub serial_number: String,
    pub chip_id: String,
}

impl DeviceInformation {
    pub fn parse(body: &[u8]) -> Self {
        let text = |number| string_field(body, number).unwrap_or_default().trim().to_string();
        Self { os_name: text(1), os_version: text(2), firmware_version: text(3), product_name: text(4), app_version: text(5), serial_number: text(8), chip_id: text(10) }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MediaEntry {
    /// Path on the screen, e.g. "preset/Festival.mp4.h264_1280x720".
    pub path: String,
    pub size: u64,
}

impl MediaEntry {
    fn parse(body: &[u8]) -> Self {
        Self { path: string_field(body, 1).unwrap_or_default(), size: varint_field(body, 3).unwrap_or(0) }
    }

    /// The name the screen's user configuration refers to it by: the path without its folder.
    pub fn file_name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    /// The name without extensions, e.g. "Festival".
    pub fn stem(&self) -> &str {
        self.file_name().split('.').next().unwrap_or_default()
    }
}

/// What the screen has stored: your uploads and its read-only built-in themes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MediaCatalog {
    pub media: Vec<MediaEntry>,
    pub presets: Vec<MediaEntry>,
}

impl MediaCatalog {
    pub fn parse(body: &[u8]) -> Self {
        let entries = |number| repeated_bytes(body, number).into_iter().map(MediaEntry::parse).collect();
        Self { media: entries(1), presets: entries(2) }
    }

    pub fn preset(&self, stem: &str) -> Option<&MediaEntry> {
        self.presets.iter().find(|entry| entry.stem().eq_ignore_ascii_case(stem))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Display {
    pub backlight: bool,
    pub brightness: u32,
    pub mirror: bool,
}

const WORK: u32 = 3;
const DISPLAY: u32 = 5;
const WORK_SINGLE_MEDIA: u32 = 3;
const WORK_MODES: [u32; 2] = [1, 2];
const DISPLAY_BACKLIGHT: u32 = 1;
const DISPLAY_BRIGHTNESS: u32 = 2;
const DISPLAY_MIRROR: u32 = 3;

/// The screen's stored settings: power-on and standby media, the media it plays (work), filter
/// and display. Changes are made on a copy of what the screen reported, so every section and
/// field Canalis doesn't touch goes back exactly as it came: KANALI always writes the whole
/// configuration, and the screen treats a missing section as empty.
#[derive(Clone, Debug, PartialEq)]
pub struct UserConfiguration {
    encoded: Vec<u8>,
}

impl UserConfiguration {
    pub fn parse(body: &[u8]) -> Self {
        Self { encoded: body.to_vec() }
    }

    /// The media played in single mode, e.g. "Festival.mp4.h264_1280x720".
    pub fn media(&self) -> String {
        bytes_field(&self.encoded, WORK).and_then(|work| string_field(work, WORK_SINGLE_MEDIA)).unwrap_or_default()
    }

    pub fn display(&self) -> Display {
        let display = bytes_field(&self.encoded, DISPLAY).unwrap_or_default();
        Display {
            backlight: varint_field(display, DISPLAY_BACKLIGHT).unwrap_or(0) != 0,
            brightness: varint_field(display, DISPLAY_BRIGHTNESS).unwrap_or(0) as u32,
            mirror: varint_field(display, DISPLAY_MIRROR).unwrap_or(0) != 0,
        }
    }

    /// A copy that plays `media` on its own and in a loop (when given) and has `display`.
    pub fn with(&self, media: Option<&str>, display: Display) -> Self {
        let mut message = Message::new();
        let (mut has_work, mut has_display) = (false, false);
        for (number, field, encoded) in fields(&self.encoded) {
            match (number, field, media) {
                (WORK, Field::Bytes(work), Some(media)) => {
                    message = message.message(WORK, &single_media(work, media));
                    has_work = true;
                }
                (DISPLAY, Field::Bytes(old), _) => {
                    message = message.message(DISPLAY, &display_fields(old, display));
                    has_display = true;
                }
                _ => message = message.raw(encoded),
            }
        }
        if let (false, Some(media)) = (has_work, media) {
            message = message.message(WORK, &single_media(&[], media));
        }
        if !has_display {
            message = message.message(DISPLAY, &display_fields(&[], display));
        }
        Self { encoded: message.into_bytes() }
    }

    pub fn to_message(&self) -> Message {
        Message::new().raw(&self.encoded)
    }
}

/// Single media mode and single loop are both 0, protobuf's default, so they are left out just
/// as the screen leaves them out.
fn single_media(old_work: &[u8], media: &str) -> Message {
    let kept: Vec<u32> = WORK_MODES.iter().copied().chain([WORK_SINGLE_MEDIA]).collect();
    Message::new().string(WORK_SINGLE_MEDIA, media).raw(&fields_except(old_work, &kept))
}

fn display_fields(old_display: &[u8], display: Display) -> Message {
    let mut message = Message::new();
    if display.backlight {
        message = message.boolean(DISPLAY_BACKLIGHT, true);
    }
    if display.brightness != 0 {
        message = message.varint(DISPLAY_BRIGHTNESS, u64::from(display.brightness));
    }
    if display.mirror {
        message = message.boolean(DISPLAY_MIRROR, true);
    }
    message.raw(&fields_except(old_display, &[DISPLAY_BACKLIGHT, DISPLAY_BRIGHTNESS, DISPLAY_MIRROR]))
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    pub fn hex(text: &str) -> Vec<u8> {
        (0..text.len()).step_by(2).map(|index| u8::from_str_radix(&text[index..index + 2], 16).unwrap()).collect()
    }

    // Responses captured from a TURRIS 620 (screen app v1.0.2.20260105).
    pub const PONG_FRAME: &str = "0a09080110989ddf8cc605120412024e4152060a0448657921";
    const DEVICE_INFORMATION_FRAME: &str = "0a09080110e9cbc7a2dc06120412024e41a21f90010a054c696e7578122723313620505245454d5054204d6f6e204a616e20352031353a34323a30372043535420323032361a1f352e31302e342d7461672d2d676231343737323034353233382d646972747922065475727269732a0f76312e302e322e323032363031303542105455524a4a32363033303030373230384801521041384339413944313641353531413135";
    pub const MEDIA_CATALOG_FRAME: &str = "0a090801108ef3fadea415120412024e41ba1fa40212360a1f7072657365742f436f736d6f732e6d70342e683236345f3132383078373230120e2e683236345f313238307837323018dcd08b0712360a1f7072657365742f457265626f732e6d70342e683236345f3132383078373230120e2e683236345f313238307837323018cbc7ba0512380a217072657365742f466573746976616c2e6d70342e683236345f3132383078373230120e2e683236345f313238307837323018ede3bc06123d0a267072657365742f4d656368616e6963616c41726d2e6d70342e683236345f3132383078373230120e2e683236345f313238307837323018f1c7ce0312390a227072657365742f4d6574656f726974652e6d70342e683236345f3132383078373230120e2e683236345f313238307837323018d09c9a07";
    pub const USER_CONFIGURATION_FRAME: &str = "0a09080110daa09d9ce81f120412024e41c21f740a230a2164656661756c745f706f7765726f6e2e6d70342e683236345f313238307837323012250801122164656661756c745f7374616e6462792e6d70342e683236345f31323830783732301a1c1a1a466573746976616c2e6d70342e683236345f3132383078373230220208012a0408011064";

    fn body(frame: &str, expected: u32) -> Vec<u8> {
        let response = Response::parse(&hex(frame)).unwrap();
        assert_eq!(response.error, None);
        let (number, body) = response.body.unwrap();
        assert_eq!(number, expected);
        body
    }

    #[test]
    fn requests_carry_header_then_body() {
        // header {version 1, track id 300}, then field 100 (key a2 06) holding {1: "NA"}.
        let encoded = request(1, 300, DEVICE_INFORMATION_QUERY, &query_body());
        assert_eq!(encoded, hex("0a05080110ac02a206040a024e41"));
    }

    #[test]
    fn transfer_requests_leave_version_zero_out() {
        // header {track id 981521}, then field 402 (key 92 19) holding {1: "media"}.
        let encoded = request(0, TRANSFER_TRACK_ID, TRANSFER_END, &restart_playback());
        assert_eq!(encoded, hex("0a041091f43b9219070a056d65646961"));
    }

    #[test]
    fn parses_a_pong() {
        let response = Response::parse(&hex(PONG_FRAME)).unwrap();
        assert_eq!(response.track_id, 190_615_899_800);
        assert_eq!(response.body, Some((PONG, Message::new().string(1, "Hey!").into_bytes())));
    }

    #[test]
    fn parses_device_information() {
        let information = DeviceInformation::parse(&body(DEVICE_INFORMATION_FRAME, DEVICE_INFORMATION));
        assert_eq!(information.product_name, "Turris");
        assert_eq!(information.app_version, "v1.0.2.20260105");
        assert_eq!(information.os_name, "Linux");
        assert_eq!(information.firmware_version, "5.10.4-tag--gb14772045238-dirty");
        assert_eq!(information.serial_number, "TURJJ26030007208");
        assert_eq!(information.chip_id, "A8C9A9D16A551A15");
    }

    #[test]
    fn parses_the_media_catalog() {
        let catalog = MediaCatalog::parse(&body(MEDIA_CATALOG_FRAME, MEDIA_CATALOG));
        assert!(catalog.media.is_empty());
        let stems: Vec<&str> = catalog.presets.iter().map(MediaEntry::stem).collect();
        assert_eq!(stems, ["Cosmos", "Erebos", "Festival", "MechanicalArm", "Meteorite"]);
        let festival = catalog.preset("festival").unwrap();
        assert_eq!(festival.file_name(), "Festival.mp4.h264_1280x720");
        assert_eq!(festival.size, 13_578_733);
    }

    #[test]
    fn reads_the_user_configuration() {
        let configuration = UserConfiguration::parse(&body(USER_CONFIGURATION_FRAME, USER_CONFIGURATION_REPLY));
        assert_eq!(configuration.media(), "Festival.mp4.h264_1280x720");
        assert_eq!(configuration.display(), Display { backlight: true, brightness: 100, mirror: false });
    }

    #[test]
    fn an_unchanged_configuration_is_written_back_byte_for_byte() {
        let original = body(USER_CONFIGURATION_FRAME, USER_CONFIGURATION_REPLY);
        let configuration = UserConfiguration::parse(&original);
        let same = configuration.with(Some("Festival.mp4.h264_1280x720"), configuration.display());
        assert_eq!(same.to_message().into_bytes(), original);
    }

    #[test]
    fn changes_media_and_display_and_keeps_everything_else() {
        let original = body(USER_CONFIGURATION_FRAME, USER_CONFIGURATION_REPLY);
        let changed = UserConfiguration::parse(&original).with(Some("Cosmos.mp4.h264_1280x720"), Display { backlight: true, brightness: 40, mirror: true });
        assert_eq!(changed.media(), "Cosmos.mp4.h264_1280x720");
        assert_eq!(changed.display(), Display { backlight: true, brightness: 40, mirror: true });
        for section in [1, 2, 4] {
            assert_eq!(bytes_field(changed.to_message().as_bytes(), section), bytes_field(&original, section), "section {section}");
        }
    }

    #[test]
    fn keeps_the_media_when_none_is_chosen() {
        let original = body(USER_CONFIGURATION_FRAME, USER_CONFIGURATION_REPLY);
        let dimmed = UserConfiguration::parse(&original).with(None, Display { backlight: false, brightness: 10, mirror: false });
        assert_eq!(dimmed.media(), "Festival.mp4.h264_1280x720");
        assert_eq!(dimmed.display(), Display { backlight: false, brightness: 10, mirror: false });
    }
}
