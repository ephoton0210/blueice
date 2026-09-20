// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Throughput and ETA: a time-weighted exponential moving average, with
//! the clock passed in so it is unit-testable without sleeping.

use std::time::{Duration, Instant};

/// How quickly the average follows the instantaneous rate. Long enough
/// that a per-tick burst doesn't make the displayed speed jump around,
/// short enough that a stall shows up within a few seconds.
const TIME_CONSTANT_SECS: f64 = 1.0;

/// Smoothed transfer speed, fed with `(time, bytes completed so far)`
/// samples.
#[derive(Debug, Default)]
pub struct Progress {
    last: Option<(Instant, u64)>,
    speed: f64,
}

impl Progress {
    pub fn new() -> Self {
        Progress::default()
    }

    /// Records that `completed` bytes were done at `now`. The first sample
    /// only sets the baseline. A sample at the same instant is ignored (no
    /// elapsed time to divide by), and a `completed` lower than the last
    /// one -- a single-stream transfer restarting from byte 0 -- counts as
    /// zero progress rather than underflowing.
    pub fn record(&mut self, now: Instant, completed: u64) {
        let Some((then, before)) = self.last else {
            self.last = Some((now, completed));
            return;
        };
        let dt = now.saturating_duration_since(then).as_secs_f64();
        if dt <= 0.0 {
            return;
        }
        let instantaneous = completed.saturating_sub(before) as f64 / dt;
        let alpha = 1.0 - (-dt / TIME_CONSTANT_SECS).exp();
        self.speed += alpha * (instantaneous - self.speed);
        self.last = Some((now, completed));
    }

    pub fn speed_bps(&self) -> u64 {
        self.speed.round() as u64
    }

    /// Time to move `remaining` bytes at the current speed. Nothing left
    /// is zero; a speed under one byte a second is "not moving", for which
    /// no estimate is more honest than a huge one.
    pub fn eta(&self, remaining: u64) -> Option<Duration> {
        if remaining == 0 {
            return Some(Duration::ZERO);
        }
        if self.speed < 1.0 {
            return None;
        }
        Some(Duration::from_secs_f64(remaining as f64 / self.speed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// Feeds `rate` bytes/second, sampled every `step`, for `duration`.
    fn feed(progress: &mut Progress, start: Instant, from: Duration, duration: Duration, step: Duration, rate: u64, base: u64) -> u64 {
        let mut completed = base;
        let mut t = Duration::ZERO;
        while t < duration {
            t += step;
            completed += (rate as f64 * step.as_secs_f64()) as u64;
            progress.record(start + from + t, completed);
        }
        completed
    }

    #[test]
    fn before_any_progress_there_is_no_speed_and_no_eta() {
        let progress = Progress::new();
        assert_eq!(progress.speed_bps(), 0);
        assert_eq!(progress.eta(1_000), None);
    }

    #[test]
    fn the_first_sample_only_sets_a_baseline() {
        let start = Instant::now();
        let mut progress = Progress::new();
        progress.record(start, 5_000);
        assert_eq!(progress.speed_bps(), 0, "one sample can't say how fast anything is moving");
    }

    #[test]
    fn a_steady_rate_converges_to_that_rate() {
        let start = Instant::now();
        let mut progress = Progress::new();
        progress.record(start, 0);
        feed(&mut progress, start, Duration::ZERO, Duration::from_secs(10), Duration::from_millis(100), 1_000, 0);
        let speed = progress.speed_bps();
        assert!((980..=1_020).contains(&speed), "speed was {speed}");
    }

    #[test]
    fn eta_is_the_remaining_bytes_over_the_smoothed_speed() {
        let start = Instant::now();
        let mut progress = Progress::new();
        progress.record(start, 0);
        feed(&mut progress, start, Duration::ZERO, Duration::from_secs(10), Duration::from_millis(100), 1_000, 0);
        let eta = progress.eta(5_000).unwrap();
        assert!(eta > Duration::from_millis(4_800) && eta < Duration::from_millis(5_200), "eta was {eta:?}");
        assert_eq!(progress.eta(0), Some(Duration::ZERO));
    }

    #[test]
    fn speed_falls_when_the_transfer_stalls_and_the_eta_disappears() {
        let start = Instant::now();
        let mut progress = Progress::new();
        progress.record(start, 0);
        let completed = feed(&mut progress, start, Duration::ZERO, Duration::from_secs(10), Duration::from_millis(100), 1_000, 0);
        feed(&mut progress, start, Duration::from_secs(10), Duration::from_secs(3), Duration::from_millis(100), 0, completed);
        assert!(progress.speed_bps() < 100, "speed was {}", progress.speed_bps());
        feed(&mut progress, start, Duration::from_secs(13), Duration::from_secs(60), Duration::from_millis(100), 0, completed);
        assert_eq!(progress.speed_bps(), 0);
        assert_eq!(progress.eta(1_000), None, "no ETA is honest when nothing is moving");
    }

    #[test]
    fn progress_going_backwards_is_zero_progress_not_an_underflow() {
        // A single-stream transfer that restarts from byte 0 reports a
        // smaller completed count than before.
        let start = Instant::now();
        let mut progress = Progress::new();
        progress.record(start, 0);
        progress.record(start + Duration::from_secs(1), 1_000);
        progress.record(start + Duration::from_secs(2), 0);
        let after_restart = progress.speed_bps();
        assert!(after_restart < 1_000);
        progress.record(start + Duration::from_secs(3), 500);
        assert!(progress.speed_bps() > 0, "measuring resumes from the new, lower baseline");
    }

    #[test]
    fn two_samples_at_the_same_instant_are_ignored() {
        let start = Instant::now();
        let mut progress = Progress::new();
        progress.record(start, 0);
        progress.record(start, 10_000);
        assert_eq!(progress.speed_bps(), 0);
        progress.record(start + Duration::from_secs(1), 10_000);
        assert!(progress.speed_bps() > 0, "the ignored sample must not have advanced the baseline's clock incorrectly");
    }
}
