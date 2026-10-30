mod model;
mod paint;
mod theme;

use std::{collections::HashSet, process, rc::Rc, sync::Arc, time::Duration};

use gpui::{
    AnyElement, App, Bounds, Context, IntoElement, MouseMoveEvent, Render, RenderImage,
    ScrollHandle, SharedString, Window, WindowBounds, WindowOptions, canvas, div, prelude::*, px,
    size,
};
use gpui_component::{
    ActiveTheme, Root,
    scroll::{Scrollbar, ScrollbarMode},
};
use model::{Card, PADDING, Section, sections, summary};
use sysglance_core::{history::History, reading::Value};
use theme::{Palette, follow_omarchy};

use crate::{
    bench::Bench,
    layout::{Grid, Layout, masonry, visible},
    sampler::{Shared, lock},
};

const DEPTH: usize = 120;
const POLL: Duration = Duration::from_millis(100);
const CARD: f32 = 400.0;
const GAP: f32 = 12.0;
const HEADING: f32 = 32.0;
const MARGIN: f32 = 200.0;

pub struct Dashboard {
    shared: Shared,
    seen: u64,
    history: History,
    sections: Vec<Section>,
    summary: SharedString,
    scroll: ScrollHandle,
    hovered: Option<(usize, usize)>,
    retired: Vec<Arc<RenderImage>>,
    bench: Option<Bench>,
}

impl Dashboard {
    fn fresh(&self) -> bool {
        lock(&self.shared).generation != self.seen
    }

    fn pick_up(&mut self, palette: &Palette, scale: f32) {
        let (devices, took) = {
            let published = lock(&self.shared);
            if published.generation == self.seen {
                return;
            }
            self.seen = published.generation;
            (published.devices.clone(), published.took)
        };
        let mut live: HashSet<&str> = HashSet::new();
        for reading in devices.iter().flat_map(|device| &device.readings) {
            if let Value::Measured { measure, .. } = reading.value {
                self.history.record(&reading.key, measure.magnitude());
                live.insert(&reading.key);
            }
        }
        self.history.retain(|key| live.contains(key));
        self.summary = summary(&devices, took).into();
        let old = std::mem::replace(
            &mut self.sections,
            sections(&devices, &self.history, palette, scale),
        );
        self.retired.extend(
            old.iter()
                .flat_map(|section| &section.cards)
                .flat_map(|card| card.sparks()),
        );
    }

    fn tip(&self) -> Option<SharedString> {
        let (card, row) = self.hovered?;
        let card = self
            .sections
            .iter()
            .flat_map(|section| &section.cards)
            .nth(card)?;
        Some(card.rows.get(row)?.tip.clone())
    }

    fn card(
        &self,
        index: usize,
        card: &Rc<Card>,
        at: (f32, f32),
        palette: &Palette,
        cx: &Context<Self>,
    ) -> AnyElement {
        let hovered = self
            .hovered
            .and_then(|(card, row)| (card == index).then_some(row));
        let painted = Rc::clone(card);
        let hit = Rc::clone(card);
        let palette = palette.clone();
        div()
            .id(("card", index))
            .absolute()
            .left(px(at.0))
            .top(px(at.1))
            .w(px(CARD))
            .h(px(card.height()))
            .bg(palette.surface)
            .border_1()
            .border_color(palette.border)
            .rounded_md()
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                let next = hit.row_at(event.position.y).map(|row| (index, row));
                if this.hovered != next {
                    this.hovered = next;
                    cx.notify();
                }
            }))
            .on_hover(cx.listener(move |this, inside: &bool, _, cx| {
                if !inside && this.hovered.is_some_and(|(card, _)| card == index) {
                    this.hovered = None;
                    cx.notify();
                }
            }))
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, (), window, cx| {
                        paint::card(&painted, hovered, bounds, &palette, window, cx);
                    },
                )
                .size_full(),
            )
            .into_any_element()
    }

    fn layout(&self, window: &Window) -> (Grid, Layout, f32) {
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
            .map(|s| s.cards.iter().map(|card| card.height()).collect())
            .collect();
        (grid, masonry(&heights, grid), height)
    }

    fn visible(
        &self,
        layout: &Layout,
        height: f32,
        palette: &Palette,
        cx: &Context<Self>,
    ) -> Vec<AnyElement> {
        let top = -f32::from(self.scroll.offset().y) - MARGIN;
        let span = 2.0f32.mul_add(MARGIN, height);
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
        let cards: Vec<Rc<Card>> = self
            .sections
            .iter()
            .flat_map(|s| s.cards.iter().cloned())
            .collect();
        for placed in layout
            .cards
            .iter()
            .filter(|placed| visible(placed, top, span))
        {
            children.push(self.card(
                placed.index,
                &cards[placed.index],
                (placed.x, placed.y),
                palette,
                cx,
            ));
        }
        children
    }
}

fn header(summary: SharedString, palette: &Palette) -> impl IntoElement {
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
        .child(div().text_xs().text_color(palette.muted).child(summary))
}

fn footer(tip: SharedString, palette: &Palette) -> impl IntoElement {
    div()
        .flex_none()
        .px_4()
        .py_1()
        .border_t_1()
        .border_color(palette.border)
        .text_xs()
        .text_color(palette.muted)
        .truncate()
        .child(tip)
}

impl Render for Dashboard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::of(cx);
        self.pick_up(&palette, window.scale_factor());
        for image in self.retired.drain(..) {
            let _ = window.drop_image(image);
        }
        if let Some(bench) = self.bench.as_mut().filter(|_| !self.sections.is_empty()) {
            bench.frame(&self.scroll, window);
        }
        let (grid, layout, height) = self.layout(window);
        let children = self.visible(&layout, height, &palette, cx);
        let tip = self
            .tip()
            .unwrap_or_else(|| "hover a reading for its source and limits".into());
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(palette.text)
            .font_family(palette.sans.family.clone())
            .text_sm()
            .child(header(self.summary.clone(), &palette))
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
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .child(Scrollbar::vertical(&self.scroll).mode(ScrollbarMode::Always)),
                    ),
            )
            .child(footer(tip, &palette))
    }
}

pub fn run(shared: Shared, bench: bool) {
    gpui_platform::application().run(move |cx: &mut App| {
        gpui_component::init(cx);
        follow_omarchy(cx);
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
                    history: History::new(DEPTH),
                    sections: Vec::new(),
                    summary: SharedString::default(),
                    scroll: ScrollHandle::new(),
                    hovered: None,
                    retired: Vec::new(),
                    bench: bench.then(Bench::default),
                }
            });
            cx.new(|cx| Root::new(dashboard, window, cx))
        });
        if let Err(why) = opened {
            eprintln!("sysglance: could not open a window: {why}");
            process::exit(1);
        }
        cx.on_window_closed(|cx, _| cx.quit()).detach();
        cx.activate(true);
    });
}
