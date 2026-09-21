//! Statistics, the delivery curve, and its shape analysis.
//!
//! Phase 1's acceptance criterion is "the cumulative food returned must grow in
//! an S shape within 5000 ticks". That is a claim about a curve, so the code
//! has to produce the curve and measure it rather than assert it by eye:
//! [`analyze`] reports the milestones and an explicit sigmoid score, and
//! [`ascii_curve`] draws the thing in the terminal.
//!
//! With two colonies every counter is kept per colony as well as in total, so
//! "did colony A beat colony B?" is a question the data can answer.
//!
//! # Why `f64` here
//!
//! Per-cell pheromone and per-ant cargo are `f32`: they are read once and
//! thrown away, so precision is irrelevant to them. The *cumulative* totals are
//! different - they are running sums updated once per delivery for tens of
//! thousands of ticks, and adding 1.0 repeatedly into an `f32` sitting near
//! 9000 rounds at a spacing of ~0.001 per addition. A 5000-tick default run
//! drifted 0.013 units that way. `f64` costs nothing outside the hot loop and
//! makes the food-conservation invariant exact enough to be a real regression
//! test.

use bevy_ecs::prelude::*;
use std::fmt::Write as _;

use crate::grid::MAX_COLONIES;

/// One point on the delivery curve.
#[derive(Debug, Clone, Copy, Default)]
pub struct Sample {
    /// Tick the sample was taken at.
    pub tick: u64,
    /// Cumulative food delivered up to this tick, all colonies.
    pub delivered: f64,
    /// Cumulative food delivered by each colony.
    pub colony_delivered: [f64; MAX_COLONIES],
    /// Cumulative delivery count up to this tick.
    pub deliveries: u64,
    /// Food delivered since the previous sample.
    pub delta: f64,
    /// Ants currently searching.
    pub searching: u32,
    /// Ants currently returning.
    pub returning: u32,
    /// Food still on the map.
    pub food_remaining: f64,
    /// Total pheromone mass in the food fields, all colonies.
    pub food_pheromone: f64,
    /// Total pheromone mass in the home fields, all colonies.
    pub home_pheromone: f64,
}

impl Sample {
    /// One colony's cumulative delivery.
    #[inline]
    pub fn colony(&self, colony: usize) -> f64 {
        self.colony_delivered
            .get(colony)
            .copied()
            .unwrap_or_default()
    }
}

/// Live simulation statistics, updated once per tick.
#[derive(Resource, Debug, Clone, Default)]
pub struct SimStats {
    /// Ticks elapsed.
    pub tick: u64,
    /// Cumulative food returned to the nests, all colonies.
    pub delivered_total: f64,
    /// Cumulative food returned, per colony.
    pub colony_delivered: [f64; MAX_COLONIES],
    /// Number of successful round trips, all colonies.
    pub deliveries: u64,
    /// Successful round trips, per colony.
    pub colony_deliveries: [u64; MAX_COLONIES],
    /// Ants currently searching.
    pub searching: u32,
    /// Ants currently returning.
    pub returning: u32,
    /// Ants eaten by predators over the whole run.
    pub ants_eaten: u64,
    /// Food still on the map.
    pub food_remaining: f64,
    /// Food present at t=0.
    pub food_initial: f64,
    /// Food the player deleted by removing a pile.
    pub food_removed: f64,
    /// Tick of the very first delivery (`None` if the colony never found food).
    pub first_delivery_tick: Option<u64>,
    /// Tick of each colony's first delivery.
    pub colony_first_delivery: [Option<u64>; MAX_COLONIES],
    /// Peak delivery rate observed over a sampling window, in units per tick.
    pub peak_rate: f64,
    /// Tick at which `peak_rate` was observed.
    pub peak_rate_tick: u64,
    /// Total pheromone mass in the food fields (diagnostic).
    pub food_pheromone_total: f64,
    /// Total pheromone mass in the home fields (diagnostic).
    pub home_pheromone_total: f64,
    /// Downsampled delivery curve.
    pub samples: Vec<Sample>,
}

impl SimStats {
    /// Mean delivery rate over the whole run so far, in food units per tick.
    pub fn mean_rate(&self) -> f64 {
        if self.tick == 0 {
            0.0
        } else {
            self.delivered_total / self.tick as f64
        }
    }

