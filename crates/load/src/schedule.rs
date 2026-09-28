//! Pure scheduling math: the stage profile (target over time), arrival times
//! for the open model, and the smooth weighted round-robin target picker.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use zorvik_formats::LoadStage;

/// Stage targets as a piecewise-linear function of time. The first stage
/// ramps from 0; a 0-second stage jumps to its target.
#[derive(Debug, Clone)]
pub(crate) struct Profile {
    segments: Vec<Segment>,
    end: f64,
    last: f64,
    duration: Duration,
}

#[derive(Debug, Clone, Copy)]
struct Segment {
    start: f64,
    len: f64,
    from: f64,
    to: f64,
}

impl Profile {
    pub(crate) fn new(stages: &[LoadStage]) -> Self {
        let (mut t, mut level) = (0.0, 0.0);
        let mut segments = Vec::with_capacity(stages.len());
        for stage in stages {
            let (len, to) = (f64::from(stage.duration_secs), f64::from(stage.target));
            segments.push(Segment { start: t, len, from: level, to });
            t += len;
            level = to;
        }
        let secs = stages.iter().map(|s| u64::from(s.duration_secs)).sum();
        Self { segments, end: t, last: level, duration: Duration::from_secs(secs) }
    }

    /// Planned length of the run.
    pub(crate) fn duration(&self) -> Duration {
        self.duration
    }

    /// Users or requests per second wanted `t` seconds into the run.
    pub(crate) fn target_at(&self, t: f64) -> f64 {
        if t >= self.end {
            return self.last;
        }
        self.segments
            .iter()
            .find(|s| t < s.start + s.len)
            .map(|s| if t <= s.start { s.from } else { s.from + (s.to - s.from) * (t - s.start) / s.len })
            .unwrap_or(self.last)
    }

    /// Average target between `a` and `b` seconds (`a < b`): what a request
    /// rate over that window should come to.
    pub(crate) fn mean(&self, a: f64, b: f64) -> f64 {
        (self.integral(b) - self.integral(a)) / (b - a)
    }

    /// Area under the target from 0 to `t`.
    fn integral(&self, t: f64) -> f64 {
        let mut area = 0.0;
        for s in &self.segments {
            if t <= s.start {
                return area;
            }
            let tau = (t - s.start).min(s.len);
            if tau > 0.0 {
                area += s.from * tau + (s.to - s.from) / s.len * tau * tau / 2.0;
            }
        }
        area + self.last * (t - self.end).max(0.0)
    }

    /// Open model: start times (seconds since the start) of every request, in
    /// order. The k-th request (from 0) starts when the integral of the rate
    /// reaches k + ½, so a constant 200/s for 3 s gives exactly 600 evenly
    /// spaced requests and a ramp speeds up smoothly.
    pub(crate) fn arrivals(&self) -> Arrivals {
        Arrivals { segments: self.segments.clone(), segment: 0, base: 0.0, next: 0 }
    }
}

pub(crate) struct Arrivals {
    segments: Vec<Segment>,
    segment: usize,
    /// Requests due before the current segment (fractional).
    base: f64,
    next: u64,
}

impl Iterator for Arrivals {
    type Item = f64;

    fn next(&mut self) -> Option<f64> {
        loop {
            let s = *self.segments.get(self.segment)?;
            let count = (s.from + s.to) / 2.0 * s.len;
            // Requests into this segment when the next one is due (> 0 by construction).
            let m = self.next as f64 + 0.5 - self.base;
            if m <= count {
                // Solve from·τ + slope·τ²/2 = m; this form is stable for slope → 0.
                let slope = (s.to - s.from) / s.len;
                let root = (s.from * s.from + 2.0 * slope * m).max(0.0).sqrt();
                let tau = (2.0 * m / (s.from + root)).clamp(0.0, s.len);
                self.next += 1;
                return Some(s.start + tau);
            }
            self.base += count;
            self.segment += 1;
        }
    }
}

/// Picks targets in proportion to their weights, interleaved (nginx's smooth
/// weighted round-robin): weights 3:1 give A A B A, not A A A B. The order of
/// one cycle is computed up front, so picking is a single atomic increment.
pub(crate) struct Picker {
    order: Vec<u32>,
    next: AtomicUsize,
}

/// Longest precomputed cycle; larger weight sums are scaled down to it.
const MAX_CYCLE: u64 = 10_000;

