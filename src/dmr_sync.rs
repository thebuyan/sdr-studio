// SPDX-License-Identifier: GPL-2.0-only

//! DMR burst acquisition using the outer 4FSK symbols in ETSI TS 102 361-1, table 9.2.
const PHASES: usize = 8;
const SYMBOLS: usize = 24;
const BURST: usize = 132;
const FILTER_TAPS: usize = PHASES * 8 + 1;
const TIMING_MARGIN: usize = 3;
const WINDOW: usize = PHASES * BURST + TIMING_MARGIN * 2;
const BURST_HISTORY: u64 = ((BURST - 1) * PHASES) as u64;
const SLOT_PERIOD: u64 = 288 * PHASES as u64;
const HALF_PAYLOAD: u64 = 54 * PHASES as u64;
pub type VoiceBurst = [u8; BURST];

pub enum Event {
    Sync(Sync),
    Voice(VoiceBurst, bool),
}

struct Candidate {
    sync: Sync,
    index: usize,
    end: u64,
    deviation: f32,
    quality: f32,
}

struct Voice {
    center: u64,
    offset: f32,
    deviation: f32,
    burst: usize,
    color: Option<u8>,
}

pub const PATTERNS: [(u64, &str); 8] = [
    (0x755f_d7df_75f7, "Estación base · voz"),
    (0xdff5_7d75_df5d, "Estación base · datos"),
    (0x7f7d_5dd5_7dfd, "Móvil · voz"),
    (0xd5d7_f77f_d757, "Móvil · datos"),
    (0x5d57_7f77_57ff, "Directo TS1 · voz"),
    (0xf7fd_d5dd_fd55, "Directo TS1 · datos"),
    (0x7dff_d5f5_5d5f, "Directo TS2 · voz"),
    (0xd755_7f5f_f7f5, "Directo TS2 · datos"),
];

const fn pattern_signs(mut pattern: u64) -> u32 {
    let mut bits = 0;
    let mut n = 0;
    while n < SYMBOLS {
        bits |= ((pattern as u32 >> 1) & 1) << n;
        pattern >>= 2;
        n += 1;
    }
    bits
}

const SIGNS: [u32; PATTERNS.len()] = {
    let mut signs = [0; PATTERNS.len()];
    let mut i = 0;
    while i < signs.len() {
        signs[i] = pattern_signs(PATTERNS[i].0);
        i += 1;
    }
    signs
};

pub struct Sync {
    pub label: &'static str,
    pub errors: u32,
    pub offset_hz: f32,
}

pub struct Detector {
    rate: u32,
    clock: u32,
    sum: f32,
    count: u32,
    filter: [f32; FILTER_TAPS],
    filter_history: [f32; FILTER_TAPS],
    filter_next: usize,
    samples: [f32; WINDOW],
    next: usize,
    tick: u64,
    search_end: u64,
    hold_until: u64,
    candidate: Option<Candidate>,
    voice: Option<Voice>,
}

impl Detector {
    pub fn new(rate: u32) -> Self {
        Self {
            rate,
            clock: 0,
            sum: 0.0,
            count: 0,
            filter: matched_filter(),
            filter_history: [0.0; FILTER_TAPS],
            filter_next: 0,
            samples: [0.0; WINDOW],
            next: 0,
            tick: 0,
            search_end: 0,
            hold_until: 0,
            candidate: None,
            voice: None,
        }
    }

