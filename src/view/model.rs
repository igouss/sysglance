use std::{cell::Cell, rc::Rc, sync::Arc, time::Duration};

use gpui::{Bounds, Hsla, Pixels, RenderImage, Rgba, SharedString};
use sysglance_core::{
    history::History,
    level::Level,
    reading::{Category, Device, Reading, Unavailable, Value},
};

use super::theme::Palette;
use crate::{
    layout::buckets,
    spark::{Rgb, raster},
};

pub const BARS: usize = 40;
pub const PADDING: f32 = 12.0;
pub const TITLE: f32 = 22.0;
pub const DETAIL: f32 = 18.0;
pub const ROW: f32 = 20.0;
pub const SPARK_WIDTH: f32 = 72.0;
pub const SPARK_HEIGHT: f32 = 14.0;

pub struct Row {
    pub label: SharedString,
    pub value: SharedString,
    pub level: Level,
    pub unavailable: bool,
    pub text: bool,
    pub spark: Option<Arc<RenderImage>>,
    pub tip: SharedString,
}

pub struct Card {
    pub title: SharedString,
    pub detail: SharedString,
    pub worst: Level,
    pub rows: Vec<Row>,
    pub painted: Rc<Cell<Bounds<Pixels>>>,
}

impl Card {
    #[expect(clippy::cast_precision_loss, reason = "a card holds a few dozen rows")]
    pub fn height(&self) -> f32 {
        (self.rows.len() as f32).mul_add(ROW, PADDING.mul_add(2.0, TITLE + DETAIL + 2.0))
    }

    pub fn row_at(&self, y: Pixels) -> Option<usize> {
        let inside = f32::from(y - self.painted.get().origin.y) - PADDING - TITLE - DETAIL;
        if inside < 0.0 {
            return None;
        }
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a non-negative row index"
        )]
        let row = (inside / ROW) as usize;
        (row < self.rows.len()).then_some(row)
    }

    pub fn sparks(&self) -> impl Iterator<Item = Arc<RenderImage>> + '_ {
        self.rows.iter().filter_map(|row| row.spark.clone())
    }
}

pub struct Section {
    pub title: SharedString,
    pub cards: Vec<Rc<Card>>,
}

pub fn sections(
    devices: &[Device],
    history: &History,
    palette: &Palette,
    scale: f32,
) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    for device in devices {
        let card = Rc::new(card(device, history, palette, scale));
        let title: SharedString = heading(device.category).into();
        match sections.last_mut() {
            Some(section) if section.title == title => section.cards.push(card),
            Some(_) | None => sections.push(Section {
                title,
                cards: vec![card],
            }),
        }
    }
    sections
}

const fn heading(category: Category) -> &'static str {
    match category {
        Category::Processor => "Processor",
        Category::Power => "Power",
        Category::Memory => "Memory",
        Category::Graphics => "Graphics",
        Category::Board => "Motherboard",
        Category::Cooling => "Cooling",
        Category::Storage => "Storage",
        Category::Network => "Network",
        Category::System => "System",
    }
}

fn card(device: &Device, history: &History, palette: &Palette, scale: f32) -> Card {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a sparkline is a few dozen pixels"
    )]
    let (width, height) = (
        (SPARK_WIDTH * scale).round() as usize,
        (SPARK_HEIGHT * scale).round() as usize,
    );
    let rows = device
        .readings
        .iter()
        .map(|reading| {
            let level = reading.value.level();
            Row {
                label: reading.label.clone().into(),
                value: shown(&reading.value).into(),
                level,
                unavailable: matches!(reading.value, Value::Unavailable(_)),
                text: matches!(reading.value, Value::Text(_)),
                spark: spark(history, &reading.key, palette.level(level), width, height),
                tip: explained(reading).into(),
            }
        })
        .collect();
    Card {
        title: device.title.clone().into(),
        detail: device.detail.clone().into(),
        worst: device.worst(),
        rows,
        painted: Rc::default(),
    }
}

fn spark(
    history: &History,
    key: &str,
    colour: Hsla,
    width: usize,
    height: usize,
) -> Option<Arc<RenderImage>> {
    let series = history.series(key)?;
    let (front, back) = series.as_slices();
    let joined: Vec<f64> = front.iter().chain(back).copied().collect();
    let bars = buckets(&joined, BARS);
    if bars.len() < 2 {
        return None;
    }
    let bgra = raster(&bars, BARS, width, height, rgb(colour));
    let buffer = image::RgbaImage::from_raw(
        u32::try_from(width).ok()?,
        u32::try_from(height).ok()?,
        bgra,
    )?;
    Some(Arc::new(RenderImage::new([image::Frame::new(buffer)])))
}

fn rgb(colour: Hsla) -> Rgb {
    let rgba = Rgba::from(colour);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a colour channel in 0..=1"
    )]
    let byte = |channel: f32| (channel.clamp(0.0, 1.0) * 255.0).round() as u8;
    Rgb {
        red: byte(rgba.r),
        green: byte(rgba.g),
        blue: byte(rgba.b),
    }
}

fn shown(value: &Value) -> String {
    match value {
        Value::Measured { measure, .. } => measure.to_string(),
        Value::Text(text) => text.clone(),
        Value::Unavailable(why) => match why {
            Unavailable::Pending => "…".to_owned(),
            Unavailable::NeedsPrivilege(_) => "needs privilege".to_owned(),
            Unavailable::ModuleNotLoaded(module) => format!("load {module}"),
            Unavailable::NotSupported => "not supported".to_owned(),
            Unavailable::Unreadable(_) => "n/a".to_owned(),
        },
    }
}

fn explained(reading: &Reading) -> String {
    let mut parts = vec![reading.label.clone(), reading.origin.clone()];
    match &reading.value {
        Value::Measured { limits, .. } => {
            let limits: Vec<String> = [
                ("low", limits.low()),
                ("high", limits.high()),
                ("critical", limits.critical()),
            ]
            .into_iter()
            .filter_map(|(name, limit)| limit.map(|limit| format!("{name} {limit}")))
            .collect();
            if !limits.is_empty() {
                parts.push(format!("limits {}", limits.join(", ")));
            }
        }
        Value::Text(_) => {}
        Value::Unavailable(why) => parts.push(match why {
            Unavailable::Pending => "a rate needs a second sample".to_owned(),
            Unavailable::NeedsPrivilege(how) => format!("needs privilege: {how}"),
            Unavailable::ModuleNotLoaded(module) => format!("kernel module {module} is not loaded"),
            Unavailable::NotSupported => "the driver does not support this reading".to_owned(),
            Unavailable::Unreadable(error) => format!("unreadable: {error}"),
        }),
    }
    parts.join("  ·  ")
}

pub fn summary(devices: &[Device], took: Duration) -> String {
    let (mut high, mut critical, mut unavailable, mut total) = (0, 0, 0, 0);
    for reading in devices.iter().flat_map(|device| &device.readings) {
        total += 1;
        match reading.value.level() {
            Level::High => high += 1,
            Level::Critical => critical += 1,
            Level::Unrated | Level::Normal | Level::Low => {}
        }
        if matches!(reading.value, Value::Unavailable(_)) {
            unavailable += 1;
        }
    }
    format!(
        "{} devices · {total} readings · {high} high · {critical} critical · {unavailable} unavailable · sampled in {:.1} ms",
        devices.len(),
        took.as_secs_f64() * 1000.0
    )
}
