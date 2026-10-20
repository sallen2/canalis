//! What the service does while a Turris is connected: it reads what the screen is and stores,
//! makes it show what the settings describe, and then sends the stats each second.

use super::link::TurrisLink;
use super::messages::{
    DEVICE_INFORMATION, DEVICE_INFORMATION_QUERY, DeviceInformation, Display, MEDIA_CATALOG, MEDIA_CATALOG_QUERY, METRIC_BATCH, MediaCatalog, OVERLAY_LAYOUT, PING, PONG, USER_CONFIGURATION,
    USER_CONFIGURATION_QUERY, USER_CONFIGURATION_REPLY, UserConfiguration, query_body,
};
use super::overlay;
use super::wire::Message;
use crate::link::{LinkError, Port};
use crate::settings::{OverlayStyle, Settings};
use chrono::Local;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

const QUERY_TIMEOUT: Duration = Duration::from_secs(3);
/// KANALI pings the Turris this often unless the screen says it keeps the link alive itself.
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(2);
const SCREEN_SIZE: &str = "1280 × 720";

pub struct TurrisScreen<P: Port> {
    link: TurrisLink<P>,
    catalog: MediaCatalog,
    /// The layout last sent, so an unchanged one isn't sent again.
    sent_layout: Option<Vec<u8>>,
    last_ping: Instant,
}

impl<P: Port> TurrisScreen<P> {
    /// Asks the screen what it is and what it stores; returns the details for the status file.
    pub fn connect(port: P) -> Result<(Self, Value), LinkError> {
        let mut link = TurrisLink::new(port);
        link.discard_responses()?;
        let information = DeviceInformation::parse(&link.query(DEVICE_INFORMATION_QUERY, &query_body(), DEVICE_INFORMATION, QUERY_TIMEOUT)?);
        let catalog = MediaCatalog::parse(&link.query(MEDIA_CATALOG_QUERY, &query_body(), MEDIA_CATALOG, QUERY_TIMEOUT)?);
        let device = device_details(&information, &catalog);
        Ok((Self { link, catalog, sent_layout: None, last_ping: Instant::now() }, device))
    }

    /// Makes the screen show what `settings` describe. Only what differs is written: every
    /// configuration write makes the screen flash.
    pub fn apply(&mut self, settings: &Settings, spec: &Value) -> Result<(), LinkError> {
        let current = UserConfiguration::parse(&self.link.query(USER_CONFIGURATION_QUERY, &query_body(), USER_CONFIGURATION_REPLY, QUERY_TIMEOUT)?);
        let media = self.media_for(settings);
        let display = Display { backlight: settings.enable, brightness: u32::from(settings.brightness), mirror: settings.is_mirrored() };
        let media_changes = media.as_deref().is_some_and(|media| media != current.media());
        let configuration_changes = media_changes || display != current.display();
        if configuration_changes {
            self.link.send(USER_CONFIGURATION, &current.with(media.as_deref(), display).to_message())?;
        }
        let (fields, style) = active_overlay(settings);
        let layout = overlay::layout(fields, style, &settings.temperature_unit, spec);
        // The layout also makes a newly written configuration take effect.
        if configuration_changes || self.sent_layout.as_deref() != Some(layout.as_bytes()) {
            self.link.send(OVERLAY_LAYOUT, &layout)?;
            self.sent_layout = Some(layout.into_bytes());
        }
        if media_changes {
            self.link.restart_playback()?;
        }
        Ok(())
    }

    /// Sends the stats' new values, and a keepalive ping every couple of seconds; the ping's
    /// answer is also how a screen that went away is noticed.
    pub fn update(&mut self, settings: &Settings, snapshot: &Value) -> Result<(), LinkError> {
        let (fields, _) = active_overlay(settings);
        if let Some(batch) = overlay::metric_batch(fields, &settings.temperature_unit, snapshot, &Local::now()) {
            self.link.send(METRIC_BATCH, &batch)?;
        }
        if self.last_ping.elapsed() >= KEEPALIVE_INTERVAL {
            self.link.query(PING, &Message::new().string(1, "canalis"), PONG, QUERY_TIMEOUT)?;
            self.last_ping = Instant::now();
        }
        Ok(())
    }

