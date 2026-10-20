//! User settings (~/.config/canalis/settings.json), edited by the UI and the
//! command line and applied to the screen by the background service.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The built-in themes KANALI 1.2.1 offers for the PANORAMA (the SE and WB coolers have others).
pub const PRESETS: [&str; 14] = [
    "Pre-set 1: Cooling delivery",
    "Pre-set 2: Migration",
    "Pre-set 3: Quantum time capsule",
    "Pre-set 4: Exo-Ecologies",
    "Pre-set 5: Racing",
    "Pre-set 6: Shuttle",
    "Pre-set 7: Gift of TRYX",
    "Pre-set 8: Sweet Cool Acrylic",
    "Pre-set 9: Catch Me If You Can",
    "Pre-set 10: The Battle of Ice and Fire",
    "Pre-set 11: Oasis",
    "Pre-set 12: Cyber Bunker",
    "Pre-set 13: Edge Of Dream",
    "Pre-set 14: Thermal Energy Prohibited",
];
/// The built-in themes stored on the TURRIS 620's screen, named as its files are
/// (preset/Festival.mp4.h264_1280x720).
pub const TURRIS_PRESETS: [&str; 5] = ["Cosmos", "Erebos", "Festival", "MechanicalArm", "Meteorite"];
pub const CUSTOMIZATION: &str = "Customization";
pub const ROTATIONS: [u16; 4] = [0, 90, 180, 270];
/// KANALI's Mirror Mode (for left-mounted cases) only switches the screen between these two rotations.
pub const MIRRORED_ROTATION: u16 = 90;
pub const NORMAL_ROTATION: u16 = 270;
pub const PLAY_MODES: [&str; 3] = ["Single", "Shuffle", "Loop"];
pub const FULL_SCREEN: &str = "Full Screen";
/// Two square halves, each with its own media and stats (KANALI's "Screen Splitting").
pub const SPLIT_SCREEN: &str = "Screen Splitting";
pub const DARK_TEXT: &str = "#000000";
pub const LIGHT_TEXT: &str = "#dcdcdc";
pub const FILTERS: [&str; 2] = ["Rain", "Smoke"];
pub const CPU_BADGE: &str = "CPU Badge";
pub const GPU_BADGE: &str = "GPU Badge";
pub const DATE_AND_TIME: &str = "Date&Time";
pub const SMART_MODE: &str = "Smart Mode";
pub const FIXED_MODE: &str = "Fixed Mode";
pub const CELSIUS: &str = "Celsius";
pub const FAHRENHEIT: &str = "Fahrenheit";
pub const ALIGNMENTS: [&str; 3] = ["Left", "Center", "Right"];
pub const MAX_OVERLAY_FIELDS: usize = 3;
pub const OVERLAY_FIELDS: [&str; 14] = [
    "CPU Temperature",
    "CPU Usage",
    "CPU Frequency",
    "CPU Voltage",
    "CPU Power",
    "GPU Temperature",
    "GPU Usage",
    "GPU Frequency",
    "GPU Voltage",
    "GPU Power",
    "Memory Utilization",
    "Memory Frequency",
    "Motherboard Temperature",
    DATE_AND_TIME,
];

pub type FanCurve = Vec<[u8; 2]>;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub temperature_unit: String,
    pub enable: bool,
    pub display_in_sleep: bool,
    pub brightness: u8,
    pub rotate: u16,
    pub theme: String,
    /// Live stats over the built-in themes, and how they look.
    pub overlay: Vec<String>,
    pub overlay_style: OverlayStyle,
    pub custom: CustomTheme,
    pub fan: Fan,
    pub update_interval_seconds: f64,
    #[serde(flatten)]
    pub unknown_fields: Map<String, Value>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct CustomTheme {
    /// Every file copied to the screen; full screen plays them all, split screen picks one per half.
    pub media: Vec<String>,
    pub ratio: String,
    pub play_mode: String,
    pub screen_mode: String,
    #[serde(flatten)]
    pub style: OverlayStyle,
    pub overlay: Vec<String>,
    /// The left and right halves in split screen.
    pub halves: [ScreenHalf; 2],
    #[serde(flatten)]
    pub unknown_fields: Map<String, Value>,
}

