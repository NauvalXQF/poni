use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimerKind {
    Countdown,
    Stopwatch,
}

impl TimerKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Countdown => "Timer",
            Self::Stopwatch => "Stopwatch",
        }
    }
}

/// A countdown or stopwatch driven entirely by the monotonic clock.
///
/// Elapsed time is accumulated only when the timer is paused. While running it
/// is derived from the start instant, so the timer stays accurate without a
/// tick; the renderer simply reads `display()` and `progress()` with
/// `Instant::now()`.
#[derive(Clone, Debug)]
pub struct Timer {
    kind: TimerKind,
    duration: Duration,
    accumulated: Duration,
    started_at: Option<Instant>,
}

impl Timer {
    pub fn countdown(duration: Duration) -> Self {
        Self {
            kind: TimerKind::Countdown,
            duration,
            accumulated: Duration::ZERO,
            started_at: Some(Instant::now()),
        }
    }

    pub fn stopwatch() -> Self {
        Self {
            kind: TimerKind::Stopwatch,
            duration: Duration::ZERO,
            accumulated: Duration::ZERO,
            started_at: Some(Instant::now()),
        }
    }

    pub fn kind(&self) -> TimerKind {
        self.kind
    }

    pub fn is_running(&self) -> bool {
        self.started_at.is_some()
    }

    pub fn elapsed(&self, now: Instant) -> Duration {
        let live = self
            .started_at
            .map(|started| now.saturating_duration_since(started))
            .unwrap_or_default();
        self.accumulated + live
    }

    pub fn remaining(&self, now: Instant) -> Duration {
        self.duration.saturating_sub(self.elapsed(now))
    }

    /// Fraction of a countdown still left, in the 0..=1 range. Stopwatches
    /// report 0.0 because they have no fixed horizon.
    pub fn progress(&self, now: Instant) -> f32 {
        if self.kind != TimerKind::Countdown || self.duration.is_zero() {
            return 0.0;
        }
        (self.remaining(now).as_secs_f32() / self.duration.as_secs_f32()).clamp(0.0, 1.0)
    }

    pub fn finished(&self, now: Instant) -> bool {
        self.kind == TimerKind::Countdown && self.remaining(now).is_zero()
    }

    /// `MM:SS`, or `H:MM:SS` once an hour has accumulated. Countdowns round
    /// up so a fresh 5-minute timer reads 5:00 for its first full second.
    pub fn display(&self, now: Instant) -> String {
        let seconds = match self.kind {
            TimerKind::Countdown => {
                let remaining = self.remaining(now).as_nanos();
                (remaining.saturating_add(999_999_999) / 1_000_000_000) as u64
            }
            TimerKind::Stopwatch => self.elapsed(now).as_secs(),
        };
        format_seconds(seconds)
    }

    pub fn pause(&mut self, now: Instant) {
        if let Some(started) = self.started_at.take() {
            self.accumulated += now.saturating_duration_since(started);
            if self.kind == TimerKind::Countdown {
                self.accumulated = self.accumulated.min(self.duration);
            }
        }
    }

    pub fn resume(&mut self, now: Instant) {
        if self.started_at.is_none() && !self.finished(now) {
            self.started_at = Some(now);
        }
    }

    pub fn toggle(&mut self, now: Instant) {
        if self.is_running() {
            self.pause(now);
        } else {
            self.resume(now);
        }
    }
}

fn format_seconds(total: u64) -> String {
    if total >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            total / 3600,
            (total / 60) % 60,
            total % 60
        )
    } else {
        format!("{}:{:02}", total / 60, total % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn countdown_rounds_up_until_the_first_second_passes() {
        let timer = Timer::countdown(Duration::from_secs(300));
        let now = Instant::now();
        assert_eq!(timer.display(now), "5:00");
        assert_eq!(timer.display(now + Duration::from_millis(999)), "5:00");
        assert_eq!(timer.display(now + Duration::from_millis(1_000)), "4:59");
        assert!((timer.progress(now) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn pause_freezes_and_resume_continues() {
        let mut timer = Timer::countdown(Duration::from_secs(60));
        let start = Instant::now();
        assert!(timer.is_running());
        timer.pause(start + Duration::from_secs(10));
        assert!(!timer.is_running());
        assert_eq!(timer.display(start + Duration::from_secs(30)), "0:50");
        timer.resume(start + Duration::from_secs(30));
        assert_eq!(timer.display(start + Duration::from_secs(40)), "0:40");
    }

    #[test]
    fn countdown_finishes_at_zero_and_cannot_resume() {
        let mut timer = Timer::countdown(Duration::from_secs(5));
        let start = Instant::now();
        let end = start + Duration::from_secs(5);
        assert!(timer.finished(end));
        assert_eq!(timer.display(end), "0:00");
        assert_eq!(timer.progress(end), 0.0);
        timer.pause(end);
        timer.resume(end + Duration::from_secs(1));
        assert!(!timer.is_running());
    }

    #[test]
    fn stopwatch_counts_up_and_wraps_into_hours() {
        let timer = Timer::stopwatch();
        let now = Instant::now();
        assert_eq!(timer.display(now), "0:00");
        assert_eq!(timer.display(now + Duration::from_secs(65)), "1:05");
        assert_eq!(timer.display(now + Duration::from_secs(3_723)), "1:02:03");
        assert!(!timer.finished(now + Duration::from_secs(86_400)));
    }

    #[test]
    fn progress_depletes_with_remaining_time() {
        let timer = Timer::countdown(Duration::from_secs(100));
        let now = Instant::now();
        assert!((timer.progress(now + Duration::from_secs(25)) - 0.75).abs() < 1e-4);
        assert!((timer.progress(now + Duration::from_secs(100)) - 0.0).abs() < 1e-4);
    }
}
