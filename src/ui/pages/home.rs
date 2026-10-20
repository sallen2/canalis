//! HOMEPAGE: KANALI's live dashboard, plus device and computer specifications.

use super::PageContext;
use crate::status::LiveStatus;
use crate::ui::charts::{self, GradientFill, Sparkline};
use crate::ui::format;
use crate::ui::icons::{self, Icon};
use crate::ui::network_history::NetworkHistory;
use crate::ui::theme::{self, CARD_RAISED, DANGER, DOWNLOAD_GREEN, LIME, MUTED, OK, ORANGE, TEXT, UPLOAD_BLUE};
use crate::ui::widgets::{bold, card, card_at, card_content_height, divider, info_line, muted, page_header, page_scroll, status_dot, tabs};
use eframe::egui::{self, Align, Align2, Color32, FontId, Id, Layout, Painter, Pos2, Rect, Sense, Ui, pos2, vec2};
use std::time::Duration;

const CARD_GAP: f32 = 20.0;
const MIN_COLUMN_WIDTH: f32 = 230.0;
const MIN_ROW_HEIGHT: f32 = 230.0;
const MAX_ROW_HEIGHT: f32 = 340.0;
const FOOTER_HEIGHT: f32 = 26.0;
const BAR_HEIGHT: f32 = 26.0;
const NETWORK_LABEL_HEIGHT: f32 = 26.0;
const SPEC_CARD_MIN_WIDTH: f32 = 360.0;
const MEMORY_GRADIENT: [Color32; 2] = [CARD_RAISED, LIME];
const DISK_GRADIENT: [Color32; 3] = [CARD_RAISED, LIME, ORANGE];
/// How long a reading takes to glide to its new value.
const EASE_SECONDS: f32 = 0.6;
/// The network graphs scroll at about 60 frames a second.
const SCROLL_FRAME_INTERVAL: Duration = Duration::from_millis(16);

#[derive(Clone, Copy, PartialEq, Default)]
pub enum HomeTab {
    #[default]
    Dashboard,
    Specifications,
}

pub fn show(ui: &mut Ui, tab: &mut HomeTab, page: &mut PageContext) {
    page_header(ui, "HOMEPAGE");
    ui.horizontal_top(|ui| {
        tabs(ui, tab, &[(HomeTab::Dashboard, "Dashboard"), (HomeTab::Specifications, "Specifications")]);
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| connection_status(ui, page));
    });
    ui.add_space(18.0);
    divider(ui);
    ui.add_space(CARD_GAP);
    page_scroll(ui, |ui| match tab {
        HomeTab::Dashboard => dashboard(ui, page),
        HomeTab::Specifications => specifications(ui, page),
    });
}

fn connection_status(ui: &mut Ui, page: &PageContext) {
    if page.connected() {
        status_dot(ui, OK, "Cooler screen connected");
    } else if page.service_running() {
        status_dot(ui, LIME, "Looking for the cooler screen…");
    } else {
        status_dot(ui, DANGER, "Service not running — start it in SETTING");
    }
}

fn dashboard(ui: &mut Ui, page: &PageContext) {
    let row_height = ((ui.available_height() - CARD_GAP) / 2.0).clamp(MIN_ROW_HEIGHT, MAX_ROW_HEIGHT);
    let layout = DashboardLayout::fit(ui.available_width(), row_height);
    let (area, _) = ui.allocate_exact_size(layout.size(), Sense::hover());
    let place = |rect: Rect| rect.translate(area.min.to_vec2());
    let unit = page.temperature_unit();
    card_at(ui, place(layout.network), |ui| network_card(ui, page.network));
    card_at(ui, place(layout.cpu), |ui| gauge_card(ui, &LoadGauge::read(page.status, Processor::Cpu, &unit)));
    card_at(ui, place(layout.gpu), |ui| gauge_card(ui, &LoadGauge::read(page.status, Processor::Gpu, &unit)));
    card_at(ui, place(layout.memory), |ui| bar_card(ui, &memory_bar(page.status)));
    card_at(ui, place(layout.disk), |ui| bar_card(ui, &disk_bar(page.status)));
}

/// Card positions relative to the dashboard's top-left corner.
#[derive(Debug)]
struct DashboardLayout {
    network: Rect,
    cpu: Rect,
    gpu: Rect,
    memory: Rect,
    disk: Rect,
}