    /// Fraction of the original food that has been recovered.
    pub fn recovery_fraction(&self) -> f64 {
        if self.food_initial <= 0.0 {
            0.0
        } else {
            (self.delivered_total / self.food_initial).clamp(0.0, 1.0)
        }
    }

    /// One colony's cumulative delivery.
    #[inline]
    pub fn colony_total(&self, colony: usize) -> f64 {
        self.colony_delivered
            .get(colony)
            .copied()
            .unwrap_or_default()
    }

    /// One colony's round-trip count.
    #[inline]
    pub fn colony_trips(&self, colony: usize) -> u64 {
        self.colony_deliveries
            .get(colony)
            .copied()
            .unwrap_or_default()
    }

    /// Book a delivery, crediting the colony that made it.
    pub fn record_delivery(&mut self, colony: usize, amount: f64, tick: u64) {
        let index = colony.min(MAX_COLONIES - 1);
        self.delivered_total += amount;
        self.deliveries += 1;
        self.colony_delivered[index] += amount;
        self.colony_deliveries[index] += 1;
        if self.first_delivery_tick.is_none() {
            self.first_delivery_tick = Some(tick);
        }
        if self.colony_first_delivery[index].is_none() {
            self.colony_first_delivery[index] = Some(tick);
        }
    }

    /// Drop all accumulated history, keeping the world as-is.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Record one curve point, called every `sample_interval` ticks.
    pub fn push_sample(&mut self, interval: u64) {
        let previous = self.samples.last().map(|s| s.delivered).unwrap_or(0.0);
        let delta = self.delivered_total - previous;
        let rate = if interval == 0 {
            0.0
        } else {
            delta / interval as f64
        };
        if rate > self.peak_rate {
            self.peak_rate = rate;
            self.peak_rate_tick = self.tick;
        }
        self.samples.push(Sample {
            tick: self.tick,
            delivered: self.delivered_total,
            colony_delivered: self.colony_delivered,
            deliveries: self.deliveries,
            delta,
            searching: self.searching,
            returning: self.returning,
            food_remaining: self.food_remaining,
            food_pheromone: self.food_pheromone_total,
            home_pheromone: self.home_pheromone_total,
        });
    }
}

/// Shape report for a delivery curve.
#[derive(Debug, Clone, Default)]
pub struct CurveReport {
    /// Final cumulative delivery.
    pub total: f64,
    /// Tick of the first delivery.
    pub t_first: Option<u64>,
    /// Tick at which 10% of `total` had been delivered.
    pub t10: Option<u64>,
    /// Tick at which 50% of `total` had been delivered.
    pub t50: Option<u64>,
    /// Tick at which 90% of `total` had been delivered.
    pub t90: Option<u64>,
    /// Tick of the highest delivery rate.
    pub t_peak_rate: Option<u64>,
    /// Mean rate over the run.
    pub mean_rate: f64,
    /// Highest sampling-window gain observed.
    pub peak_rate: f64,
    /// `peak_window_gain / mean_window_gain`.
    ///
    /// This is the honest test for "S shaped". A straight line accumulates at
    /// a constant rate, so its peak equals its mean and the score is 1.0. A
    /// sigmoid has a slow lag phase, a steep middle and a plateau, so its peak
    /// rate is a multiple of its mean. >= 1.5 reads as clearly sigmoid,
    /// >= 2.0 as a strong S.
    ///
    /// Caveat, measured rather than assumed: a colony with *no pheromones at
    /// all* also scores above 1.5 on this metric, because ants spreading out
    /// from a single nest cover more ground every tick. The score detects
    /// "accelerating", not "self-organising". See
    /// `tests/phase1_acceptance.rs::pheromone_trails_measurably_speed_up_foraging`
    /// for the controlled comparison that isolates the pheromone effect.
    pub sigmoid_score: f64,
    /// Whether the curve was still climbing at the end of the window.
    pub still_rising: bool,
    /// Fraction of the original food recovered, derived as
    /// `delivered / (delivered + still on the map)`.
    pub recovery_fraction: f64,
}

impl CurveReport {
    /// Human-readable verdict on the S-shape criterion.
    pub fn verdict(&self) -> &'static str {
        if self.total <= 0.0 {
            "NO FORAGING - the colony never delivered any food"
        } else if self.sigmoid_score >= 2.0 {
            "STRONG S-CURVE - clear lag, steep growth phase, plateau"
        } else if self.sigmoid_score >= 1.5 {
            "S-CURVE - growth clearly accelerates then flattens"
        } else if self.sigmoid_score >= 1.15 {
            "WEAK S - some acceleration, mostly steady accumulation"
        } else {
            "LINEAR - constant rate, no positive feedback visible"
        }
    }
}

