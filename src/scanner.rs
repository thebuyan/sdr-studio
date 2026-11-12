// SPDX-License-Identifier: GPL-2.0-only

use crate::receiver::State;
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
pub struct Range {
    pub first: u32,
    pub last: u32,
    pub step: u32,
    pub threshold_db: f32,
    pub settle: Duration,
}

#[derive(Clone, Copy)]
enum Phase {
    Tuning(Instant),
    Settling(Instant),
    FreshFrame { after: u64, deadline: Instant },
    Holding(f32),
}

pub enum Update {
    None,
    Tune(u32),
    Finished,
    Failed(String),
}

pub struct Sweep {
    range: Range,
    current: u32,
    phase: Phase,
}

const TIMEOUT: Duration = Duration::from_secs(8);

impl Sweep {
    pub fn new(range: Range, now: Instant) -> Self {
        Self {
            current: range.first,
            range,
            phase: Phase::Tuning(now + TIMEOUT),
        }
    }

    pub fn progress(&self) -> (u32, u32) {
        (
            (self.current - self.range.first) / self.range.step + 1,
            (self.range.last - self.range.first) / self.range.step + 1,
        )
    }

    pub fn held_level(&self) -> Option<f32> {
        if let Phase::Holding(level) = self.phase {
            Some(level)
        } else {
            None
        }
    }

    pub fn resume(&mut self, now: Instant) -> Update {
        let Some(next) = self
            .current
            .checked_add(self.range.step)
            .filter(|&hz| hz <= self.range.last)
        else {
            return Update::Finished;
        };
        self.current = next;
        self.phase = Phase::Tuning(now + TIMEOUT);
        Update::Tune(next)
    }

    pub fn poll(&mut self, state: &State, rate: u32, edges: [f32; 2], now: Instant) -> Update {
        if !state.running {
            return Update::Failed(state.status.clone());
        }
        if let Some(error) = &state.tune_error {
            return Update::Failed(error.clone());
        }
        match self.phase {
            Phase::Tuning(deadline) => {
                if state.frequency == self.current
                    && state.spectrum_center == self.current
                    && state.spectrum_frame != 0
                {
                    self.phase = Phase::Settling(now + self.range.settle);
                } else if now >= deadline {
                    return Update::Failed(
                        crate::tr!(
                            "El SDR no confirmó la sintonía",
                            "The SDR did not confirm tuning"
                        )
                        .into(),
                    );
                }
            }
            Phase::Settling(until) if now >= until => {
                self.phase = Phase::FreshFrame {
                    after: state.spectrum_frame,
                    deadline: now + TIMEOUT,
                };
            }
            Phase::FreshFrame { after, deadline } => {
                if state.frequency == self.current && state.spectrum_frame > after {
                    let level = channel_peak(&state.spectrum, rate, edges);
                    if level >= self.range.threshold_db {
                        self.phase = Phase::Holding(level);
                    } else {
                        return self.resume(now);
                    }
                } else if now >= deadline {
                    return Update::Failed(
                        crate::tr!(
                            "No llegaron muestras nuevas del SDR",
                            "No new samples arrived from the SDR"
                        )
                        .into(),
                    );
                }
            }
            _ => {}
        }
        Update::None
    }
}

fn channel_peak(bins: &[f32], rate: u32, edges: [f32; 2]) -> f32 {
    if bins.is_empty() || rate == 0 {
        return f32::NEG_INFINITY;
    }
    let bin = |hz: f32| bins.len() as f32 * (0.5 + hz / rate as f32);
    let clamp = |x: f32| (x as usize).min(bins.len() - 1);
    let first = clamp(bin(edges[0]).ceil());
    let last = clamp(bin(edges[1]).floor());
    if first > last {
        return bins[clamp(bin((edges[0] + edges[1]) * 0.5).round())];
    }
    bins[first..=last]
        .iter()
        .copied()
        .fold(f32::NEG_INFINITY, f32::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_ignores_old_frames_holds_on_channel_and_finishes_at_the_range_end() {
        let now = Instant::now();
        let settle = Duration::from_millis(100);
        let range = Range {
            first: 100,
            last: 125,
            step: 10,
            threshold_db: -50.0,
            settle,
        };
        let mut scan = Sweep::new(range, now);
        let mut state = State {
            running: true,
            frequency: 100,
            spectrum_center: 100,
            spectrum_frame: 1,
            spectrum: vec![-90.0; 8],
            ..State::default()
        };
        let edges = [-1000.0, 1000.0];
        state.spectrum[0] = -10.0;
        state.spectrum[4] = -30.0;
        assert!(matches!(scan.poll(&state, 8000, edges, now), Update::None));
        assert!(matches!(
            scan.poll(&state, 8000, edges, now + settle),
            Update::None
        ));
        assert!(matches!(
            scan.poll(&state, 8000, edges, now + settle),
            Update::None
        ));
        assert!(scan.held_level().is_none());
        state.spectrum_frame += 1;
        assert!(matches!(
            scan.poll(&state, 8000, edges, now + settle),
            Update::None
        ));
        assert_eq!(scan.held_level(), Some(-30.0));

        assert!(matches!(scan.resume(now), Update::Tune(110)));
        state.frequency = 110;
        state.spectrum_center = 110;
        state.spectrum[4] = -90.0;
        assert!(matches!(scan.poll(&state, 8000, edges, now), Update::None));
        assert!(matches!(
            scan.poll(&state, 8000, edges, now + settle),
            Update::None
        ));
        state.spectrum_frame += 1;
        assert!(matches!(
            scan.poll(&state, 8000, edges, now + settle),
            Update::Tune(120)
        ));
        assert_eq!(scan.progress(), (3, 3));
        assert!(matches!(scan.resume(now), Update::Finished));
    }
}
