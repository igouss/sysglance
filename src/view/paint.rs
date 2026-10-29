use std::sync::Arc;

use gpui::{
    App, Bounds, ContentMask, Font, Hsla, Pixels, SharedString, TextAlign, TextRun, Window, point,
    px, size,
};

use super::{
    model::{Card, DETAIL, PADDING, ROW, Row, SPARK_HEIGHT, SPARK_WIDTH, TITLE},
    theme::Palette,
};

const VALUE: f32 = 124.0;
const SPACING: f32 = 8.0;
const DOT: f32 = 8.0;

#[derive(Clone, Copy)]
enum Align {
    Left,
    Right,
}

struct Painter<'a> {
    palette: &'a Palette,
    window: &'a mut Window,
    cx: &'a mut App,
}

impl Painter<'_> {
    fn run(content: &SharedString, face: &Font, colour: Hsla) -> TextRun {
        TextRun {
            len: content.len(),
            font: face.clone(),
            color: colour,
            background_color: None,
            underline: None,
            strikethrough: None,
        }
    }

    fn width(&self, content: &SharedString, face: &Font) -> Pixels {
        let run = Self::run(content, face, self.palette.text);
        self.window
            .text_system()
            .shape_line(content.clone(), self.palette.size, &[run], None)
            .width
    }

    fn text(
        &mut self,
        content: &SharedString,
        face: &Font,
        colour: Hsla,
        clip: Bounds<Pixels>,
        align: Align,
    ) {
        if content.is_empty() {
            return;
        }
        let run = Self::run(content, face, colour);
        let line =
            self.window
                .text_system()
                .shape_line(content.clone(), self.palette.size, &[run], None);
        let at = match align {
            Align::Right if line.width <= clip.size.width => {
                point(clip.right() - line.width, clip.origin.y)
            }
            Align::Right | Align::Left => clip.origin,
        };
        let cx = &mut *self.cx;
        self.window
            .with_content_mask(Some(ContentMask { bounds: clip }), |window| {
                let _ = line.paint(at, px(ROW), TextAlign::Left, None, window, cx);
            });
    }

    fn header(&mut self, card: &Card, area: Bounds<Pixels>) {
        let palette = self.palette;
        let dot = Bounds::new(
            area.origin + point(px(0.0), px((TITLE - DOT) / 2.0)),
            size(px(DOT), px(DOT)),
        );
        self.window
            .paint_quad(gpui::fill(dot, palette.level(card.worst)).corner_radii(px(DOT / 2.0)));
        let title = Bounds::new(
            area.origin + point(px(DOT + SPACING), px(0.0)),
            size(area.size.width - px(DOT + SPACING), px(TITLE)),
        );
        self.text(&card.title, &palette.sans, palette.text, title, Align::Left);
        let detail = Bounds::new(
            area.origin + point(px(0.0), px(TITLE - 2.0)),
            size(area.size.width, px(DETAIL)),
        );
        self.text(
            &card.detail,
            &palette.sans,
            palette.faint,
            detail,
            Align::Left,
        );
    }

    fn row(&mut self, row: &Row, line: Bounds<Pixels>, hovered: bool) {
        let palette = self.palette;
        if hovered {
            self.window
                .paint_quad(gpui::fill(line.dilate(px(4.0)), palette.hover));
        }
        let colour = if row.unavailable {
            palette.faint
        } else {
            palette.level(row.level)
        };
        if row.text {
            self.text(&row.label, &palette.sans, palette.muted, line, Align::Left);
            let label = self.width(&row.label, &palette.sans) + px(SPACING);
            let value = Bounds::new(
                line.origin + point(label, px(0.0)),
                size((line.size.width - label).max(px(0.0)), line.size.height),
            );
            self.text(&row.value, &palette.mono, colour, value, Align::Right);
            return;
        }
        let label = Bounds::new(
            line.origin,
            size(
                line.size.width - px(2.0f32.mul_add(SPACING, VALUE + SPARK_WIDTH)),
                line.size.height,
            ),
        );
        self.text(&row.label, &palette.sans, palette.muted, label, Align::Left);
        if let Some(spark) = &row.spark {
            let at = Bounds::new(
                point(
                    line.right() - px(VALUE + SPACING + SPARK_WIDTH),
                    line.top() + px((ROW - SPARK_HEIGHT) / 2.0),
                ),
                size(px(SPARK_WIDTH), px(SPARK_HEIGHT)),
            );
            let _ = self
                .window
                .paint_image(at, at, px(0.0).into(), Arc::clone(spark), 0, false);
        }
        let value = Bounds::new(
            point(line.right() - px(VALUE), line.top()),
            size(px(VALUE), line.size.height),
        );
        self.text(&row.value, &palette.mono, colour, value, Align::Right);
    }
}

pub fn card(
    card: &Card,
    hovered: Option<usize>,
    bounds: Bounds<Pixels>,
    palette: &Palette,
    window: &mut Window,
    cx: &mut App,
) {
    card.painted.set(bounds);
    let inner = Bounds::new(
        bounds.origin + point(px(PADDING), px(PADDING)),
        size(
            bounds.size.width - px(2.0 * PADDING),
            bounds.size.height - px(2.0 * PADDING),
        ),
    );
    let mut painter = Painter {
        palette,
        window,
        cx,
    };
    painter.header(card, inner);
    let mut top = inner.top() + px(TITLE + DETAIL);
    for (index, row) in card.rows.iter().enumerate() {
        painter.row(
            row,
            Bounds::new(point(inner.left(), top), size(inner.size.width, px(ROW))),
            hovered == Some(index),
        );
        top += px(ROW);
    }
}
