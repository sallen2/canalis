//! Live stats drawn over the Turris's media: an overlay layout (label groups with their place,
//! font and colour) sent once, then the changing texts every second.
//!
//! The layout and label ids follow KANALI's 1280×720 Turris layout, as recorded in
//! DXVSI/Tryx-Linux-GUI (MIT licence): one row per stat, each row a title above a large value
//! with its unit, rows spread evenly down the screen, and the processor badges on top. The text
//! is larger than KANALI's; see `TextSizes`.

use super::wire::Message;
use crate::settings::{CPU_BADGE, DATE_AND_TIME, FAHRENHEIT, GPU_BADGE, OverlayStyle, badge_is_available};
use chrono::{DateTime, Local};
use serde_json::Value;

const SCREEN_WIDTH: u32 = 1280;
const SCREEN_HEIGHT: u32 = 720;
const MARGIN_X: u32 = 30;
const GROUP_WIDTH: u32 = SCREEN_WIDTH - 2 * MARGIN_X;
/// A row's title comes first; its value and unit follow, this much closer than a new line.
const LINE_GAP: i32 = -10;
const BADGE_ROW_Y: u32 = 60;
const BADGE_GROUP: u32 = 300;
const FONT: &str = "roboto-regular";
const BADGE_TEXT_COLOR: u32 = 0x00DC_DCDC;
const DEFAULT_TEXT_COLOR: u32 = 0x00DC_DCDC;
/// What a value shows until the first reading arrives.
const NO_VALUE: &str = "--";
const GRADIENT_HORIZONTAL: u64 = 3;

#[derive(Clone, Copy)]
enum Unit {
    Temperature,
    Text(&'static str),
}

/// One stat as the Turris draws it. `field` is Canalis's name for it (settings.json), `title`
/// KANALI's label on the screen.
struct Metric {
    field: &'static str,
    title: &'static str,
    unit: Unit,
    group: u32,
    title_label: u32,
    value_label: u32,
    unit_label: u32,
    /// Where the reading is in the hardware snapshot, and how many decimals it shows.
    reading: &'static str,
    decimals: usize,
}

const fn metric(field: &'static str, title: &'static str, unit: Unit, group: u32, first_label: u32, reading: &'static str, decimals: usize) -> Metric {
    Metric { field, title, unit, group, title_label: first_label, value_label: first_label + 1, unit_label: first_label + 2, reading, decimals }
}

/// KANALI's ten stats and its date and time keep their ids (groups 100-110, labels 101-132).
/// The three Canalis stats KANALI doesn't offer on the Turris continue the numbering.
const METRICS: [Metric; 13] = [
    metric("CPU Temperature", "CPU TEMP", Unit::Temperature, 100, 101, "/cpu/temperature", 0),
    metric("CPU Frequency", "CPU Frequency", Unit::Text("MHZ"), 101, 104, "/cpu/speedAverage", 0),
    metric("CPU Usage", "CPU Usage", Unit::Text("%"), 102, 107, "/cpu/usage", 0),
    metric("CPU Power", "CPU Power", Unit::Text("W"), 103, 110, "/cpu/power", 0),
    metric("GPU Temperature", "GPU TEMP", Unit::Temperature, 104, 113, "/gpu/temperature", 0),
    metric("GPU Frequency", "GPU Frequency", Unit::Text("MHZ"), 105, 116, "/gpu/speed", 0),
    metric("GPU Usage", "GPU Usage", Unit::Text("%"), 106, 119, "/gpu/load", 0),
    metric("GPU Power", "GPU Power", Unit::Text("W"), 107, 122, "/gpu/power", 0),
    metric("Memory Frequency", "Memory Frequency", Unit::Text("MHZ"), 108, 125, "/memory/speed", 0),
    metric("Memory Utilization", "Memory Usage", Unit::Text("%"), 109, 128, "/memory/load", 0),
    metric("CPU Voltage", "CPU Voltage", Unit::Text("V"), 111, 133, "/cpu/voltage", 2),
    metric("GPU Voltage", "GPU Voltage", Unit::Text("V"), 112, 136, "/gpu/voltage", 2),
    metric("Motherboard Temperature", "MB TEMP", Unit::Temperature, 113, 139, "/motherboard/temperature", 0),
];
const DATE_AND_TIME_GROUP: u32 = 110;
const DATE_LABEL: u32 = 131;
const TIME_LABEL: u32 = 132;

/// Text sizes. KANALI's for the Turris are 15 (titles), 120 (values), 25 (units) and 25 (badges):
/// at the screen's 294 PPI its titles are about 1.3 mm tall. `LARGE` was picked by comparing
/// sizes on a TURRIS 620; three rows don't fit at that size, so they use `COMPACT`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct TextSizes {
    title: u32,
    value: u32,
    unit: u32,
    badge: u32,
}

