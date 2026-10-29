#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub index: usize,
    pub x: f32,
    pub y: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    pub width: f32,
    pub card: f32,
    pub gap: f32,
    pub heading: f32,
}

impl Grid {
    pub fn columns(self) -> usize {
        let fit = ((self.width + self.gap) / (self.card + self.gap)).floor();
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a small non-negative column count"
        )]
        let columns = fit.max(1.0) as usize;
        columns
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct Layout {
    pub headings: Vec<(usize, f32)>,
    pub cards: Vec<Placed>,
    pub height: f32,
}

pub fn masonry(sections: &[Vec<f32>], grid: Grid) -> Layout {
    let columns = grid.columns();
    let mut layout = Layout::default();
    let mut top = 0.0_f32;
    let mut index = 0;
    for (section, heights) in sections.iter().enumerate() {
        layout.headings.push((section, top));
        let start = top + grid.heading;
        let mut bottoms = vec![start; columns];
        for &height in heights {
            let (column, &y) = bottoms
                .iter()
                .enumerate()
                .min_by(|a, b| a.1.total_cmp(b.1))
                .unwrap_or((0, &start));
            #[expect(clippy::cast_precision_loss, reason = "a small column index")]
            let x = column as f32 * (grid.card + grid.gap);
            layout.cards.push(Placed {
                index,
                x,
                y,
                height,
            });
            bottoms[column] = y + height + grid.gap;
            index += 1;
        }
        top = bottoms.into_iter().fold(start, f32::max);
    }
    layout.height = top;
    layout
}

pub fn visible(placed: &Placed, top: f32, height: f32) -> bool {
    placed.y < top + height && placed.y + placed.height > top
}

pub fn buckets(series: &[f64], count: usize) -> Vec<f32> {
    if series.len() < 2 || count == 0 {
        return Vec::new();
    }
    let (low, high) = series
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), &v| {
            (low.min(v), high.max(v))
        });
    let span = if high > low { high - low } else { 1.0 };
    let per = series.len().div_ceil(count);
    series
        .chunks(per)
        .map(|chunk| {
            let peak = chunk.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            #[expect(clippy::cast_possible_truncation, reason = "a fraction in 0..=1")]
            let fraction = ((peak - low) / span) as f32;
            fraction
        })
        .collect()
}

#[cfg(test)]
mod tests;
