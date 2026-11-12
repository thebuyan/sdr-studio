// SPDX-License-Identifier: GPL-2.0-only

use eframe::egui::{self, Color32, RichText};
use serde_json::Value;
use std::{
    process::Command,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const KP_URL: &str = "https://services.swpc.noaa.gov/products/noaa-planetary-k-index.json";
const FLUX_URL: &str = "https://services.swpc.noaa.gov/products/summary/10cm-flux.json";
const AMATEUR: [(&str, f64); 8] = [
    ("80 m", 3.6),
    ("40 m", 7.1),
    ("30 m", 10.1),
    ("20 m", 14.2),
    ("17 m", 18.1),
    ("15 m", 21.2),
    ("12 m", 24.9),
    ("10 m", 28.5),
];
const SHORTWAVE: [(&str, f64); 8] = [
    ("49 m", 6.05),
    ("41 m", 7.35),
    ("31 m", 9.65),
    ("25 m", 11.85),
    ("22 m", 13.72),
    ("19 m", 15.45),
    ("16 m", 17.69),
    ("13 m", 21.65),
];

struct Observation {
    value: f64,
    utc: String,
}

struct Weather {
    kp: Option<Observation>,
    flux: Option<Observation>,
}

impl Weather {
    fn fetch() -> Self {
        thread::scope(|scope| {
            let kp = scope.spawn(|| fetch_observation(KP_URL, "Kp", 0.0..=9.0));
            let flux = fetch_observation(FLUX_URL, "flux", 1.0..=1000.0);
            Self {
                kp: kp.join().unwrap_or(None),
                flux,
            }
        })
    }
}

fn fetch_observation(
    url: &str,
    field: &str,
    bounds: std::ops::RangeInclusive<f64>,
) -> Option<Observation> {
    let result = Command::new("curl")
        .args([
            "--disable",
            "--fail",
            "--silent",
            "--proto",
            "=https",
            "--max-time",
            "8",
            "--max-filesize",
            "262144",
            url,
        ])
        .output()
        .ok()?;
    if !result.status.success() {
        return None;
    }
    latest_observation(&result.stdout, field, bounds)
}

fn latest_observation(
    json: &[u8],
    field: &str,
    bounds: std::ops::RangeInclusive<f64>,
) -> Option<Observation> {
    let value: Value = serde_json::from_slice(json).ok()?;
    value
        .as_array()?
        .iter()
        .filter_map(|row| {
            let utc = row.get("time_tag")?.as_str()?;
            let value = row.get(field)?.as_f64()?;
            bounds.contains(&value).then(|| Observation {
                value,
                utc: utc.trim_end_matches('Z').replace('T', " "),
            })
        })
        .max_by(|a, b| a.utc.cmp(&b.utc))
}

#[derive(Default)]
pub struct Panel {
    open: bool,
    latitude: String,
    longitude: String,
    weather: Option<Weather>,
    pending: Option<Receiver<Weather>>,
    last_refresh: Option<Instant>,
}

impl Panel {
    pub fn location(&self) -> (&str, &str) {
        (&self.latitude, &self.longitude)
    }

    pub fn set_location(&mut self, latitude: String, longitude: String) {
        self.latitude = latitude;
        self.longitude = longitude;
    }

    pub fn button(&mut self, ui: &mut egui::Ui) {
        if ui
            .button(crate::tr!("Propagación HF", "HF Propagation"))
            .clicked()
        {
            self.open = true;
        }
    }

    fn refresh(&mut self) {
        let (sender, pending) = mpsc::channel();
        thread::spawn(move || {
            let _ = sender.send(Weather::fetch());
        });
        self.pending = Some(pending);
        self.last_refresh = Some(Instant::now());
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        if let Some(pending) = &self.pending {
            match pending.try_recv() {
                Ok(weather) => {
                    self.weather = Some(weather);
                    self.pending = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.weather = Some(Weather {
                        kp: None,
                        flux: None,
                    });
                    self.pending = None;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        if !self.open {
            return;
        }
        if self.pending.is_none()
            && self
                .last_refresh
                .is_none_or(|time| time.elapsed() >= Duration::from_secs(900))
        {
            self.refresh();
        }
        let mut open = self.open;
        let mut refresh = false;
        let position = ctx.screen_rect().center() - egui::vec2(260.0, 245.0);
        egui::Window::new(crate::tr!("Propagación HF", "HF Propagation"))
            .id(egui::Id::new("hf_propagation"))
            .default_pos(position)
            .default_width(520.0)
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_width(520.0);
                ui.spacing_mut().item_spacing.y = 7.0;
                ui.horizontal(|ui| {
                    ui.label(crate::tr!("Latitud", "Latitude"));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.latitude)
                            .id_source("hf_latitude")
                            .desired_width(90.0)
                            .hint_text(crate::tr!("-90 a 90", "-90 to 90")),
                    );
                    ui.label(crate::tr!("Longitud", "Longitude"));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.longitude)
                            .id_source("hf_longitude")
                            .desired_width(95.0)
                            .hint_text(crate::tr!("-180 a 180", "-180 to 180")),
                    );
                });
                ui.label(
                    RichText::new(crate::tr!(
                        "Grados decimales. Sur y oeste con signo negativo.",
                        "Decimal degrees. Use negative values for south and west."
                    ))
                    .small(),
                );
                ui.separator();
                let kp = self
                    .weather
                    .as_ref()
                    .and_then(|weather| weather.kp.as_ref());
                let flux = self
                    .weather
                    .as_ref()
                    .and_then(|weather| weather.flux.as_ref());
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(crate::tr!("Clima espacial · NOAA", "Space weather · NOAA"))
                            .strong(),
                    );
                    refresh = ui
                        .add_enabled(
                            self.pending.is_none(),
                            egui::Button::new(crate::tr!("Actualizar", "Refresh")),
                        )
                        .clicked();
                    let status = if self.pending.is_some() {
                        crate::tr!("Consultando…", "Fetching…")
                    } else if kp.is_none() && flux.is_none() {
                        crate::tr!("Sin datos de NOAA", "No NOAA data")
                    } else if kp.is_some_and(|value| value.value >= 5.0) {
                        crate::tr!("Tormenta geomagnética", "Geomagnetic storm")
                    } else {
                        crate::tr!("Última consulta", "Last update")
                    };
                    ui.label(RichText::new(status).small().color(crate::panel::AMBER));
                });
                show_observation(ui, "Kp", kp, "");
                show_observation(
                    ui,
                    crate::tr!("Flujo solar F10.7", "F10.7 solar flux"),
                    flux,
                    " sfu",
                );
                ui.separator();

                let elevation = self.elevation();
                let light = elevation.map(Light::from_elevation);
                let summary = match elevation.zip(light) {
                    Some((elevation, light)) => {
                        crate::tr_format!(
                            "Sol en tu ubicación: {elevation:+.0}° · {}",
                            "Sun at your location: {elevation:+.0}° · {}",
                            light.name(),
                        )
                    }
                    None if self.latitude.trim().is_empty() || self.longitude.trim().is_empty() => {
                        crate::tr!("Introduce las dos coordenadas", "Enter both coordinates").into()
                    }
                    None => crate::tr!(
                        "Revisa los límites de latitud y longitud",
                        "Check the latitude and longitude limits"
                    )
                    .into(),
                };
                ui.label(summary);
                ui.horizontal_top(|ui| {
                    table(
                        ui,
                        crate::tr!("Radioafición", "Amateur radio"),
                        AMATEUR,
                        light,
                        self.weather.as_ref(),
                    );
                    ui.add_space(12.0);
                    table(
                        ui,
                        crate::tr!("Onda corta", "Shortwave"),
                        SHORTWAVE,
                        light,
                        self.weather.as_ref(),
                    );
                });
                ui.label(
                    RichText::new(crate::tr!(
                        "Orientación local. Una apertura real depende de la ruta.",
                        "Local guidance. Actual openings depend on the path."
                    ))
                    .small(),
                );
            });
        self.open = open;
        if refresh {
            self.refresh();
        }
    }

    fn elevation(&self) -> Option<f64> {
        let latitude = coordinate(&self.latitude, 90.0)?;
        let longitude = coordinate(&self.longitude, 180.0)?;
        let unix_seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_secs_f64();
        Some(solar_elevation(unix_seconds, latitude, longitude))
    }
}

