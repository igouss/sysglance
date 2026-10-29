use std::{collections::HashSet, time::Duration};

use gpui::{
    AnyElement, App, Bounds, Context, Hsla, IntoElement, Pixels, Render, SharedString, Window,
    WindowBounds, WindowOptions, canvas, div, point, prelude::*, px, rgb, size,
};
use sysglance_core::{
    history::History,
    level::Level,
    reading::{Category, Device, Reading, Unavailable, Value},
};

use crate::sampler::{Shared, lock};

const FONT: &str = "JetBrainsMono Nerd Font";
const DEPTH: usize = 120;
const POLL: Duration = Duration::from_millis(100);
const CARD: f32 = 400.0;

const BACKGROUND: u32 = 0x0f_14_1c;
const SURFACE: u32 = 0x17_1e_29;
const EDGE: u32 = 0x26_30_3f;
const TEXT: u32 = 0xe5_e7_eb;
const MUTED: u32 = 0x8b_94_a3;
const FAINT: u32 = 0x55_5e_6c;
const GREEN: u32 = 0x4a_de_80;
const BLUE: u32 = 0x60_a5_fa;
const AMBER: u32 = 0xfb_bf_24;
const RED: u32 = 0xf8_71_71;

const fn colour(level: Level) -> u32 {
    match level {
        Level::Unrated => TEXT,
        Level::Normal => GREEN,
        Level::Low => BLUE,
        Level::High => AMBER,
        Level::Critical => RED,
    }
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

fn explained(reading: &Reading) -> Vec<String> {
    let mut lines = vec![reading.label.clone(), reading.origin.clone()];
    match &reading.value {
        Value::Measured { limits, .. } => {
            let parts: Vec<String> = [
                ("low", limits.low()),
                ("high", limits.high()),
                ("critical", limits.critical()),
            ]
            .into_iter()
            .filter_map(|(name, limit)| limit.map(|limit| format!("{name} {limit}")))
            .collect();
            if !parts.is_empty() {
                lines.push(format!("limits: {}", parts.join(", ")));
            }
        }
        Value::Text(_) => {}
        Value::Unavailable(why) => lines.push(match why {
            Unavailable::Pending => "a rate needs a second sample".to_owned(),
            Unavailable::NeedsPrivilege(how) => format!("needs privilege: {how}"),
            Unavailable::ModuleNotLoaded(module) => format!("kernel module {module} is not loaded"),
            Unavailable::NotSupported => "the driver does not support this reading".to_owned(),
            Unavailable::Unreadable(error) => format!("unreadable: {error}"),
        }),
    }
    lines
}

struct Tip(Vec<String>);

impl Render for Tip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .p_2()
            .bg(rgb(SURFACE))
            .border_1()
            .border_color(rgb(EDGE))
            .rounded_md()
            .text_xs()
            .text_color(rgb(TEXT))
            .font_family(FONT)
            .flex()
            .flex_col()
            .children(self.0.iter().map(|line| div().child(line.clone())))
    }
}

pub struct Dashboard {
    shared: Shared,
    seen: u64,
    devices: Vec<Device>,
    took: Duration,
    history: History,
}

impl Dashboard {
    fn fresh(&self) -> bool {
        lock(&self.shared).generation != self.seen
    }

    fn pick_up(&mut self) {
        {
            let published = lock(&self.shared);
            if published.generation == self.seen {
                return;
            }
            self.seen = published.generation;
            self.devices.clone_from(&published.devices);
            self.took = published.took;
        }
        let mut live: HashSet<&str> = HashSet::new();
        for reading in self.devices.iter().flat_map(|device| &device.readings) {
            if let Value::Measured { measure, .. } = reading.value {
                self.history.record(&reading.key, measure.magnitude());
                live.insert(&reading.key);
            }
        }
        self.history.retain(|key| live.contains(key));
    }

