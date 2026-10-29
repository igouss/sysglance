use std::{fs, process, time::Instant};

use gpui::{ScrollHandle, Window, point, px};

const WARMUP: usize = 60;
const FRAMES: usize = 900;
const STEP: f32 = 37.0;
const TICK_MS: f64 = 10.0;

#[derive(Default)]
pub struct Bench {
    frames: Vec<Instant>,
    cpu_at_warmup: f64,
}

fn thread_cpu_ticks() -> f64 {
    let stat = fs::read_to_string("/proc/thread-self/stat").unwrap_or_default();
    stat.split_whitespace()
        .skip(13)
        .take(2)
        .filter_map(|field| field.parse::<f64>().ok())
        .sum()
}

fn percentile(sorted: &[f64], share: f64) -> f64 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "an index into a few hundred frames"
    )]
    let at = ((sorted.len() - 1) as f64 * share).round() as usize;
    sorted[at]
}

impl Bench {
    pub fn frame(&mut self, scroll: &ScrollHandle, window: &Window) {
        let bottom = f32::from(scroll.max_offset().y);
        let next = -f32::from(scroll.offset().y) + STEP;
        scroll.set_offset(point(px(0.0), px(if next > bottom { 0.0 } else { -next })));
        self.frames.push(Instant::now());
        if self.frames.len() == WARMUP {
            self.cpu_at_warmup = thread_cpu_ticks();
        }
        if self.frames.len() >= FRAMES {
            self.report();
            process::exit(0);
        }
        window.request_animation_frame();
    }

    fn report(&self) {
        let measured = &self.frames[WARMUP - 1..];
        let mut gaps: Vec<f64> = measured
            .windows(2)
            .map(|pair| (pair[1] - pair[0]).as_secs_f64() * 1000.0)
            .collect();
        gaps.sort_by(f64::total_cmp);
        #[expect(clippy::cast_precision_loss, reason = "a few hundred frames")]
        let count = gaps.len() as f64;
        let seconds = (measured[measured.len() - 1] - measured[0]).as_secs_f64();
        let cpu = (thread_cpu_ticks() - self.cpu_at_warmup) * TICK_MS / count;
        println!(
            "frames {count} in {seconds:.2}s = {:.1} fps; frame ms p50 {:.2} p95 {:.2} p99 {:.2}; main-thread cpu ms/frame {cpu:.2}",
            count / seconds,
            percentile(&gaps, 0.5),
            percentile(&gaps, 0.95),
            percentile(&gaps, 0.99),
        );
    }
}