fn coordinate(text: &str, limit: f64) -> Option<f64> {
    let value = text.trim().replace(',', ".").parse::<f64>().ok()?;
    (-limit..=limit).contains(&value).then_some(value)
}

fn show_observation(ui: &mut egui::Ui, name: &str, observation: Option<&Observation>, unit: &str) {
    if let Some(observation) = observation {
        ui.label(format!(
            "{name}: {:.1}{unit} · {} UTC",
            observation.value, observation.utc
        ));
    } else {
        ui.label(crate::tr_format!(
            "{name}: dato no disponible",
            "{name}: no data available"
        ));
    }
}

// Solar coordinates and sidereal time approximations from USNO, epoch J2000.0.
fn solar_elevation(unix_seconds: f64, latitude: f64, longitude: f64) -> f64 {
    let d = (unix_seconds - 946_728_000.0) / 86_400.0;
    let g = (357.529 + 0.985_600_28 * d).rem_euclid(360.0).to_radians();
    let q = 280.459 + 0.985_647_36 * d;
    let l = (q + 1.915 * g.sin() + 0.020 * (2.0 * g).sin())
        .rem_euclid(360.0)
        .to_radians();
    let e = (23.439 - 0.000_000_36 * d).to_radians();
    let declination = (e.sin() * l.sin()).asin();
    let right_ascension = (e.cos() * l.sin()).atan2(l.cos());
    let sidereal = (280.460_618_37 + 360.985_647_366_29 * d + longitude).rem_euclid(360.0);
    let hour_angle = sidereal.to_radians() - right_ascension;
    let lat = latitude.to_radians();
    (lat.sin() * declination.sin() + lat.cos() * declination.cos() * hour_angle.cos())
        .clamp(-1.0, 1.0)
        .asin()
        .to_degrees()
}