const LARGE: TextSizes = TextSizes { title: 42, value: 180, unit: 50, badge: 36 };
const COMPACT: TextSizes = TextSizes { title: 32, value: 136, unit: 38, badge: 30 };

fn text_sizes(rows: usize) -> TextSizes {
    if rows <= 2 { LARGE } else { COMPACT }
}

enum Row {
    Stat(&'static Metric),
    DateAndTime,
}

/// The rows to draw, in the order chosen, ignoring names the Turris has no row for.
fn rows(overlay: &[String]) -> Vec<Row> {
    overlay
        .iter()
        .filter_map(|field| if field == DATE_AND_TIME { Some(Row::DateAndTime) } else { METRICS.iter().find(|metric| metric.field == field).map(Row::Stat) })
        .collect()
}

/// The layout for `overlay` (the stats chosen, top to bottom) in `style`.
pub fn layout(overlay: &[String], style: &OverlayStyle, temperature_unit: &str, spec: &Value) -> Message {
    let rows = rows(overlay);
    let sizes = text_sizes(rows.len());
    let (alignment, title_gap) = alignment(&style.align);
    let color = text_color(&style.color);
    let mut layout = Message::new();
    for (index, row) in rows.iter().enumerate() {
        let group_id = match row {
            Row::Stat(metric) => metric.group,
            Row::DateAndTime => DATE_AND_TIME_GROUP,
        };
        // KANALI makes a row as tall as its value text.
        let mut group = Message::new()
            .varint(1, u64::from(group_id))
            .varint(2, u64::from(MARGIN_X))
            .varint(3, u64::from(row_y(index, rows.len(), sizes.value)))
            .varint(4, u64::from(GROUP_WIDTH))
            .varint(5, u64::from(sizes.value))
            .varint(6, alignment)
            .sint32(7, LINE_GAP);
        match row {
            Row::Stat(metric) => {
                group = group
                    .message(8, &label(metric.title_label, 1, title_gap, sizes.title, color, metric.title))
                    .message(8, &label(metric.value_label, 0, 0, sizes.value, color, NO_VALUE))
                    .message(8, &label(metric.unit_label, 0, 0, sizes.unit, color, &unit_text(metric.unit, temperature_unit)));
            }
            Row::DateAndTime => {
                let now = Local::now();
                group = group
                    .message(8, &label(DATE_LABEL, 1, title_gap, sizes.title, color, &date_text(&now)))
                    .message(8, &label(TIME_LABEL, 0, 0, sizes.value, color, &time_text(&now)));
            }
        }
        layout = layout.message(1, &group);
    }
    if let Some(badges) = badges(overlay, style, alignment, sizes.badge, spec) {
        layout = layout.message(1, &badges);
    }
    layout
}

/// The new texts for the rows of `overlay`, or None when there are no rows.
pub fn metric_batch(overlay: &[String], temperature_unit: &str, snapshot: &Value, now: &DateTime<Local>) -> Option<Message> {
    let rows = rows(overlay);
    if rows.is_empty() {
        return None;
    }
    let mut batch = Message::new();
    for row in rows {
        let update = match row {
            Row::Stat(metric) => {
                let mut update = Message::new().varint(1, u64::from(metric.group)).message(2, &text_update(metric.value_label, &reading(metric, temperature_unit, snapshot)));
                if let Unit::Temperature = metric.unit {
                    update = update.message(2, &text_update(metric.unit_label, &unit_text(metric.unit, temperature_unit)));
                }
                update
            }
            Row::DateAndTime => Message::new()
                .varint(1, u64::from(DATE_AND_TIME_GROUP))
                .message(2, &text_update(DATE_LABEL, &date_text(now)))
                .message(2, &text_update(TIME_LABEL, &time_text(now))),
        };
        batch = batch.message(1, &update);
    }
    Some(batch)
}

fn text_update(label_id: u32, text: &str) -> Message {
    Message::new().varint(1, u64::from(label_id)).string(2, text)
}

fn label(id: u32, line: u64, gap_left: i32, size: u32, color: u32, text: &str) -> Message {
    let mut label = Message::new().varint(1, u64::from(id));
    if line != 0 {
        label = label.varint(2, line);
    }
    if gap_left != 0 {
        label = label.sint32(4, gap_left);
    }
    label.string(8, FONT).varint(9, u64::from(size)).varint(10, u64::from(color)).string(11, text)
}

/// Rows are spread evenly down the screen, each centred on its value.
fn row_y(index: usize, count: usize, value_size: u32) -> u32 {
    let centre = (f64::from(SCREEN_HEIGHT) / (count + 1) as f64 * (index + 1) as f64).round();
    (centre - f64::from(value_size) / 2.0).round().max(0.0) as u32
}

/// The screen's alignment code, and the gap that sets titles in from the left edge.
fn alignment(align: &str) -> (u64, i32) {
    match align {
        "Center" => (2, 0),
        "Right" => (3, 0),
        _ => (1, 13),
    }
}

fn text_color(color: &str) -> u32 {
    u32::from_str_radix(color.trim_start_matches('#'), 16).ok().filter(|_| color.len() == 7).unwrap_or(DEFAULT_TEXT_COLOR)
}

fn unit_text(unit: Unit, temperature_unit: &str) -> String {
    match unit {
        Unit::Temperature if temperature_unit == FAHRENHEIT => "°F".into(),
        Unit::Temperature => "°C".into(),
        Unit::Text(text) => text.into(),
    }
}

fn reading(metric: &Metric, temperature_unit: &str, snapshot: &Value) -> String {
    let Some(mut value) = snapshot.pointer(metric.reading).and_then(Value::as_f64) else {
        return NO_VALUE.into();
    };
    if matches!(metric.unit, Unit::Temperature) && temperature_unit == FAHRENHEIT {
        value = value * 9.0 / 5.0 + 32.0;
    }
    format!("{value:.*}", metric.decimals)
}

fn date_text(now: &DateTime<Local>) -> String {
    now.format("%-d %b %Y").to_string()
}

fn time_text(now: &DateTime<Local>) -> String {
    now.format("%H:%M").to_string()
}

/// The CPU and GPU badges: the processor names in coloured pills along the top, shown only while
/// a stat of that processor is.
fn badges(overlay: &[String], style: &OverlayStyle, alignment: u64, text_size: u32, spec: &Value) -> Option<Message> {
    let mut group = Message::new()
        .varint(1, u64::from(BADGE_GROUP))
        .varint(2, u64::from(MARGIN_X))
        .varint(3, u64::from(BADGE_ROW_Y))
        .varint(4, u64::from(GROUP_WIDTH))
        .varint(6, alignment)
        .sint32(7, 1);
    let mut shown = 0;
    for badge in &style.badges {
        let (label_id, name) = match badge.as_str() {
            CPU_BADGE => (BADGE_GROUP + 1, spec["cpu"].as_str().unwrap_or("CPU")),
            GPU_BADGE => (BADGE_GROUP + 2, spec["gpu"].as_str().unwrap_or("GPU")),
            _ => continue,
        };
        if !badge_is_available(badge, overlay) {
            continue;
        }
        let text = badge_text(name);
        let (background, gradient) = badge_colors(&text);
        let mut label = Message::new().varint(1, u64::from(label_id));
        if shown > 0 {
            label = label.sint32(4, 10);
        }
        label = label
            .varint(5, GRADIENT_HORIZONTAL)
            .varint(6, u64::from(background))
            .varint(7, u64::from(gradient))
            .string(8, FONT)
            .varint(9, u64::from(text_size))
            .varint(10, u64::from(BADGE_TEXT_COLOR))
            .string(11, &format!("  {text}  "));
        group = group.message(8, &label);
        shown += 1;
    }
    (shown > 0).then_some(group)
}

/// A processor name short enough for a badge: "13th Gen Intel(R) Core(TM) i5-13600K" becomes
/// "Intel Core i5-13600K" and "AMD Ryzen 7 9800X3D 8-Core Processor" "AMD Ryzen 7 9800X3D".
fn badge_text(name: &str) -> String {
    let cleaned = name.replace("(R)", "").replace("(TM)", "").replace("(tm)", "");
    let words: Vec<&str> = cleaned
        .split_whitespace()
        .filter(|word| !word.ends_with("-Core") && !word.eq_ignore_ascii_case("processor") && !word.eq_ignore_ascii_case("cpu"))
        .collect();
    let start = words.iter().position(|word| *word == "Gen").map(|index| index + 1).unwrap_or(0);
    let end = words.iter().position(|word| *word == "@").unwrap_or(words.len());
    let text = words[start.min(end)..end].join(" ");
    if text.is_empty() { name.trim().to_string() } else { text }
}

/// KANALI's badge colours: green for NVIDIA, blue for Intel, red for AMD, grey otherwise.
fn badge_colors(text: &str) -> (u32, u32) {
    let text = text.to_lowercase();
    if text.contains("nvidia") || text.contains("geforce") {
        (0x0062_9A00, 0x0079_AB51)
    } else if text.contains("intel") {
        (0x0000_68B5, 0x0056_6D98)
    } else if ["amd", "radeon", "ryzen"].iter().any(|vendor| text.contains(vendor)) {
        (0x00A9_2F2C, 0x00CB_6236)
    } else {
        (0x004A_4A4A, 0x0070_7070)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::turris::wire::{bytes_field, repeated_bytes, string_field, varint_field};
    use serde_json::json;

    fn fields(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    fn style() -> OverlayStyle {
        OverlayStyle { color: "#dcdcdc".into(), align: "Left".into(), ..OverlayStyle::default() }
    }

    #[test]
    fn rows_are_spread_down_the_screen() {
        let layout = layout(&fields(&["CPU Temperature", "CPU Usage"]), &style(), "Celsius", &json!({}));
        let groups = repeated_bytes(layout.as_bytes(), 1);
        assert_eq!(groups.len(), 2);
        let numbers = |group: &[u8], field| varint_field(group, field).unwrap();
        // 720 / 3 = 240 and 480, minus half the value text (90).
        assert_eq!([numbers(groups[0], 1), numbers(groups[0], 2), numbers(groups[0], 3), numbers(groups[0], 4), numbers(groups[0], 5)], [100, 30, 150, 1220, 180]);
        assert_eq!([numbers(groups[1], 1), numbers(groups[1], 3)], [102, 390]);
        assert_eq!(numbers(groups[0], 6), 1);
        assert_eq!(numbers(groups[0], 7), 19, "line gap -10, zigzag-encoded");
        let labels = repeated_bytes(groups[0], 8);
        let texts: Vec<String> = labels.iter().map(|label| string_field(label, 11).unwrap()).collect();
        assert_eq!(texts, ["CPU TEMP", "--", "°C"]);
        let ids: Vec<u64> = labels.iter().map(|label| varint_field(label, 1).unwrap()).collect();
        assert_eq!(ids, [101, 102, 103]);
        assert_eq!(varint_field(labels[0], 4), Some(26), "titles set in by 13 when left-aligned");
        let sizes: Vec<u64> = labels.iter().map(|label| varint_field(label, 9).unwrap()).collect();
        assert_eq!(sizes, [42, 180, 50]);
    }

    #[test]
    fn one_row_is_centred() {
        let layout = layout(&fields(&["GPU Usage"]), &style(), "Celsius", &json!({}));
        let group = bytes_field(layout.as_bytes(), 1).unwrap();
        assert_eq!(varint_field(group, 3), Some(270));
        assert_eq!(varint_field(group, 1), Some(106));
    }

    /// Where a row's text runs: from its y down through the title, then the value a line gap
    /// closer.
    fn row_extent(index: usize, count: usize, sizes: TextSizes) -> (u32, u32) {
        let top = row_y(index, count, sizes.value);
        (top, (top as i32 + sizes.title as i32 + LINE_GAP + sizes.value as i32) as u32)
    }

    #[test]
    fn rows_fit_between_the_badges_and_the_bottom_without_overlapping() {
        for count in 1..=3 {
            let sizes = text_sizes(count);
            assert!(row_extent(0, count, sizes).0 > BADGE_ROW_Y + sizes.badge, "{count} rows: below the badges");
            for index in 1..count {
                assert!(row_extent(index - 1, count, sizes).1 < row_extent(index, count, sizes).0, "{count} rows: row {index} overlaps");
            }
            assert!(row_extent(count - 1, count, sizes).1 <= SCREEN_HEIGHT, "{count} rows: off the screen");
        }
        assert_eq!(text_sizes(2), LARGE);
        assert_eq!(text_sizes(3), COMPACT);
        // At the large size, three rows would overlap.
        assert!(row_extent(0, 3, LARGE).1 > row_extent(1, 3, LARGE).0);
    }

    #[test]
    fn unknown_names_are_skipped_and_an_empty_overlay_sends_no_metrics() {
        assert!(repeated_bytes(layout(&fields(&["Nope"]), &style(), "Celsius", &json!({})).as_bytes(), 1).is_empty());
        assert!(metric_batch(&[], "Celsius", &json!({}), &Local::now()).is_none());
    }

    #[test]
    fn metric_batch_carries_values_and_temperature_units() {
        let snapshot = json!({"cpu": {"temperature": 47, "usage": 12}, "gpu": {"voltage": 0.875}});
        let batch = metric_batch(&fields(&["CPU Temperature", "CPU Usage", "GPU Voltage"]), "Fahrenheit", &snapshot, &Local::now()).unwrap();
        let updates = repeated_bytes(batch.as_bytes(), 1);
        let texts = |update: &[u8]| -> Vec<(u64, String)> { repeated_bytes(update, 2).iter().map(|text| (varint_field(text, 1).unwrap(), string_field(text, 2).unwrap())).collect() };
        assert_eq!(texts(updates[0]), [(102, "117".to_string()), (103, "°F".to_string())]);
        assert_eq!(texts(updates[1]), [(108, "12".to_string())]);
        assert_eq!(texts(updates[2]), [(137, "0.88".to_string())]);
    }

    #[test]
    fn missing_readings_show_dashes() {
        let batch = metric_batch(&fields(&["CPU Power"]), "Celsius", &json!({}), &Local::now()).unwrap();
        let update = bytes_field(batch.as_bytes(), 1).unwrap();
        assert_eq!(string_field(bytes_field(update, 2).unwrap(), 2).as_deref(), Some("--"));
    }

    #[test]
    fn date_and_time_fill_title_and_value() {
        let now = Local::now();
        let batch = metric_batch(&fields(&[DATE_AND_TIME]), "Celsius", &json!({}), &now).unwrap();
        let update = bytes_field(batch.as_bytes(), 1).unwrap();
        assert_eq!(varint_field(update, 1), Some(110));
        let texts: Vec<String> = repeated_bytes(update, 2).iter().map(|text| string_field(text, 2).unwrap()).collect();
        assert_eq!(texts, [date_text(&now), time_text(&now)]);
    }

    #[test]
    fn badges_need_a_stat_of_their_processor() {
        let spec = json!({"cpu": "13th Gen Intel(R) Core(TM) i5-13600K", "gpu": "NVIDIA GeForce RTX 4070"});
        let with_badges = OverlayStyle { badges: fields(&[CPU_BADGE, GPU_BADGE]), ..style() };
        let layout = layout(&fields(&["CPU Temperature"]), &with_badges, "Celsius", &spec);
        let groups = repeated_bytes(layout.as_bytes(), 1);
        assert_eq!(groups.len(), 2);
        let badge_group = groups[1];
        assert_eq!(varint_field(badge_group, 1), Some(300));
        let labels = repeated_bytes(badge_group, 8);
        assert_eq!(labels.len(), 1, "no GPU stat, so no GPU badge");
        assert_eq!(string_field(labels[0], 11).as_deref(), Some("  Intel Core i5-13600K  "));
        assert_eq!(varint_field(labels[0], 6), Some(0x0068B5));
    }

    #[test]
    fn badge_names_are_shortened() {
        assert_eq!(badge_text("AMD Ryzen 7 9800X3D 8-Core Processor"), "AMD Ryzen 7 9800X3D");
        assert_eq!(badge_text("Intel(R) Core(TM) i7-8700K CPU @ 3.70GHz"), "Intel Core i7-8700K");
        assert_eq!(badge_text("NVIDIA GeForce RTX 4070"), "NVIDIA GeForce RTX 4070");
    }

    #[test]
    fn colours_come_from_the_style() {
        assert_eq!(text_color("#000000"), 0);
        assert_eq!(text_color("#dcdcdc"), 0xDCDCDC);
        assert_eq!(text_color("bogus"), DEFAULT_TEXT_COLOR);
    }
}
