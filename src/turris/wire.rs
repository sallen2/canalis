//! Protobuf encoding and the "TRYX" framing that the Turris speaks over its USB printer pipe.
//!
//! Frame: b"TRYX" | payload length: u32 little-endian | payload (one protobuf message)
//!
//! Only what the screen's messages use is implemented: varints, zigzag sint32 and
//! length-delimited fields (strings, bytes and nested messages).

pub const MAGIC: &[u8; 4] = b"TRYX";
const HEADER_SIZE: usize = 8;
/// The screen never sends more in one frame; a larger length means the stream lost its place.
pub const MAX_PAYLOAD_SIZE: usize = 1024 * 1024;

const VARINT: u64 = 0;
const FIXED64: u64 = 1;
const LENGTH_DELIMITED: u64 = 2;
const FIXED32: u64 = 5;

/// A protobuf message, written field by field.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Message(Vec<u8>);

impl Message {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn varint(mut self, field: u32, value: u64) -> Self {
        put_varint(&mut self.0, (u64::from(field) << 3) | VARINT);
        put_varint(&mut self.0, value);
        self
    }

    pub fn boolean(self, field: u32, value: bool) -> Self {
        self.varint(field, u64::from(value))
    }

    /// A `sint32` field, zigzag-encoded so small negative numbers stay short.
    pub fn sint32(self, field: u32, value: i32) -> Self {
        self.varint(field, u64::from((value.wrapping_shl(1) ^ (value >> 31)) as u32))
    }

    pub fn bytes(mut self, field: u32, value: &[u8]) -> Self {
        put_varint(&mut self.0, (u64::from(field) << 3) | LENGTH_DELIMITED);
        put_varint(&mut self.0, value.len() as u64);
        self.0.extend_from_slice(value);
        self
    }

    pub fn string(self, field: u32, value: &str) -> Self {
        self.bytes(field, value.as_bytes())
    }

    pub fn message(self, field: u32, value: &Message) -> Self {
        self.bytes(field, &value.0)
    }

    /// Appends fields that are already encoded, e.g. ones kept from a message the screen sent.
    pub fn raw(mut self, encoded_fields: &[u8]) -> Self {
        self.0.extend_from_slice(encoded_fields);
        self
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

fn put_varint(buffer: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7F) as u8;
        value >>= 7;
        if value == 0 {
            buffer.push(byte);
            return;
        }
        buffer.push(byte | 0x80);
    }
}

fn take_varint(data: &[u8], position: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *data.get(*position)?;
        *position += 1;
        value |= u64::from(byte & 0x7F) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

/// One decoded field value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Field<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
    Fixed32(u32),
    Fixed64(u64),
}

/// The fields of an encoded message, in order, as (field number, value, the field's own bytes).
/// Iteration stops at the first malformed field.
pub fn fields(data: &[u8]) -> Fields<'_> {
    Fields { data, position: 0 }
}

pub struct Fields<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> Iterator for Fields<'a> {
    type Item = (u32, Field<'a>, &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        let start = self.position;
        let decoded = self.decode_one();
        if decoded.is_none() {
            self.position = self.data.len();
        }
        decoded.map(|(number, field)| (number, field, &self.data[start..self.position]))
    }
}

impl<'a> Fields<'a> {
    fn decode_one(&mut self) -> Option<(u32, Field<'a>)> {
        if self.position >= self.data.len() {
            return None;
        }
        let key = take_varint(self.data, &mut self.position)?;
        let number = u32::try_from(key >> 3).ok()?;
        let field = match key & 7 {
            VARINT => Field::Varint(take_varint(self.data, &mut self.position)?),
            LENGTH_DELIMITED => {
                let length = usize::try_from(take_varint(self.data, &mut self.position)?).ok()?;
                let end = self.position.checked_add(length).filter(|end| *end <= self.data.len())?;
                let value = &self.data[self.position..end];
                self.position = end;
                Field::Bytes(value)
            }
            FIXED32 => Field::Fixed32(u32::from_le_bytes(self.take(4)?.try_into().ok()?)),
            FIXED64 => Field::Fixed64(u64::from_le_bytes(self.take(8)?.try_into().ok()?)),
            _ => return None,
        };
        Some((number, field))
    }

    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let end = self.position.checked_add(count).filter(|end| *end <= self.data.len())?;
        let value = &self.data[self.position..end];
        self.position = end;
        Some(value)
    }
}

/// The last value of a varint field (protobuf's rule when a field repeats), or None.
pub fn varint_field(data: &[u8], number: u32) -> Option<u64> {
    fields(data).filter(|(field, _, _)| *field == number).filter_map(|(_, value, _)| if let Field::Varint(value) = value { Some(value) } else { None }).last()
}

pub fn bytes_field(data: &[u8], number: u32) -> Option<&[u8]> {
    fields(data).filter(|(field, _, _)| *field == number).filter_map(|(_, value, _)| if let Field::Bytes(value) = value { Some(value) } else { None }).last()
}

pub fn string_field(data: &[u8], number: u32) -> Option<String> {
    bytes_field(data, number).map(|value| String::from_utf8_lossy(value).into_owned())
}

