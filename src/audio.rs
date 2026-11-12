// SPDX-License-Identifier: GPL-2.0-only

use crate::dsp::iq_samples;
use rustfft::num_complex::{Complex32, Complex64};
use std::{
    io::Write,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, SyncSender, TrySendError},
    thread::{self, JoinHandle},
};

const RATE: u32 = 48_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Digital,
    Nfm,
    Wfm,
    Am,
    Usb,
    Lsb,
    Dsb,
    Cw,
    Raw,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Self::Digital => "Digital",
            Self::Nfm => "NFM",
            Self::Wfm => "WFM",
            Self::Am => "AM",
            Self::Usb => "USB",
            Self::Lsb => "LSB",
            Self::Dsb => "DSB",
            Self::Cw => "CW",
            Self::Raw => "RAW",
        }
    }

    pub fn default_bandwidth(self) -> u32 {
        match self {
            Self::Digital => 12_500,
            Self::Nfm => 12_500,
            Self::Wfm => 180_000,
            Self::Am => 10_000,
            Self::Usb | Self::Lsb => 2700,
            Self::Dsb => 6000,
            Self::Cw => 500,
            Self::Raw => 12_000,
        }
    }

    pub fn channels(self) -> u32 {
        match self {
            Self::Digital => 0,
            Self::Raw => 2,
            _ => 1,
        }
    }

    pub fn bandwidth_limits(self, sample_rate: u32) -> std::ops::RangeInclusive<u32> {
        match self {
            Self::Usb | Self::Lsb => 500..=10_000,
            Self::Cw => 50..=2000,
            Self::Dsb => 1000..=20_000,
            Self::Raw => 1000..=24_000,
            _ => 2000..=sample_rate * 9 / 10,
        }
    }

    pub fn filter_edges(self, bandwidth: u32) -> [f32; 2] {
        let width = bandwidth as f32;
        match self {
            Self::Usb => [200.0, 200.0 + width],
            Self::Lsb => [-200.0 - width, -200.0],
            _ => [-width * 0.5, width * 0.5],
        }
    }

    fn uses_baseband_audio(self) -> bool {
        matches!(
            self,
            Self::Usb | Self::Lsb | Self::Dsb | Self::Cw | Self::Raw
        )
    }
}

#[derive(Clone, Copy, PartialEq)]
pub struct Settings {
    pub mode: Mode,
    pub bandwidth: u32,
    pub volume: f32,
    pub squelch: Option<f32>,
    pub muted: bool,
    pub am_agc: bool,
    pub cw_pitch_hz: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mode: Mode::Digital,
            bandwidth: 12_500,
            volume: 0.55,
            squelch: None,
            muted: false,
            am_agc: true,
            cw_pitch_hz: 700,
        }
    }
}

pub struct Analog {
    input_rate: u32,
    clock: u32,
    filtered: Complex32,
    previous: Complex32,
    power: f32,
    open: bool,
    squelch: Option<f32>,
    dc: f32,
    low_pass: [f32; 2],
    channel_filter: ChannelFilter,
    anti_alias: ChannelFilter,
    sideband: SidebandFilter,
    bfo_phase: f32,
    am_carrier: f32,
    am_samples: u32,
}

impl Analog {
    pub fn new(input_rate: u32) -> Self {
        Self {
            input_rate,
            clock: 0,
            filtered: Complex32::default(),
            previous: Complex32::new(1.0, 0.0),
            power: 0.0,
            open: false,
            squelch: None,
            dc: 0.0,
            low_pass: [0.0; 2],
            channel_filter: ChannelFilter::default(),
            anti_alias: ChannelFilter::default(),
            sideband: SidebandFilter::default(),
            bfo_phase: 0.0,
            am_carrier: 0.0,
            am_samples: 0,
        }
    }

    pub fn process(&mut self, bytes: &[u8], settings: Settings) -> Vec<u8> {
        self.process_samples(iq_samples(bytes), settings)
    }

