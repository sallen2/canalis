//! Live status the service publishes each second and the UI displays.

use crate::link::unix_millis;
use crate::settings::write_atomically;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::SystemTime;

/// A status older than this means the service is not running.
const FRESH_FOR_MILLIS: u64 = 5_000;

pub struct StatusWriter {
    path: PathBuf,
    status: Value,
}

impl StatusWriter {
    pub fn new(path: PathBuf) -> Self {
        Self { path, status: json!({ "connected": false }) }
    }

    pub fn connected(&mut self, port: &str, device_info: Option<Value>, spec: &Value) {
        self.status = json!({ "connected": true, "port": port, "device": device_info.unwrap_or(json!({})), "spec": spec });
        self.write();
    }

    pub fn update(&mut self, snapshot: &Value, device_status: Option<&Value>) {
        self.status["snapshot"] = snapshot.clone();
        self.status["device_status"] = device_status.cloned().unwrap_or(json!({}));
        self.write();
    }

    pub fn disconnected(&mut self, error: &str) {
        self.status["connected"] = json!(false);
        self.status["error"] = json!(error);
        self.write();
    }

    fn write(&mut self) {
        self.status["updated_ms"] = json!(unix_millis());
        self.status["pid"] = json!(std::process::id());
        if let Err(error) = write_atomically(&self.path, &self.status.to_string()) {
            eprintln!("canalis: cannot write status file {}: {error}", self.path.display());
        }
    }
}

/// Read-only view of the status file for the UI.
#[derive(Clone, Debug, Default)]
pub struct LiveStatus {
    value: Value,
}

impl LiveStatus {
    pub fn from_value(value: Value) -> Self {
        Self { value }
    }

    pub fn is_service_running(&self, now_millis: u64) -> bool {
        self.number("/updated_ms") as u64 + FRESH_FOR_MILLIS >= now_millis
    }

    pub fn is_connected(&self, now_millis: u64) -> bool {
        self.is_service_running(now_millis) && self.value["connected"].as_bool().unwrap_or(false)
    }

    pub fn number(&self, pointer: &str) -> f64 {
        match self.value.pointer(pointer) {
            Some(Value::Number(number)) => number.as_f64().unwrap_or(0.0),
            Some(Value::String(text)) => text.parse().unwrap_or(0.0),
            _ => 0.0,
        }
    }

    pub fn text(&self, pointer: &str) -> String {
        match self.value.pointer(pointer) {
            Some(Value::String(text)) => text.clone(),
            Some(Value::Null) | None => String::new(),
            Some(other) => other.to_string(),
        }
    }

    /// The strings of a list, e.g. the presets a Turris stores.
    pub fn list(&self, pointer: &str) -> Vec<String> {
        self.value.pointer(pointer).and_then(Value::as_array).map(|items| items.iter().filter_map(|item| item.as_str().map(String::from)).collect()).unwrap_or_default()
    }
}

/// Reloads the status file whenever it changes on disk.
pub struct StatusReader {
    path: PathBuf,
    last_modified: Option<SystemTime>,
    pub current: LiveStatus,
}

impl StatusReader {
    pub fn new(path: PathBuf) -> Self {
        Self { path, last_modified: None, current: LiveStatus::default() }
    }

    pub fn refresh(&mut self) {
        let modified = std::fs::metadata(&self.path).and_then(|metadata| metadata.modified()).ok();
        if modified.is_none() || modified == self.last_modified {
            return;
        }
        if let Some(value) = std::fs::read_to_string(&self.path).ok().and_then(|text| serde_json::from_str(&text).ok()) {
            self.current = LiveStatus::from_value(value);
            self.last_modified = modified;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::tests::temporary_path;

    #[test]
    fn written_status_is_read_back() {
        let path = temporary_path("status").with_file_name("status.json");
        let mut writer = StatusWriter::new(path.clone());
        writer.connected("/dev/ttyACM0", Some(json!({"version": {"firmware": "V1.0.11"}})), &json!({"cpu": "Ryzen"}));
        writer.update(&json!({"cpu": {"load": 12}}), Some(&json!({"status": {"fanLCD": "1980"}})));
        let mut reader = StatusReader::new(path);
        reader.refresh();
        let status = &reader.current;
        assert!(status.is_connected(unix_millis()));
        assert_eq!(status.number("/snapshot/cpu/load"), 12.0);
        assert_eq!(status.number("/device_status/status/fanLCD"), 1980.0);
        assert_eq!(status.text("/device/version/firmware"), "V1.0.11");
    }

    #[test]
    fn stale_status_means_service_stopped() {
        let status = LiveStatus::from_value(json!({"connected": true, "updated_ms": 1000}));
        assert!(!status.is_service_running(10_000));
        assert!(!status.is_connected(10_000));
    }

    #[test]
    fn disconnected_keeps_error() {
        let path = temporary_path("status-error").with_file_name("status.json");
        let mut writer = StatusWriter::new(path.clone());
        writer.disconnected("no TRYX Panorama found");
        let mut reader = StatusReader::new(path);
        reader.refresh();
        assert!(!reader.current.is_connected(unix_millis()));
        assert_eq!(reader.current.text("/error"), "no TRYX Panorama found");
    }
}