impl DashboardLayout {
    /// KANALI's three columns with the network card spanning both rows; two or one column when narrow.
    fn fit(width: f32, row_height: f32) -> Self {
        let columns = ((width + CARD_GAP) / (MIN_COLUMN_WIDTH + CARD_GAP)).clamp(1.0, 3.0) as usize;
        let grid = Grid { column_width: (width - CARD_GAP * (columns - 1) as f32) / columns as f32, row_height };
        match columns {
            3 => Self { network: grid.span((0, 0), (1, 2)), cpu: grid.cell(1, 0), gpu: grid.cell(2, 0), memory: grid.cell(1, 1), disk: grid.cell(2, 1) },
            2 => Self { network: grid.span((0, 0), (2, 1)), cpu: grid.cell(0, 1), gpu: grid.cell(1, 1), memory: grid.cell(0, 2), disk: grid.cell(1, 2) },
            _ => Self { network: grid.cell(0, 0), cpu: grid.cell(0, 1), gpu: grid.cell(0, 2), memory: grid.cell(0, 3), disk: grid.cell(0, 4) },
        }
    }

    fn size(&self) -> egui::Vec2 {
        [self.network, self.cpu, self.gpu, self.memory, self.disk].iter().fold(Rect::NOTHING, |all, card| all.union(*card)).max.to_vec2()
    }
}

struct Grid {
    column_width: f32,
    row_height: f32,
}

impl Grid {
    fn cell(&self, column: usize, row: usize) -> Rect {
        self.span((column, row), (1, 1))
    }

    fn span(&self, (column, row): (usize, usize), (columns, rows): (usize, usize)) -> Rect {
        let min = pos2(column as f32 * (self.column_width + CARD_GAP), row as f32 * (self.row_height + CARD_GAP));
        let width = columns as f32 * self.column_width + (columns - 1) as f32 * CARD_GAP;
        let height = rows as f32 * self.row_height + (rows - 1) as f32 * CARD_GAP;
        Rect::from_min_size(min, vec2(width, height))
    }
}

struct NetworkSeries<'a> {
    name: &'static str,
    samples: &'a [f32],
    color: Color32,
    icon: Icon,
}

fn network_card(ui: &mut Ui, network: &NetworkHistory) {
    ui.label(bold("Network Status", 19.0));
    ui.add_space(8.0);
    let (download_area, upload_area) = split_network_area(ui.available_rect_before_wrap());
    network_graph(ui, download_area, network, &NetworkSeries { name: "Download", samples: network.download(), color: DOWNLOAD_GREEN, icon: icons::arrow_down });
    network_graph(ui, upload_area, network, &NetworkSeries { name: "Upload", samples: network.upload(), color: UPLOAD_BLUE, icon: icons::arrow_up });
    ui.ctx().request_repaint_after(SCROLL_FRAME_INTERVAL);
}

/// Graphs side by side in a wide, short card; stacked otherwise.
fn split_network_area(region: Rect) -> (Rect, Rect) {
    if region.width() > region.height() * 1.6 {
        let half = (region.width() - CARD_GAP) / 2.0;
        (region.with_max_x(region.left() + half), region.with_min_x(region.right() - half))
    } else {
        let half = (region.height() - CARD_GAP) / 2.0;
        (region.with_max_y(region.top() + half), region.with_min_y(region.bottom() - half))
    }
}

fn network_graph(ui: &Ui, area: Rect, network: &NetworkHistory, series: &NetworkSeries) {
    let painter = ui.painter();
    let graph = Rect::from_min_max(area.min + vec2(0.0, 12.0), pos2(area.right(), area.bottom() - NETWORK_LABEL_HEIGHT - 8.0));
    let peak = eased(ui, (series.name, "peak"), charts::sparkline_peak(series.samples) as f64) as f32;
    charts::sparkline(painter, graph, &Sparkline { samples: series.samples, peak, scroll: network.scroll(), color: series.color });
    let label_y = area.bottom() - NETWORK_LABEL_HEIGHT / 2.0;
    (series.icon)(painter, pos2(area.left() + 10.0, label_y), series.color);
    let rate = eased(ui, (series.name, "rate"), series.samples.last().copied().unwrap_or(0.0) as f64);
    let (value, unit) = format::rate(rate);
    let pieces = [
        (format!("{}: ", series.name), FontId::proportional(15.0), TEXT),
        (value, FontId::new(15.0, theme::bold_family()), TEXT),
        (format!(" {unit}"), FontId::proportional(11.5), MUTED),
    ];
    paint_text_run(painter, pos2(area.left() + 30.0, label_y), &pieces);
}

/// Paints text pieces one after another along a shared vertical centre line.
fn paint_text_run(painter: &Painter, start: Pos2, pieces: &[(String, FontId, Color32)]) {
    let mut x = start.x;
    for (text, font, color) in pieces {
        x = painter.text(pos2(x, start.y), Align2::LEFT_CENTER, text, font.clone(), *color).right();
    }
}

#[derive(Clone, Copy)]
enum Processor {
    Cpu,
    Gpu,
}

impl Processor {
    fn label(self) -> &'static str {
        match self {
            Processor::Cpu => "CPU Load",
            Processor::Gpu => "GPU Load",
        }
    }

    fn snapshot_key(self) -> &'static str {
        match self {
            Processor::Cpu => "cpu",
            Processor::Gpu => "gpu",
        }
    }
}