    pub fn process_samples(
        &mut self,
        samples: impl ExactSizeIterator<Item = Complex32>,
        settings: Settings,
    ) -> Vec<u8> {
        if settings.mode.uses_baseband_audio() {
            return self.baseband_audio(samples, settings);
        }
        let rate = self.input_rate as f32;
        let cutoff = (settings.bandwidth as f32 * 0.5).min(rate * 0.45);
        let channel_alpha = low_pass_alpha(cutoff, rate);
        let audio_cutoff = match settings.mode {
            Mode::Wfm => 12_000.0,
            Mode::Am => cutoff.min(4500.0),
            _ => 3_000.0,
        };
        let audio_alpha = low_pass_alpha(audio_cutoff, rate);
        let am = settings.mode == Mode::Am;
        let dc_alpha = if am {
            low_pass_alpha(65.0, RATE as f32)
        } else {
            0.025
        };
        if am {
            self.channel_filter
                .configure(self.input_rate, settings.bandwidth);
        }
        let mut pcm =
            Vec::with_capacity(samples.len() * 2 * RATE as usize / self.input_rate as usize + 2);
        let threshold = self.squelch_threshold(settings.squelch);
        for iq in samples {
            if am {
                self.filtered = self.channel_filter.process(iq);
            } else {
                self.filtered += (iq - self.filtered) * channel_alpha;
            }
            let value = match settings.mode {
                Mode::Am => self.filtered.norm(),
                Mode::Nfm | Mode::Wfm => {
                    let deviation = if settings.mode == Mode::Wfm {
                        75_000.0
                    } else {
                        2_500.0
                    };
                    (self.filtered * self.previous.conj()).arg() * rate
                        / (std::f32::consts::TAU * deviation)
                }
                _ => 0.0,
            };
            self.previous = self.filtered;
            self.low_pass[0] += audio_alpha * (value - self.low_pass[0]);
            self.low_pass[1] += audio_alpha * (self.low_pass[0] - self.low_pass[1]);
            if !self.next_audio_sample() {
                continue;
            }
            let open = self.update_squelch(self.filtered, threshold);
            let audio = self.low_pass[1];
            self.dc += dc_alpha * (audio - self.dc);
            let mut level = audio - self.dc;
            if am {
                // Track the carrier more slowly than the audio, with a gain ceiling.
                let alpha = if self.dc > self.am_carrier {
                    0.005
                } else {
                    0.00025
                };
                self.am_carrier += alpha * (self.dc - self.am_carrier);
                level *= if settings.am_agc {
                    0.8 / self.am_carrier.max(0.025)
                } else {
                    2.5
                };
            }
            let fade = if am {
                const FADE_SAMPLES: u32 = RATE / 50;
                self.am_samples = (self.am_samples + 1).min(FADE_SAMPLES);
                self.am_samples as f32 / FADE_SAMPLES as f32
            } else {
                1.0
            };
            let pcm_value = if open && !settings.muted {
                (level.clamp(-1.0, 1.0) * 22_000.0 * settings.volume * fade) as i16
            } else {
                0
            };
            pcm.extend_from_slice(&pcm_value.to_le_bytes());
        }
        pcm
    }