/// Reduce a delivery curve to milestone ticks plus the sigmoid score.
pub fn analyze(samples: &[Sample]) -> CurveReport {
    analyze_series(samples, |s| s.delivered)
}

/// [`analyze`] over an arbitrary series extracted from the samples.
///
/// Used to run the identical milestone-and-sigmoid analysis on one colony's
/// curve, so a two-colony contest can be reported with the same vocabulary as
/// a single-colony run.
pub fn analyze_series(samples: &[Sample], pick: impl Fn(&Sample) -> f64) -> CurveReport {
    let mut report = CurveReport::default();
    let Some(last) = samples.last().copied() else {
        return report;
    };
    let total = pick(&last);
    report.total = total;

    // Recovery is exact without needing the initial total: every unit
    // delivered was taken off the map, so initial = delivered + remaining.
    let initial = last.delivered + last.food_remaining;
    report.recovery_fraction = if initial > 0.0 {
        (last.delivered / initial).clamp(0.0, 1.0)
    } else {
        0.0
    };

    if total <= 0.0 {
        return report;
    }
    let ticks = last.tick.max(1);
    report.mean_rate = total / ticks as f64;

    report.t_first = samples.iter().find(|s| pick(s) > 0.0).map(|s| s.tick);
    let milestone = |frac: f64| -> Option<u64> {
        let target = total * frac;
        samples.iter().find(|s| pick(s) >= target).map(|s| s.tick)
    };
    report.t10 = milestone(0.10);
    report.t50 = milestone(0.50);
    report.t90 = milestone(0.90);

    // Peak window gain. Every sampling window is the same length, so comparing
    // a series' first difference compares rates; convert the window length back
    // to ticks to make the ratio against `mean_rate` unit-free.
    let window_len = if samples.len() >= 2 {
        let span = samples[samples.len() - 1]
            .tick
            .saturating_sub(samples[0].tick) as f64;
        (span / (samples.len() - 1) as f64).max(1.0)
    } else {
        1.0
    };
    let mut previous = pick(&samples[0]);
    let mut peak_gain = 0.0f64;
    let mut peak_tick = None;
    for sample in samples.iter().skip(1) {
        let value = pick(sample);
        let gain = value - previous;
        if gain > peak_gain {
            peak_gain = gain;
            peak_tick = Some(sample.tick);
        }
        previous = value;
    }
    if peak_gain > 0.0 {
        report.peak_rate = peak_gain;
        report.t_peak_rate = peak_tick;
        let mean_per_window = report.mean_rate * window_len;
        if mean_per_window > 0.0 {
            report.sigmoid_score = report.peak_rate / mean_per_window;
        }
    }

    // "Still rising" = the final quarter added at least half as much as the
    // middle quarter did.
    let n = samples.len();
    if n >= 8 {
        let quarter = (n / 4).max(1);
        let mid_start = pick(&samples[n / 2 - quarter / 2]);
        let mid_end = pick(&samples[n / 2 + quarter / 2]);
        let tail_start = pick(&samples[n - 1 - quarter]);
        let mid_gain = mid_end - mid_start;
        let tail_gain = total - tail_start;
        report.still_rising = mid_gain > 1e-9 && tail_gain > mid_gain * 0.5;
    }

    report
}

/// Render the delivery curve as a filled ASCII area chart.
///
/// Terminal-friendly on purpose: pure ASCII so it renders identically in
/// `cmd.exe`, Windows Terminal, and a CI log.
pub fn ascii_curve(samples: &[Sample], width: usize, height: usize) -> String {
    ascii_series(samples, width, height, |s| s.delivered)
}

