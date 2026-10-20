//! The TRYX Turris: the 5" 1280×720 screen of the TURRIS 620 air cooler (USB 391a:2011).
//!
//! It works differently from the Panorama's Android screen. The Turris is a small Linux device
//! that shows up as a bidirectional USB printer, which the kernel's usblp driver turns into
//! /dev/usb/lp*. Requests and responses are protobuf messages in "TRYX" frames; its media
//! library holds the built-in themes, and live stats are labels it draws over the media.

mod link;
mod messages;
mod overlay;
mod session;
mod wire;

pub use link::{UsbPrinterPort, find_device};
pub use session::TurrisScreen;
