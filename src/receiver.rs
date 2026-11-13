// SPDX-License-Identifier: GPL-2.0-only

use crate::{
    audio::{Analog, Mode, Playback, Settings},
    dmr_audio::{self, Vocoder},
    dsp::{iq_samples, DigitalEvent, Dsp, Mixer},
    plugins::Plugin,
    protocol::{Decoder, Event},
    source::{self, Config, SourceKind},
};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub struct State {
    pub running: bool,
    pub status: String,
    pub spectrum: Vec<f32>,
    pub scope_epoch: u64,
    pub spectrum_frame: u64,
    pub events: VecDeque<Event>,
    pub samples: u64,
    pub hits: u64,
    pub plugins: Vec<String>,
    pub frequency: u32,
    pub spectrum_center: u32,
    pub tune_error: Option<String>,
    pub audio_error: Option<String>,
    pub gain_error: Option<String>,
    pub signal_db: f32,
}
impl Default for State {
    fn default() -> Self {
        Self {
            running: false,
            status: "Desconectado".into(),
            spectrum: vec![-100.0; 1024],
            scope_epoch: 0,
            spectrum_frame: 0,
            events: VecDeque::new(),
            samples: 0,
            hits: 0,
            plugins: vec![],
            frequency: 0,
            spectrum_center: 0,
            tune_error: None,
            audio_error: None,
            gain_error: None,
            signal_db: -120.0,
        }
    }
}
impl State {
    fn publish_spectrum(&mut self, dsp: &Dsp, center: u32, pending_retune: &mut Option<u64>) {
        if let Some(before) = *pending_retune {
            if dsp.spectrum_frame == before {
                return;
            }
            *pending_retune = None;
        }
        self.spectrum.clone_from(&dsp.spectrum);
        self.spectrum_frame = dsp.spectrum_frame;
        self.spectrum_center = center;
    }
}

pub struct Receiver {
    pub state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    tune_request: Arc<AtomicU64>,
    gain_request: Arc<Mutex<Option<i32>>>,
    radio: Arc<Mutex<Settings>>,
    thread: Option<JoinHandle<()>>,
}
impl Default for Receiver {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(State::default())),
            stop: Arc::new(AtomicBool::new(false)),
            tune_request: Arc::new(AtomicU64::new(0)),
            gain_request: Arc::new(Mutex::new(None)),
            radio: Arc::new(Mutex::new(Settings::default())),
            thread: None,
        }
    }
}
impl Receiver {
    pub fn start(&mut self, config: Config, plugins: Vec<PathBuf>) -> Result<(), String> {
        config.validate()?;
        self.reap();
        if self.thread.is_some() {
            return Err(crate::tr!(
                "Espera a que termine el receptor anterior",
                "Wait for the previous receiver session to stop"
            )
            .into());
        }
        self.stop.store(false, Ordering::Relaxed);
        self.tune_request.store(0, Ordering::Relaxed);
        self.set_gain(config.gain);
        {
            let mut state = self.state.lock().unwrap();
            *state = State {
                running: true,
                status: "Conectando…".into(),
                scope_epoch: state.scope_epoch.wrapping_add(1),
                ..State::default()
            };
        }
        let state = self.state.clone();
        let stop = self.stop.clone();
        let tune_request = self.tune_request.clone();
        let radio = self.radio.clone();
        let gain_request = self.gain_request.clone();
        self.thread = Some(thread::spawn(move || {
            let result = run(
                config,
                plugins,
                &state,
                &stop,
                &tune_request,
                &gain_request,
                &radio,
            );
            let mut state = state.lock().unwrap();
            state.running = false;
            state.status = match result {
                Ok(message) => message,
                Err(error) => format!("Error: {error}"),
            };
        }));
        Ok(())
    }
    pub fn set_radio(&self, settings: Settings) {
        *self.radio.lock().unwrap() = settings;
    }

    pub fn set_gain(&self, gain: Option<i32>) {
        *self.gain_request.lock().unwrap() = gain;
    }

    pub fn tune(&self, frequency: u32) {
        self.tune_in(frequency, frequency);
    }