#[derive(Clone, Copy)]
enum Light {
    Day,
    Twilight,
    Night,
}

impl Light {
    fn from_elevation(degrees: f64) -> Self {
        if degrees >= 5.0 {
            Self::Day
        } else if degrees >= -9.0 {
            Self::Twilight
        } else {
            Self::Night
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Day => crate::tr!("Día", "Day"),
            Self::Twilight => crate::tr!("Crepúsculo", "Twilight"),
            Self::Night => crate::tr!("Noche", "Night"),
        }
    }

    fn guidance(self, mhz: f64, weather: Option<&Weather>) -> (&'static str, Color32) {
        let try_first: (&str, Color32) = (
            crate::tr!("Explorar primero", "Try first"),
            Color32::from_rgb(136, 213, 147),
        );
        const VARIABLE: (&str, Color32) = ("Variable", crate::panel::AMBER);
        let secondary: (&str, Color32) = (
            crate::tr!("Menor prioridad", "Lower priority"),
            Color32::from_rgb(166, 168, 163),
        );
        let disturbed = weather
            .and_then(|weather| weather.kp.as_ref())
            .is_some_and(|kp| kp.value >= 5.0);
        let elevated_flux = weather
            .and_then(|weather| weather.flux.as_ref())
            .is_some_and(|flux| flux.value >= 100.0);
        match self {
            Self::Day if disturbed && mhz >= 9.0 => VARIABLE,
            Self::Day if (9.0..=18.0).contains(&mhz) => try_first,
            Self::Day if (18.0..=22.0).contains(&mhz) && elevated_flux => try_first,
            Self::Day if mhz >= 18.0 => VARIABLE,
            Self::Twilight if mhz <= 15.0 => try_first,
            Self::Twilight => VARIABLE,
            Self::Night if mhz < 9.0 => try_first,
            Self::Night if mhz <= 15.0 => VARIABLE,
            _ => secondary,
        }
    }
}

fn table(
    ui: &mut egui::Ui,
    title: &str,
    bands: [(&str, f64); 8],
    light: Option<Light>,
    weather: Option<&Weather>,
) {
    ui.vertical(|ui| {
        ui.label(RichText::new(title).strong());
        egui::Grid::new(title)
            .striped(true)
            .min_col_width(105.0)
            .show(ui, |ui| {
                for (band, mhz) in bands {
                    ui.label(format!("{band} · {mhz:.2} MHz"));
                    let (label, color) = light.map_or(
                        (
                            crate::tr!("Pendiente", "Awaiting location"),
                            Color32::from_gray(135),
                        ),
                        |light| light.guidance(mhz, weather),
                    );
                    ui.label(RichText::new(label).color(color));
                    ui.end_row();
                }
            });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn location_and_time_change_the_solar_elevation() {
        let equinox_noon = 1_742_472_000.0;
        assert!(solar_elevation(equinox_noon, 0.0, 0.0) > 85.0);
        assert!(solar_elevation(equinox_noon, 0.0, 180.0) < -85.0);
        assert_eq!(coordinate("-33,45", 90.0), Some(-33.45));
        assert_eq!(coordinate("91", 90.0), None);
    }

    #[test]
    fn noaa_parser_keeps_latest_valid_observation() {
        let json = br#"[
            {"time_tag":"2026-11-11T18:00:00","Kp":2.67},
            {"time_tag":"2026-11-11T21:00:00","Kp":-1},
            {"time_tag":"2026-11-11T15:00:00","Kp":1.33}
        ]"#;
        let observed = latest_observation(json, "Kp", 0.0..=9.0).unwrap();
        assert_eq!(observed.value, 2.67);
        assert_eq!(observed.utc, "2026-11-11 18:00:00");
    }
}
