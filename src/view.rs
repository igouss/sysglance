use std::{collections::HashSet, rc::Rc, time::Duration};

use gpui::{
    AnyElement, App, Bounds, Context, Hsla, IntoElement, Pixels, Render, ScrollHandle,
    SharedString, Window, WindowBounds, WindowOptions, canvas, div, point, prelude::*, px, size,
};
use gpui_component::{
    ActiveTheme, Root, Theme, ThemeMode,
    scroll::{ScrollableElement, ScrollbarMode},
    tooltip::Tooltip,
};
use sysglance_core::{
    history::History,
    level::Level,
    reading::{Category, Device, Reading, Unavailable, Value},
};

use crate::{
    layout::{Grid, buckets, masonry, visible},
    sampler::{Shared, lock},
};

const DEPTH: usize = 120;
const BARS: usize = 40;
const POLL: Duration = Duration::from_millis(100);
const CARD: f32 = 400.0;
const GAP: f32 = 12.0;
const HEADING: f32 = 32.0;
const PADDING: f32 = 12.0;
const TITLE: f32 = 22.0;
const DETAIL: f32 = 18.0;
const ROW: f32 = 20.0;
const SPARK: f32 = 72.0;
const MARGIN: f32 = 400.0;

#[derive(Clone)]
struct Palette {
    mono: SharedString,
    surface: Hsla,
    border: Hsla,
    text: Hsla,
    muted: Hsla,
    faint: Hsla,
    normal: Hsla,
    low: Hsla,
    high: Hsla,
    critical: Hsla,
}

impl Palette {
    fn of(cx: &App) -> Self {
        let theme = cx.theme();
        Self {
            mono: theme.mono_font_family.clone(),
            surface: theme.secondary,
            border: theme.border,
            text: theme.foreground,
            muted: theme.muted_foreground,
            faint: theme.muted_foreground.opacity(0.55),
            normal: theme.success,
            low: theme.info,
            high: theme.warning,
            critical: theme.danger,
        }
    }

