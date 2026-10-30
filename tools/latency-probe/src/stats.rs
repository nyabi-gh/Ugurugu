// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

pub struct Lcg(pub u64);

impl Lcg {
    pub fn next_unit(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

pub fn summary(name: &str, mut values: Vec<f64>) {
    if values.is_empty() {
        println!("{name}: no samples");
        return;
    }
    values.sort_by(f64::total_cmp);
    let rank = |fraction: f64| values[(fraction * (values.len() - 1) as f64).round() as usize];
    println!(
        "{name}: n={} p50={:.2}ms p95={:.2}ms max={:.2}ms min={:.2}ms",
        values.len(),
        rank(0.5),
        rank(0.95),
        values[values.len() - 1],
        values[0],
    );
}
