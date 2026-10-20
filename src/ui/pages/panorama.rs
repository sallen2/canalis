//! PANORAMA, laid out like KANALI's page: built-in themes in a carousel, your own media, and the
//! live stats drawn over either, plus screen and fan settings.

use super::PageContext;
use crate::media::SUPPORTED_EXTENSIONS;
use crate::settings::{
    ALIGNMENTS, CELSIUS, CPU_BADGE, CUSTOMIZATION, CustomTheme, DARK_TEXT, FAHRENHEIT, FILTERS, FIXED_MODE, FanCurvePreset, Filter, GPU_BADGE, LIGHT_TEXT,
    MAX_OVERLAY_FIELDS, OVERLAY_FIELDS, OverlayStyle, PLAY_MODES, PRESETS, SMART_MODE, Settings, TURRIS_PRESETS, badge_is_available,
};
use crate::crop::Shape;
use crate::ui::carousel::Carousel;
use crate::ui::crop_dialog::CropDialog;
use crate::ui::fan_curve::fan_curve_editor;
use crate::ui::format::field_title;
use crate::ui::screen_preview::{parse_hex_color, screen_preview};
use crate::ui::theme::{BORDER, LIME, MUTED};
use crate::ui::widgets::{bold, card, muted, page_header, page_scroll, pill, pill_button, primary_button, segmented, setting_row, tabs, toggle};
use eframe::egui::style::HandleShape;
use eframe::egui::{self, Align2, Color32, Sense, Stroke, StrokeKind, Ui, vec2};

const STYLE_LABEL_WIDTH: f32 = 104.0;
const STYLE_ROW_HEIGHT: f32 = 32.0;

#[derive(Clone, Copy, PartialEq, Default)]
pub enum PanoramaTab {
    #[default]
    Presets,
    Customization,
    Screen,
    Fan,
}

pub struct PanoramaPage {
    pub tab: PanoramaTab,
    carousel: Carousel,
    /// Which half of the split screen is being edited: 0 left, 1 right.
    half: usize,
    cropper: CropDialog,
}

impl PanoramaPage {
    pub fn new(tab: PanoramaTab, settings: &Settings) -> Self {
        Self { tab, carousel: Carousel::new(settings.preset_index().unwrap_or(0)), half: 0, cropper: CropDialog::default() }
    }
}

const HALF_NAMES: [&str; 2] = ["Left half", "Right half"];

pub fn show(ui: &mut Ui, state: &mut PanoramaPage, page: &mut PageContext) {
    let turris = page.turris_connected();
    page_header(ui, if turris { "TURRIS" } else { "PANORAMA" });
    let mut options = vec![(PanoramaTab::Presets, "Pre-set"), (PanoramaTab::Customization, "Customization"), (PanoramaTab::Screen, "Screen")];
    if turris {
        // The fan settings are the Panorama pump head's; the TURRIS 620's fans are the board's.
        if state.tab == PanoramaTab::Fan {
            state.tab = PanoramaTab::Presets;
        }
    } else {
        options.push((PanoramaTab::Fan, "Fan"));
    }
    tabs(ui, &mut state.tab, &options);
    page_scroll(ui, |ui| match state.tab {
        PanoramaTab::Presets if turris => turris_preset_tab(ui, page),
        PanoramaTab::Presets => preset_tab(ui, &mut state.carousel, page),
        PanoramaTab::Customization if turris => turris_customization_note(ui),
        PanoramaTab::Customization => customization_tab(ui, state, page),
        PanoramaTab::Screen => screen_settings(ui, &mut page.editor.settings, turris),
        PanoramaTab::Fan => fan_settings(ui, page),
    });
    if let Some(uploads) = state.cropper.show(ui.ctx()) {
        page.jobs.add_media(uploads, ui.ctx());
    }
}

/// The TURRIS 620 keeps its built-in themes on the screen itself; they are listed by name.
fn turris_preset_tab(ui: &mut Ui, page: &mut PageContext) {
    let stored: Vec<String> = page.status.list("/device/presets");
    card(ui, |ui| {
        ui.label(bold("Pre-set", 19.0));
        ui.label(muted("The themes stored on the TURRIS 620's screen. Click one to show it."));
        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui| {
            for preset in TURRIS_PRESETS.iter().filter(|preset| stored.is_empty() || stored.iter().any(|name| name == *preset)) {
                let selected = page.editor.settings.theme == *preset;
                if pill(ui, &spaced_words(preset), selected).clicked() {
                    page.editor.settings.theme = (*preset).into();
                }
            }
        });
    });
    ui.add_space(14.0);
    let settings = &mut page.editor.settings;
    stats_card(ui, "", &mut settings.overlay, &mut settings.overlay_style);
}

