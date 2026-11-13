// SPDX-License-Identifier: GPL-2.0-only

mod audio;
mod bands;
mod dmr_audio;
mod dmr_sync;
mod dsp;
mod i18n;
mod p25_nid;
mod panel;
mod plugins;
mod preferences;
mod propagation;
mod protocol;
mod receiver;
mod scanner;
mod scope;
mod source;

use audio::{Mode, Settings};
use eframe::egui::{self, Color32, RichText, Stroke, Vec2};
use i18n::Language;
use panel::{AMBER, INK};
use receiver::Receiver;
use source::{Config, Converter, SourceKind};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

const MUTED: Color32 = Color32::from_rgb(174, 164, 146);
const MIN_FREQUENCY_HZ: u32 = 1_000;
const AM_HF_START_MHZ: f64 = 9.650;

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--self-check") {
        let mut demo = source::Demo::new(240_000);
        let mut dsp = dsp::Dsp::new(240_000);
        let mut decoder = protocol::Decoder::default();
        dsp.process(&demo.generate(480_000), |event| match event {
            dsp::DigitalEvent::Dibit(d) => decoder.push(d),
            dsp::DigitalEvent::Dmr(sync) => decoder.dmr_sync(sync),
            dsp::DigitalEvent::DmrVoice(..) => {}
        });
        for event in &decoder.events {
            println!("{}: {}", event.protocol, event.text);
        }
        if !decoder.events.iter().any(|e| e.protocol == "DMR")
            || !decoder.events.iter().any(|e| e.text.contains("NAC 293"))
        {
            eprintln!("Falló la prueba IQ → símbolos → sincronización");
            std::process::exit(1);
        }
        if let Some(index) = args.iter().position(|a| a == "--plugin") {
            let path = args.get(index + 1).expect("Falta ruta del complemento");
            let plugin = plugins::Plugin::load(std::path::Path::new(path))
                .expect("No se pudo cargar complemento");
            plugin.process(&[0, 1, 2, 3]);
            println!("Complemento: {}", plugin.name);
        }
        return Ok(());
    }
    eframe::run_native(
        "SDR Studio",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1250.0, 840.0])
                .with_min_inner_size([930.0, 660.0]),
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}

struct ScanInput {
    first_mhz: f64,
    last_mhz: f64,
    step_khz: f64,
    threshold_db: f32,
    dwell_ms: u64,
    preset: Option<&'static str>,
}