pub fn repeated_bytes(data: &[u8], number: u32) -> Vec<&[u8]> {
    fields(data).filter(|(field, _, _)| *field == number).filter_map(|(_, value, _)| if let Field::Bytes(value) = value { Some(value) } else { None }).collect()
}

/// Every field of `data` except those numbered in `numbers`, still encoded.
pub fn fields_except(data: &[u8], numbers: &[u32]) -> Vec<u8> {
    fields(data).filter(|(field, _, _)| !numbers.contains(field)).flat_map(|(_, _, encoded)| encoded.iter().copied()).collect()
}

pub fn encode_frame(payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(HEADER_SIZE + payload.len());
    frame.extend_from_slice(MAGIC);
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(payload);
    frame
}

/// Reassembles frame payloads from whatever the USB pipe delivers, skipping anything that
/// doesn't start with the magic.
#[derive(Default)]
pub struct FrameReader {
    buffer: Vec<u8>,
}

impl FrameReader {
    pub fn feed(&mut self, data: &[u8]) -> Vec<Vec<u8>> {
        self.buffer.extend_from_slice(data);
        let mut frames = Vec::new();
        loop {
            self.skip_to_magic();
            if self.buffer.len() < HEADER_SIZE {
                return frames;
            }
            let size = u32::from_le_bytes(self.buffer[4..8].try_into().unwrap()) as usize;
            if size > MAX_PAYLOAD_SIZE {
                // Text inside a payload can contain "TRYX"; look for the next real header.
                self.buffer.remove(0);
                continue;
            }
            if self.buffer.len() < HEADER_SIZE + size {
                return frames;
            }
            frames.push(self.buffer[HEADER_SIZE..HEADER_SIZE + size].to_vec());
            self.buffer.drain(..HEADER_SIZE + size);
        }
    }

    fn skip_to_magic(&mut self) {
        if let Some(start) = self.buffer.windows(MAGIC.len()).position(|window| window == MAGIC) {
            self.buffer.drain(..start);
            return;
        }
        // Keep a tail that could be the start of a magic split across two reads.
        let keep = (1..MAGIC.len()).rev().find(|&length| self.buffer.ends_with(&MAGIC[..length])).unwrap_or(0);
        let discard = self.buffer.len().saturating_sub(keep);
        self.buffer.drain(..discard);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varints_and_strings_encode_like_protobuf() {
        let message = Message::new().varint(1, 1).varint(2, 300).string(3, "NA");
        assert_eq!(message.as_bytes(), [0x08, 0x01, 0x10, 0xAC, 0x02, 0x1A, 0x02, b'N', b'A']);
    }

    #[test]
    fn sint32_is_zigzag_encoded() {
        assert_eq!(Message::new().sint32(7, -10).as_bytes(), [0x38, 19]);
        assert_eq!(Message::new().sint32(4, 13).as_bytes(), [0x20, 26]);
        assert_eq!(Message::new().sint32(7, 1).as_bytes(), [0x38, 2]);
    }

    #[test]
    fn field_numbers_above_fifteen_use_two_byte_keys() {
        // Field 500 (DeviceInformation) as the screen sends it.
        assert_eq!(&Message::new().bytes(500, b"").into_bytes()[..2], [0xA2, 0x1F]);
    }

    #[test]
    fn decodes_nested_fields_and_keeps_their_bytes() {
        let inner = Message::new().varint(1, 1280).varint(2, 720);
        let outer = Message::new().string(1, "Turris").message(10, &inner);
        let decoded: Vec<u32> = fields(outer.as_bytes()).map(|(number, _, _)| number).collect();
        assert_eq!(decoded, [1, 10]);
        assert_eq!(string_field(outer.as_bytes(), 1).as_deref(), Some("Turris"));
        assert_eq!(varint_field(bytes_field(outer.as_bytes(), 10).unwrap(), 1), Some(1280));
        assert_eq!(fields_except(outer.as_bytes(), &[10]), Message::new().string(1, "Turris").into_bytes());
    }

    #[test]
    fn malformed_data_ends_iteration() {
        assert_eq!(fields(&[0x0A, 0x05, b'a']).count(), 0);
        assert_eq!(fields(&[0x08]).count(), 0);
    }

    #[test]
    fn frames_round_trip_across_split_reads() {
        let frame = encode_frame(b"hello");
        assert_eq!(&frame[..8], b"TRYX\x05\x00\x00\x00");
        let mut reader = FrameReader::default();
        assert!(reader.feed(&frame[..2]).is_empty());
        assert!(reader.feed(&frame[2..9]).is_empty());
        assert_eq!(reader.feed(&frame[9..]), [b"hello".to_vec()]);
    }

    #[test]
    fn reader_skips_noise_and_oversized_headers() {
        let mut stream = b"noise TRYX\xFF\xFF\xFF\xFF".to_vec();
        stream.extend(encode_frame(b"one"));
        stream.extend(encode_frame(b"two"));
        assert_eq!(FrameReader::default().feed(&stream), [b"one".to_vec(), b"two".to_vec()]);
    }
}