    pub fn push(&mut self, hz: f32) -> Option<Event> {
        self.sum += hz;
        self.count += 1;
        self.clock += 4800 * PHASES as u32;
        if self.clock < self.rate {
            return None;
        }
        self.clock -= self.rate;
        self.filter_history[self.filter_next] = self.sum / self.count as f32;
        self.filter_next = (self.filter_next + 1) % FILTER_TAPS;
        let (head, tail) = self.filter_history.split_at(self.filter_next);
        self.samples[self.next] = tail
            .iter()
            .chain(head)
            .zip(&self.filter)
            .map(|(&sample, &tap)| sample * tap)
            .sum();
        self.next = (self.next + 1) % WINDOW;
        self.sum = 0.0;
        self.count = 0;
        self.tick += 1;
        if self.tick >= (SYMBOLS * PHASES) as u64 && self.tick >= self.hold_until {
            if let Some(candidate) = self.find_sync() {
                if self.candidate.is_none() {
                    self.search_end = self.tick + PHASES as u64 - 1;
                }
                if self
                    .candidate
                    .as_ref()
                    .is_none_or(|best| candidate.quality > best.quality)
                {
                    self.candidate = Some(candidate);
                }
            }
        }
        if self.tick == self.search_end {
            if let Some(candidate) = self.candidate.take() {
                self.hold_until = self.tick + 3 * PHASES as u64;
                self.track(&candidate);
                return Some(Event::Sync(candidate.sync));
            }
        }
        if let Some(voice) = &mut self.voice {
            if voice.burst < 6 && self.tick == voice.center + HALF_PAYLOAD + TIMING_MARGIN as u64 {
                let mut best = None;
                for delay in 0..=2 * TIMING_MARGIN {
                    let symbols: [f32; BURST] = std::array::from_fn(|n| {
                        self.samples
                            [(WINDOW + self.next - 1 - delay - (BURST - 1 - n) * PHASES) % WINDOW]
                    });
                    let (burst, mut cost) = slice_burst(symbols, voice.offset, voice.deviation);
                    let embedded = embedded_signalling(&burst);
                    if voice.burst != 0 {
                        // Prefer phases with a consistent colour code and a
                        // valid fragment type. The final burst may carry RC.
                        let good = embedded.is_some_and(|(data, _)| {
                            voice.color.is_none_or(|color| color == data >> 3)
                                && fragment_matches(voice.burst, data)
                        });
                        if !good {
                            cost += 0.40;
                        } else if let Some((_, errors)) = embedded {
                            cost += errors as f32 * 0.035;
                        }
                    }
                    if best
                        .as_ref()
                        .is_none_or(|&(best_cost, _, _, _)| cost < best_cost)
                    {
                        best = Some((cost, burst, delay, embedded));
                    }
                }
                let (_, burst, delay, embedded) = best.unwrap();
                if voice.burst == 1 {
                    voice.color = embedded
                        .filter(|&(data, _)| fragment_matches(1, data))
                        .map(|(data, _)| data >> 3);
                }
                // Follow the recovered clock across the gaps between bursts.
                voice.center = voice.center + SLOT_PERIOD + TIMING_MARGIN as u64 - delay as u64;
                voice.burst += 1;
                return Some(Event::Voice(burst, voice.burst == 1));
            }
            if voice.burst == 6 && self.tick > voice.center + (5 * PHASES) as u64 {
                self.voice = None;
            }
        }
        None
    }

    fn track(&mut self, candidate: &Candidate) {
        if self
            .voice
            .as_ref()
            .is_some_and(|voice| candidate.end.abs_diff(voice.center) > (4 * PHASES) as u64)
        {
            return;
        }
        let have_payload_before_sync =
            candidate.end + HALF_PAYLOAD > BURST_HISTORY + TIMING_MARGIN as u64;
        if candidate.index.is_multiple_of(2) && have_payload_before_sync {
            self.voice = Some(Voice {
                center: candidate.end,
                offset: candidate.sync.offset_hz,
                deviation: candidate.deviation,
                burst: 0,
                color: None,
            });
        } else {
            self.voice = None;
        }
    }

    fn find_sync(&self) -> Option<Candidate> {
        let symbols: [f32; SYMBOLS] = std::array::from_fn(|n| {
            self.samples[(WINDOW + self.next - 1 - (SYMBOLS - 1 - n) * PHASES) % WINDOW]
        });
        let offset = symbols.iter().sum::<f32>() / SYMBOLS as f32;
        if offset.abs() > 3000.0 {
            return None;
        }
        let centered = symbols.map(|sample| sample - offset);
        let signs = centered
            .iter()
            .fold(0, |word, &sample| (word << 1) | u32::from(sample < 0.0));
        for (index, ((_, label), expected)) in PATTERNS.into_iter().zip(SIGNS).enumerate() {
            let errors = (signs ^ expected).count_ones();
            if errors > 1 {
                continue;
            }
            let mut dot = 0.0;
            let mut power = 0.0;
            for (n, sample) in centered.into_iter().enumerate() {
                let sign = if expected & (1 << (SYMBOLS - 1 - n)) == 0 {
                    1.0
                } else {
                    -1.0
                };
                dot += sample * sign;
                power += sample * sample;
            }
            let deviation = dot / SYMBOLS as f32;
            let quality = dot * dot / (SYMBOLS as f32 * power);
            if (1250.0..=2500.0).contains(&deviation) && quality >= 0.88 {
                return Some(Candidate {
                    sync: Sync {
                        label,
                        errors,
                        offset_hz: offset,
                    },
                    index,
                    end: self.tick,
                    deviation,
                    quality,
                });
            }
        }
        None
    }
}