struct App {
    config: Config,
    receiver: Receiver,
    frequency_mhz: f64,
    frequency_edit: Option<String>,
    bands_open: bool,
    scan_open: bool,
    propagation: propagation::Panel,
    scan_input: Option<ScanInput>,
    sweep: Option<scanner::Sweep>,
    scan_error: String,
    scan_notice: String,
    dial_remainder: f32,
    rate_edit: String,
    gain_db: f32,
    automatic_gain: bool,
    converter_shift_hz: i64,
    plugin_path: String,
    plugin_paths: Vec<PathBuf>,
    message: String,
    scope: scope::Scope,
    radio: Settings,
    squelch_level: f32,
    step_khz: f64,
    show_dmr: bool,
    show_p25: bool,
    auto_start: bool,
    preferences: Option<preferences::Store>,
    preferences_error: Option<String>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let mut style = (*cc.egui_ctx.style()).clone();
        style.spacing.item_spacing = Vec2::new(10.0, 10.0);
        style.visuals.panel_fill = Color32::from_rgb(27, 27, 26);
        style.visuals.window_fill = Color32::from_rgb(34, 33, 31);
        style.visuals.selection.bg_fill = Color32::from_rgb(115, 75, 38);
        style.visuals.override_text_color = Some(INK);
        style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(52, 50, 47);
        style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(46, 44, 42);
        style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(74, 67, 57);
        cc.egui_ctx.set_style(style);
        let demo = std::env::args().any(|a| a == "--demo");
        let hf = !demo && std::env::args().any(|a| a == "--hf");
        let (mode, frequency_mhz, step_khz) = if demo {
            (Mode::Digital, 446.0, 12.5)
        } else if hf {
            (Mode::Am, AM_HF_START_MHZ, 5.0)
        } else {
            (Mode::Wfm, 100.0, 100.0)
        };
        let mut app = Self {
            config: Config {
                kind: if demo {
                    SourceKind::Demo
                } else {
                    SourceKind::Usb
                },
                ..Config::default()
            },
            receiver: Receiver::default(),
            frequency_mhz,
            frequency_edit: None,
            bands_open: false,
            scan_open: false,
            propagation: propagation::Panel::default(),
            scan_input: None,
            sweep: None,
            scan_error: String::new(),
            scan_notice: String::new(),
            dial_remainder: 0.0,
            rate_edit: String::new(),
            gain_db: 28.0,
            automatic_gain: true,
            converter_shift_hz: Converter::NANO_LO,
            plugin_path: String::new(),
            plugin_paths: vec![],
            message: String::new(),
            scope: scope::Scope::default(),
            radio: Settings {
                mode,
                bandwidth: if hf { 8000 } else { mode.default_bandwidth() },
                ..Settings::default()
            },
            squelch_level: -45.0,
            step_khz,
            show_dmr: true,
            show_p25: true,
            auto_start: demo,
            preferences: None,
            preferences_error: None,
        };
        if let Some(path) = preferences::Store::path() {
            match preferences::Store::load(&path) {
                Ok(saved) => {
                    if saved["language"].as_str() == Some("en") {
                        Language::English.set();
                    }
                    if !demo && !hf {
                        app.restore_preferences(&saved);
                    }
                }
                Err(error) if !demo && !hf => {
                    app.preferences_error = Some(crate::tr_format!(
                        "No se pudieron leer los ajustes ({error})",
                        "Could not read settings ({error})"
                    ));
                }
                Err(_) => {}
            }
            if !demo && !hf {
                app.preferences = Some(preferences::Store::new(path, app.preferences_snapshot()));
            }
        }
        app
    }

    fn preferences_snapshot(&self) -> serde_json::Value {
        use serde_json::json;
        let (latitude, longitude) = self.propagation.location();
        json!({
            "language": Language::current().code(),
            "frequency_mhz": self.frequency_mhz,
            "step_khz": self.step_khz,
            "mode": self.radio.mode.name(),
            "bandwidth": self.radio.bandwidth,
            "volume": self.radio.volume,
            "wfm_deemphasis_us": self.radio.wfm_deemphasis_us,
            "muted": self.radio.muted,
            "squelch": self.radio.squelch.is_some(),
            "squelch_level": self.squelch_level,
            "am_agc": self.radio.am_agc,
            "cw_pitch_hz": self.radio.cw_pitch_hz,
            "source": match self.config.kind {
                SourceKind::Usb => "usb", SourceKind::Tcp => "tcp",
                SourceKind::File => "file", SourceKind::Demo => "demo",
            },
            "sample_rate": self.config.sample_rate,
            "ppm": self.config.ppm,
            "device": self.config.device,
            "address": self.config.address,
            "iq_path": self.config.path,
            "converter": match self.config.converter {
                Converter::Direct => "direct", Converter::HamItUpNano => "ham_it_up_nano",
                Converter::Custom(_) => "custom",
            },
            "converter_shift_hz": self.converter_shift_hz,
            "gain_db": self.gain_db,
            "automatic_gain": self.automatic_gain,
            "zoom": self.scope.zoom(),
            "scope_3d": self.scope.three_dimensional,
            "fixed_center_hz": self.scope.fixed_center(),
            "show_dmr": self.show_dmr,
            "show_p25": self.show_p25,
            "plugins": self.plugin_paths,
            "latitude": latitude,
            "longitude": longitude,
            "scan": self.scan_input.as_ref().map(|scan| json!({
                "first_mhz": scan.first_mhz,
                "last_mhz": scan.last_mhz,
                "step_khz": scan.step_khz,
                "threshold_db": scan.threshold_db,
                "dwell_ms": scan.dwell_ms,
                "preset": scan.preset,
            })),
        })
    }

    fn restore_preferences(&mut self, data: &serde_json::Value) {
        use preferences::get;
        let mhz = f64::from(MIN_FREQUENCY_HZ) / 1e6..=4294.0;
        self.frequency_mhz = get(data, "frequency_mhz")
            .filter(|f| mhz.contains(f))
            .unwrap_or(self.frequency_mhz);
        self.step_khz = get(data, "step_khz")
            .filter(|s| (0.001..=1_000_000.0).contains(s))
            .unwrap_or(self.step_khz);
        self.config.kind = match data["source"].as_str() {
            Some("tcp") => SourceKind::Tcp,
            Some("file") => SourceKind::File,
            Some("demo") => SourceKind::Demo,
            _ => SourceKind::Usb,
        };
        self.config.sample_rate = get(data, "sample_rate")
            .filter(|&rate| source::valid_sample_rate(rate))
            .unwrap_or(self.config.sample_rate);
        self.config.ppm = get(data, "ppm")
            .filter(|p| (-1000..=1000).contains(p))
            .unwrap_or(self.config.ppm);
        self.config.device = get(data, "device")
            .filter(|n| *n <= 64)
            .unwrap_or(self.config.device);
        if let Some(address) = get(data, "address") {
            self.config.address = address;
        }
        if let Some(path) = get(data, "iq_path") {
            self.config.path = path;
        }
        self.converter_shift_hz = get(data, "converter_shift_hz")
            .filter(|s| (-i64::from(u32::MAX)..=i64::from(u32::MAX)).contains(s))
            .unwrap_or(self.converter_shift_hz);
        self.config.converter = match data["converter"].as_str() {
            Some("ham_it_up_nano") => Converter::HamItUpNano,
            Some("custom") => Converter::Custom(self.converter_shift_hz),
            _ => Converter::Direct,
        };
        self.gain_db = get(data, "gain_db")
            .filter(|g| (0.0..=49.6).contains(g))
            .unwrap_or(self.gain_db);
        self.automatic_gain = get(data, "automatic_gain").unwrap_or(self.automatic_gain);

        if let Some(mode) = [
            Mode::Digital,
            Mode::Nfm,
            Mode::Wfm,
            Mode::Am,
            Mode::Usb,
            Mode::Lsb,
            Mode::Dsb,
            Mode::Cw,
            Mode::Raw,
        ]
        .into_iter()
        .find(|mode| Some(mode.name()) == data["mode"].as_str())
        {
            self.radio.mode = mode;
            self.radio.bandwidth = mode.default_bandwidth();
        }
        let widths = self.radio.mode.bandwidth_limits(self.config.sample_rate);
        self.radio.bandwidth = get(data, "bandwidth")
            .filter(|b| widths.contains(b))
            .unwrap_or(self.radio.bandwidth);
        self.radio.wfm_deemphasis_us = get(data, "wfm_deemphasis_us")
            .filter(|value| [0, 50, 75].contains(value))
            .unwrap_or(50);
        self.radio.volume = get(data, "volume")
            .filter(|v| (0.0..=1.0).contains(v))
            .unwrap_or(self.radio.volume);
        self.radio.muted = get(data, "muted").unwrap_or(self.radio.muted);
        self.radio.am_agc = get(data, "am_agc").unwrap_or(self.radio.am_agc);
        self.radio.cw_pitch_hz = get(data, "cw_pitch_hz")
            .filter(|p| (300..=1200).contains(p))
            .unwrap_or(self.radio.cw_pitch_hz);
        self.squelch_level = get(data, "squelch_level")
            .filter(|l| (-100.0..=0.0).contains(l))
            .unwrap_or(self.squelch_level);
        self.radio.squelch = get(data, "squelch")
            .unwrap_or(false)
            .then_some(self.squelch_level);
        self.receiver.set_radio(self.radio);

        self.scope.set_zoom(get(data, "zoom").unwrap_or(1));
        self.scope.three_dimensional = get(data, "scope_3d").unwrap_or(false);
        if self.config.kind != SourceKind::File {
            let fixed = get::<u32>(data, "fixed_center_hz")
                .filter(|&center| self.config.tuner_frequency(center).is_ok());
            self.scope.set_fixed_center(fixed);
            let hz = (self.frequency_mhz * 1e6).round() as u32;
            if self
                .scope
                .tuning_center(hz, self.config.sample_rate)
                .is_err()
            {
                self.scope.set_fixed_center(None);
            }
        }
        self.show_dmr = get(data, "show_dmr").unwrap_or(self.show_dmr);
        self.show_p25 = get(data, "show_p25").unwrap_or(self.show_p25);
        self.plugin_paths = get(data, "plugins").unwrap_or_default();
        self.propagation.set_location(
            get(data, "latitude").unwrap_or_default(),
            get(data, "longitude").unwrap_or_default(),
        );
        let scan = &data["scan"];
        if let (Some(first_mhz), Some(last_mhz)) = (get(scan, "first_mhz"), get(scan, "last_mhz")) {
            if mhz.contains(&first_mhz) && mhz.contains(&last_mhz) {
                let preset = bands::GROUPS
                    .iter()
                    .flat_map(|(_, group)| group.iter())
                    .map(|band| band.label)
                    .find(|label| Some(*label) == scan["preset"].as_str());
                self.scan_input = Some(ScanInput {
                    first_mhz,
                    last_mhz,
                    step_khz: get(scan, "step_khz")
                        .filter(|s| (0.001..=1_000_000.0).contains(s))
                        .unwrap_or(self.step_khz),
                    threshold_db: get(scan, "threshold_db")
                        .filter(|db| (-120.0..=0.0).contains(db))
                        .unwrap_or(-48.0),
                    dwell_ms: get(scan, "dwell_ms")
                        .filter(|ms| (80..=2000).contains(ms))
                        .unwrap_or(180),
                    preset,
                });
            }
        }
    }

    fn save_preferences(&mut self, closing: bool) {
        if self.preferences.is_none() {
            return;
        }
        let snapshot = self.preferences_snapshot();
        match self.preferences.as_mut().unwrap().save(snapshot, closing) {
            Ok(true) => self.preferences_error = None,
            Ok(false) => {}
            Err(error) => {
                let message = crate::tr_format!(
                    "No se pudieron guardar los ajustes ({error})",
                    "Could not save settings ({error})"
                );
                if closing {
                    eprintln!("{message}");
                }
                self.preferences_error = Some(message);
            }
        }
    }

    fn connect(&mut self) {
        let min_mhz = f64::from(MIN_FREQUENCY_HZ) / 1e6;
        if !self.frequency_mhz.is_finite() || !(min_mhz..=4294.0).contains(&self.frequency_mhz) {
            self.message = crate::tr!("Frecuencia fuera de rango", "Frequency out of range").into();
            return;
        }
        let frequency = (self.frequency_mhz * 1e6).round() as u32;
        let center = match self.scope.tuning_center(frequency, self.config.sample_rate) {
            Ok(center) => center,
            Err(error) => {
                self.message = error;
                return;
            }
        };
        self.config.frequency = center;
        self.receiver.set_radio(self.radio);
        self.config.gain = self.tuner_gain();
        match self
            .receiver
            .start(self.config.clone(), self.plugin_paths.clone())
        {
            Ok(()) => {
                self.message.clear();
                if frequency != center {
                    self.receiver.tune_in(frequency, center);
                }
            }
            Err(error) => self.message = error,
        }
    }

    fn front_panel(&mut self, ui: &mut egui::Ui, busy: bool, receiving: bool, signal_db: f32) {
        let bezel = egui::Frame::new()
            .fill(Color32::from_rgb(45, 44, 41))
            .stroke(Stroke::new(1.0_f32, Color32::from_gray(90)))
            .corner_radius(7)
            .inner_margin(egui::Margin::symmetric(14, 10));
        bezel.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("SDR STUDIO").strong().size(20.0));
                let source = match self.config.kind {
                    SourceKind::Usb => "RTL-SDR · USB",
                    SourceKind::Tcp => "RTL-SDR · TCP",
                    SourceKind::File => crate::tr!("Archivo IQ", "IQ file"),
                    SourceKind::Demo => crate::tr!("Demostración", "Demo"),
                };
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(source).color(MUTED).monospace());
                });
            });
            ui.separator();
            let editable = !busy || self.config.kind != SourceKind::File;
            let mut power_pressed = false;
            let step_mhz = self.step_khz / 1000.0;
            let before = self.frequency_mhz;
            let position =
                ((before / step_mhz).rem_euclid(36.0) / 36.0) as f32 * std::f32::consts::TAU;
            let width = ui.available_width();
            let display_width =
                (width - 92.0 - 174.0 - 152.0 - 3.0 * ui.spacing().item_spacing.x).max(300.0);
            ui.allocate_ui_with_layout(
                Vec2::new(width, 154.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.allocate_ui_with_layout(
                        Vec2::new(92.0, 100.0),
                        egui::Layout::top_down(egui::Align::Center),
                        |ui| {
                            let (led, _) =
                                ui.allocate_exact_size(Vec2::new(12.0, 12.0), egui::Sense::hover());
                            ui.painter().circle_filled(
                                led.center(),
                                5.0,
                                if receiving {
                                    AMBER
                                } else {
                                    Color32::from_rgb(81, 67, 52)
                                },
                            );
                            power_pressed = ui
                                .add_sized(
                                    [87.0, 34.0],
                                    egui::Button::new(if busy {
                                        crate::tr!("Apagar", "Power off")
                                    } else {
                                        crate::tr!("Encender", "Power on")
                                    })
                                    .fill(Color32::from_rgb(32, 31, 30))
                                    .stroke(Stroke::new(1.0_f32, Color32::from_gray(102))),
                                )
                                .clicked();
                        },
                    );
                    panel::meter(ui, signal_db, self.radio.mode == Mode::Digital, receiving);
                    if let Some(value) = panel::readout(
                        ui,
                        display_width,
                        self.frequency_mhz,
                        self.radio.mode.name(),
                        receiving,
                        editable,
                        &mut self.frequency_edit,
                    ) {
                        self.frequency_mhz = value;
                    }
                    let steps = ui
                        .add_enabled_ui(editable, |ui| {
                            panel::dial(ui, position, &mut self.dial_remainder)
                        })
                        .inner;
                    self.frequency_mhz += f64::from(steps) * step_mhz;
                },
            );
            ui.separator();
            ui.horizontal(|ui| {
                self.mode_controls(ui);
                ui.separator();
                self.step_control(ui);
                ui.separator();
                ui.add_enabled_ui(editable, |ui| {
                    if ui
                        .add_sized([42.0, 28.0], egui::Button::new("−"))
                        .on_hover_text(crate::tr!("Bajar un paso", "Tune down one step"))
                        .clicked()
                    {
                        self.frequency_mhz -= step_mhz;
                    }
                    if ui
                        .add_sized([42.0, 28.0], egui::Button::new("+"))
                        .on_hover_text(crate::tr!("Subir un paso", "Tune up one step"))
                        .clicked()
                    {
                        self.frequency_mhz += step_mhz;
                    }
                    ui.separator();
                    if ui
                        .add_sized(
                            [122.0, 28.0],
                            egui::Button::new(crate::tr!("Bandas", "Bands")),
                        )
                        .clicked()
                    {
                        self.bands_open = true;
                    }
                });
            });
            self.apply_frequency(before, self.frequency_mhz, busy);
            if power_pressed {
                if busy {
                    self.cancel_scan(crate::tr!("Barrido detenido", "Scan stopped"));
                    self.receiver.stop();
                } else {
                    self.connect();
                }
            }
        });
    }

    fn apply_frequency(&mut self, before: f64, requested: f64, busy: bool) {
        self.frequency_mhz = if requested.is_finite() {
            requested.clamp(f64::from(MIN_FREQUENCY_HZ) / 1e6, 4294.0)
        } else {
            before
        };
        if self.frequency_mhz != before {
            let rf_hz = (self.frequency_mhz * 1e6).round() as u32;
            let center = self.scope.tuning_center(rf_hz, self.config.sample_rate);
            match center.and_then(|center| {
                if busy {
                    self.config.tuner_frequency(center)?;
                }
                Ok(center)
            }) {
                Ok(center) => {
                    self.message.clear();
                    if busy {
                        self.cancel_scan(crate::tr!(
                            "Barrido detenido por sintonía manual",
                            "Scan stopped by manual tuning"
                        ));
                        self.receiver.tune_in(rf_hz, center);
                    }
                }
                Err(error) => {
                    self.frequency_mhz = before;
                    self.message = error;
                }
            }
        }
    }

    fn mode_controls(&mut self, ui: &mut egui::Ui) {
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            for mode in [
                Mode::Wfm,
                Mode::Nfm,
                Mode::Am,
                Mode::Usb,
                Mode::Lsb,
                Mode::Dsb,
                Mode::Cw,
                Mode::Raw,
                Mode::Digital,
            ] {
                let selected = self.radio.mode == mode;
                let text = RichText::new(mode.name()).monospace().color(if selected {
                    AMBER
                } else {
                    INK
                });
                let button = egui::Button::new(text)
                    .fill(if selected {
                        Color32::from_rgb(61, 42, 24)
                    } else {
                        Color32::from_rgb(30, 29, 28)
                    })
                    .stroke(Stroke::new(
                        1.0_f32,
                        if selected {
                            Color32::from_rgb(143, 94, 48)
                        } else {
                            Color32::from_gray(86)
                        },
                    ));
                let width = if mode == Mode::Digital { 60.0 } else { 44.0 };
                if ui.add_sized([width, 28.0], button).clicked() && !selected {
                    self.radio.mode = mode;
                    self.radio.bandwidth = mode.default_bandwidth();
                    self.step_khz = match mode {
                        Mode::Wfm => 100.0,
                        Mode::Am => 5.0,
                        Mode::Nfm | Mode::Digital => 12.5,
                        Mode::Usb | Mode::Lsb | Mode::Dsb => 0.1,
                        Mode::Cw => 0.01,
                        Mode::Raw => 1.0,
                    };
                    self.receiver.set_radio(self.radio);
                }
            }
        });
    }

    fn step_control(&mut self, ui: &mut egui::Ui) {
        let label = |step: f64| {
            if step < 1.0 {
                format!("{:.0} Hz", step * 1000.0)
            } else {
                format!("{step} kHz")
            }
        };
        ui.label(
            RichText::new(crate::tr!("PASO", "STEP"))
                .monospace()
                .small()
                .color(MUTED),
        );
        egui::ComboBox::from_id_salt("step")
            .selected_text(label(self.step_khz))
            .show_ui(ui, |ui| {
                for step in [
                    0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 6.25, 9.0, 10.0, 12.5, 25.0, 50.0, 100.0, 200.0,
                ] {
                    ui.selectable_value(&mut self.step_khz, step, label(step));
                }
            });
    }

    fn band_menu(&mut self, ctx: &egui::Context, busy: bool) {
        if !self.bands_open {
            return;
        }
        let current_hz = (self.frequency_mhz * 1e6).round() as u32;
        let mut picked = None;
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("band_selector")).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(crate::tr!("Bandas de recepción", "Receive bands"));
                ui.label(RichText::new("100 kHz – 1 GHz").monospace().color(AMBER));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close = ui.button(crate::tr!("Cerrar", "Close")).clicked();
                });
            });
            ui.separator();
            ui.spacing_mut().item_spacing = Vec2::new(9.0, 2.0);
            ui.horizontal_top(|ui| {
                for (title, entries) in bands::GROUPS {
                    ui.vertical(|ui| {
                        ui.set_width(186.0);
                        ui.label(RichText::new(i18n::label(title)).monospace().color(AMBER));
                        ui.add_space(3.0);
                        for band in entries {
                            let preset = band.preset;
                            let active =
                                band.contains(current_hz) && preset.mode == self.radio.mode;
                            let mode = if preset.mode == Mode::Digital {
                                "RF"
                            } else {
                                preset.mode.name()
                            };
                            let label = format!(
                                "{}\n{} · {mode}",
                                i18n::label(band.label),
                                band.range_label()
                            );
                            let compatible = self.config.tuner_frequency(band.center_hz()).is_ok();
                            let button =
                                egui::Button::new(RichText::new(label).size(11.0)).selected(active);
                            let response = ui
                                .add_enabled_ui(compatible, |ui| {
                                    ui.add_sized([186.0, 28.0], button)
                                })
                                .inner;
                            if !compatible {
                                response.on_disabled_hover_text(crate::tr!(
                                    "Fuera del rango del conversor seleccionado",
                                    "Outside the selected converter range"
                                ));
                            } else if response
                                .on_hover_text(crate::tr_format!(
                                    "Paso {} kHz · Filtro {} kHz",
                                    "Step {} kHz · Filter {} kHz",
                                    f64::from(preset.step_hz) / 1000.0,
                                    f64::from(preset.filter_hz) / 1000.0,
                                ))
                                .clicked()
                            {
                                picked = Some(band);
                            }
                        }
                    });
                }
            });
        });
        if let Some(band) = picked {
            self.apply_band(band, busy);
            close = true;
        }
        self.bands_open = !close && !modal.should_close();
    }

    fn apply_band(&mut self, band: &bands::Band, busy: bool) {
        self.cancel_scan(crate::tr!(
            "Barrido detenido por cambio de banda",
            "Scan stopped by band change"
        ));
        let preset = band.preset;
        self.radio.mode = preset.mode;
        self.radio.bandwidth = preset.filter_hz;
        self.step_khz = f64::from(preset.step_hz) / 1000.0;
        if let Some((first, last)) = self
            .config
            .scan_limits(band.low_khz * 1000, band.high_khz * 1000)
        {
            let (threshold_db, dwell_ms) = self
                .scan_input
                .as_ref()
                .map_or((-48.0, 180), |input| (input.threshold_db, input.dwell_ms));
            self.scan_input = Some(ScanInput {
                first_mhz: f64::from(first) / 1e6,
                last_mhz: f64::from(last) / 1e6,
                step_khz: self.step_khz,
                threshold_db,
                dwell_ms,
                preset: Some(band.label),
            });
        }
        if !busy && self.config.kind != SourceKind::File {
            self.config.sample_rate = preset.sample_rate;
        }
        self.scope.set_span(self.config.sample_rate, preset.span_hz);
        self.receiver.set_radio(self.radio);
        let center = band.center_hz();
        let old_center = self.scope.fixed_center();
        if old_center.is_some() {
            self.scope.set_fixed_center(Some(center));
        }
        let before = self.frequency_mhz;
        let requested = f64::from(center) / 1e6;
        self.apply_frequency(before, requested, busy);
        if self.frequency_mhz != requested {
            self.scope.set_fixed_center(old_center);
        } else if busy && before == requested && old_center.is_some_and(|old| old != center) {
            self.receiver.tune(center);
        }
    }

    fn select_am_hf(&mut self, frequency_mhz: f64) {
        self.frequency_mhz = frequency_mhz;
        self.radio.mode = Mode::Am;
        self.radio.bandwidth = 8000;
        self.step_khz = 5.0;
        self.receiver.set_radio(self.radio);
    }

    fn converter_controls(&mut self, ui: &mut egui::Ui) {
        let before = self.config.converter;
        let name = match before {
            Converter::Direct => crate::tr!("Directo", "Direct"),
            Converter::HamItUpNano => "Ham It Up Nano · +125 MHz",
            Converter::Custom(_) => crate::tr!("Desplazamiento personalizado", "Custom offset"),
        };
        ui.label(crate::tr!("Conversor externo", "External converter"));
        egui::ComboBox::from_id_salt("converter")
            .selected_text(name)
            .width(218.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut self.config.converter,
                    Converter::Direct,
                    crate::tr!("Directo", "Direct"),
                );
                ui.selectable_value(
                    &mut self.config.converter,
                    Converter::HamItUpNano,
                    "Ham It Up Nano · +125 MHz",
                );
                ui.selectable_value(
                    &mut self.config.converter,
                    Converter::Custom(self.converter_shift_hz),
                    crate::tr!("Desplazamiento personalizado", "Custom offset"),
                );
            });
        if matches!(self.config.converter, Converter::Custom(_)) {
            ui.add(
                egui::DragValue::new(&mut self.converter_shift_hz)
                    .range(-i64::from(u32::MAX)..=i64::from(u32::MAX))
                    .speed(1000.0)
                    .prefix(crate::tr!("Desplazamiento: ", "Offset:"))
                    .suffix(" Hz"),
            )
            .on_hover_text(crate::tr!(
                "Frecuencia del SDR = frecuencia del visor + desplazamiento",
                "SDR frequency = dial frequency + offset"
            ));
            self.config.converter = Converter::Custom(self.converter_shift_hz);
        }
        if self.config.converter != before {
            self.scope.set_fixed_center(None);
            self.message.clear();
            if self.config.converter == Converter::HamItUpNano
                && self
                    .config
                    .tuner_frequency((self.frequency_mhz * 1e6).round() as u32)
                    .is_err()
            {
                self.select_am_hf(AM_HF_START_MHZ);
            }
        }
        if self.config.converter != Converter::Direct {
            match self
                .config
                .tuner_frequency((self.frequency_mhz * 1e6).round() as u32)
            {
                Ok(hz) => ui.label(crate::tr_format!(
                    "Sintonizador: {:.6} MHz",
                    "Tuner: {:.6} MHz",
                    f64::from(hz) / 1e6
                )),
                Err(error) => ui.colored_label(Color32::LIGHT_RED, error),
            };
        }
    }

    fn radio_controls(&mut self, ui: &mut egui::Ui) {
        let before = self.radio;
        ui.add_enabled_ui(self.radio.mode != Mode::Digital, |ui| {
            ui.horizontal(|ui| {
                ui.label(crate::tr!("Filtro", "Filter"));
                ui.add(
                    egui::DragValue::new(&mut self.radio.bandwidth)
                        .range(self.radio.mode.bandwidth_limits(self.config.sample_rate))
                        .speed(match self.radio.mode {
                            Mode::Cw => 10.0,
                            Mode::Usb | Mode::Lsb | Mode::Dsb => 100.0,
                            _ => 500.0,
                        })
                        .suffix(" Hz"),
                );
            });
            if self.radio.mode == Mode::Nfm {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.radio.bandwidth, 12_500, "12.5 kHz");
                    ui.selectable_value(
                        &mut self.radio.bandwidth,
                        16_000,
                        crate::tr!("Marina · 16 kHz", "Marine · 16 kHz"),
                    );
                });
            }
            if self.radio.mode == Mode::Wfm {
                ui.horizontal(|ui| {
                    ui.label(crate::tr!("Deénfasis", "De-emphasis"));
                    for (value, label) in
                        [(0, crate::tr!("Sin", "Off")), (50, "50 µs"), (75, "75 µs")]
                    {
                        ui.selectable_value(&mut self.radio.wfm_deemphasis_us, value, label);
                    }
                });
            }
            if self.radio.mode == Mode::Am {
                ui.horizontal(|ui| {
                    for (label, width) in [
                        ("4 kHz", 4000),
                        ("6 kHz", 6000),
                        ("8 kHz", 8000),
                        ("10 kHz", 10_000),
                    ] {
                        ui.selectable_value(&mut self.radio.bandwidth, width, label);
                    }
                });
                ui.checkbox(
                    &mut self.radio.am_agc,
                    crate::tr!("Nivel automático de audio AM", "Automatic AM audio level"),
                );
            }
            if self.radio.mode == Mode::Cw {
                ui.horizontal(|ui| {
                    ui.label(crate::tr!("Tono CW", "CW tone"));
                    ui.add(
                        egui::DragValue::new(&mut self.radio.cw_pitch_hz)
                            .range(300..=1200)
                            .speed(10.0)
                            .suffix(" Hz"),
                    );
                });
            }
            if self.radio.mode == Mode::Raw {
                ui.label(
                    RichText::new(crate::tr!(
                        "I izquierda · Q derecha · 48 kHz",
                        "I left · Q right · 48 kHz"
                    ))
                    .small()
                    .color(MUTED),
                );
            }
            let mut squelch = self.radio.squelch.is_some();
            ui.checkbox(&mut squelch, "Squelch");
            ui.add_enabled(
                squelch,
                egui::Slider::new(&mut self.squelch_level, -100.0..=0.0).suffix(" dBFS"),
            );
            self.radio.squelch = squelch.then_some(self.squelch_level);
        });
        ui.add(
            egui::Slider::new(&mut self.radio.volume, 0.0..=1.0)
                .text(crate::tr!("Volumen", "Volume")),
        );
        ui.checkbox(&mut self.radio.muted, crate::tr!("Silenciar", "Mute"));
        if self.radio != before {
            self.receiver.set_radio(self.radio);
        }
    }

    fn scan_controls(&mut self, ui: &mut egui::Ui) {
        let can_scan = self.config.kind != SourceKind::File;
        if ui
            .add_enabled(
                can_scan && self.sweep.is_none(),
                egui::Button::new(crate::tr!("Escanear rango…", "Scan range…")),
            )
            .on_disabled_hover_text(crate::tr!(
                "Disponible con una fuente que permita sintonizar",
                "Available with a tunable source"
            ))
            .clicked()
        {
            if self.scan_input.is_none() {
                self.scan_input = Some(ScanInput {
                    first_mhz: (self.frequency_mhz - 0.1).max(f64::from(MIN_FREQUENCY_HZ) / 1e6),
                    last_mhz: (self.frequency_mhz + 0.1).min(4294.0),
                    step_khz: self.step_khz,
                    threshold_db: -48.0,
                    dwell_ms: 180,
                    preset: None,
                });
            }
            self.scan_error.clear();
            self.scan_open = true;
        }
        if let Some(sweep) = &mut self.sweep {
            let (channel, total) = sweep.progress();
            if let Some(level) = sweep.held_level() {
                ui.label(
                    RichText::new(crate::tr_format!(
                        "Señal · {level:.0} dBFS/bin",
                        "Signal · {level:.0} dBFS/bin"
                    ))
                    .color(AMBER),
                );
            }
            ui.label(crate::tr_format!(
                "Canal {channel} de {total}",
                "Channel {channel} of {total}"
            ));
            let mut next = scanner::Update::None;
            let mut stop = false;
            ui.horizontal(|ui| {
                if sweep.held_level().is_some()
                    && ui.button(crate::tr!("Continuar", "Continue")).clicked()
                {
                    next = sweep.resume(Instant::now());
                }
                stop = ui
                    .button(crate::tr!("Detener barrido", "Stop scan"))
                    .clicked();
            });
            if stop {
                self.cancel_scan(crate::tr!("Barrido detenido", "Scan stopped"));
            } else {
                self.apply_scan_update(next);
            }
        } else if !self.scan_notice.is_empty() {
            ui.label(RichText::new(&self.scan_notice).small().color(MUTED));
        }
        ui.separator();
    }

    fn scan_dialog(&mut self, ctx: &egui::Context, busy: bool) {
        if !self.scan_open {
            return;
        }
        let input = self.scan_input.as_mut().unwrap();
        let mut begin = false;
        let mut close = false;
        let dialog = egui::Modal::new(egui::Id::new("scan_range")).show(ctx, |ui| {
            ui.heading(crate::tr!("Escanear rango", "Scan range"));
            if let Some(preset) = input.preset {
                let preset = i18n::label(preset);
                ui.label(
                    RichText::new(crate::tr_format!("Preajuste: {preset}", "Preset: {preset}"))
                        .color(AMBER),
                );
            }
            ui.add_space(8.0);
            egui::Grid::new("scan_parameters")
                .spacing([14.0, 9.0])
                .show(ui, |ui| {
                    let min_mhz = f64::from(MIN_FREQUENCY_HZ) / 1e6;
                    for (label, value) in [
                        (crate::tr!("Inicio", "Start"), &mut input.first_mhz),
                        (crate::tr!("Fin", "End"), &mut input.last_mhz),
                    ] {
                        ui.label(label);
                        ui.add(
                            egui::DragValue::new(value)
                                .range(min_mhz..=4294.0)
                                .fixed_decimals(6)
                                .speed(0.001)
                                .suffix(" MHz")
                                .custom_parser(|s| s.trim().replace(',', ".").parse().ok()),
                        );
                        ui.end_row();
                    }
                    ui.label(crate::tr!("Paso", "Step"));
                    ui.add(
                        egui::DragValue::new(&mut input.step_khz)
                            .range(0.001..=1_000_000.0)
                            .max_decimals(3)
                            .speed(0.1)
                            .suffix(" kHz")
                            .custom_parser(|s| s.trim().replace(',', ".").parse().ok()),
                    );
                    ui.end_row();
                    ui.label(crate::tr!("Umbral de pausa", "Hold threshold"));
                    ui.add(
                        egui::DragValue::new(&mut input.threshold_db)
                            .range(-120.0..=0.0)
                            .speed(1.0)
                            .suffix(" dBFS/bin"),
                    );
                    ui.end_row();
                    ui.label(crate::tr!("Espera por canal", "Dwell per channel"));
                    ui.add(
                        egui::DragValue::new(&mut input.dwell_ms)
                            .range(80..=2000)
                            .speed(10.0)
                            .suffix(" ms"),
                    );
                    ui.end_row();
                });
            if !self.scan_error.is_empty() {
                ui.colored_label(Color32::LIGHT_RED, &self.scan_error);
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                begin = ui
                    .button(crate::tr!("Iniciar barrido", "Start scan"))
                    .clicked();
                close = ui.button(crate::tr!("Cancelar", "Cancel")).clicked();
            });
        });
        if begin {
            match self.begin_scan(busy) {
                Ok(()) => close = true,
                Err(error) => self.scan_error = error,
            }
        }
        self.scan_open = !close && !dialog.should_close();
    }

    fn begin_scan(&mut self, busy: bool) -> Result<(), String> {
        let input = self.scan_input.as_ref().unwrap();
        if !input.first_mhz.is_finite()
            || !input.last_mhz.is_finite()
            || !input.step_khz.is_finite()
            || !input.threshold_db.is_finite()
        {
            return Err(
                crate::tr!("Introduce valores numéricos válidos", "Enter valid numbers").into(),
            );
        }
        let first = (input.first_mhz * 1e6).round() as u32;
        let last = (input.last_mhz * 1e6).round() as u32;
        let step = (input.step_khz * 1000.0).round() as u32;
        if first < MIN_FREQUENCY_HZ || last < first || step == 0 {
            return Err(crate::tr!(
                "El inicio debe ser menor o igual al fin y el paso debe ser positivo",
                "Start must be at or below end, and step must be positive"
            )
            .into());
        }
        if self.config.kind == SourceKind::File {
            return Err(crate::tr!(
                "El archivo IQ tiene una frecuencia fija",
                "The IQ file has a fixed frequency"
            )
            .into());
        }
        self.config.tuner_frequency(first)?;
        self.config.tuner_frequency(last)?;
        let range = scanner::Range {
            first,
            last,
            step,
            threshold_db: input.threshold_db,
            settle: Duration::from_millis(input.dwell_ms),
        };
        let before = self.frequency_mhz;
        let fixed = self.scope.fixed_center();
        self.scope.set_fixed_center(None);
        self.frequency_mhz = f64::from(first) / 1e6;
        self.message.clear();
        if busy {
            self.receiver.tune(first);
        } else {
            self.connect();
            if !self.message.is_empty() {
                self.frequency_mhz = before;
                self.scope.set_fixed_center(fixed);
                return Err(self.message.clone());
            }
        }
        self.sweep = Some(scanner::Sweep::new(range, Instant::now()));
        self.scan_notice.clear();
        Ok(())
    }

    fn update_scan(&mut self) {
        let Some(sweep) = &mut self.sweep else { return };
        let update = {
            let state = self.receiver.state.lock().unwrap();
            sweep.poll(
                &state,
                self.config.sample_rate,
                self.radio.mode.filter_edges(self.radio.bandwidth),
                Instant::now(),
            )
        };
        self.apply_scan_update(update);
    }

    fn apply_scan_update(&mut self, update: scanner::Update) {
        match update {
            scanner::Update::None => {}
            scanner::Update::Tune(hz) => {
                self.frequency_mhz = f64::from(hz) / 1e6;
                self.receiver.tune(hz);
            }
            scanner::Update::Finished => {
                self.cancel_scan(crate::tr!("Barrido completo", "Scan complete"))
            }
            scanner::Update::Failed(error) => {
                let frequency = self.receiver.state.lock().unwrap().frequency;
                if frequency != 0 {
                    self.frequency_mhz = f64::from(frequency) / 1e6;
                }
                self.cancel_scan(&crate::tr_format!(
                    "Barrido interrumpido: {error}",
                    "Scan interrupted: {error}"
                ));
            }
        }
    }

    fn cancel_scan(&mut self, notice: &str) {
        if self.sweep.take().is_some() {
            self.scan_notice = notice.into();
        }
    }

    fn tuner_gain(&self) -> Option<i32> {
        (!self.automatic_gain).then(|| (self.gain_db * 10.0).round() as i32)
    }

    fn source_controls(&mut self, ui: &mut egui::Ui, busy: bool) {
        ui.add_enabled_ui(!busy, |ui| {
            let previous_kind = self.config.kind;
            let previous_rate = self.config.sample_rate;
            egui::ComboBox::from_id_salt("source")
                .selected_text(match self.config.kind {
                    SourceKind::Usb => "RTL-SDR · USB",
                    SourceKind::Tcp => "RTL-SDR · TCP",
                    SourceKind::File => crate::tr!("Archivo IQ · CU8", "IQ file · CU8"),
                    SourceKind::Demo => crate::tr!("Demostración sintética", "Synthetic demo"),
                })
                .width(218.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.config.kind, SourceKind::Usb, "RTL-SDR · USB");
                    ui.selectable_value(&mut self.config.kind, SourceKind::Tcp, "RTL-SDR · TCP");
                    ui.selectable_value(
                        &mut self.config.kind,
                        SourceKind::File,
                        crate::tr!("Archivo IQ · CU8", "IQ file · CU8"),
                    );
                    ui.selectable_value(
                        &mut self.config.kind,
                        SourceKind::Demo,
                        crate::tr!("Demostración sintética", "Synthetic demo"),
                    );
                });
            if matches!(self.config.kind, SourceKind::Usb | SourceKind::Tcp) {
                self.converter_controls(ui);
            }
            if self.config.kind == SourceKind::Usb {
                ui.horizontal(|ui| {
                    ui.label(crate::tr!("Índice USB", "USB device index"));
                    ui.add(egui::DragValue::new(&mut self.config.device).range(0..=64));
                });
            }
            if self.config.kind == SourceKind::Tcp {
                ui.label(crate::tr!("Servidor host:puerto", "Server host:port"));
                ui.text_edit_singleline(&mut self.config.address);
            }
            if self.config.kind == SourceKind::File {
                ui.label(crate::tr!(
                    "Ruta absoluta del archivo CU8",
                    "Absolute path to CU8 file"
                ));
                ui.text_edit_singleline(&mut self.config.path);
            }
            ui.add_space(10.0);
            ui.label(crate::tr!("Muestras por segundo", "Samples per second"));
            egui::ComboBox::from_id_salt("rate")
                .selected_text(format!("{} S/s", self.config.sample_rate))
                .show_ui(ui, |ui| {
                    for rate in [
                        240_000, 250_000, 256_000, 960_000, 1_024_000, 2_048_000, 2_400_000,
                        2_560_000, 2_800_000, 3_200_000,
                    ] {
                        ui.selectable_value(
                            &mut self.config.sample_rate,
                            rate,
                            format!("{rate} S/s"),
                        );
                    }
                });
            if self.config.sample_rate != previous_rate {
                self.rate_edit.clear();
                self.message.clear();
            }
            ui.horizontal(|ui| {
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut self.rate_edit)
                        .id_salt("custom_sample_rate")
                        .desired_width(126.0)
                        .hint_text(crate::tr!("Tasa en S/s", "Rate in S/s")),
                );
                let enter =
                    edit.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                if ui.button(crate::tr!("Aplicar", "Apply")).clicked() || enter {
                    let normalized: String = self
                        .rate_edit
                        .chars()
                        .filter(|c| !c.is_whitespace() && *c != '_')
                        .collect();
                    match normalized.parse::<u32>() {
                        Ok(rate) if source::valid_sample_rate(rate) => {
                            self.config.sample_rate = rate;
                            self.rate_edit.clear();
                            self.message.clear();
                        }
                        _ => self.message = source::sample_rate_error().into(),
                    }
                }
            });
            if self.config.kind != previous_kind || self.config.sample_rate != previous_rate {
                self.scope.set_fixed_center(None);
            }
            ui.horizontal(|ui| {
                ui.label(crate::tr!("Corrección · PPM", "PPM correction"));
                ui.add(egui::DragValue::new(&mut self.config.ppm).range(-1000..=1000));
            });
        });
        if matches!(self.config.kind, SourceKind::Usb | SourceKind::Tcp) {
            let before = self.tuner_gain();
            ui.checkbox(
                &mut self.automatic_gain,
                crate::tr!(
                    "Ganancia automática del sintonizador",
                    "Automatic tuner gain"
                ),
            );
            ui.add_enabled(
                !self.automatic_gain,
                egui::Slider::new(&mut self.gain_db, 0.0..=49.6)
                    .fixed_decimals(1)
                    .suffix(" dB")
                    .text(crate::tr!("Ganancia RF", "RF gain")),
            );
            let gain = self.tuner_gain();
            if busy && gain != before {
                self.receiver.set_gain(gain);
            }
        }
    }

    fn controls(&mut self, ui: &mut egui::Ui, busy: bool) {
        ui.horizontal(|ui| {
            ui.label(crate::tr!("Idioma", "Language"));
            let mut language = Language::current();
            egui::ComboBox::from_id_salt("language")
                .selected_text(match language {
                    Language::Spanish => "Español",
                    Language::English => "English",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut language, Language::Spanish, "Español");
                    ui.selectable_value(&mut language, Language::English, "English");
                });
            language.set();
        });
        ui.separator();
        self.propagation.button(ui);
        self.scan_controls(ui);
        egui::CollapsingHeader::new(crate::tr!("Fuente", "Source"))
            .id_salt("source_controls")
            .default_open(true)
            .show(ui, |ui| self.source_controls(ui, busy));
        ui.separator();
        egui::CollapsingHeader::new(crate::tr!("Radio", "Radio"))
            .id_salt("radio_controls")
            .default_open(true)
            .show(ui, |ui| self.radio_controls(ui));
        ui.add_space(8.0);
        ui.separator();
        ui.label(
            RichText::new(crate::tr!(
                "ANÁLISIS NATIVO · EXPERIMENTAL",
                "NATIVE ANALYSIS · EXPERIMENTAL"
            ))
            .color(MUTED)
            .size(11.0),
        );
        ui.checkbox(
            &mut self.show_dmr,
            crate::tr!("Mostrar sincronismos DMR", "Show DMR sync"),
        );
        ui.checkbox(
            &mut self.show_p25,
            crate::tr!("Mostrar P25 Phase 1 / NID", "Show P25 Phase 1 / NID"),
        );

        ui.separator();
        ui.label(
            RichText::new(crate::tr!("COMPLEMENTOS", "PLUGINS"))
                .color(MUTED)
                .size(12.0),
        );
        ui.add_enabled_ui(!busy, |ui| {
            ui.label(crate::tr!(
                "Ruta absoluta de biblioteca .so",
                "Absolute path to .so library"
            ));
            ui.text_edit_singleline(&mut self.plugin_path);
            if ui
                .button(crate::tr!(
                    "+ Añadir al próximo inicio",
                    "+ Add for next start"
                ))
                .clicked()
            {
                let path = PathBuf::from(self.plugin_path.trim());
                if !path.is_absolute() || !path.is_file() {
                    self.message = crate::tr!(
                        "El complemento debe ser un archivo existente con ruta absoluta",
                        "The plugin must be an existing file with an absolute path"
                    )
                    .into();
                } else if !self.plugin_paths.contains(&path) {
                    self.plugin_paths.push(path);
                    self.plugin_path.clear();
                }
            }
            let mut remove = None;
            for (i, path) in self.plugin_paths.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(path.file_name().unwrap_or_default().to_string_lossy());
                    if ui.small_button("×").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                self.plugin_paths.remove(i);
            }
        });
        ui.label(RichText::new(crate::tr!("ABI v1 · dibits a 4800 símbolos/s. Carga solo código de confianza: se ejecuta dentro del receptor.", "ABI v1 · 4800 symbols/s. Only load trusted plugins. They run inside the receiver.")).small().color(MUTED));
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.receiver.reap();
        if self.auto_start {
            self.auto_start = false;
            self.connect();
        }
        self.update_scan();
        let busy = self.receiver.busy();
        let (signal_db, receiving) = {
            let state = self.receiver.state.lock().unwrap();
            (state.signal_db, state.running && state.frequency != 0)
        };
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            self.front_panel(ui, busy, receiving, signal_db);
        });
        egui::SidePanel::left("controls")
            .exact_width(275.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.controls(ui, busy);
                });
            });
        let state_handle = self.receiver.state.clone();
        let state = state_handle.lock().unwrap();
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(if state.running { "●" } else { "○" }).color(AMBER));
                ui.label(i18n::label(&state.status));
                if state.running && state.frequency != 0 {
                    ui.label(crate::tr_format!(
                        "Centro: {:.6} MHz",
                        "Center: {:.6} MHz",
                        f64::from(state.spectrum_center) / 1e6
                    ));
                    if self.config.converter != Converter::Direct
                        && matches!(self.config.kind, SourceKind::Usb | SourceKind::Tcp)
                    {
                        if let Ok(tuner_hz) = self.config.tuner_frequency(state.spectrum_center) {
                            ui.label(format!("SDR: {:.6} MHz", f64::from(tuner_hz) / 1e6));
                        }
                    }
                }
                ui.separator();
                ui.label(crate::tr_format!(
                    "{} muestras · {} eventos",
                    "{} samples · {} events",
                    state.samples,
                    state.hits
                ));
                if !state.plugins.is_empty() {
                    ui.separator();
                    ui.label(format!("Plugins: {}", state.plugins.join(", ")));
                }
            });
            if !self.message.is_empty() {
                ui.colored_label(Color32::LIGHT_RED, &self.message);
            }
            for error in [
                &state.tune_error,
                &state.audio_error,
                &state.gain_error,
                &self.preferences_error,
            ]
            .into_iter()
            .flatten()
            {
                ui.colored_label(Color32::LIGHT_RED, error);
            }
        });
        let previous_center = self.scope.fixed_center();
        let received_frequency = state.frequency;
        let mut tune = None;
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.config.kind == SourceKind::Demo {
                ui.label(RichText::new(crate::tr!("Demostración · Señal IQ sintética", "Demo · Synthetic IQ signal")).color(Color32::from_rgb(240, 190, 103)));
            }
            let digital = self.radio.mode == Mode::Digital;
            tune = self.scope.show(ui, &state, scope::View {
                frequency_mhz: self.frequency_mhz,
                sample_rate: self.config.sample_rate,
                filter_edges: self.radio.mode.filter_edges(self.radio.bandwidth),
                can_tune: self.config.kind != SourceKind::File,
                reserve_height: if digital { 112.0 } else { 0.0 },
            });
            if self.radio.mode != Mode::Digital { return; }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(crate::tr!("Actividad digital", "Digital activity")).size(18.0));
                ui.label(RichText::new(crate::tr!("C4FM / 4FSK · 4800 símbolos/s", "C4FM / 4FSK · 4800 symbols/s")).small().color(MUTED));
            });
            ui.label(RichText::new(crate::tr!("Voz DMR sin cifrar y cabeceras P25.", "Unencrypted DMR voice and P25 headers.")).small().color(MUTED));
            egui::ScrollArea::vertical().id_salt("events").auto_shrink([false, false]).show(ui, |ui| {
                if state.events.is_empty() { ui.add_space(20.0); ui.label(crate::tr!("Sin eventos. Inicia la demostración o sintoniza una portadora digital.", "No events. Start the demo or tune to a digital carrier.")); }
                for event in state.events.iter().rev().filter(|e| (e.protocol == "DMR" && self.show_dmr) || (e.protocol == "P25" && self.show_p25)) {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(event.protocol).monospace().strong().color(if event.protocol == "DMR" { AMBER } else { Color32::from_rgb(130, 176, 250) }));
                        ui.label(event.display_text());
                        if event.errors > 0 { ui.label(RichText::new(format!("Δ {} bit", event.errors)).small().color(MUTED)); }
                    }); ui.separator();
                }
            });
        });
        drop(state);
        if previous_center != self.scope.fixed_center() {
            self.message.clear();
            if busy {
                self.cancel_scan(crate::tr!(
                    "Barrido detenido por cambio de vista",
                    "Scan stopped by view change"
                ));
                let requested = (self.frequency_mhz * 1e6).round() as u32;
                if let Some(center) = self.scope.fixed_center() {
                    let frequency = if self
                        .scope
                        .tuning_center(requested, self.config.sample_rate)
                        .is_ok()
                    {
                        requested
                    } else {
                        received_frequency.max(MIN_FREQUENCY_HZ)
                    };
                    self.frequency_mhz = f64::from(frequency) / 1e6;
                    self.receiver.tune_in(frequency, center);
                } else {
                    self.receiver.tune(requested);
                }
            }
        }
        if let Some(frequency) = tune {
            self.apply_frequency(self.frequency_mhz, frequency, busy);
        }
        self.band_menu(ctx, busy);
        self.scan_dialog(ctx, busy);
        self.propagation.show(ctx);
        self.save_preferences(false);
        ctx.request_repaint_after(Duration::from_millis(50));
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.save_preferences(true);
    }
}