/// [`ascii_curve`] over an arbitrary series.
pub fn ascii_series(
    samples: &[Sample],
    width: usize,
    height: usize,
    pick: impl Fn(&Sample) -> f64,
) -> String {
    let width = width.max(20);
    let height = height.max(4);
    if samples.is_empty() {
        return "(no samples recorded)\n".to_string();
    }

    let max_y = samples
        .iter()
        .map(&pick)
        .fold(0.0f64, f64::max)
        .max(1.0);
    let last_tick = samples.last().map(|s| s.tick).unwrap_or(0);

    // Resample the (possibly much longer) sample list onto `width` columns.
    let mut columns = vec![0.0f64; width];
    for (i, col) in columns.iter_mut().enumerate() {
        let t = i as f64 / (width - 1).max(1) as f64;
        let idx = (t * (samples.len() - 1) as f64).round() as usize;
        *col = pick(&samples[idx.min(samples.len() - 1)]);
    }

    let mut grid = vec![vec![' '; width]; height];
    for (x, value) in columns.iter().enumerate() {
        let norm = (value / max_y).clamp(0.0, 1.0);
        let top = ((1.0 - norm) * (height - 1) as f64).round() as usize;
        for row in grid.iter_mut().skip(top.min(height - 1)) {
            row[x] = '#';
        }
    }

    const LABEL: usize = 9;
    let mut out = String::new();
    for (row, line) in grid.iter().enumerate() {
        let label = if row == 0 {
            let top = format!("{max_y:.0}");
            format!("{top:>LABEL$}")
        } else if row == height - 1 {
            format!("{:>LABEL$}", "0")
        } else {
            " ".repeat(LABEL)
        };
        let _ = writeln!(out, "{label} |{}", line.iter().collect::<String>());
    }
    let _ = writeln!(out, "{} +{}", " ".repeat(LABEL), "-".repeat(width));
    let _ = writeln!(
        out,
        "{}  tick 0{}tick {}",
        " ".repeat(LABEL),
        " ".repeat(width.saturating_sub(18)),
        last_tick
    );
    out
}