fn turris_customization_note(ui: &mut Ui) {
    card(ui, |ui| {
        ui.label(bold("Customization", 19.0));
        ui.label(muted("Your own images, GIFs and videos can't be sent to the TURRIS 620 yet. Choose one of its built-in themes in Pre-set."));
    });
}

/// "MechanicalArm" -> "Mechanical Arm".
fn spaced_words(name: &str) -> String {
    let mut spaced = String::new();
    for (index, character) in name.char_indices() {
        if index > 0 && character.is_uppercase() {
            spaced.push(' ');
        }
        spaced.push(character);
    }
    spaced
}

fn preset_tab(ui: &mut Ui, carousel: &mut Carousel, page: &mut PageContext) {
    card(ui, |ui| {
        ui.label(bold("Pre-set", 19.0));
        ui.label(muted("Click a side card to bring it forward, then click the centre card to show it on the screen."));
        ui.add_space(10.0);
        let selected = page.editor.settings.preset_index();
        if let Some(index) = carousel.show(ui, &mut page.preview.presets, selected) {
            page.editor.settings.theme = PRESETS[index].into();
        }
    });
    ui.add_space(14.0);
    let settings = &mut page.editor.settings;
    stats_card(ui, "", &mut settings.overlay, &mut settings.overlay_style);
}

fn customization_tab(ui: &mut Ui, state: &mut PanoramaPage, page: &mut PageContext) {
    preview_card(ui, page);
    ui.add_space(14.0);
    media_library(ui, &mut state.cropper, page);
    ui.add_space(14.0);
    let custom = &mut page.editor.settings.custom;
    let half = state.half;
    if custom.is_split() {
        half_card(ui, &mut state.half, custom);
        ui.add_space(14.0);
        let shown = &mut custom.halves[half];
        stats_card(ui, HALF_NAMES[half], &mut shown.overlay, &mut shown.style);
    } else {
        stats_card(ui, "", &mut custom.overlay, &mut custom.style);
    }
}

/// Split screen: pick a half, then the file it shows; the stats card below edits the same half.
fn half_card(ui: &mut Ui, half: &mut usize, custom: &mut CustomTheme) {
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(bold("Halves", 19.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let options: Vec<(usize, String)> = HALF_NAMES.iter().enumerate().rev().map(|(index, name)| (index, name.to_string())).collect();
                segmented(ui, half, &options);
            });
        });
        ui.label(muted("Each half shows one file. Square (1:1) media fills a half best: 1120×1080."));
        ui.add_space(12.0);
        let library = custom.media.clone();
        style_row(ui, "Media", |ui| {
            let shown = &mut custom.halves[*half].media;
            if pill(ui, "None", shown.is_none()).clicked() {
                *shown = None;
            }
            for name in library {
                if pill(ui, &name, shown.as_deref() == Some(name.as_str())).clicked() {
                    *shown = Some(name);
                }
            }
        });
    });
}

fn preview_card(ui: &mut Ui, page: &mut PageContext) {
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(bold("Preview", 19.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut live = page.preview.live.is_on();
                if toggle(ui, &mut live) {
                    page.preview.live.set_on(live, ui.ctx());
                }
                ui.label("Live view").on_hover_text("Screenshots of the cooler's screen over USB, every couple of seconds");
            });
        });
        ui.add_space(10.0);
        ui.vertical_centered(|ui| screen_preview(ui, &page.editor.settings, page.status, page.preview));
    });
}

fn media_library(ui: &mut Ui, cropper: &mut CropDialog, page: &mut PageContext) {
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(bold("Your media", 19.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| add_media_button(ui, cropper, page));
        });
        ui.label(muted("Images, GIFs and videos are copied to the screen over USB (adb). The panel is 2240×1080 (2:1)."));
        ui.add_space(12.0);
        let settings = &mut page.editor.settings;
        style_row(ui, "Layout", |ui| {
            let mut split = settings.custom.is_split();
            if segmented(ui, &mut split, &[(false, "Full screen".into()), (true, "Split screen".into())]) {
                settings.custom.set_split(split);
            }
        });
        media_list(ui, &mut settings.custom);
        if !settings.is_custom_theme() && !settings.custom.media.is_empty() {
            ui.add_space(10.0);
            if primary_button(ui, "Show my media on the screen").clicked() {
                settings.theme = CUSTOMIZATION.into();
            }
        }
    });
}