    pub fn tune_in(&self, frequency: u32, center: u32) {
        self.state.lock().unwrap().tune_error = None;
        let request = (u64::from(center) << 32) | u64::from(frequency);
        self.tune_request.store(request, Ordering::Relaxed);
    }
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        let mut state = self.state.lock().unwrap();
        if state.running {
            state.status = "Deteniendo…".into();
        }
    }
    pub fn busy(&self) -> bool {
        self.thread.as_ref().is_some_and(|t| !t.is_finished())
    }
    pub fn reap(&mut self) {
        if self.thread.as_ref().is_some_and(|t| t.is_finished())
            && self.thread.take().unwrap().join().is_err()
        {
            let mut state = self.state.lock().unwrap();
            state.running = false;
            state.status = "Falló el hilo receptor".into();
        }
    }
}
impl Drop for Receiver {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn run(
    config: Config,
    paths: Vec<PathBuf>,
    state: &Arc<Mutex<State>>,
    stop: &AtomicBool,
    tune_request: &AtomicU64,
    gain_request: &Mutex<Option<i32>>,
    radio: &Mutex<Settings>,
) -> Result<String, String> {
    let mut source = source::open(&config)?;
    let plugins: Vec<_> = paths
        .iter()
        .map(|p| Plugin::load(p))
        .collect::<Result<_, _>>()?;
    {
        let mut state = state.lock().unwrap();
        state.status = "Recibiendo".into();
        state.frequency = config.frequency;
        state.spectrum_center = config.frequency;
        state.plugins = plugins.iter().map(|p| p.name.clone()).collect();
    }
    let mut dsp = Dsp::new(config.sample_rate);
    let mut audio = None;
    let mut dmr_voice = None;
    let mut dmr_audio_failed = false;
    let mut analog = Analog::new(config.sample_rate);
    let mut mode = Mode::Digital;
    let mut decoder = Decoder::default();
    let mut bytes = vec![0; 16384];
    let mut pending_i = None;
    let mut total = 0u64;
    let mut published = Instant::now();
    let started = Instant::now();
    let mut frequency = config.frequency;
    let mut center = frequency;
    let mut pending_retune = None;
    let mut mixer = None;
    let mut shifted = Vec::new();
    let mut gain = config.gain;
    let mut last_gain_request = gain;
    while !stop.load(Ordering::Relaxed) {
        let requested_gain = *gain_request.lock().unwrap();
        if requested_gain != last_gain_request {
            last_gain_request = requested_gain;
            let result = if requested_gain == gain {
                Ok(())
            } else {
                source.set_gain(gain, requested_gain)
            };
            match result {
                Ok(()) => {
                    gain = requested_gain;
                    state.lock().unwrap().gain_error = None;
                }
                Err(error) if config.kind == SourceKind::Tcp => {
                    return Err(crate::tr_format!(
                        "Ganancia rtl_tcp: {error}",
                        "rtl_tcp gain: {error}"
                    ));
                }
                Err(error) => {
                    state.lock().unwrap().gain_error = Some(crate::tr_format!(
                        "Ganancia RF: {error}",
                        "RF gain: {error}"
                    ))
                }
            }
        }
        let request = tune_request.swap(0, Ordering::Relaxed);
        let requested = request as u32;
        let requested_center = (request >> 32) as u32;
        if requested != 0 && (requested != frequency || requested_center != center) {
            let offset = i64::from(requested) - i64::from(requested_center);
            if offset.unsigned_abs() > u64::from(config.sample_rate / 2) {
                state.lock().unwrap().tune_error = Some(
                    crate::tr!(
                        "Frecuencia fuera del espectro recibido",
                        "Frequency outside the received spectrum"
                    )
                    .into(),
                );
                continue;
            }
            let tuner_hz = match config.tuner_frequency(requested_center) {
                Ok(tuner_hz) => tuner_hz,
                Err(error) => {
                    state.lock().unwrap().tune_error = Some(error);
                    continue;
                }
            };
            let result = if center == requested_center || config.kind == SourceKind::Demo {
                Ok(())
            } else {
                source.tune(tuner_hz)
            };
            match result {
                Ok(()) => {
                    let hardware_changed = center != requested_center;
                    frequency = requested;
                    center = requested_center;
                    mixer = (offset != 0).then(|| Mixer::new(offset, config.sample_rate));
                    dsp.retune(hardware_changed);
                    pending_retune = Some(dsp.spectrum_frame);
                    decoder = Decoder::default();
                    analog = Analog::new(config.sample_rate);
                    if mode == Mode::Digital {
                        audio = None;
                        dmr_voice = None;
                        dmr_audio_failed = false;
                    }
                    let mut state = state.lock().unwrap();
                    state.frequency = frequency;
                    state.tune_error = None;
                    if mode == Mode::Digital {
                        state.audio_error = None;
                    }
                    state.events.clear();
                    state.hits = 0;
                    state.signal_db = -120.0;
                }
                Err(error) if config.kind == SourceKind::Tcp => {
                    return Err(crate::tr_format!(
                        "Sintonía rtl_tcp: {error}",
                        "rtl_tcp tuning: {error}"
                    ));
                }
                Err(error) => state.lock().unwrap().tune_error = Some(error),
            }
        }
        let count = match source.read(&mut bytes) {
            Err(e) if e == "timeout" => continue,
            other => other?,
        };
        if count == 0 {
            return if pending_i.is_some() {
                Err(crate::tr!(
                    "Archivo o flujo IQ truncado: falta la muestra Q final",
                    "Truncated IQ file or stream: final Q sample is missing"
                )
                .into())
            } else {
                Ok("Fin de la entrada IQ".into())
            };
        }
        // TCP and files can return odd byte counts; carry the I byte across reads.
        let mut aligned = Vec::with_capacity(count + 1);
        if let Some(i) = pending_i.take() {
            aligned.push(i);
        }
        aligned.extend_from_slice(&bytes[..count]);
        if aligned.len() % 2 == 1 {
            pending_i = aligned.pop();
        }
        let settings = *radio.lock().unwrap();
        if settings.mode != mode {
            if mode.channels() != settings.mode.channels() {
                audio = None;
            }
            if settings.mode == Mode::Digital && plugins.is_empty() {
                dsp.retune(false);
                decoder = Decoder::default();
            }
            mode = settings.mode;
            dmr_voice = None;
            dmr_audio_failed = false;
            analog = Analog::new(config.sample_rate);
            let error = if mode == Mode::Digital {
                audio = None;
                None
            } else if audio.is_none() {
                match Playback::open(mode.channels()) {
                    Ok(playback) => {
                        audio = Some(playback);
                        None
                    }
                    Err(error) => Some(error),
                }
            } else {
                None
            };
            state.lock().unwrap().audio_error = error;
        }
        let mut dibits = Vec::new();
        if let Some(mixer) = &mut mixer {
            mixer.shift(&aligned, &mut shifted);
        }
        let emit = |event| match event {
            DigitalEvent::Dibit(d) => {
                decoder.push(d);
                dibits.push(d);
            }
            DigitalEvent::Dmr(sync) => decoder.dmr_sync(sync),
            DigitalEvent::DmrVoice(burst, first) if mode == Mode::Digital && !dmr_audio_failed => {
                let result = (|| {
                    if dmr_voice.is_none() {
                        dmr_voice = Some(Vocoder::open()?);
                    }
                    let volume = if settings.muted { 0.0 } else { settings.volume };
                    let pcm = dmr_voice.as_mut().unwrap().process(burst, first, volume)?;
                    if !pcm.is_empty() {
                        if audio.is_none() {
                            audio = Some(Playback::open_at(1, dmr_audio::RATE)?);
                        }
                        audio.as_mut().unwrap().send(pcm)?;
                    }
                    Ok::<(), String>(())
                })();
                if let Err(error) = result {
                    audio = None;
                    dmr_voice = None;
                    dmr_audio_failed = true;
                    state.lock().unwrap().audio_error = Some(error);
                }
            }
            DigitalEvent::DmrVoice(..) => {}
        };
        let decode = mode == Mode::Digital || !plugins.is_empty();
        if mixer.is_some() {
            dsp.process_samples(
                iq_samples(&aligned).zip(shifted.iter().copied()),
                decode,
                emit,
            );
        } else {
            dsp.process_samples(iq_samples(&aligned).map(|iq| (iq, iq)), decode, emit);
        }
        if mode != Mode::Digital {
            let pcm = if mixer.is_some() {
                analog.process_samples(shifted.iter().copied(), settings)
            } else {
                analog.process(&aligned, settings)
            };
            if let Some(playback) = &mut audio {
                if let Err(error) = playback.send(pcm) {
                    state.lock().unwrap().audio_error = Some(error);
                    audio = None;
                }
            }
        }
        for plugin in &plugins {
            plugin.process(&dibits);
        }
        total += (aligned.len() / 2) as u64;
        let eof_file = config.kind == SourceKind::File;
        if published.elapsed() >= Duration::from_millis(40) || eof_file {
            let mut state = state.lock().unwrap();
            state.publish_spectrum(&dsp, center, &mut pending_retune);
            state.samples = total;
            state.signal_db = if mode == Mode::Digital {
                dsp.spectrum.iter().copied().fold(-120.0, f32::max)
            } else {
                analog.signal_db()
            };
            state.hits = decoder.hits;
            state.events.clone_from(&decoder.events);
            published = Instant::now();
        }
        // Pace synthetic and file sources, retaining responsiveness to stop.
        if matches!(config.kind, SourceKind::Demo | SourceKind::File) {
            let target = Duration::from_secs_f64(total as f64 / config.sample_rate as f64);
            while started.elapsed() < target && !stop.load(Ordering::Relaxed) {
                thread::sleep(
                    (target - started.elapsed().min(target)).min(Duration::from_millis(10)),
                );
            }
        }
    }
    let mut state = state.lock().unwrap();
    state.samples = total;
    state.hits = decoder.hits;
    state.events.clone_from(&decoder.events);
    state.publish_spectrum(&dsp, center, &mut pending_retune);
    Ok("Desconectado".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, sync::atomic::AtomicUsize};
    static NEXT_FILE: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn retune_keeps_the_trace_until_a_complete_fft_arrives() {
        use crate::dsp::FFT_SIZE;
        let mut state = State::default();
        let mut dsp = Dsp::new(240_000);
        let mut pending = None;
        let original = 446_000_000;
        dsp.process(&[180; FFT_SIZE * 3], |_| {});
        state.publish_spectrum(&dsp, original, &mut pending);
        let old_trace = state.spectrum.clone();
        let old_frame = state.spectrum_frame;
        assert_eq!(old_frame, 1);

        let new_center = original + 12_500;
        dsp.retune(true);
        pending = Some(dsp.spectrum_frame);
        // Half a new FFT must not combine with the pre-retune samples.
        dsp.process(&[145; FFT_SIZE], |_| {});
        state.publish_spectrum(&dsp, new_center, &mut pending);
        assert_eq!(state.spectrum, old_trace);
        assert_eq!(state.spectrum_frame, old_frame);
        assert_eq!(state.spectrum_center, original);

        dsp.process(&[145; FFT_SIZE], |_| {});
        state.publish_spectrum(&dsp, new_center, &mut pending);
        assert_eq!(state.spectrum_frame, old_frame + 1);
        assert_eq!(state.spectrum_center, new_center);
        assert_ne!(state.spectrum, old_trace);

        // A software retune keeps partial samples from the same RF band.
        dsp.process(&[145; FFT_SIZE], |_| {});
        dsp.retune(false);
        pending = Some(dsp.spectrum_frame);
        dsp.process(&[145; FFT_SIZE], |_| {});
        state.publish_spectrum(&dsp, new_center, &mut pending);
        assert_eq!(state.spectrum_frame, old_frame + 2);
        assert_eq!(state.spectrum_center, new_center);
    }

    #[test]
    fn tcp_converter_tunes_the_if_and_keeps_the_rf_in_receiver_state() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let (ack, received) = std::sync::mpsc::channel();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            stream.write_all(b"RTL0\0\0\0\0\0\0\0\0").unwrap();
            let mut setup = [0; 20];
            stream.read_exact(&mut setup).unwrap();
            assert_eq!(setup[5], 1);
            assert_eq!(
                u32::from_be_bytes(setup[6..10].try_into().unwrap()),
                134_650_000
            );
            for commands in [
                &[[3, 0, 0, 0, 1], [4, 0, 0, 1, 24]][..],
                &[[4, 0, 0, 1, 88]][..],
                &[[3, 0, 0, 0, 0]][..],
            ] {
                for expected in commands {
                    let mut actual = [0; 5];
                    stream.read_exact(&mut actual).unwrap();
                    assert_eq!(actual, *expected);
                }
                ack.send(()).unwrap();
            }
            let mut tune = [0; 5];
            stream.read_exact(&mut tune).unwrap();
            assert_eq!(tune[0], 1);
            assert_eq!(
                u32::from_be_bytes(tune[1..].try_into().unwrap()),
                131_050_000
            );
            stream.write_all(&[127; 4096]).unwrap();
            assert_eq!(stream.read(&mut [0; 1]).unwrap(), 0);
        });
        let mut receiver = Receiver::default();
        receiver
            .start(
                Config {
                    kind: SourceKind::Tcp,
                    converter: source::Converter::HamItUpNano,
                    frequency: 9_650_000,
                    address,
                    ..Config::default()
                },
                vec![],
            )
            .unwrap();

        let wait = |condition: &dyn Fn(&State) -> bool| {
            let deadline = Instant::now() + Duration::from_secs(3);
            while !condition(&receiver.state.lock().unwrap()) {
                assert!(
                    Instant::now() < deadline,
                    "No se actualizó el estado del receptor"
                );
                thread::sleep(Duration::from_millis(10));
            }
        };
        wait(&|state| state.running && state.frequency == 9_650_000);
        receiver.tune(100_000_000);
        wait(&|state| state.tune_error.is_some() && state.frequency == 9_650_000);
        // The server's next command must still be the hardware retune below.
        receiver.tune_in(9_675_000, 9_650_000);
        wait(&|state| state.frequency == 9_675_000 && state.spectrum_center == 9_650_000);
        for gain in [Some(280), Some(344), None] {
            receiver.set_gain(gain);
            received.recv_timeout(Duration::from_secs(3)).unwrap();
        }
        receiver.tune(6_050_000);
        wait(&|state| {
            state.running
                && state.frequency == 6_050_000
                && state.spectrum_center == 6_050_000
                && state.samples > 0
        });
        receiver.stop();
        server.join().unwrap();
        receiver.reap();
    }

    #[test]
    fn file_lifecycle_reports_eof_and_rejects_truncated_iq() {
        for (contents, expected) in [
            (vec![128, 127, 129, 126], "Fin de la entrada IQ"),
            (vec![128], "falta la muestra Q final"),
        ] {
            let path = std::env::temp_dir().join(format!(
                "sdr-studio-{}-{}.cu8",
                std::process::id(),
                NEXT_FILE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::write(&path, contents).unwrap();
            let mut receiver = Receiver::default();
            receiver
                .start(
                    Config {
                        kind: SourceKind::File,
                        path: path.to_string_lossy().into_owned(),
                        ..Config::default()
                    },
                    vec![],
                )
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(3);
            while receiver.busy() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(5));
            }
            assert!(!receiver.busy());
            receiver.reap();
            let state = receiver.state.lock().unwrap();
            assert!(!state.running);
            assert!(state.status.contains(expected), "{}", state.status);
            fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn synthetic_receiver_can_stop_and_restart() {
        let mut receiver = Receiver::default();
        for _ in 0..2 {
            receiver.start(Config::default(), vec![]).unwrap();
            let deadline = Instant::now() + Duration::from_secs(3);
            while receiver.state.lock().unwrap().hits == 0 && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(10));
            }
            let (before_tune, epoch, previous_frame) = {
                let state = receiver.state.lock().unwrap();
                assert!(state.hits > 0);
                assert!(state.spectrum_frame > 0);
                (state.samples, state.scope_epoch, state.spectrum_frame)
            };
            receiver.tune(446_012_500);
            let retune_deadline = Instant::now() + Duration::from_secs(3);
            loop {
                {
                    let state = receiver.state.lock().unwrap();
                    if state.frequency == 446_012_500
                        && state.samples > before_tune
                        && state.spectrum_center == 446_012_500
                        && state.spectrum_frame > previous_frame
                    {
                        assert!(state.running);
                        assert_eq!(state.scope_epoch, epoch);
                        break;
                    }
                }
                assert!(Instant::now() < retune_deadline, "La sintonía no se aplicó");
                thread::sleep(Duration::from_millis(5));
            }
            receiver.stop();
            let stop_deadline = Instant::now() + Duration::from_secs(3);
            while receiver.busy() && Instant::now() < stop_deadline {
                thread::sleep(Duration::from_millis(5));
            }
            assert!(!receiver.busy());
            receiver.reap();
            assert_eq!(receiver.state.lock().unwrap().status, "Desconectado");
        }
    }
}