/// Icon and text in a card's bottom-left corner.
struct Footer {
    icon: Icon,
    text: String,
}

struct LoadGauge {
    label: &'static str,
    load: f64,
    footer: Footer,
}

impl LoadGauge {
    fn read(status: &LiveStatus, processor: Processor, unit: &str) -> Self {
        let number = |field: &str| status.number(&format!("/snapshot/{}/{field}", processor.snapshot_key()));
        let footer = Footer { icon: icons::thermometer, text: format::temperature(number("temperature"), unit) };
        Self { label: processor.label(), load: number("load"), footer }
    }
}

struct LoadBar {
    label: &'static str,
    load: f64,
    stops: &'static [Color32],
    footer: Footer,
}

fn memory_bar(status: &LiveStatus) -> LoadBar {
    let usage = format::memory_usage(status.number("/snapshot/memory/used"), status.number("/snapshot/memory/total"));
    LoadBar { label: "Memory Load", load: status.number("/snapshot/memory/load"), stops: &MEMORY_GRADIENT, footer: Footer { icon: icons::memory, text: usage } }
}

fn disk_bar(status: &LiveStatus) -> LoadBar {
    let usage = format::disk_usage(status.number("/snapshot/disk/used"), status.number("/snapshot/disk/total"));
    LoadBar { label: "Hard disk Load", load: status.number("/snapshot/disk/load"), stops: &DISK_GRADIENT, footer: Footer { icon: icons::disk, text: usage } }
}

fn gauge_card(ui: &mut Ui, gauge: &LoadGauge) {
    let load = eased(ui, gauge.label, gauge.load);
    let region = card_heading(ui, load, gauge.label);
    charts::gauge(ui.painter(), region.with_max_y(region.bottom() - FOOTER_HEIGHT), (load / 100.0) as f32);
    paint_footer(ui.painter(), region, &gauge.footer);
}

fn bar_card(ui: &mut Ui, bar: &LoadBar) {
    let load = eased(ui, bar.label, bar.load);
    let region = card_heading(ui, load, bar.label);
    let bar_centre = pos2(region.center().x, region.top() + (region.height() - FOOTER_HEIGHT) / 2.0);
    let bar_rect = Rect::from_center_size(bar_centre, vec2(region.width(), BAR_HEIGHT));
    charts::gradient_bar(ui.painter(), bar_rect, &GradientFill { fraction: (load / 100.0) as f32, stops: bar.stops });
    paint_footer(ui.painter(), region, &bar.footer);
}

/// The reading `key` on its way to `target`: it glides there over `EASE_SECONDS` instead of jumping.
fn eased(ui: &Ui, key: impl std::hash::Hash + std::fmt::Debug, target: f64) -> f64 {
    ui.ctx().animate_value_with_time(Id::new(("dashboard", key)), target as f32, EASE_SECONDS) as f64
}

/// Big percentage and its label; returns the space left below them.
fn card_heading(ui: &mut Ui, load: f64, label: &str) -> Rect {
    ui.spacing_mut().item_spacing.y = 4.0;
    ui.label(bold(format::percent(load), 40.0));
    ui.label(muted(label).size(15.0));
    ui.add_space(8.0);
    ui.available_rect_before_wrap()
}

fn paint_footer(painter: &Painter, region: Rect, footer: &Footer) {
    let centre_y = region.bottom() - FOOTER_HEIGHT / 2.0;
    (footer.icon)(painter, pos2(region.left() + 10.0, centre_y), TEXT);
    painter.text(pos2(region.left() + 32.0, centre_y), Align2::LEFT_CENTER, &footer.text, FontId::proportional(15.0), TEXT);
}

fn specifications(ui: &mut Ui, page: &PageContext) {
    if ui.available_width() >= 2.0 * SPEC_CARD_MIN_WIDTH + CARD_GAP {
        let half = (ui.available_width() - CARD_GAP) / 2.0;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = CARD_GAP;
            let panorama_height = panorama_card(ui, page, half);
            computer_card(ui, page.status, vec2(half, panorama_height));
        });
    } else {
        panorama_card(ui, page, ui.available_width());
        ui.add_space(CARD_GAP);
        computer_card(ui, page.status, vec2(ui.available_width(), 0.0));
    }
}

/// Returns the card's height so the card beside it can match.
fn panorama_card(ui: &mut Ui, page: &PageContext, width: f32) -> f32 {
    let turris = page.turris_connected();
    let response = ui.allocate_ui(vec2(width, 0.0), |ui| {
        card(ui, |ui| {
            ui.label(bold(if turris { "Turris" } else { "Panorama" }, 19.0));
            ui.add_space(10.0);
            device_identity(ui, page.status, turris);
            ui.add_space(6.0);
            divider(ui);
            ui.add_space(6.0);
            device_readings(ui, page, turris);
        });
    });
    response.response.rect.height()
}