    const fn level(&self, level: Level) -> Hsla {
        match level {
            Level::Unrated => self.text,
            Level::Normal => self.normal,
            Level::Low => self.low,
            Level::High => self.high,
            Level::Critical => self.critical,
        }
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

fn explained(reading: &Reading) -> String {
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
    lines.join("\n")
}

struct Row {
    key: SharedString,
    label: SharedString,
    value: SharedString,
    level: Level,
    unavailable: bool,
    text: bool,
    bars: Rc<[f32]>,
    tip: SharedString,
}

struct Card {
    title: SharedString,
    detail: SharedString,
    worst: Level,
    rows: Vec<Row>,
}

impl Card {
    #[expect(clippy::cast_precision_loss, reason = "a card holds a few dozen rows")]
    fn height(&self) -> f32 {
        (self.rows.len() as f32).mul_add(ROW, PADDING.mul_add(2.0, TITLE + DETAIL + 2.0))
    }
}

struct Section {
    title: SharedString,
    cards: Vec<Card>,
}

pub struct Dashboard {
    shared: Shared,
    seen: u64,
    took: Duration,
    history: History,
    sections: Vec<Section>,
    summary: SharedString,
    scroll: ScrollHandle,
}

impl Dashboard {
    fn fresh(&self) -> bool {
        lock(&self.shared).generation != self.seen
    }

    fn pick_up(&mut self) {
        let devices: Vec<Device> = {
            let published = lock(&self.shared);
            if published.generation == self.seen {
                return;
            }
            self.seen = published.generation;
            self.took = published.took;
            published.devices.clone()
        };
        let mut live: HashSet<&str> = HashSet::new();
        for reading in devices.iter().flat_map(|device| &device.readings) {
            if let Value::Measured { measure, .. } = reading.value {
                self.history.record(&reading.key, measure.magnitude());
                live.insert(&reading.key);
            }
        }
        self.history.retain(|key| live.contains(key));
        self.summary = summary(&devices, self.took).into();
        let mut sections: Vec<Section> = Vec::new();
        for device in &devices {
            let card = self.card(device);
            let title: SharedString = heading(device.category).into();
            match sections.last_mut() {
                Some(section) if section.title == title => section.cards.push(card),
                Some(_) | None => sections.push(Section {
                    title,
                    cards: vec![card],
                }),
            }
        }
        self.sections = sections;
    }

    fn card(&self, device: &Device) -> Card {
        let rows = device
            .readings
            .iter()
            .map(|reading| {
                let bars: Vec<f32> =
                    self.history
                        .series(&reading.key)
                        .map_or_else(Vec::new, |series| {
                            let (front, back) = series.as_slices();
                            let joined: Vec<f64> = front.iter().chain(back).copied().collect();
                            buckets(&joined, BARS)
                        });
                Row {
                    key: reading.key.clone().into(),
                    label: reading.label.clone().into(),
                    value: shown(&reading.value).into(),
                    level: reading.value.level(),
                    unavailable: matches!(reading.value, Value::Unavailable(_)),
                    text: matches!(reading.value, Value::Text(_)),
                    bars: bars.into(),
                    tip: explained(reading).into(),
                }
            })
            .collect();
        Card {
            title: device.title.clone().into(),
            detail: device.detail.clone().into(),
            worst: device.worst(),
            rows,
        }
    }
}

fn summary(devices: &[Device], took: Duration) -> String {
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

fn spark(bounds: Bounds<Pixels>, bars: &[f32], colour: Hsla, window: &mut Window) {
    if bars.is_empty() {
        return;
    }
    let height = f32::from(bounds.size.height);
    #[expect(clippy::cast_precision_loss, reason = "a sparkline has 40 bars")]
    let step = f32::from(bounds.size.width) / BARS as f32;
    let offset = BARS.saturating_sub(bars.len());
    let fill = colour.opacity(0.7);
    for (i, &bar) in bars.iter().enumerate() {
        let tall = bar.mul_add(height - 1.0, 1.0);
        #[expect(clippy::cast_precision_loss, reason = "a sparkline has 40 bars")]
        let x = (offset + i) as f32 * step;
        window.paint_quad(gpui::fill(
            Bounds::new(
                bounds.origin + point(px(x), px(height - tall)),
                size(px((step - 0.5).max(1.0)), px(tall)),
            ),
            fill,
        ));
    }
}

fn row(row: &Row, palette: &Palette) -> AnyElement {
    let colour = palette.level(row.level);
    let value_colour = if row.unavailable {
        palette.faint
    } else {
        colour
    };
    let tip = row.tip.clone();
    let line = div()
        .id(row.key.clone())
        .flex()
        .items_center()
        .gap_2()
        .h(px(ROW))
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx));
    let value = div()
        .text_right()
        .truncate()
        .text_color(value_colour)
        .font_family(palette.mono.clone())
        .child(row.value.clone());
    if row.text {
        return line
            .child(
                div()
                    .flex_none()
                    .text_color(palette.muted)
                    .child(row.label.clone()),
            )
            .child(value.flex_1().min_w_0())
            .into_any_element();
    }
    let bars = Rc::clone(&row.bars);
    line.child(
        div()
            .flex_1()
            .min_w_0()
            .truncate()
            .text_color(palette.muted)
            .child(row.label.clone()),
    )
    .child(
        canvas(
            |_, _, _| (),
            move |bounds, (), window, _| spark(bounds, &bars, colour, window),
        )
        .w(px(SPARK))
        .h(px(14.0))
        .flex_none(),
    )
    .child(value.w(px(124.0)).flex_none())
    .into_any_element()
}

fn card(card: &Card, x: f32, y: f32, palette: &Palette) -> AnyElement {
    div()
        .absolute()
        .left(px(x))
        .top(px(y))
        .w(px(CARD))
        .h(px(card.height()))
        .overflow_hidden()
        .p(px(PADDING))
        .bg(palette.surface)
        .border_1()
        .border_color(palette.border)
        .rounded_md()
        .child(
            div()
                .h(px(TITLE))
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .size(px(8.0))
                        .flex_none()
                        .rounded_full()
                        .bg(palette.level(card.worst)),
                )
                .child(
                    div()
                        .flex_1()
                        .truncate()
                        .text_color(palette.text)
                        .child(card.title.clone()),
                ),
        )
        .child(
            div()
                .h(px(DETAIL))
                .text_xs()
                .text_color(palette.faint)
                .truncate()
                .child(card.detail.clone()),
        )
        .children(card.rows.iter().map(|r| row(r, palette)))
        .into_any_element()
}