impl Picker {
    /// `weights` must contain at least one non-zero weight (checked by `validate`).
    pub(crate) fn new(weights: &[u32]) -> Self {
        let g = weights.iter().copied().filter(|w| *w > 0).fold(0, gcd);
        let mut w: Vec<u64> = weights.iter().map(|&x| u64::from(x.checked_div(g).unwrap_or(0))).collect();
        let total: u64 = w.iter().sum();
        if total > MAX_CYCLE {
            for x in w.iter_mut().filter(|x| **x > 0) {
                *x = ((*x as f64 * MAX_CYCLE as f64 / total as f64).round() as u64).max(1);
            }
        }
        let total: u64 = w.iter().sum();
        let mut current = vec![0i64; w.len()];
        let mut order = Vec::with_capacity(total as usize);
        for _ in 0..total {
            let mut best = 0;
            for i in 0..w.len() {
                current[i] += w[i] as i64;
                if current[i] > current[best] {
                    best = i;
                }
            }
            current[best] -= total as i64;
            order.push(best as u32);
        }
        if order.is_empty() {
            order.push(0);
        }
        Self { order, next: AtomicUsize::new(0) }
    }

    pub(crate) fn next(&self) -> usize {
        self.at(self.next.fetch_add(1, Ordering::Relaxed))
    }

    /// The `i`-th pick of the order (a user going through it on its own).
    pub(crate) fn at(&self, i: usize) -> usize {
        self.order[i % self.order.len()] as usize
    }
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stages(list: &[(u32, u32)]) -> Vec<LoadStage> {
        list.iter().map(|&(duration_secs, target)| LoadStage { duration_secs, target }).collect()
    }

    #[test]
    fn target_interpolates_from_zero_and_jumps_on_zero_length_stages() {
        let p = Profile::new(&stages(&[(10, 10), (40, 10), (10, 0)]));
        assert_eq!(p.duration(), Duration::from_secs(60));
        assert_eq!(p.target_at(0.0), 0.0);
        assert_eq!(p.target_at(5.0), 5.0);
        assert_eq!(p.target_at(30.0), 10.0);
        assert_eq!(p.target_at(55.0), 5.0);
        assert_eq!(p.target_at(60.0), 0.0);
        let jump = Profile::new(&stages(&[(0, 200), (3, 200)]));
        assert_eq!(jump.target_at(0.0), 200.0);
        assert_eq!(jump.target_at(2.9), 200.0);
        assert_eq!(p.mean(0.0, 1.0), 0.5);
        assert_eq!(p.mean(9.0, 11.0), 9.75);
        assert_eq!(p.mean(59.0, 60.0), 0.5);
        assert_eq!(jump.mean(0.0, 1.0), 200.0);
    }

    #[test]
    fn arrivals_match_the_rate_integral() {
        let constant: Vec<f64> = Profile::new(&stages(&[(0, 200), (3, 200)])).arrivals().collect();
        assert_eq!(constant.len(), 600);
        assert!((constant[0] - 0.0025).abs() < 1e-9 && (constant[1] - constant[0] - 0.005).abs() < 1e-9);
        assert!(constant.windows(2).all(|w| w[0] < w[1]) && *constant.last().unwrap() < 3.0);

        // Ramp 0 → 100/s over 10 s: 500 requests, a quarter of them in the first half.
        let ramp: Vec<f64> = Profile::new(&stages(&[(10, 100)])).arrivals().collect();
        assert_eq!(ramp.len(), 500);
        assert_eq!(ramp.iter().filter(|t| **t < 5.0).count(), 125);
        // Ramp down and a pause.
        let down: Vec<f64> = Profile::new(&stages(&[(0, 100), (2, 0), (5, 0), (1, 10)])).arrivals().collect();
        assert_eq!(down.len(), 105);
        assert!(down[..100].iter().all(|t| *t <= 2.0) && down[100..].iter().all(|t| *t >= 7.0));
    }

    #[test]
    fn picker_interleaves_by_weight() {
        let p = Picker::new(&[3, 1]);
        let picks: Vec<usize> = (0..8).map(|_| p.next()).collect();
        assert_eq!(picks, [0, 0, 1, 0, 0, 0, 1, 0]);
        let p = Picker::new(&[0, 2, 2]);
        assert!((0..10).map(|_| p.next()).all(|i| i != 0));
        let p = Picker::new(&[2, 1]);
        assert_eq!((0..6).map(|i| p.at(i)).collect::<Vec<_>>(), [0, 1, 0, 0, 1, 0]);
        let big = Picker::new(&[u32::MAX, 1]);
        assert!(big.order.len() <= MAX_CYCLE as usize + 1 && big.order.contains(&1));
    }
}
