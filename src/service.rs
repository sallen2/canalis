//! Background service: keeps the screen configured and fed with live stats.
//!
//! Panorama start-up order copies KANALI: "POST conn", "POST config", "POST waterBlockScreen",
//! then "STATE all" with a hardware snapshot every second. Without the STATE
//! stream the screen shows its idle animation instead of live readings.
//!
//! A Turris (Linux only) is configured once, then sent the readings of the chosen stats every
//! second; see `turris`.

use crate::device_config::build_device_config;
use crate::link::{DEFAULT_TIMEOUT, LinkError, PanoramaLink, Port, find_serial_port, open_serial_device};
use crate::sensors::{SystemMonitor, read_spec};
use crate::settings::{Settings, SettingsStore};
use crate::status::StatusWriter;
use serde_json::{Value, json};
use std::time::{Duration, SystemTime};

const RECONNECT_DELAY: Duration = Duration::from_secs(3);

pub struct ScreenService {
    store: SettingsStore,
    port_override: Option<String>,
    status: StatusWriter,
    spec: Value,
    monitor: SystemMonitor,
    update_interval: Duration,
}

impl ScreenService {
    pub fn new(store: SettingsStore, status: StatusWriter, port_override: Option<String>) -> Self {
        Self { store, port_override, status, spec: read_spec(), monitor: SystemMonitor::new(), update_interval: Duration::from_secs(1) }
    }

    pub fn run_forever(&mut self) -> ! {
        loop {
            if let Err(error) = self.run_session() {
                eprintln!("canalis: screen unavailable ({error}); retrying in {}s", RECONNECT_DELAY.as_secs());
                self.status.disconnected(&error.to_string());
                std::thread::sleep(RECONNECT_DELAY);
            }
        }
    }

    fn run_session(&mut self) -> Result<(), LinkError> {
        let port_name = match &self.port_override {
            Some(port) => port.clone(),
            None => match find_serial_port() {
                Ok(port) => port,
                #[cfg(not(windows))]
                Err(LinkError::NotFound) => return self.run_turris_session(),
                Err(error) => return Err(error),
            },
        };
        let device = open_serial_device(&port_name)?;
        eprintln!("canalis: connected to screen on {port_name}");
        self.stream(PanoramaLink::new(device), &port_name)
    }

    fn stream(&mut self, mut link: PanoramaLink<impl Port>, port_name: &str) -> Result<(), LinkError> {
        let handshake = link.request("POST", "conn", None, DEFAULT_TIMEOUT)?;
        self.status.connected(port_name, handshake.json(), &self.spec);
        let mut applied_at = self.apply_settings(&mut link)?;
        loop {
            if self.store.modified_time() != applied_at {
                applied_at = self.apply_settings(&mut link)?;
            }
            let snapshot = self.monitor.snapshot();
            link.request("STATE", "all", Some(&snapshot), DEFAULT_TIMEOUT)?;
            self.status.update(&snapshot, link.latest_status());
            std::thread::sleep(self.update_interval);
        }
    }

    fn apply_settings(&mut self, link: &mut PanoramaLink<impl Port>) -> Result<Option<SystemTime>, LinkError> {
        let modified_time = self.store.modified_time();
        let Some(settings) = self.valid_settings() else {
            return Ok(modified_time);
        };
        link.request("POST", "config", Some(&build_device_config(&settings, &self.spec)), DEFAULT_TIMEOUT)?;
        link.request("POST", "waterBlockScreen", Some(&json!({ "enable": settings.enable })), DEFAULT_TIMEOUT)?;
        eprintln!("canalis: applied settings (theme={}, brightness={})", settings.theme, settings.brightness);
        Ok(modified_time)
    }

    /// The settings file, when it is valid; its update interval takes effect straight away.
    fn valid_settings(&mut self) -> Option<Settings> {
        match self.store.load().and_then(|settings| settings.validate().map(|()| settings)) {
            Ok(settings) => {
                self.update_interval = Duration::from_secs_f64(settings.update_interval_seconds);
                Some(settings)
            }
            Err(error) => {
                eprintln!("canalis: ignoring invalid settings file: {error}");
                None
            }
        }
    }

    #[cfg(not(windows))]
    fn run_turris_session(&mut self) -> Result<(), LinkError> {
        use crate::turris::{TurrisScreen, UsbPrinterPort, find_device};
        let device = find_device().ok_or(LinkError::NotFound)?;
        let port = UsbPrinterPort::open(&device).map_err(|error| LinkError::Io(std::io::Error::new(error.kind(), format!("cannot open {device}: {error}"))))?;
        let (mut screen, details) = TurrisScreen::connect(port)?;
        eprintln!("canalis: connected to the Turris on {device}");
        self.status.connected(&device, Some(details), &self.spec);
        let mut settings = Settings::default();
        let mut applied_at = None;
        let mut applied_once = false;
        loop {
            let modified_time = self.store.modified_time();
            if !applied_once || modified_time != applied_at {
                if let Some(loaded) = self.valid_settings() {
                    screen.apply(&loaded, &self.spec)?;
                    eprintln!("canalis: applied settings (theme={}, brightness={})", loaded.theme, loaded.brightness);
                    settings = loaded;
                }
                applied_at = modified_time;
                applied_once = true;
            }
            let snapshot = self.monitor.snapshot();
            screen.update(&settings, &snapshot)?;
            self.status.update(&snapshot, None);
            std::thread::sleep(self.update_interval);
        }
    }
}