impl Render for Dashboard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.pick_up();
        let palette = Palette::of(cx);
        let viewport = self.scroll.bounds();
        let (width, height) = if viewport.size.width > px(0.0) {
            (
                f32::from(viewport.size.width),
                f32::from(viewport.size.height),
            )
        } else {
            let window_size = window.viewport_size();
            (f32::from(window_size.width), f32::from(window_size.height))
        };
        let grid = Grid {
            width: 2.0f32.mul_add(-PADDING, width),
            card: CARD,
            gap: GAP,
            heading: HEADING,
        };
        let heights: Vec<Vec<f32>> = self
            .sections
            .iter()
            .map(|s| s.cards.iter().map(Card::height).collect())
            .collect();
        let layout = masonry(&heights, grid);
        let top = -f32::from(self.scroll.offset().y) - MARGIN;
        let span = 2.0f32.mul_add(MARGIN, height);
        let cards: Vec<&Card> = self.sections.iter().flat_map(|s| &s.cards).collect();
        let mut children: Vec<AnyElement> = layout
            .headings
            .iter()
            .filter(|(_, y)| *y + HEADING > top && *y < top + span)
            .map(|&(section, y)| {
                div()
                    .absolute()
                    .left_0()
                    .top(px(y))
                    .h(px(HEADING))
                    .flex()
                    .items_end()
                    .pb_1()
                    .text_sm()
                    .text_color(palette.muted)
                    .child(self.sections[section].title.clone())
                    .into_any_element()
            })
            .collect();
        children.extend(
            layout
                .cards
                .iter()
                .filter(|placed| visible(placed, top, span))
                .map(|placed| card(cards[placed.index], placed.x, placed.y, &palette)),
        );
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(palette.text)
            .text_sm()
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap_4()
                    .px_4()
                    .py_2()
                    .border_b_1()
                    .border_color(palette.border)
                    .child(div().text_lg().child("sysglance"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(palette.muted)
                            .child(self.summary.clone()),
                    ),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .id("body")
                            .size_full()
                            .track_scroll(&self.scroll)
                            .overflow_y_scroll()
                            .child(
                                div()
                                    .relative()
                                    .m(px(PADDING))
                                    .w(px(grid.width))
                                    .h(px(layout.height + PADDING))
                                    .children(children),
                            ),
                    )
                    .vertical_scrollbar(&self.scroll),
            )
    }
}

pub fn run(shared: Shared) {
    gpui_platform::application().run(move |cx: &mut App| {
        gpui_component::init(cx);
        Theme::change(ThemeMode::Dark, None, cx);
        Theme::global_mut(cx).scrollbar_mode = ScrollbarMode::Always;
        let bounds = Bounds::centered(None, size(px(1720.0), px(1000.0)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            app_id: Some("sysglance".to_owned()),
            ..Default::default()
        };
        let opened = cx.open_window(options, |window, cx| {
            let dashboard = cx.new(|cx| {
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
                    took: Duration::ZERO,
                    history: History::new(DEPTH),
                    sections: Vec::new(),
                    summary: SharedString::default(),
                    scroll: ScrollHandle::new(),
                }
            });
            cx.new(|cx| Root::new(dashboard, window, cx))
        });
        if opened.is_err() {
            cx.quit();
        }
        cx.on_window_closed(|cx, _| cx.quit()).detach();
        cx.activate(true);
    });
}