    fn baseband_audio(
        &mut self,
        samples: impl ExactSizeIterator<Item = Complex32>,
        settings: Settings,
    ) -> Vec<u8> {
        let mode = settings.mode;
        let ssb = matches!(mode, Mode::Usb | Mode::Lsb);
        let raw = mode == Mode::Raw;
        let limits = mode.bandwidth_limits(self.input_rate);
        let bandwidth = settings.bandwidth.clamp(*limits.start(), *limits.end());
        let rf_width = if ssb { 24_000 } else { bandwidth };
        self.channel_filter.configure(self.input_rate, rf_width);
        if ssb || raw {
            self.anti_alias.configure(self.input_rate, 24_000);
        }
        if ssb {
            self.sideband.configure(mode, bandwidth);
        }
        let threshold = self.squelch_threshold(settings.squelch);
        let mut pcm = Vec::with_capacity(
            samples.len() * 2 * RATE as usize * mode.channels() as usize / self.input_rate as usize
                + 4,
        );
        let bfo_step = std::f32::consts::TAU * settings.cw_pitch_hz as f32 / RATE as f32;
        for iq in samples {
            let mut filtered = self.channel_filter.process(iq);
            if ssb || raw {
                filtered = self.anti_alias.process(filtered);
            }
            if !self.next_audio_sample() {
                continue;
            }
            if ssb {
                filtered = self.sideband.process(filtered);
            }
            let volume = if self.update_squelch(filtered, threshold) && !settings.muted {
                settings.volume
            } else {
                0.0
            };
            if raw {
                push_pcm(&mut pcm, filtered.re, volume);
                push_pcm(&mut pcm, filtered.im, volume);
            } else {
                let audio = if mode == Mode::Cw {
                    let tone = (filtered * Complex32::from_polar(1.0, self.bfo_phase)).re;
                    self.bfo_phase = (self.bfo_phase + bfo_step) % std::f32::consts::TAU;
                    tone
                } else {
                    filtered.re
                };
                self.dc += 0.008 * (audio - self.dc);
                push_pcm(&mut pcm, (audio - self.dc) * 2.0, volume);
            }
        }
        pcm
    }

    fn next_audio_sample(&mut self) -> bool {
        self.clock += RATE;
        if self.clock < self.input_rate {
            return false;
        }
        self.clock -= self.input_rate;
        true
    }

    fn squelch_threshold(&mut self, squelch: Option<f32>) -> Option<f32> {
        if squelch != self.squelch {
            self.open = false;
            self.squelch = squelch;
        }
        squelch.map(|db| 10.0_f32.powf(db / 10.0))
    }

    fn update_squelch(&mut self, iq: Complex32, threshold: Option<f32>) -> bool {
        self.power += 0.004 * (iq.norm_sqr() - self.power);
        self.open = threshold
            .is_none_or(|threshold| self.power >= threshold * if self.open { 0.5 } else { 1.0 });
        self.open
    }

    pub fn signal_db(&self) -> f32 {
        (10.0 * self.power.max(1e-12).log10()).min(0.0)
    }
}

fn push_pcm(pcm: &mut Vec<u8>, sample: f32, volume: f32) {
    pcm.extend_from_slice(&((sample.clamp(-1.0, 1.0) * 22_000.0 * volume) as i16).to_le_bytes());
}

#[derive(Default)]
struct ChannelFilter {
    bandwidth: u32,
    sections: [LowPass; 2],
}

impl ChannelFilter {
    fn configure(&mut self, rate: u32, bandwidth: u32) {
        if self.bandwidth == bandwidth {
            return;
        }
        let cutoff = (f64::from(bandwidth) * 0.5).clamp(1.0, f64::from(rate) * 0.45);
        let omega = std::f64::consts::TAU * cutoff / f64::from(rate);
        self.sections = [0.541_196_1, 1.306_563].map(|q| LowPass::new(omega, q));
        self.bandwidth = bandwidth;
    }

    fn process(&mut self, iq: Complex32) -> Complex32 {
        let first = self.sections[0].process(Complex64::new(f64::from(iq.re), f64::from(iq.im)));
        let output = self.sections[1].process(first);
        Complex32::new(output.re as f32, output.im as f32)
    }
}

#[derive(Default)]
struct SidebandFilter {
    edges: [f32; 2],
    taps: Vec<Complex32>,
    history: Vec<Complex32>,
    cursor: usize,
}