    fn row(&self, reading: &Reading) -> AnyElement {
        let level = reading.value.level();
        let series: Vec<f64> = self
            .history
            .series(&reading.key)
            .map(|series| series.iter().copied().collect())
            .unwrap_or_default();
        let tip = explained(reading);
        let value_colour = match reading.value {
            Value::Unavailable(_) => FAINT,
            Value::Measured { .. } | Value::Text(_) => colour(level),
        };
        div()
            .id(SharedString::from(reading.key.clone()))
            .flex()
            .items_center()
            .gap_2()
            .h(px(20.0))
            .tooltip(move |_, cx| cx.new(|_| Tip(tip.clone())).into())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(rgb(MUTED))
                    .child(reading.label.clone()),
            )
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, (), window, _| spark(bounds, &series, colour(level), window),
                )
                .w(px(72.0))
                .h(px(14.0))
                .flex_none(),
            )
            .child(
                div()
                    .w(px(118.0))
                    .flex_none()
                    .text_right()
                    .truncate()
                    .text_color(rgb(value_colour))
                    .child(shown(&reading.value)),
            )
            .into_any_element()
    }

    fn card(&self, device: &Device) -> AnyElement {
        div()
            .w(px(CARD))
            .flex_none()
            .flex()
            .flex_col()
            .gap_1()
            .p_3()
            .bg(rgb(SURFACE))
            .border_1()
            .border_color(rgb(EDGE))
            .rounded_md()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .size(px(8.0))
                            .rounded_full()
                            .bg(rgb(colour(device.worst()))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_color(rgb(TEXT))
                            .child(device.title.clone()),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(FAINT))
                    .truncate()
                    .child(device.detail.clone()),
            )
            .children(device.readings.iter().map(|reading| self.row(reading)))
            .into_any_element()
    }

    fn summary(&self) -> String {
        let readings = self.devices.iter().flat_map(|device| &device.readings);
        let (mut high, mut critical, mut unavailable, mut total) = (0, 0, 0, 0);
        for reading in readings {
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
            self.devices.len(),
            self.took.as_secs_f64() * 1000.0
        )
    }
}

fn spark(bounds: Bounds<Pixels>, series: &[f64], hex: u32, window: &mut Window) {
    if series.len() < 2 {
        return;
    }
    let (low, high) = series
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), &v| {
            (low.min(v), high.max(v))
        });
    let span = if high > low { high - low } else { 1.0 };
    let height = f32::from(bounds.size.height);
    #[expect(
        clippy::cast_precision_loss,
        reason = "the history holds at most 120 values"
    )]
    let bar = f32::from(bounds.size.width) / DEPTH as f32;
    let offset = DEPTH.saturating_sub(series.len());
    let fill: Hsla = rgb(hex).into();
    let fill = fill.opacity(0.7);
    for (i, &v) in series.iter().enumerate() {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a fraction of a sparkline's height"
        )]
        let tall = (((v - low) / span) as f32).mul_add(height - 1.0, 1.0);
        #[expect(
            clippy::cast_precision_loss,
            reason = "the history holds at most 120 values"
        )]
        let x = (offset + i) as f32 * bar;
        window.paint_quad(gpui::fill(
            Bounds::new(
                bounds.origin + point(px(x), px(height - tall)),
                size(px(bar.max(1.0)), px(tall)),
            ),
            fill,
        ));
    }
}

impl Render for Dashboard {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.pick_up();
        let mut body: Vec<AnyElement> = Vec::new();
        let mut current: Option<Category> = None;
        for device in &self.devices {
            if current != Some(device.category) {
                current = Some(device.category);
                body.push(
                    div()
                        .w_full()
                        .pt_2()
                        .text_sm()
                        .text_color(rgb(MUTED))
                        .child(heading(device.category))
                        .into_any_element(),
                );
            }
            body.push(self.card(device));
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(BACKGROUND))
            .text_color(rgb(TEXT))
            .font_family(FONT)
            .text_sm()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .px_4()
                    .py_2()
                    .border_b_1()
                    .border_color(rgb(EDGE))
                    .child(div().text_lg().child("sysglance"))
                    .child(div().text_xs().text_color(rgb(MUTED)).child(self.summary())),
            )
            .child(
                div()
                    .id("body")
                    .flex_1()
                    .overflow_y_scroll()
                    .p_4()
                    .child(div().flex().flex_wrap().gap_3().children(body)),
            )
    }
}

pub fn run(shared: Shared) {
    gpui_platform::application().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1720.0), px(1000.0)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            app_id: Some("sysglance".to_owned()),
            ..Default::default()
        };
        let opened = cx.open_window(options, |_, cx| {
            cx.new(|cx| {
                cx.spawn(async move |this, cx| {
                    loop {
                        cx.background_executor().timer(POLL).await;
                        let alive = this.update(cx, |dashboard: &mut Dashboard, cx| {
                            if dashboard.fresh() {
                                cx.notify();
                            }
                        });
                        if alive.is_err() {
                            break;
                        }
                    }
                })
                .detach();
                Dashboard {
                    shared,
                    seen: 0,
                    devices: Vec::new(),
                    took: Duration::ZERO,
                    history: History::new(DEPTH),
                }
            })
        });
        if opened.is_err() {
            cx.quit();
        }
        cx.on_window_closed(|cx, _| cx.quit()).detach();
        cx.activate(true);
    });
}