/// One half of the split screen: a file from the media library, and the stats drawn over it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct ScreenHalf {
    pub media: Option<String>,
    pub style: OverlayStyle,
    pub overlay: Vec<String>,
}

/// How the live stats are drawn: text colour, alignment, a rain or smoke filter, and CPU/GPU name badges.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct OverlayStyle {
    pub color: String,
    pub align: String,
    pub filter: Filter,
    pub badges: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Filter {
    pub value: Option<String>,
    pub opacity: u8,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Fan {
    pub mode: String,
    pub smart_curve: FanCurve,
    pub fixed_percent: u8,
    #[serde(flatten)]
    pub unknown_fields: Map<String, Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            temperature_unit: CELSIUS.into(),
            enable: true,
            display_in_sleep: false,
            brightness: 100,
            rotate: NORMAL_ROTATION,
            theme: PRESETS[0].into(),
            overlay: Vec::new(),
            overlay_style: OverlayStyle::default(),
            custom: CustomTheme::default(),
            fan: Fan::default(),
            update_interval_seconds: 1.0,
            unknown_fields: Map::new(),
        }
    }
}

impl Default for CustomTheme {
    fn default() -> Self {
        Self {
            media: Vec::new(),
            ratio: "2:1".into(),
            play_mode: PLAY_MODES[0].into(),
            screen_mode: FULL_SCREEN.into(),
            style: OverlayStyle::default(),
            overlay: Vec::new(),
            halves: Default::default(),
            unknown_fields: Map::new(),
        }
    }
}

impl Default for OverlayStyle {
    fn default() -> Self {
        Self { color: LIGHT_TEXT.into(), align: ALIGNMENTS[0].into(), filter: Filter::default(), badges: Vec::new() }
    }
}

impl Default for Filter {
    fn default() -> Self {
        Self { value: None, opacity: 100 }
    }
}

impl OverlayStyle {
    /// Badges that can be shown: KANALI only allows a CPU (GPU) badge while a CPU (GPU) stat is chosen.
    pub fn shown_badges(&self, overlay: &[String]) -> Vec<String> {
        self.badges.iter().filter(|badge| badge_is_available(badge, overlay)).cloned().collect()
    }

    fn validate(&self) -> Result<(), String> {
        require(ALIGNMENTS.contains(&self.align.as_str()), "align must be Left, Center or Right")?;
        require(is_hex_color(&self.color), "colour must look like #dcdcdc")?;
        require(self.filter.value.as_deref().is_none_or(|filter| FILTERS.contains(&filter)), "filter must be Rain or Smoke")?;
        require(self.filter.opacity <= 100, "filter opacity must be between 0 and 100")?;
        require(self.badges.iter().all(|badge| [CPU_BADGE, GPU_BADGE].contains(&badge.as_str())), "unknown badge")
    }
}

/// "Pre-set 12: Cyber Bunker" -> "Cyber Bunker".
pub fn preset_name(preset: &str) -> &str {
    preset.split_once(": ").map_or(preset, |(_, name)| name)
}

pub fn badge_is_available(badge: &str, overlay: &[String]) -> bool {
    let processor = badge.split(' ').next().unwrap_or_default();
    overlay.iter().any(|field| field.contains(processor))
}

fn is_hex_color(text: &str) -> bool {
    text.len() == 7 && text.starts_with('#') && text[1..].chars().all(|c| c.is_ascii_hexdigit())
}

impl Default for Fan {
    fn default() -> Self {
        Self {
            mode: SMART_MODE.into(),
            smart_curve: FanCurvePreset::Quiet.curve(),
            fixed_percent: 40,
            unknown_fields: Map::new(),
        }
    }
}

impl Settings {
    pub fn is_custom_theme(&self) -> bool {
        self.theme == CUSTOMIZATION
    }

    pub fn active_overlay_mut(&mut self) -> &mut Vec<String> {
        if self.is_custom_theme() { &mut self.custom.overlay } else { &mut self.overlay }
    }

    /// Index into `PRESETS` of the theme on the screen, unless it shows your own media.
    pub fn preset_index(&self) -> Option<usize> {
        PRESETS.iter().position(|preset| *preset == self.theme)
    }

