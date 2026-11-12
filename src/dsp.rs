// SPDX-License-Identifier: GPL-2.0-only

use rustfft::{num_complex::Complex32, FftPlanner};
use std::sync::Arc;

pub const FFT_SIZE: usize = 1024;

pub enum DigitalEvent {
    Dibit(u8),
    Dmr(crate::dmr_sync::Sync),
    DmrVoice(crate::dmr_sync::VoiceBurst, bool),
}

pub struct Dsp {
    previous: Complex32,
    dmr: crate::dmr_sync::Detector,
    filtered: Complex32,
    alpha: f32,
    sample_rate: f32,
    symbol_clock: f32,
    integrator: f32,
    count: usize,
    pub spectrum: Vec<f32>,
    pub spectrum_frame: u64,
    fft_buffer: Vec<Complex32>,
    fft: Arc<dyn rustfft::Fft<f32>>,
}

impl Dsp {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            previous: Complex32::new(1.0, 0.0),
            dmr: crate::dmr_sync::Detector::new(sample_rate),
            filtered: Complex32::default(),
            alpha: 1.0 - (-2.0 * std::f32::consts::PI * 6500.0 / sample_rate as f32).exp(),
            sample_rate: sample_rate as f32,
            symbol_clock: 0.0,
            integrator: 0.0,
            count: 0,
            spectrum: vec![-100.0; FFT_SIZE],
            spectrum_frame: 0,
            fft_buffer: Vec::with_capacity(FFT_SIZE),
            fft: FftPlanner::new().plan_fft_forward(FFT_SIZE),
        }
    }

    pub fn retune(&mut self, hardware_changed: bool) {
        self.previous = Complex32::new(1.0, 0.0);
        self.dmr = crate::dmr_sync::Detector::new(self.sample_rate as u32);
        self.filtered = Complex32::default();
        self.symbol_clock = 0.0;
        self.integrator = 0.0;
        self.count = 0;
        if hardware_changed {
            self.fft_buffer.clear();
        }
    }

    pub fn process(&mut self, bytes: &[u8], emit: impl FnMut(DigitalEvent)) {
        self.process_samples(iq_samples(bytes).map(|iq| (iq, iq)), emit);
    }

    pub fn process_samples(
        &mut self,
        samples: impl Iterator<Item = (Complex32, Complex32)>,
        mut emit: impl FnMut(DigitalEvent),
    ) {
        for (raw, x) in samples {
            // A single pole channel filter is a prototype, not a production channelizer.
            self.filtered += (x - self.filtered) * self.alpha;
            let product = self.filtered * self.previous.conj();
            self.previous = self.filtered;
            let hz = product.arg() * self.sample_rate / (2.0 * std::f32::consts::PI);
            if let Some(event) = self.dmr.push(hz) {
                use crate::dmr_sync::Event;
                emit(match event {
                    Event::Sync(sync) => DigitalEvent::Dmr(sync),
                    Event::Voice(burst, first) => DigitalEvent::DmrVoice(burst, first),
                });
            }
            self.integrator += hz;
            self.count += 1;
            self.symbol_clock += 4800.0;
            if self.symbol_clock >= self.sample_rate {
                self.symbol_clock -= self.sample_rate;
                let frequency = self.integrator / self.count as f32;
                // C4FM: +1800 => 01, +600 => 00, -600 => 10, -1800 => 11.
                emit(DigitalEvent::Dibit(if frequency > 1200.0 {
                    1
                } else if frequency > 0.0 {
                    0
                } else if frequency > -1200.0 {
                    2
                } else {
                    3
                }));
                self.integrator = 0.0;
                self.count = 0;
            }
            self.fft_buffer.push(raw);
            if self.fft_buffer.len() == FFT_SIZE {
                for (n, v) in self.fft_buffer.iter_mut().enumerate() {
                    *v *= 0.5
                        - 0.5
                            * (2.0 * std::f32::consts::PI * n as f32 / (FFT_SIZE - 1) as f32).cos();
                }
                self.fft.process(&mut self.fft_buffer);
                for i in 0..FFT_SIZE {
                    let power = self.fft_buffer[(i + FFT_SIZE / 2) % FFT_SIZE].norm_sqr()
                        / (FFT_SIZE * FFT_SIZE) as f32;
                    self.spectrum[i] = 10.0 * power.max(1e-12).log10();
                }
                self.fft_buffer.clear();
                self.spectrum_frame += 1;
            }
        }
    }
}

pub fn iq_samples(bytes: &[u8]) -> impl ExactSizeIterator<Item = Complex32> + '_ {
    bytes.as_chunks::<2>().0.iter().map(|&[i, q]| {
        Complex32::new(
            (f32::from(i) - 127.5) / 128.0,
            (f32::from(q) - 127.5) / 128.0,
        )
    })
}

pub struct Mixer {
    phase: Complex32,
    step: Complex32,
}

