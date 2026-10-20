pub mod home;
pub mod panorama;
pub mod setting;

use super::editor::SettingsEditor;
use super::jobs::Jobs;
use super::screen_preview::PreviewSources;
use super::network_history::NetworkHistory;
use crate::status::LiveStatus;

/// Everything a page needs for one frame.
pub struct PageContext<'a> {
    pub editor: &'a mut SettingsEditor,
    pub status: &'a LiveStatus,
    pub network: &'a NetworkHistory,
    pub now_millis: u64,
    pub jobs: &'a mut Jobs,
    pub preview: &'a mut PreviewSources,
    /// Set by a page while the user drags something, so saving waits for release.
    pub interacting: bool,
}

impl PageContext<'_> {
    pub fn connected(&self) -> bool {
        self.status.is_connected(self.now_millis)
    }

    pub fn service_running(&self) -> bool {
        self.status.is_service_running(self.now_millis)
    }

    /// A TURRIS 620 is connected rather than a Panorama.
    pub fn turris_connected(&self) -> bool {
        self.connected() && self.status.text("/device/model") == "Turris"
    }

    pub fn temperature_unit(&self) -> String {
        self.editor.settings.temperature_unit.clone()
    }
}