/// Picked files go through the crop window first, in the shape they will be shown in.
fn add_media_button(ui: &mut Ui, cropper: &mut CropDialog, page: &mut PageContext) {
    if let Some(label) = page.jobs.busy_label() {
        ui.spinner();
        ui.label(muted(label));
        return;
    }
    if primary_button(ui, "Add media…").clicked() {
        let picked = rfd::FileDialog::new().add_filter("Images and videos", &SUPPORTED_EXTENSIONS).pick_files();
        if let Some(files) = picked.filter(|files| !files.is_empty()) {
            let shape = if page.editor.settings.custom.is_split() { Shape::Square } else { Shape::Wide };
            cropper.open(files, shape);
        }
    }
}

fn media_list(ui: &mut Ui, custom: &mut CustomTheme) {
    if custom.media.is_empty() {
        ui.label(muted("No media yet. Add a file to show it on the screen."));
        return;
    }
    let mut remove = None;
    for (index, name) in custom.media.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("{}.", index + 1)).color(MUTED));
            ui.label(name);
            if let Some(where_shown) = custom.is_split().then(|| shown_on(custom, name)).flatten() {
                ui.label(muted(format!("· {where_shown}")));
            }
            if ui.small_button("Remove").clicked() {
                remove = Some(name.clone());
            }
        });
    }
    if let Some(name) = remove {
        custom.remove_media(&name);
    }
    if !custom.is_split() {
        ui.add_space(10.0);
        style_row(ui, "Play mode", |ui| {
            let options: Vec<(String, String)> = PLAY_MODES.iter().map(|mode| (mode.to_string(), mode.to_string())).collect();
            segmented(ui, &mut custom.play_mode, &options);
        });
    }
}

/// "left half", "right half" or "both halves": where a library file is shown in split screen.
fn shown_on(custom: &CustomTheme, name: &str) -> Option<String> {
    let halves: Vec<usize> = (0..HALF_NAMES.len()).filter(|&index| custom.halves[index].media.as_deref() == Some(name)).collect();
    match halves[..] {
        [] => None,
        [index] => Some(HALF_NAMES[index].to_lowercase()),
        _ => Some("both halves".into()),
    }
}

/// KANALI's "System Information Display": up to three stats and how they are drawn; `area`
/// names the half of a split screen being edited.
fn stats_card(ui: &mut Ui, area: &str, overlay: &mut Vec<String>, style: &mut OverlayStyle) {
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(bold("System Information Display", 19.0));
            if !area.is_empty() {
                ui.label(bold(format!("· {area}"), 19.0).color(LIME));
            }
            ui.label(muted(format!("Select up to {MAX_OVERLAY_FIELDS} items")));
        });
        ui.add_space(14.0);
        style_row(ui, "Stats", |ui| {
            for field in OVERLAY_FIELDS {
                overlay_chip(ui, overlay, field);
            }
        });
        style_row(ui, "Text colour", |ui| colour_swatches(ui, &mut style.color));
        style_row(ui, "Alignment", |ui| {
            let alignments: Vec<(String, String)> = ALIGNMENTS.iter().map(|align| (align.to_string(), align.to_string())).collect();
            segmented(ui, &mut style.align, &alignments);
        });
        style_row(ui, "Filter", |ui| filter_controls(ui, &mut style.filter));
        style_row(ui, "Badges", |ui| {
            for badge in [CPU_BADGE, GPU_BADGE] {
                badge_chip(ui, &mut style.badges, badge, badge_is_available(badge, overlay));
            }
        });
    });
}

/// A label column on the left, then the row's controls, wrapping onto more 32-point lines.
fn style_row(ui: &mut Ui, label: &str, add_contents: impl FnOnce(&mut Ui)) {
    ui.horizontal_top(|ui| {
        let (label_rect, _) = ui.allocate_exact_size(vec2(STYLE_LABEL_WIDTH, STYLE_ROW_HEIGHT), Sense::hover());
        ui.painter().text(label_rect.left_center(), Align2::LEFT_CENTER, label, egui::TextStyle::Body.resolve(ui.style()), MUTED);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().interact_size.y = STYLE_ROW_HEIGHT;
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            add_contents(ui);
        });
    });
    ui.add_space(12.0);
}

fn colour_swatches(ui: &mut Ui, current: &mut String) {
    for color in [DARK_TEXT, LIGHT_TEXT] {
        if colour_swatch(ui, color, *current == color).clicked() {
            *current = color.into();
        }
    }
}

