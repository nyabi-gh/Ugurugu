// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

//! Input-to-present latency: from the input's own timestamp until the present
//! call that first shows it returns. Not pen-to-photon.

#[derive(Default)]
pub struct LatencyLog {
    millis: Vec<f64>,
}

impl LatencyLog {
    pub fn record(&mut self, seconds: f64) {
        self.millis.push(seconds * 1000.0);
    }

    pub fn summary(&self) -> Option<String> {
        if self.millis.is_empty() {
            return None;
        }
        let mut sorted = self.millis.clone();
        sorted.sort_by(f64::total_cmp);
        let rank = |fraction: f64| {
            let index = (fraction * (sorted.len() - 1) as f64).round() as usize;
            sorted[index.min(sorted.len() - 1)]
        };
        Some(format!(
            "frames={} p50={:.2}ms p95={:.2}ms max={:.2}ms",
            sorted.len(),
            rank(0.5),
            rank(0.95),
            sorted[sorted.len() - 1]
        ))
    }
}
