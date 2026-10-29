use std::collections::{HashMap, VecDeque};

#[derive(Debug)]
pub struct History {
    depth: usize,
    series: HashMap<String, VecDeque<f64>>,
}

impl History {
    pub fn new(depth: usize) -> Self {
        Self {
            depth,
            series: HashMap::new(),
        }
    }

    pub fn record(&mut self, key: &str, value: f64) {
        if self.depth == 0 {
            return;
        }
        let series = self
            .series
            .entry(key.to_owned())
            .or_insert_with(|| VecDeque::with_capacity(self.depth));
        if series.len() == self.depth {
            series.pop_front();
        }
        series.push_back(value);
    }

    pub fn series(&self, key: &str) -> Option<&VecDeque<f64>> {
        self.series.get(key)
    }

    pub fn retain(&mut self, live: impl Fn(&str) -> bool) {
        self.series.retain(|key, _| live(key));
    }
}

#[cfg(test)]
mod tests {
    use super::History;

    #[test]
    fn an_unrecorded_key_has_no_series() {
        assert!(History::new(3).series("x").is_none());
    }

    #[test]
    fn a_zero_depth_history_keeps_nothing() {
        let mut history = History::new(0);
        history.record("x", 1.0);
        assert!(history.series("x").is_none());
    }

    #[test]
    fn one_value_is_kept() {
        let mut history = History::new(3);
        history.record("x", 1.0);
        assert_eq!(
            history
                .series("x")
                .unwrap()
                .iter()
                .copied()
                .collect::<Vec<f64>>(),
            [1.0]
        );
    }

    #[test]
    fn the_oldest_value_falls_off_at_depth() {
        let mut history = History::new(2);
        history.record("x", 1.0);
        history.record("x", 2.0);
        history.record("x", 3.0);
        assert_eq!(
            history
                .series("x")
                .unwrap()
                .iter()
                .copied()
                .collect::<Vec<f64>>(),
            [2.0, 3.0]
        );
    }

    #[test]
    fn retain_drops_dead_keys() {
        let mut history = History::new(2);
        history.record("live", 1.0);
        history.record("dead", 1.0);
        history.retain(|key| key == "live");
        assert!(history.series("dead").is_none() && history.series("live").is_some());
    }
}