fn colour_swatch(ui: &mut Ui, color: &str, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::click());
    ui.painter().rect_filled(rect.shrink(5.0), 4, parse_hex_color(color).unwrap_or(Color32::WHITE));
    let border = if selected { Stroke::new(2.0, LIME) } else { Stroke::new(1.0, BORDER) };
    ui.painter().rect_stroke(rect, 8, border, StrokeKind::Inside);
    response.on_hover_text(if color == DARK_TEXT { "Black text" } else { "Light grey text" }).on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn filter_controls(ui: &mut Ui, filter: &mut Filter) {
    let mut options = vec![(None, "No filter".to_string())];
    options.extend(FILTERS.iter().map(|name| (Some(name.to_string()), name.to_string())));
    segmented(ui, &mut filter.value, &options);
    ui.add_space(12.0);
    ui.add_enabled_ui(filter.value.is_some(), |ui| {
        ui.label(muted("Opacity"));
        ui.spacing_mut().slider_width = 110.0;
        ui.add(egui::Slider::new(&mut filter.opacity, 0..=100).step_by(25.0).show_value(false).handle_shape(HandleShape::Circle));
        ui.add_sized(vec2(40.0, STYLE_ROW_HEIGHT), egui::Label::new(format!("{}%", filter.opacity)));
    });
}

fn badge_chip(ui: &mut Ui, badges: &mut Vec<String>, badge: &str, available: bool) {
    let selected = available && badges.iter().any(|chosen| chosen == badge);
    let hint = format!("Pick a {} stat to show this badge", badge.replace(" Badge", ""));
    let response = ui.add_enabled(available, pill_button(badge, selected)).on_disabled_hover_text(hint);
    if response.clicked() {
        if selected {
            badges.retain(|chosen| chosen != badge);
        } else {
            badges.push(badge.to_string());
        }
    }
}

fn overlay_chip(ui: &mut Ui, overlay: &mut Vec<String>, field: &str) {
    let selected = overlay.iter().any(|chosen| chosen == field);
    let full = overlay.len() >= MAX_OVERLAY_FIELDS;
    let response = ui.add_enabled(selected || !full, pill_button(field_title(field), selected));
    if response.clicked() {
        if selected {
            overlay.retain(|chosen| chosen != field);
        } else {
            overlay.push(field.to_string());
        }
    }
}

fn screen_settings(ui: &mut Ui, settings: &mut Settings, turris: bool) {
    card(ui, |ui| {
        setting_row(ui, "Screen display", "Turn the cooler's screen on or off.", |ui| {
            toggle(ui, &mut settings.enable);
        });
        setting_row(ui, "Brightness", "", |ui| {
            ui.add(egui::Slider::new(&mut settings.brightness, 0..=100).suffix("%"));
        });
        let mirror_description = if turris {
            "A mirror image of the screen for left-mounted cases."
        } else {
            "A mirror image of the screen for left-mounted cases. The cooler restarts when this changes."
        };
        setting_row(ui, "Mirror mode", mirror_description, |ui| {
            let mut mirrored = settings.is_mirrored();
            if toggle(ui, &mut mirrored) {
                settings.set_mirrored(mirrored);
            }
        });
        if !turris {
            setting_row(
                ui,
                "Black screen while the PC sleeps",
                "On: the screen goes dark when the PC is off or the app is closed. Off: it shows a sleep animation.",
                |ui| {
                    toggle(ui, &mut settings.display_in_sleep);
                },
            );
        }
        setting_row(ui, "Temperature unit", "", |ui| {
            let options = [(FAHRENHEIT.to_string(), "°F".to_string()), (CELSIUS.to_string(), "°C".to_string())];
            segmented(ui, &mut settings.temperature_unit, &options);
        });
    });
}

fn fan_settings(ui: &mut Ui, page: &mut PageContext) {
    let current_temperature = page.connected().then(|| page.status.number("/snapshot/cpu/temperature") as f32);
    let fan_rpm = page.status.number("/device_status/status/fanLCD");
    let connected = page.connected();
    let fan = &mut page.editor.settings.fan;
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(bold("Pump-head fan", 19.0));
            if connected {
                ui.label(muted(format!("· {fan_rpm:.0} RPM now")));
            }
        });
        ui.label(muted("The pump speed itself is fixed by the cooler; only the small fan on the pump head can be controlled."));
        ui.add_space(12.0);
        segmented(ui, &mut fan.mode, &[(SMART_MODE.into(), "Smart (follows CPU temperature)".into()), (FIXED_MODE.into(), "Fixed speed".into())]);
        ui.add_space(14.0);
        if fan.mode == FIXED_MODE {
            ui.add(egui::Slider::new(&mut fan.fixed_percent, 0..=100).suffix("%").text("Fan speed"));
            return;
        }
        style_row(ui, "Start from", |ui| {
            for preset in FanCurvePreset::ALL {
                if pill(ui, preset.label(), fan.smart_curve == preset.curve()).clicked() {
                    fan.smart_curve = preset.curve();
                }
            }
        });
        page.interacting |= fan_curve_editor(ui, &mut fan.smart_curve, current_temperature);
        ui.label(muted("Drag the points. Temperatures are CPU °C; values are fan %.").size(12.5));
    });
}