impl SidebandFilter {
    fn configure(&mut self, mode: Mode, bandwidth: u32) {
        let edges = mode.filter_edges(bandwidth);
        if self.edges == edges {
            return;
        }
        self.edges = edges;
        const TAPS: usize = 321;
        let width = (edges[1] - edges[0]) / RATE as f32;
        let center = (edges[1] + edges[0]) / (2.0 * RATE as f32);
        self.taps = (0..TAPS)
            .map(|i| {
                let n = i as f32 - (TAPS / 2) as f32;
                let x = std::f32::consts::PI * width * n;
                let sinc = if n == 0.0 { 1.0 } else { x.sin() / x };
                let t = std::f32::consts::TAU * i as f32 / (TAPS - 1) as f32;
                let window = 0.42 - 0.5 * t.cos() + 0.08 * (2.0 * t).cos();
                Complex32::from_polar(width * sinc * window, std::f32::consts::TAU * center * n)
            })
            .collect();
        self.history.resize(TAPS, Complex32::default());
    }

    fn process(&mut self, sample: Complex32) -> Complex32 {
        self.history[self.cursor] = sample;
        self.cursor = (self.cursor + 1) % self.history.len();
        let (head, tail) = self.history.split_at(self.cursor);
        head.iter()
            .rev()
            .chain(tail.iter().rev())
            .zip(&self.taps)
            .map(|(&iq, &tap)| iq * tap)
            .sum()
    }
}

#[derive(Default)]
struct LowPass {
    b: f64,
    a1: f64,
    a2: f64,
    z1: Complex64,
    z2: Complex64,
}

impl LowPass {
    fn new(omega: f64, q: f64) -> Self {
        let (sin, cos) = omega.sin_cos();
        let alpha = sin / (2.0 * q);
        let a0 = 1.0 + alpha;
        Self {
            b: (1.0 - cos) / (2.0 * a0),
            a1: -2.0 * cos / a0,
            a2: (1.0 - alpha) / a0,
            ..Self::default()
        }
    }

    fn process(&mut self, sample: Complex64) -> Complex64 {
        let input = sample * self.b;
        let output = input + self.z1;
        self.z1 = input * 2.0 - output * self.a1 + self.z2;
        self.z2 = input - output * self.a2;
        output
    }
}

fn low_pass_alpha(cutoff: f32, rate: f32) -> f32 {
    1.0 - (-std::f32::consts::TAU * cutoff / rate).exp()
}

pub struct Playback {
    child: Child,
    sender: Option<SyncSender<Vec<u8>>>,
    writer: Option<JoinHandle<()>>,
}

impl Playback {
    pub fn open(channels: u32) -> Result<Self, String> {
        Self::open_at(channels, RATE)
    }