// Fit four equally spaced discriminator levels for each trial phase.
// The sync calibration seeds the fit and bounds it when symbols are noisy.
fn slice_burst(samples: [f32; BURST], sync_offset: f32, deviation: f32) -> (VoiceBurst, f32) {
    let mut offset = sync_offset;
    let step = deviation / 3.0;
    let mut unit = step;
    let mut dibits = [0; BURST];
    for _ in 0..3 {
        let (mut x, mut xx, mut xy, mut y) = (0.0, 0.0, 0.0, 0.0);
        for (&sample, dibit) in samples.iter().zip(&mut dibits) {
            let level = (sample - offset) / unit;
            let outer = level.abs() > 2.0;
            *dibit = (u8::from(level < 0.0) << 1) | u8::from(outer);
            let target = [1.0, 3.0, -1.0, -3.0][*dibit as usize];
            x += target;
            xx += target * target;
            xy += target * sample;
            y += sample;
        }
        let n = BURST as f32;
        let variance = xx - x * x / n;
        if variance <= n * 0.5 {
            break;
        }
        unit = ((xy - x * y / n) / variance).clamp(step * 0.75, step * 1.35);
        offset = ((y - unit * x) / n).clamp(sync_offset - step * 0.9, sync_offset + step * 0.9);
    }
    let cost = samples
        .iter()
        .zip(dibits)
        .map(|(&sample, dibit)| {
            let ideal = [1.0, 3.0, -1.0, -3.0][dibit as usize];
            ((sample - offset) / unit - ideal).powi(2).min(4.0)
        })
        .sum::<f32>()
        / BURST as f32;
    (dibits, cost)
}

pub fn fragment_matches(burst: usize, data: u8) -> bool {
    let fragment = data & 3;
    fragment == 0 || fragment == [0, 1, 3, 3, 2, 0][burst]
}

pub fn embedded_signalling(burst: &VoiceBurst) -> Option<(u8, u32)> {
    let received = burst[54..58]
        .iter()
        .chain(&burst[74..78])
        .fold(0u16, |bits, &dibit| (bits << 2) | u16::from(dibit));
    const ROWS: [u16; 7] = [0x804f, 0x411e, 0x21b7, 0x11e2, 0x09c9, 0x04e5, 0x0273];
    const WORDS: [u16; 128] = {
        let mut codes = [0; 128];
        let mut data = 0;
        while data < codes.len() {
            let mut bit = 0;
            while bit < ROWS.len() {
                if data & (1 << (6 - bit)) != 0 {
                    codes[data] ^= ROWS[bit];
                }
                bit += 1;
            }
            data += 1;
        }
        codes
    };
    WORDS.iter().enumerate().find_map(|(data, &word)| {
        let errors = (word ^ received).count_ones();
        (errors <= 2).then_some((data as u8, errors))
    })
}

// ETSI TS 102 361-1 §10.2.2.2. The 1920–2880 Hz transition at
// 4800 symbols/s gives a roll-off of 0.2.
fn matched_filter() -> [f32; FILTER_TAPS] {
    use std::f64::consts::PI;
    let beta = 0.2;
    let taps: [f64; FILTER_TAPS] = std::array::from_fn(|i| {
        let t = (i as f64 - (FILTER_TAPS / 2) as f64) / PHASES as f64;
        if t == 0.0 {
            1.0 + beta * (4.0 / PI - 1.0)
        } else if (t.abs() - 1.0 / (4.0 * beta)).abs() < f64::EPSILON {
            let angle = PI / (4.0 * beta);
            beta / 2.0_f64.sqrt()
                * ((1.0 + 2.0 / PI) * angle.sin() + (1.0 - 2.0 / PI) * angle.cos())
        } else {
            ((PI * t * (1.0 - beta)).sin() + 4.0 * beta * t * (PI * t * (1.0 + beta)).cos())
                / (PI * t * (1.0 - (4.0 * beta * t).powi(2)))
        }
    });
    let gain: f64 = taps.iter().sum();
    taps.map(|tap| (tap / gain) as f32)
}