    /// The file the screen should play, when the settings choose one of its presets.
    fn media_for(&self, settings: &Settings) -> Option<String> {
        let preset = settings.turris_preset()?;
        match self.catalog.preset(preset) {
            Some(entry) => Some(entry.file_name().to_string()),
            None => {
                eprintln!("canalis: the Turris has no preset called {preset}");
                None
            }
        }
    }
}

/// The stats and style shown over the media the settings choose.
fn active_overlay(settings: &Settings) -> (&[String], &OverlayStyle) {
    if settings.is_custom_theme() { (&settings.custom.overlay, &settings.custom.style) } else { (&settings.overlay, &settings.overlay_style) }
}

/// The screen's details in the shape the window reads (the Panorama's keys where they fit).
fn device_details(information: &DeviceInformation, catalog: &MediaCatalog) -> Value {
    let system = format!("{} {}", information.os_name, information.firmware_version).trim().to_string();
    json!({
        "model": "Turris",
        "productId": "TURRIS 620",
        "OS": system,
        "version": { "firmware": information.app_version },
        "sn": information.serial_number,
        "screen": SCREEN_SIZE,
        "presets": catalog.presets.iter().map(|entry| entry.stem()).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::DATE_AND_TIME;
    use crate::turris::link::tests::{FakeTurris, answering};
    use crate::turris::messages::tests::{MEDIA_CATALOG_FRAME, PONG_FRAME, USER_CONFIGURATION_FRAME, hex};
    use crate::turris::messages::{PING, TRANSFER_END};
    use crate::turris::wire::{bytes_field, fields};
    use std::cell::RefCell;
    use std::rc::Rc;

    /// An answer with an empty header (the fake fills in the track id) and one body field.
    fn answer(body_field: u32, body: &Message) -> Vec<u8> {
        Message::new().message(1, &Message::new()).message(body_field, body).into_bytes()
    }

    /// The command number of each request, in order.
    fn commands(received: &[Vec<u8>]) -> Vec<u32> {
        received.iter().map(|request| fields(request).map(|(number, _, _)| number).find(|number| *number != 1).unwrap()).collect()
    }

    /// A screen that answers queries like the captured TURRIS 620 and stores configuration writes.
    fn turris() -> (FakeTurris, Rc<RefCell<Vec<u8>>>) {
        let stored = Rc::new(RefCell::new(hex(USER_CONFIGURATION_FRAME)));
        let configuration = stored.clone();
        let fake = FakeTurris::new(move |request| {
            let command = fields(request).map(|(number, _, _)| number).find(|number| *number != 1).unwrap();
            let canned = match command {
                DEVICE_INFORMATION_QUERY => answer(DEVICE_INFORMATION, &Message::new().string(4, "Turris").string(5, "v1.0.2.20260105")),
                MEDIA_CATALOG_QUERY => hex(MEDIA_CATALOG_FRAME),
                USER_CONFIGURATION_QUERY => configuration.borrow().clone(),
                PING => hex(PONG_FRAME),
                // No file is being transferred: status 2, as the real screen answers.
                TRANSFER_END => answer(crate::turris::messages::TRANSFER_END_STATUS, &Message::new().varint(1, 2)),
                USER_CONFIGURATION => {
                    let written = bytes_field(request, USER_CONFIGURATION).unwrap().to_vec();
                    *configuration.borrow_mut() = answer(USER_CONFIGURATION_REPLY, &Message::new().raw(&written));
                    return None;
                }
                _ => return None,
            };
            Some(answering(request, &canned))
        });
        (fake, stored)
    }

    fn settings_with_theme(theme: &str) -> Settings {
        Settings { theme: theme.into(), ..Settings::default() }
    }

    #[test]
    fn connect_reads_what_the_screen_is() {
        let (mut fake, _) = turris();
        let (_, device) = TurrisScreen::connect(&mut fake).unwrap();
        assert_eq!(device["productId"], "TURRIS 620");
        assert_eq!(device["presets"], json!(["Cosmos", "Erebos", "Festival", "MechanicalArm", "Meteorite"]));
        assert_eq!(commands(&fake.received), [DEVICE_INFORMATION_QUERY, MEDIA_CATALOG_QUERY]);
    }

    #[test]
    fn matching_settings_write_nothing_but_the_layout() {
        let (mut fake, _) = turris();
        {
            let (mut screen, _) = TurrisScreen::connect(&mut fake).unwrap();
            let settings = Settings { brightness: 100, ..settings_with_theme("Festival") };
            screen.apply(&settings, &json!({})).unwrap();
            screen.apply(&settings, &json!({})).unwrap();
        }
        assert_eq!(&commands(&fake.received)[2..], [USER_CONFIGURATION_QUERY, OVERLAY_LAYOUT, USER_CONFIGURATION_QUERY]);
    }

    #[test]
    fn choosing_a_preset_writes_it_and_restarts_playback() {
        let (mut fake, stored) = turris();
        {
            let (mut screen, _) = TurrisScreen::connect(&mut fake).unwrap();
            screen.apply(&Settings { brightness: 100, ..settings_with_theme("Cosmos") }, &json!({})).unwrap();
        }
        assert_eq!(&commands(&fake.received)[2..], [USER_CONFIGURATION_QUERY, USER_CONFIGURATION, OVERLAY_LAYOUT, TRANSFER_END]);
        let configuration = UserConfiguration::parse(bytes_field(&stored.borrow(), USER_CONFIGURATION_REPLY).unwrap());
        assert_eq!(configuration.media(), "Cosmos.mp4.h264_1280x720");
    }

    #[test]
    fn a_panorama_theme_leaves_the_media_alone_but_applies_the_display() {
        let (mut fake, stored) = turris();
        {
            let (mut screen, _) = TurrisScreen::connect(&mut fake).unwrap();
            let settings = Settings { brightness: 40, rotate: crate::settings::MIRRORED_ROTATION, ..Settings::default() };
            screen.apply(&settings, &json!({})).unwrap();
        }
        assert_eq!(&commands(&fake.received)[2..], [USER_CONFIGURATION_QUERY, USER_CONFIGURATION, OVERLAY_LAYOUT]);
        let configuration = UserConfiguration::parse(bytes_field(&stored.borrow(), USER_CONFIGURATION_REPLY).unwrap());
        assert_eq!(configuration.media(), "Festival.mp4.h264_1280x720");
        assert_eq!(configuration.display(), Display { backlight: true, brightness: 40, mirror: true });
    }

    #[test]
    fn update_sends_readings_and_pings_now_and_then() {
        let (mut fake, _) = turris();
        {
            let (mut screen, _) = TurrisScreen::connect(&mut fake).unwrap();
            let settings = Settings { overlay: vec!["CPU Temperature".into(), DATE_AND_TIME.into()], ..Settings::default() };
            screen.update(&settings, &json!({"cpu": {"temperature": 50}})).unwrap();
            screen.last_ping -= KEEPALIVE_INTERVAL;
            screen.update(&settings, &json!({"cpu": {"temperature": 51}})).unwrap();
        }
        assert_eq!(&commands(&fake.received)[2..], [METRIC_BATCH, METRIC_BATCH, PING]);
    }

    #[test]
    fn no_stats_means_no_readings() {
        let (mut fake, _) = turris();
        {
            let (mut screen, _) = TurrisScreen::connect(&mut fake).unwrap();
            screen.update(&Settings::default(), &json!({})).unwrap();
        }
        assert_eq!(commands(&fake.received).len(), 2);
    }
}