/// Render a compact sparkline (no axes) - handy for sweeping many runs.
pub fn sparkline(values: &[f64], width: usize) -> String {
    const RAMP: [char; 8] = ['_', '.', '-', '=', '+', '*', '#', '@'];
    if values.is_empty() || width == 0 {
        return String::new();
    }
    let max = values.iter().copied().fold(0.0f64, f64::max).max(1e-9);
    (0..width)
        .map(|i| {
            let idx = (i as f64 / (width.max(2) - 1) as f64 * (values.len() - 1) as f64).round()
                as usize;
            let v = values[idx.min(values.len() - 1)] / max;
            RAMP[((v.clamp(0.0, 1.0)) * (RAMP.len() - 1) as f64).round() as usize]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Constant-rate accumulation over 200 samples.
    fn linear(n: usize, total: f64) -> Vec<Sample> {
        (1..=n)
            .map(|i| {
                let d = total * i as f64 / n as f64;
                Sample {
                    tick: i as u64 * 10,
                    delivered: d,
                    delta: total / n as f64,
                    food_remaining: total - d,
                    ..Default::default()
                }
            })
            .collect()
    }

    /// Logistic accumulation over 200 samples.
    fn sigmoid(n: usize, total: f64) -> Vec<Sample> {
        let s0 = 1.0 / (1.0 + (0.5 * 12.0f64).exp());
        let s1 = 1.0 / (1.0 + (-0.5 * 12.0f64).exp());
        let mut prev = 0.0;
        (1..=n)
            .map(|i| {
                let x = i as f64 / n as f64;
                let s = 1.0 / (1.0 + (-(x - 0.5) * 12.0).exp());
                let d = total * (s - s0) / (s1 - s0);
                let sample = Sample {
                    tick: i as u64 * 10,
                    delivered: d,
                    delta: d - prev,
                    food_remaining: total - d,
                    ..Default::default()
                };
                prev = d;
                sample
            })
            .collect()
    }

    #[test]
    fn linear_curve_scores_about_one() {
        let report = analyze(&linear(200, 5000.0));
        assert!(
            report.sigmoid_score < 1.15,
            "linear scored {}",
            report.sigmoid_score
        );
        assert!(report.verdict().starts_with("LINEAR"));
    }

    #[test]
    fn sigmoid_curve_scores_high() {
        let report = analyze(&sigmoid(200, 5000.0));
        assert!(
            report.sigmoid_score >= 1.5,
            "sigmoid scored {}",
            report.sigmoid_score
        );
    }

    #[test]
    fn sigmoid_milestones_sit_closer_together_than_linear_ones() {
        let s = analyze(&sigmoid(200, 1000.0));
        let l = analyze(&linear(200, 1000.0));
        let s_spread = s.t90.unwrap() - s.t10.unwrap();
        let l_spread = l.t90.unwrap() - l.t10.unwrap();
        assert!(
            s_spread < l_spread,
            "sigmoid spread {s_spread} should beat linear {l_spread}"
        );
    }

    #[test]
    fn milestones_are_ordered() {
        let report = analyze(&sigmoid(200, 1000.0));
        let (t10, t50, t90) = (report.t10.unwrap(), report.t50.unwrap(), report.t90.unwrap());
        assert!(t10 <= t50 && t50 <= t90, "{t10} {t50} {t90}");
        assert!(report.t_first.unwrap() <= t10);
    }

    #[test]
    fn recovery_fraction_is_derived_without_the_initial_total() {
        let report = analyze(&linear(200, 750.0));
        assert!((report.recovery_fraction - 1.0).abs() < 1e-9);
    }

    #[test]
    fn empty_and_zero_curves_do_not_panic() {
        let report = analyze(&[]);
        assert_eq!(report.total, 0.0);
        assert!(report.verdict().starts_with("NO FORAGING"));
        assert!(ascii_curve(&[], 40, 8).contains("no samples"));

        let flat = vec![Sample::default(); 10];
        assert!(analyze(&flat).verdict().starts_with("NO FORAGING"));
    }

    #[test]
    fn ascii_curve_is_rectangular() {
        let chart = ascii_curve(&sigmoid(50, 100.0), 40, 10);
        let lines: Vec<&str> = chart.lines().collect();
        assert_eq!(lines.len(), 12, "got {} lines", lines.len());
        assert!(lines[0].contains('#'));
    }

    #[test]
    fn sparkline_has_requested_width() {
        assert_eq!(sparkline(&[0.0, 1.0, 2.0, 3.0], 20).chars().count(), 20);
        assert_eq!(sparkline(&[], 20), "");
    }

    // ---- per-colony accounting ------------------------------------------

    #[test]
    fn deliveries_are_credited_to_the_right_colony() {
        let mut stats = SimStats::default();
        stats.record_delivery(0, 1.0, 10);
        stats.record_delivery(1, 1.0, 11);
        stats.record_delivery(1, 1.0, 12);

        assert_eq!(stats.delivered_total, 3.0);
        assert_eq!(stats.deliveries, 3);
        assert_eq!(stats.colony_total(0), 1.0);
        assert_eq!(stats.colony_total(1), 2.0);
        assert_eq!(stats.colony_trips(0), 1);
        assert_eq!(stats.colony_trips(1), 2);

        // Global and per-colony first-delivery ticks.
        assert_eq!(stats.first_delivery_tick, Some(10));
        assert_eq!(stats.colony_first_delivery[0], Some(10));
        assert_eq!(stats.colony_first_delivery[1], Some(11));
    }

    #[test]
    fn an_out_of_range_colony_index_is_clamped_not_dropped() {
        let mut stats = SimStats::default();
        stats.record_delivery(99, 5.0, 1);
        assert_eq!(stats.delivered_total, 5.0, "the delivery vanished");
        assert_eq!(stats.colony_total(MAX_COLONIES - 1), 5.0);
    }

    #[test]
    fn samples_carry_the_per_colony_split() {
        let mut stats = SimStats::default();
        stats.record_delivery(0, 2.0, 1);
        stats.record_delivery(1, 3.0, 1);
        stats.push_sample(1);

        let sample = stats.samples.last().unwrap();
        assert_eq!(sample.delivered, 5.0);
        assert_eq!(sample.colony(0), 2.0);
        assert_eq!(sample.colony(1), 3.0);
        assert_eq!(sample.colony(7), 0.0, "out-of-range read must not panic");
    }

    #[test]
    fn per_colony_series_can_be_analysed_independently() {
        let samples: Vec<Sample> = (1..=200)
            .map(|i| {
                let t = i as f64;
                Sample {
                    tick: i as u64 * 10,
                    delivered: t * 2.5,
                    colony_delivered: [t * 2.0, t * 0.5],
                    food_remaining: 10_000.0,
                    ..Default::default()
                }
            })
            .collect();

        let a = analyze_series(&samples, |s| s.colony(0));
        let b = analyze_series(&samples, |s| s.colony(1));
        assert_eq!(a.total, 400.0);
        assert_eq!(b.total, 100.0);
        assert!(a.total > b.total);
        // Both are linear, so both should score near 1.
        assert!(a.sigmoid_score < 1.15);
        assert!(b.sigmoid_score < 1.15);
    }
}