    /// The TURRIS 620 theme chosen, if the theme is one of `TURRIS_PRESETS`.
    pub fn turris_preset(&self) -> Option<&'static str> {
        TURRIS_PRESETS.iter().copied().find(|preset| *preset == self.theme)
    }

    pub fn is_mirrored(&self) -> bool {
        self.rotate == MIRRORED_ROTATION
    }

    pub fn set_mirrored(&mut self, mirrored: bool) {
        self.rotate = if mirrored { MIRRORED_ROTATION } else { NORMAL_ROTATION };
    }

    pub fn add_custom_media(&mut self, names: Vec<String>, append: bool) {
        if !append {
            self.custom.media.clear();
        }
        self.custom.media.retain(|existing| !names.contains(existing));
        if self.custom.is_split() {
            self.custom.fill_empty_halves(&names);
        }
        self.custom.media.extend(names);
        self.custom.play_mode = if self.custom.media.len() > 1 { "Loop" } else { "Single" }.into();
        self.theme = CUSTOMIZATION.into();
    }

    /// Shows `left` and `right` side by side, each on its own half of the screen.
    pub fn split_screen(&mut self, left: String, right: String) {
        self.add_custom_media(vec![left.clone(), right.clone()], true);
        self.custom.screen_mode = SPLIT_SCREEN.into();
        self.custom.halves[0].media = Some(left);
        self.custom.halves[1].media = Some(right);
    }

    pub fn validate(&self) -> Result<(), String> {
        require(self.brightness <= 100, "brightness must be between 0 and 100")?;
        require(ROTATIONS.contains(&self.rotate), "rotation must be 0, 90, 180 or 270")?;
        require(self.theme == CUSTOMIZATION || PRESETS.contains(&self.theme.as_str()) || self.turris_preset().is_some(), "unknown theme")?;
        require([CELSIUS, FAHRENHEIT].contains(&self.temperature_unit.as_str()), "unknown temperature unit")?;
        require([SMART_MODE, FIXED_MODE].contains(&self.fan.mode.as_str()), "unknown fan mode")?;
        require(self.fan.fixed_percent <= 100, "fixed fan speed must be between 0 and 100")?;
        self.overlay_style.validate()?;
        require(overlay_is_valid(&self.overlay), "overlay: up to 3 known fields")?;
        self.custom.validate()?;
        require(self.update_interval_seconds >= 0.2, "update interval must be at least 0.2 s")
    }
}

impl CustomTheme {
    pub fn is_split(&self) -> bool {
        self.screen_mode == SPLIT_SCREEN
    }

    /// Switching to split screen puts the library's first unused files on any empty half.
    pub fn set_split(&mut self, split: bool) {
        self.screen_mode = if split { SPLIT_SCREEN } else { FULL_SCREEN }.into();
        if split {
            let library = self.media.clone();
            self.fill_empty_halves(&library);
        }
    }

    /// Takes a file out of the library, and off any half that shows it.
    pub fn remove_media(&mut self, name: &str) {
        self.media.retain(|existing| existing != name);
        for half in self.halves.iter_mut().filter(|half| half.media.as_deref() == Some(name)) {
            half.media = None;
        }
    }

    fn fill_empty_halves(&mut self, names: &[String]) {
        let shown: Vec<String> = self.halves.iter().filter_map(|half| half.media.clone()).collect();
        let unused = names.iter().filter(|name| !shown.contains(name));
        let empty_halves = self.halves.iter_mut().filter(|half| half.media.is_none());
        for (half, name) in empty_halves.zip(unused) {
            half.media = Some(name.clone());
        }
    }

    fn validate(&self) -> Result<(), String> {
        require(PLAY_MODES.contains(&self.play_mode.as_str()), "play mode must be Single, Shuffle or Loop")?;
        require([FULL_SCREEN, SPLIT_SCREEN].contains(&self.screen_mode.as_str()), "screen mode must be Full Screen or Screen Splitting")?;
        self.style.validate()?;
        require(overlay_is_valid(&self.overlay), "overlay: up to 3 known fields")?;
        for half in &self.halves {
            half.style.validate()?;
            require(overlay_is_valid(&half.overlay), "overlay: up to 3 known fields")?;
        }
        Ok(())
    }
}

fn overlay_is_valid(overlay: &[String]) -> bool {
    overlay.len() <= MAX_OVERLAY_FIELDS && overlay.iter().all(|field| OVERLAY_FIELDS.contains(&field.as_str()))
}