impl Mixer {
    pub fn new(offset_hz: i64, rate: u32) -> Self {
        Self {
            phase: Complex32::new(1.0, 0.0),
            step: Complex32::from_polar(
                1.0,
                (-std::f64::consts::TAU * offset_hz as f64 / f64::from(rate)) as f32,
            ),
        }
    }

    pub fn shift(&mut self, bytes: &[u8], output: &mut Vec<Complex32>) {
        output.clear();
        let mut phase = self.phase;
        output.extend(iq_samples(bytes).map(|iq| {
            let shifted = iq * phase;
            phase *= self.step;
            shifted
        }));
        self.phase = phase / phase.norm();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{protocol::Decoder, source::Demo};
    #[test]
    fn software_tuning_decodes_the_shifted_channel_and_keeps_the_raw_fft() {
        const RATE: u32 = 240_000;
        const OFFSET: i64 = 37_500;
        let bytes = Demo::new(RATE).generate(240_000);
        let mut rf = Vec::new();
        Mixer::new(-OFFSET, RATE).shift(&bytes, &mut rf);
        let rf_bytes: Vec<u8> = rf
            .iter()
            .flat_map(|iq| [iq.re, iq.im].map(|sample| (sample * 128.0 + 127.5).round() as u8))
            .collect();
        let mut dsp = Dsp::new(RATE);
        let mut mixer = Mixer::new(OFFSET, RATE);
        let mut baseband = Vec::new();
        let mut found_dmr = false;
        for block in rf_bytes.chunks(8192) {
            mixer.shift(block, &mut baseband);
            dsp.process_samples(iq_samples(block).zip(baseband.iter().copied()), |event| {
                found_dmr |= matches!(event, DigitalEvent::Dmr(_));
            });
        }
        assert!(found_dmr);
        let peak = dsp
            .spectrum
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap()
            .0;
        let peak_hz = (peak as i64 - FFT_SIZE as i64 / 2) * i64::from(RATE) / FFT_SIZE as i64;
        assert!((peak_hz - OFFSET).abs() < 5000, "{peak_hz}");
    }

    #[test]
    fn synthetic_iq_reaches_both_protocols_in_arbitrary_even_chunks() {
        let mut source = Demo::new(240000);
        let bytes = source.generate(240000 * 2);
        let mut dsp = Dsp::new(240000);
        let mut decoder = Decoder::default();
        for block in bytes.chunks(514) {
            dsp.process(block, |event| match event {
                DigitalEvent::Dibit(d) => decoder.push(d),
                DigitalEvent::Dmr(sync) => decoder.dmr_sync(sync),
                DigitalEvent::DmrVoice(..) => {}
            });
        }
        assert!(decoder.events.iter().any(|e| e.protocol == "DMR"));
        assert!(decoder.events.iter().any(|e| e.text.contains("NAC 293")));
        assert!(dsp.spectrum.iter().all(|p| p.is_finite()));
    }

    #[test]
    fn dmr_bursts_acquire_with_clock_phase_error_and_offset() {
        use crate::{dmr_sync::PATTERNS, protocol::word_dibits};
        for (pattern, expected) in PATTERNS {
            for (rate, start, offset) in [(240_000, 0.47, 1_050.0), (256_000, 0.81, -920.0)] {
                let mut symbols = [0, 2, 1, 3].repeat(11);
                symbols.extend(word_dibits(pattern, 24));
                symbols.extend([0, 3, 2, 1].repeat(5));
                let baud = 4800.0 * 1.000_050;
                let mut phase = 0.0_f64;
                let mut iq = Vec::new();
                for n in 0..(symbols.len() as f64 * rate as f64 / baud) as usize {
                    let t = n as f64 * baud / f64::from(rate) - start;
                    let symbol = t.floor() as usize;
                    let deviation = [648.0, 1944.0, -648.0, -1944.0];
                    let current = deviation[symbols[symbol] as usize];
                    let previous = deviation[symbols[symbol.saturating_sub(1)] as usize];
                    let fraction = ((t - t.floor()) * 5.0).clamp(0.0, 1.0);
                    let hz = offset + previous + (current - previous) * fraction;
                    phase += std::f64::consts::TAU * hz / f64::from(rate);
                    iq.push((127.5 + 82.0 * phase.cos()).round() as u8);
                    iq.push((127.5 + 82.0 * phase.sin()).round() as u8);
                }
                let mut found = Vec::new();
                let mut dsp = Dsp::new(rate);
                for block in iq.chunks(770) {
                    dsp.process(block, |event| {
                        if let DigitalEvent::Dmr(sync) = event {
                            found.push(sync);
                        }
                    });
                }
                assert_eq!(found.len(), 1, "{expected}: rate={rate}, offset={offset}");
                assert_eq!(found[0].label, expected);
                assert!((f64::from(found[0].offset_hz) - offset).abs() < 250.0);
            }
        }
    }
}
