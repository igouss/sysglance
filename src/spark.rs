#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

pub const FILL: f32 = 0.4;

fn level(bars: &[f32], slots: usize, x: usize, width: usize) -> Option<f32> {
    if bars.is_empty() || width < 2 {
        return None;
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "sparklines are a few dozen pixels and bars"
    )]
    let slot = x as f32 / (width - 1) as f32 * (slots - 1) as f32;
    let first = slots - bars.len();
    #[expect(clippy::cast_precision_loss, reason = "a few dozen bars")]
    let along = slot - first as f32;
    if along < 0.0 {
        return None;
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a non-negative bar index"
    )]
    let left = (along.floor() as usize).min(bars.len() - 1);
    let right = (left + 1).min(bars.len() - 1);
    #[expect(clippy::cast_precision_loss, reason = "a few dozen bars")]
    let t = along - left as f32;
    Some(
        (bars[right] - bars[left])
            .mul_add(t, bars[left])
            .clamp(0.0, 1.0),
    )
}

pub fn raster(bars: &[f32], slots: usize, width: usize, height: usize, colour: Rgb) -> Vec<u8> {
    let mut bgra = vec![0_u8; width * height * 4];
    if height == 0 || slots < bars.len() {
        return bgra;
    }
    for x in 0..width {
        let Some(value) = level(bars, slots, x, width) else {
            continue;
        };
        #[expect(
            clippy::cast_precision_loss,
            reason = "a sparkline is a few dozen pixels tall"
        )]
        let rows = (height - 1) as f32;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a row within the sparkline"
        )]
        let top = ((1.0 - value) * rows).round() as usize;
        for y in top..height {
            let alpha = if y == top { 1.0 } else { FILL };
            let at = (y * width + x) * 4;
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "alpha-scaled bytes stay in 0..=255"
            )]
            let scale = |channel: u8| (f32::from(channel) * alpha).round() as u8;
            bgra[at] = scale(colour.blue);
            bgra[at + 1] = scale(colour.green);
            bgra[at + 2] = scale(colour.red);
            bgra[at + 3] = scale(255);
        }
    }
    bgra
}

#[cfg(test)]
mod tests;