fn require(condition: bool, message: &str) -> Result<(), String> {
    if condition { Ok(()) } else { Err(message.to_string()) }
}

/// Fan curves KANALI ships as Low / Mid / High / Full speed: [temperature °C, fan %].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FanCurvePreset {
    Quiet,
    Balanced,
    Performance,
    Full,
}

impl FanCurvePreset {
    pub const ALL: [FanCurvePreset; 4] = [Self::Quiet, Self::Balanced, Self::Performance, Self::Full];

    pub fn label(self) -> &'static str {
        match self {
            Self::Quiet => "Quiet",
            Self::Balanced => "Balanced",
            Self::Performance => "Performance",
            Self::Full => "Full speed",
        }
    }

    pub fn curve(self) -> FanCurve {
        let points: [[u8; 2]; 8] = match self {
            Self::Quiet => [[0, 10], [10, 20], [30, 30], [50, 40], [65, 55], [80, 70], [90, 100], [100, 100]],
            Self::Balanced => [[0, 10], [10, 20], [30, 35], [50, 50], [65, 75], [80, 80], [90, 100], [100, 100]],
            Self::Performance => [[0, 10], [10, 20], [30, 50], [40, 70], [55, 85], [70, 90], [90, 100], [100, 100]],
            Self::Full => [[0, 10], [10, 20], [30, 70], [40, 100], [65, 100], [80, 100], [90, 100], [100, 100]],
        };
        points.to_vec()
    }
}

pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<Settings, String> {
        match fs::read_to_string(&self.path) {
            Ok(text) => serde_json::from_str(&text).map_err(|error| format!("{}: {error}", self.path.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(error) => Err(format!("{}: {error}", self.path.display())),
        }
    }

    pub fn save(&self, settings: &Settings) -> Result<(), String> {
        settings.validate()?;
        let text = serde_json::to_string_pretty(settings).map_err(|error| error.to_string())? + "\n";
        write_atomically(&self.path, &text).map_err(|error| format!("{}: {error}", self.path.display()))
    }

    pub fn modified_time(&self) -> Option<SystemTime> {
        fs::metadata(&self.path).and_then(|metadata| metadata.modified()).ok()
    }
}

pub fn write_atomically(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, text)?;
    fs::rename(&temporary, path)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub const KANALI_SETTINGS: &str = include_str!("testdata/kanali-settings.json");

    pub fn temporary_path(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!("canalis-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        directory.join("settings.json")
    }

    #[test]
    fn reads_the_settings_copied_from_kanali() {
        let settings: Settings = serde_json::from_str(KANALI_SETTINGS).unwrap();
        assert_eq!(settings.rotate, 270);
        assert!(settings.is_custom_theme());
        assert_eq!(settings.custom.overlay, ["CPU Temperature", "GPU Temperature"]);
        assert_eq!(settings.fan.smart_curve, FanCurvePreset::Quiet.curve());
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn missing_file_gives_defaults() {
        let store = SettingsStore::new(temporary_path("missing"));
        assert_eq!(store.load().unwrap(), Settings::default());
    }

    #[test]
    fn round_trip_keeps_unknown_fields() {
        let store = SettingsStore::new(temporary_path("round-trip"));
        let mut settings: Settings = serde_json::from_str(r#"{"brightness": 20, "future_option": 7}"#).unwrap();
        settings.brightness = 55;
        store.save(&settings).unwrap();
        let reloaded = store.load().unwrap();
        assert_eq!(reloaded.brightness, 55);
        assert_eq!(reloaded.unknown_fields["future_option"], 7);
    }

    #[test]
    fn rejects_invalid_rotation() {
        let settings = Settings { rotate: 45, ..Settings::default() };
        assert!(SettingsStore::new(temporary_path("invalid")).save(&settings).is_err());
    }

    #[test]
    fn overlay_target_follows_theme() {
        let mut settings = Settings { theme: CUSTOMIZATION.into(), ..Settings::default() };
        settings.active_overlay_mut().push("CPU Usage".into());
        assert_eq!(settings.custom.overlay, ["CPU Usage"]);
        assert!(settings.overlay.is_empty());
    }

    #[test]
    fn appending_media_switches_to_loop() {
        let mut settings = Settings::default();
        settings.add_custom_media(vec!["a.gif".into()], false);
        settings.add_custom_media(vec!["b.mp4".into()], true);
        assert_eq!(settings.custom.media, ["a.gif", "b.mp4"]);
        assert_eq!(settings.custom.play_mode, "Loop");
        assert!(settings.is_custom_theme());
    }

    #[test]
    fn custom_style_keeps_kanalis_flat_fields() {
        let settings: Settings = serde_json::from_str(KANALI_SETTINGS).unwrap();
        assert_eq!((settings.custom.style.color.as_str(), settings.custom.style.align.as_str()), (LIGHT_TEXT, "Left"));
        let saved = serde_json::to_value(&settings).unwrap();
        assert_eq!(saved["custom"]["color"], LIGHT_TEXT);
        assert_eq!(saved["custom"]["filter"]["opacity"], 100);
    }

    #[test]
    fn badges_need_a_matching_stat() {
        let style = OverlayStyle { badges: vec![CPU_BADGE.into(), GPU_BADGE.into()], ..OverlayStyle::default() };
        assert_eq!(style.shown_badges(&["CPU Temperature".into(), DATE_AND_TIME.into()]), [CPU_BADGE]);
        assert!(style.shown_badges(&[]).is_empty());
    }

    #[test]
    fn mirror_mode_switches_between_two_rotations() {
        let mut settings = Settings { rotate: NORMAL_ROTATION, ..Settings::default() };
        settings.set_mirrored(true);
        assert_eq!((settings.rotate, settings.is_mirrored()), (MIRRORED_ROTATION, true));
        settings.set_mirrored(false);
        assert_eq!(settings.rotate, NORMAL_ROTATION);
    }

    #[test]
    fn split_screen_puts_one_file_on_each_half() {
        let mut settings = Settings::default();
        settings.split_screen("left.gif".into(), "right.png".into());
        assert!(settings.is_custom_theme() && settings.custom.is_split());
        assert_eq!(settings.custom.media, ["left.gif", "right.png"]);
        assert_eq!(settings.custom.halves.map(|half| half.media), [Some("left.gif".into()), Some("right.png".into())]);
    }

    #[test]
    fn media_added_in_split_screen_fills_the_empty_halves() {
        let mut settings = Settings::default();
        settings.custom.screen_mode = SPLIT_SCREEN.into();
        settings.custom.halves[0].media = Some("kept.gif".into());
        settings.add_custom_media(vec!["new.gif".into(), "extra.gif".into()], true);
        assert_eq!(settings.custom.halves.map(|half| half.media), [Some("kept.gif".into()), Some("new.gif".into())]);
    }

    #[test]
    fn switching_to_split_screen_uses_files_not_already_shown() {
        let mut custom = CustomTheme { media: vec!["a.gif".into(), "b.gif".into()], ..CustomTheme::default() };
        custom.halves[0].media = Some("a.gif".into());
        custom.set_split(true);
        assert_eq!(custom.halves[1].media.as_deref(), Some("b.gif"));
        custom.set_split(false);
        assert_eq!((custom.screen_mode.as_str(), custom.halves[1].media.as_deref()), (FULL_SCREEN, Some("b.gif")));
    }

    #[test]
    fn removing_media_clears_the_half_showing_it() {
        let mut settings = Settings::default();
        settings.split_screen("a.gif".into(), "b.gif".into());
        settings.custom.remove_media("b.gif");
        assert_eq!(settings.custom.media, ["a.gif"]);
        assert_eq!(settings.custom.halves[1].media, None);
    }

    #[test]
    fn rejects_unknown_screen_mode_and_bad_half_overlay() {
        let mut settings = Settings::default();
        settings.custom.screen_mode = "Picture in picture".into();
        assert!(settings.validate().is_err());
        settings.custom.screen_mode = SPLIT_SCREEN.into();
        settings.custom.halves[1].overlay = vec!["Fan Speed".into()];
        assert!(settings.validate().is_err());
    }

    #[test]
    fn rejects_unknown_filter() {
        let mut settings = Settings::default();
        settings.overlay_style.filter.value = Some("Snow".into());
        assert!(settings.validate().is_err());
    }
}
