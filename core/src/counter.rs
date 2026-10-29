use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wrap {
    Never,
    At(u64),
}

pub fn per_second(before: u64, after: u64, wrap: Wrap, elapsed: Duration) -> Option<f64> {
    let seconds = elapsed.as_secs_f64();
    if seconds <= 0.0 {
        return None;
    }
    let delta = if after >= before {
        after - before
    } else {
        match wrap {
            Wrap::At(range) if before <= range => range - before + after,
            Wrap::Never | Wrap::At(_) => return None,
        }
    };
    #[expect(
        clippy::cast_precision_loss,
        reason = "a per-second delta fits an f64's mantissa"
    )]
    let rate = delta as f64 / seconds;
    Some(rate)
}

#[cfg(test)]
mod tests {
    use super::{Wrap, per_second};
    use proptest::prelude::*;
    use std::time::Duration;

    const SECOND: Duration = Duration::from_secs(1);

    #[test]
    fn no_elapsed_time_has_no_rate() {
        assert_eq!(per_second(0, 10, Wrap::Never, Duration::ZERO), None);
    }

    #[test]
    fn an_unchanged_counter_has_rate_zero() {
        assert_eq!(per_second(7, 7, Wrap::Never, SECOND), Some(0.0));
    }

    #[test]
    fn the_rate_divides_by_elapsed_time() {
        assert_eq!(
            per_second(100, 300, Wrap::Never, Duration::from_millis(500)),
            Some(400.0)
        );
    }

    #[test]
    fn a_counter_that_never_wraps_and_goes_back_has_no_rate() {
        assert_eq!(per_second(300, 100, Wrap::Never, SECOND), None);
    }

    #[test]
    fn a_wrapping_counter_counts_through_its_range() {
        assert_eq!(per_second(90, 10, Wrap::At(100), SECOND), Some(20.0));
    }

    #[test]
    fn a_previous_value_beyond_the_range_has_no_rate() {
        assert_eq!(per_second(150, 10, Wrap::At(100), SECOND), None);
    }

    proptest! {
        #[test]
        fn a_wrap_and_a_straight_run_agree(start in 0_u64..1_000_000, step in 0_u64..1_000_000) {
            let range = 1_000_000_u64;
            let after = (start + step) % range;
            let wrapped = per_second(start, after, Wrap::At(range), SECOND);
            let expected = if after >= start { after - start } else { range - start + after };
            #[expect(clippy::cast_precision_loss, reason = "test values are small")]
            let expected = expected as f64;
            prop_assert_eq!(wrapped, Some(expected));
        }
    }
}