    pub fn open_at(channels: u32, rate: u32) -> Result<Self, String> {
        let mut child = Command::new("paplay")
            .args([
                "--raw",
                "--format=s16le",
                &format!("--rate={rate}"),
                &format!("--channels={channels}"),
                "--latency-msec=60",
                "--stream-name=SDR Studio Radio",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .map_err(|error| {
                crate::tr_format!(
                    "Audio: no se pudo iniciar paplay ({error})",
                    "Audio: could not start paplay ({error})"
                )
            })?;
        let mut stdin = child.stdin.take().unwrap();
        let (sender, queue) = mpsc::sync_channel::<Vec<u8>>(2);
        let writer = thread::spawn(move || {
            while let Ok(pcm) = queue.recv() {
                if stdin.write_all(&pcm).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            sender: Some(sender),
            writer: Some(writer),
        })
    }

    pub fn send(&self, pcm: Vec<u8>) -> Result<(), String> {
        match self.sender.as_ref().unwrap().try_send(pcm) {
            Ok(()) | Err(TrySendError::Full(_)) => Ok(()),
            Err(TrySendError::Disconnected(_)) => {
                Err(crate::tr!("Se interrumpió el audio", "Audio playback was interrupted").into())
            }
        }
    }
}

impl Drop for Playback {
    fn drop(&mut self) {
        self.sender.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iq_tones(rate: u32, tones: &[(f64, f64)]) -> Vec<u8> {
        let mut iq = Vec::new();
        for n in 0..rate / 5 {
            let t = f64::from(n) / f64::from(rate);
            let sample: Complex64 = tones
                .iter()
                .map(|&(hz, amplitude)| {
                    Complex64::from_polar(amplitude, std::f64::consts::TAU * hz * t)
                })
                .sum();
            iq.push((127.5 + sample.re).round() as u8);
            iq.push((127.5 + sample.im).round() as u8);
        }
        iq
    }

    fn audio_tone(pcm: &[u8], channels: usize, channel: usize, hz: f64) -> f64 {
        let pcm = &pcm[RATE as usize / 10 * channels * 2..];
        let sum: Complex64 = pcm
            .chunks_exact(channels * 2)
            .enumerate()
            .map(|(n, frame)| {
                let value = i16::from_le_bytes([frame[2 * channel], frame[2 * channel + 1]]);
                let phase = std::f64::consts::TAU * hz * n as f64 / f64::from(RATE);
                Complex64::from_polar(f64::from(value), phase)
            })
            .sum();
        sum.norm() * (channels * 4) as f64 / pcm.len() as f64
    }

    #[test]
    fn usb_lsb_select_opposite_sidebands_and_dsb_keeps_both() {
        const INPUT_RATE: u32 = 240_000;
        let iq = iq_tones(INPUT_RATE, &[(900.0, 34.0), (-1550.0, 34.0)]);
        for mode in [Mode::Usb, Mode::Lsb, Mode::Dsb] {
            let mut analog = Analog::new(INPUT_RATE);
            let settings = Settings {
                mode,
                bandwidth: mode.default_bandwidth(),
                ..Settings::default()
            };
            let pcm: Vec<_> = iq
                .chunks(8190)
                .flat_map(|block| analog.process(block, settings))
                .collect();
            assert_eq!(pcm.len(), 2 * (RATE / 5) as usize);
            let upper = audio_tone(&pcm, 1, 0, 900.0);
            let lower = audio_tone(&pcm, 1, 0, 1550.0);
            if mode != Mode::Lsb {
                assert!(upper > 2500.0, "{mode:?}: USB={upper}");
            }
            if mode != Mode::Usb {
                assert!(lower > 2500.0, "{mode:?}: LSB={lower}");
            }
            match mode {
                Mode::Usb => assert!(lower < upper / 40.0, "USB leaves LSB at {lower}"),
                Mode::Lsb => assert!(upper < lower / 40.0, "LSB leaves USB at {upper}"),
                _ => {}
            }
        }
    }

    #[test]
    fn cw_uses_the_configured_beat_note_and_raw_keeps_i_and_q_separate() {
        const INPUT_RATE: u32 = 256_000;
        let carrier = iq_tones(INPUT_RATE, &[(0.0, 60.0)]);
        let cw = Settings {
            mode: Mode::Cw,
            bandwidth: 500,
            cw_pitch_hz: 850,
            ..Settings::default()
        };
        let pcm = Analog::new(INPUT_RATE).process(&carrier, cw);
        assert_eq!(pcm.len(), 2 * (RATE / 5) as usize);
        assert!(audio_tone(&pcm, 1, 0, 850.0) > 7000.0);
        assert!(audio_tone(&pcm, 1, 0, 700.0) < 50.0);

        let iq = iq_tones(
            INPUT_RATE,
            &[
                (900.0, 18.0),
                (-900.0, 18.0),
                (1700.0, 18.0),
                (-1700.0, -18.0),
            ],
        );
        let raw = Settings {
            mode: Mode::Raw,
            bandwidth: 12_000,
            ..Settings::default()
        };
        let mut analog = Analog::new(INPUT_RATE);
        let pcm = analog.process(&iq, raw);
        assert_eq!(pcm.len(), 4 * (RATE / 5) as usize);
        assert!(audio_tone(&pcm, 2, 0, 900.0) > 2500.0);
        assert!(audio_tone(&pcm, 2, 1, 1700.0) > 2500.0);
        assert!(audio_tone(&pcm, 2, 0, 1700.0) < 100.0);
        assert!(audio_tone(&pcm, 2, 1, 900.0) < 100.0);
        assert!(analog
            .process(&iq, Settings { muted: true, ..raw })
            .iter()
            .all(|&value| value == 0));
    }

    #[test]
    fn hf_am_recovers_a_weak_signal_next_to_a_stronger_carrier() {
        const INPUT_RATE: u32 = 240_000;
        let settings = Settings {
            mode: Mode::Am,
            bandwidth: 4000,
            ..Settings::default()
        };
        let mut iq = Vec::new();
        for index in 0..INPUT_RATE / 5 {
            let t = index as f64 / f64::from(INPUT_RATE);
            let tone = |hz: f64| (std::f64::consts::TAU * hz * t).sin();
            let desired = 9.0 * (1.0 + 0.5 * tone(1000.0));
            let adjacent = 65.0 * (1.0 + 0.4 * tone(700.0));
            let phase = std::f64::consts::TAU * 5000.0 * t;
            iq.push((127.5 + desired + adjacent * phase.cos()).round() as u8);
            iq.push((127.5 + adjacent * phase.sin()).round() as u8);
        }
        let pcm = Analog::new(INPUT_RATE).process(&iq, settings);
        let samples = &pcm.as_chunks::<2>().0[RATE as usize / 10..];
        let component = |hz: f64| {
            let sum: Complex64 = samples
                .iter()
                .enumerate()
                .map(|(i, &sample)| {
                    let phase = std::f64::consts::TAU * hz * i as f64 / f64::from(RATE);
                    Complex64::from_polar(f64::from(i16::from_le_bytes(sample)), phase)
                })
                .sum();
            sum.norm() * 2.0 / samples.len() as f64
        };
        let desired = component(1000.0);
        assert!(desired > 1800.0, "Audio de la señal débil: {desired}");
        assert!(
            component(700.0) < desired / 8.0,
            "La portadora vecina domina el audio"
        );
    }

    #[test]
    fn analog_modes_recover_tone_and_respect_squelch_and_mute() {
        const INPUT_RATE: u32 = 256_000;
        for mode in [Mode::Nfm, Mode::Wfm, Mode::Am] {
            let mut phase = 0.0_f32;
            let mut iq = Vec::new();
            for index in 0..INPUT_RATE / 10 {
                let t = index as f32 / INPUT_RATE as f32;
                let tau = std::f32::consts::TAU;
                let tone = (tau * 1000.0 * t).sin();
                let (deviation, amplitude) = match mode {
                    Mode::Wfm => (55_000.0, 100.0),
                    Mode::Am => (0.0, 70.0 + 35.0 * tone),
                    _ => (2000.0, 100.0),
                };
                phase += tau * deviation * tone / INPUT_RATE as f32;
                iq.push((127.5 + amplitude * phase.cos()) as u8);
                iq.push((127.5 + amplitude * phase.sin()) as u8);
            }
            let mut settings = Settings {
                mode,
                bandwidth: mode.default_bandwidth(),
                ..Settings::default()
            };
            let mut radio = Analog::new(INPUT_RATE);
            let pcm = radio.process(&iq, settings);
            assert_eq!(pcm.len(), 2 * (RATE / 10) as usize);
            let samples: Vec<_> = pcm
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&s| i16::from_le_bytes(s))
                .collect();
            assert!(samples.iter().any(|&sample| sample > 2000), "{mode:?}");
            let cycles = samples
                .windows(2)
                .filter(|pair| pair[0] <= 0 && pair[1] > 0)
                .count();
            assert!((97..=102).contains(&cycles), "{mode:?}: {cycles} ciclos");
            settings.squelch = Some(0.0);
            assert!(radio.process(&iq, settings).iter().all(|&b| b == 0));
            settings.squelch = None;
            settings.muted = true;
            assert!(radio.process(&iq, settings).iter().all(|&b| b == 0));
        }
    }
}