fn device_identity(ui: &mut Ui, status: &LiveStatus, turris: bool) {
    let screen = status.text("/device/screen");
    info_line(ui, "Model", &or_dash(true, status.text("/device/productId")));
    info_line(ui, "Screen", if screen.is_empty() { "2240 × 1080" } else { &screen });
    info_line(ui, "System", &or_dash(true, status.text("/device/OS")));
    info_line(ui, "Firmware", &or_dash(true, status.text("/device/version/firmware")));
    if !turris {
        info_line(ui, "Hardware", &or_dash(true, status.text("/device/version/hardware")));
        info_line(ui, "Screen app", &or_dash(true, status.text("/device/version/app")));
    }
    info_line(ui, "Serial number", &or_dash(true, status.text("/device/sn")));
    info_line(ui, if turris { "USB device" } else { "Serial port" }, &or_dash(true, status.text("/port")));
}

fn device_readings(ui: &mut Ui, page: &PageContext, turris: bool) {
    let (status, connected) = (page.status, page.connected());
    if !turris {
        info_line(ui, "Pump-head fan", &rpm(status.number("/device_status/status/fanLCD"), connected));
        info_line(ui, "Pump", &rpm(status.number("/device_status/status/turboPump"), connected));
        info_line(ui, "Free storage", &or_dash(connected, format::bytes_as_gigabytes(status.number("/device_status/availableStorage"))));
    } else {
        info_line(ui, "Built-in themes", &status.list("/device/presets").len().to_string());
    }
    let error = status.text("/error");
    if !connected && !error.is_empty() {
        ui.add_space(8.0);
        ui.label(muted(format!("Last error: {error}")).size(12.5));
    }
}

fn computer_card(ui: &mut Ui, status: &LiveStatus, size: egui::Vec2) {
    ui.allocate_ui(vec2(size.x, 0.0), |ui| {
        card(ui, |ui| {
            ui.set_min_height(card_content_height(size.y));
            ui.label(bold("Computer", 19.0));
            ui.add_space(10.0);
            info_line(ui, "Processor", &or_dash(true, status.text("/spec/cpu")));
            info_line(ui, "Graphics", &or_dash(true, status.text("/spec/gpu")));
            info_line(ui, "Memory", &format::megabytes_as_gigabytes(status.number("/snapshot/memory/total")));
            info_line(ui, "System disk", &format!("{:.0} GB", status.number("/snapshot/disk/total")));
        });
    });
}

fn rpm(value: f64, connected: bool) -> String {
    or_dash(connected, format!("{value:.0} RPM"))
}

fn or_dash(show: bool, text: String) -> String {
    if show && !text.is_empty() { text } else { "—".into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_dashboard_matches_kanali() {
        let layout = DashboardLayout::fit(940.0, 250.0);
        assert_eq!(layout.network, Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 520.0)));
        assert_eq!(layout.cpu, Rect::from_min_size(pos2(320.0, 0.0), vec2(300.0, 250.0)));
        assert_eq!(layout.disk, Rect::from_min_size(pos2(640.0, 270.0), vec2(300.0, 250.0)));
        assert_eq!(layout.size(), vec2(940.0, 520.0));
    }

    #[test]
    fn medium_dashboard_puts_network_across_the_top() {
        let layout = DashboardLayout::fit(600.0, 250.0);
        assert_eq!(layout.network, Rect::from_min_size(pos2(0.0, 0.0), vec2(600.0, 250.0)));
        assert_eq!(layout.gpu, Rect::from_min_size(pos2(310.0, 270.0), vec2(290.0, 250.0)));
        assert_eq!(layout.memory.min, pos2(0.0, 540.0));
        assert_eq!(layout.size(), vec2(600.0, 790.0));
    }

    #[test]
    fn narrow_dashboard_stacks_every_card() {
        let layout = DashboardLayout::fit(400.0, 250.0);
        assert_eq!(layout.disk, Rect::from_min_size(pos2(0.0, 1080.0), vec2(400.0, 250.0)));
        assert_eq!(layout.size(), vec2(400.0, 1330.0));
    }

    #[test]
    fn network_graphs_sit_side_by_side_only_in_wide_cards() {
        let wide = Rect::from_min_size(pos2(0.0, 0.0), vec2(500.0, 150.0));
        assert_eq!(split_network_area(wide).1.min, pos2(260.0, 0.0));
        let tall = Rect::from_min_size(pos2(0.0, 0.0), vec2(250.0, 420.0));
        assert_eq!(split_network_area(tall).1.min, pos2(0.0, 220.0));
    }
}
