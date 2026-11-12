// SPDX-License-Identifier: GPL-2.0-only

use crate::receiver::State;
use eframe::egui::{
    self, pos2, vec2, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui,
};
use std::collections::VecDeque;

const BACKGROUND: Color32 = Color32::from_rgb(4, 8, 17);
const GRID: Color32 = Color32::from_rgb(39, 55, 70);
const TEXT: Color32 = Color32::from_rgb(194, 213, 228);
const ROWS: usize = 112;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Band {
    epoch: u64,
    center_hz: u32,
    span_hz: f64,
    width: usize,
}

struct WaterfallRow {
    band: Band,
    bins: Vec<f32>,
}

#[derive(Default)]
struct Waterfall {
    band: Option<Band>,
    frame: u64,
    rows: VecDeque<WaterfallRow>,
}

impl Waterfall {
    fn update(&mut self, band: Band, frame: u64, bins: &[f32]) -> bool {
        let changed = self.band != Some(band);
        if self.band.is_none_or(|old| old.epoch != band.epoch) {
            self.frame = 0;
            self.rows.clear();
        }
        self.band = Some(band);
        if frame == 0 || frame == self.frame {
            return changed;
        }
        self.frame = frame;
        self.rows.push_front(WaterfallRow {
            band,
            bins: bins
                .chunks(2)
                .map(|pair| pair.iter().copied().fold(-120.0, f32::max))
                .collect(),
        });
        self.rows.truncate(ROWS);
        true
    }

    fn image(&self) -> Option<egui::ColorImage> {
        let band = self.band?;
        self.rows.front()?;
        let mut image = egui::ColorImage::new([band.width, ROWS], BACKGROUND);
        for (pixels, row) in image.pixels.chunks_exact_mut(band.width).zip(&self.rows) {
            let offset = f64::from(band.center_hz) - f64::from(row.band.center_hz);
            for (index, pixel) in pixels.iter_mut().enumerate() {
                let hz = offset + ((index as f64 + 0.5) / band.width as f64 - 0.5) * band.span_hz;
                let fraction = hz / row.band.span_hz + 0.5;
                if (0.0..1.0).contains(&fraction) {
                    *pixel = waterfall_color(row.bins[(fraction * row.bins.len() as f64) as usize]);
                }
            }
        }
        Some(image)
    }
}

pub struct Scope {
    zoom: usize,
    fixed_center: Option<u32>,
    waterfall: Waterfall,
    texture: Option<egui::TextureHandle>,
}

impl Default for Scope {
    fn default() -> Self {
        Self {
            zoom: 1,
            fixed_center: None,
            waterfall: Waterfall::default(),
            texture: None,
        }
    }
}

pub struct View {
    pub frequency_mhz: f64,
    pub sample_rate: u32,
    pub filter_edges: [f32; 2],
    pub can_tune: bool,
    pub reserve_height: f32,
}

impl Scope {
    pub fn zoom(&self) -> usize {
        self.zoom
    }

    pub fn set_zoom(&mut self, zoom: usize) {
        if [1, 2, 4, 8].contains(&zoom) {
            self.zoom = zoom;
        }
    }

    pub fn fixed_center(&self) -> Option<u32> {
        self.fixed_center
    }

    pub fn set_fixed_center(&mut self, center: Option<u32>) {
        self.fixed_center = center;
    }

    pub fn tuning_center(&self, frequency: u32, sample_rate: u32) -> Result<u32, String> {
        let center = self.fixed_center.unwrap_or(frequency);
        if f64::from(frequency.abs_diff(center)) > self.span_hz(sample_rate) / 2.0 {
            return Err(crate::tr!(
                "Fuera del rango visible. Reduce el zoom o elige Centrado.",
                "Outside the visible range. Zoom out or select Centered."
            )
            .into());
        }
        Ok(center)
    }

    fn span_hz(&self, sample_rate: u32) -> f64 {
        f64::from(sample_rate) / self.zoom as f64
    }

    pub fn set_span(&mut self, sample_rate: u32, span_hz: u32) {
        self.zoom = [1, 2, 4, 8]
            .into_iter()
            .min_by_key(|&zoom| (sample_rate / zoom as u32).abs_diff(span_hz))
            .unwrap_or(1);
    }

    pub fn show(&mut self, ui: &mut Ui, state: &State, view: View) -> Option<f64> {
        egui::Frame::new()
            .fill(Color32::from_rgb(14, 19, 25))
            .stroke(Stroke::new(1.0_f32, GRID))
            .corner_radius(4)
            .inner_margin(egui::Margin::same(6))
            .show(ui, |ui| self.draw(ui, state, view))
            .inner
    }

    fn draw(&mut self, ui: &mut Ui, state: &State, view: View) -> Option<f64> {
        ui.spacing_mut().item_spacing.y = 0.0;
        let requested_hz = (view.frequency_mhz * 1e6).round() as u32;
        let center_hz = if state.running && state.spectrum_center != 0 {
            state.spectrum_center
        } else {
            self.fixed_center.unwrap_or(requested_hz)
        };
        let tuned_hz = if state.running && state.frequency != 0 {
            state.frequency
        } else {
            requested_hz
        };
        let center_mhz = f64::from(center_hz) / 1e6;
        let span_hz = self.span_hz(view.sample_rate);
        let span_mhz = span_hz / 1e6;
        let low_mhz = center_mhz - span_mhz / 2.0;
        let high_mhz = center_mhz + span_mhz / 2.0;
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(crate::tr!("ESPECTRO RF", "RF SPECTRUM"))
                    .strong()
                    .color(TEXT),
            );
            let mut fixed = self.fixed_center.is_some();
            ui.add_enabled_ui(view.can_tune, |ui| {
                ui.selectable_value(&mut fixed, false, crate::tr!("Centrado", "Centered"))
                    .on_hover_text(crate::tr!(
                        "El espectro sigue la frecuencia sintonizada",
                        "The spectrum follows the tuned frequency"
                    ));
                ui.selectable_value(&mut fixed, true, crate::tr!("Libre", "Free"))
                    .on_hover_text(crate::tr!(
                        "Mantiene el rango mientras sintonizas dentro de él",
                        "Keeps the range fixed while you tune within it"
                    ));
            });
            if fixed != self.fixed_center.is_some() {
                self.fixed_center = fixed.then_some(center_hz);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("{center_mhz:.6} MHz"))
                        .monospace()
                        .color(TEXT),
                );
            });
        });
        ui.add_space(5.0);

        let len = state.spectrum.len() / self.zoom;
        let band = Band {
            epoch: state.scope_epoch,
            center_hz,
            span_hz,
            width: len.div_ceil(2),
        };
        let start = (state.spectrum.len() - len) / 2;
        let bins = &state.spectrum[start..start + len];
        let frame = if state.running && state.spectrum_center == center_hz {
            state.spectrum_frame
        } else {
            0
        };
        if self.waterfall.update(band, frame, bins) {
            if let Some(image) = self.waterfall.image() {
                if let Some(texture) = &mut self.texture {
                    texture.set(image, egui::TextureOptions::NEAREST);
                } else {
                    self.texture = Some(ui.ctx().load_texture(
                        "waterfall",
                        image,
                        egui::TextureOptions::NEAREST,
                    ));
                }
            } else {
                self.texture = None;
            }
        }

        let sense = if view.can_tune && state.running {
            Sense::click()
        } else {
            Sense::hover()
        };
        let available = ui.available_height() - view.reserve_height;
        let (plot, spectrum) = ui.allocate_exact_size(
            vec2(ui.available_width(), (available * 0.43).clamp(86.0, 188.0)),
            sense,
        );
        let p = ui.painter_at(plot);
        p.rect_filled(plot, 0.0, BACKGROUND);
        let tuned_fraction = (f64::from(tuned_hz) - f64::from(center_hz)) / span_hz + 0.5;
        let filter = view.filter_edges.map(|offset| {
            let fraction = (tuned_fraction + f64::from(offset) / span_hz).clamp(0.0, 1.0);
            egui::lerp(plot.x_range(), fraction as f32)
        });
        p.rect_filled(
            Rect::from_x_y_ranges(filter[0]..=filter[1], plot.y_range()),
            0.0,
            Color32::from_rgba_unmultiplied(71, 145, 203, 24),
        );
        for tick in 0..=10 {
            p.vline(
                plot.left() + plot.width() * tick as f32 / 10.0,
                plot.y_range(),
                Stroke::new(0.7_f32, GRID),
            );
        }
        for db in [-20, -40, -60, -80, -100] {
            let y = level_y(plot, db as f32);
            p.hline(plot.x_range(), y, Stroke::new(0.7_f32, GRID));
            p.text(
                pos2(plot.left() + 4.0, y),
                Align2::LEFT_BOTTOM,
                db.to_string(),
                FontId::monospace(10.0),
                TEXT,
            );
        }
        if state.spectrum_frame != 0 && state.spectrum_center == center_hz {
            paint_trace(&p, plot, bins);
        }
        if (0.0..=1.0).contains(&tuned_fraction) {
            p.vline(
                egui::lerp(plot.x_range(), tuned_fraction as f32),
                plot.y_range(),
                Stroke::new(1.0_f32, Color32::from_rgb(220, 232, 238)),
            );
        }

        let fall_height = (ui.available_height() - view.reserve_height - 49.0).max(40.0);
        let (fall, waterfall) =
            ui.allocate_exact_size(vec2(ui.available_width(), fall_height), sense);
        ui.painter().rect_filled(fall, 0.0, BACKGROUND);
        if let Some(texture) = &self.texture {
            ui.painter().image(
                texture.id(),
                fall,
                Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        let (axis, _) = ui.allocate_exact_size(vec2(ui.available_width(), 19.0), Sense::hover());
        ui.painter()
            .rect_filled(axis, 0.0, Color32::from_rgb(8, 13, 22));
        let axis_steps = if axis.width() >= 750.0 { 10 } else { 5 };
        for tick in 0..=axis_steps {
            let fraction = tick as f64 / axis_steps as f64;
            let align = if tick == 0 {
                Align2::LEFT_CENTER
            } else if tick == axis_steps {
                Align2::RIGHT_CENTER
            } else {
                Align2::CENTER_CENTER
            };
            ui.painter().text(
                pos2(egui::lerp(axis.x_range(), fraction as f32), axis.center().y),
                align,
                format!("{:.3}", low_mhz + span_mhz * fraction),
                FontId::monospace(11.0),
                TEXT,
            );
        }
        ui.add_space(3.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("SPAN {:.1} kHz", span_hz / 1000.0))
                    .monospace()
                    .color(TEXT),
            );
            for zoom in [1, 2, 4, 8] {
                let fits = self.fixed_center.is_none_or(|center| {
                    f64::from(requested_hz.abs_diff(center))
                        <= f64::from(view.sample_rate) / (2.0 * zoom as f64)
                });
                ui.add_enabled_ui(fits, |ui| {
                    ui.selectable_value(&mut self.zoom, zoom, format!("×{zoom}"));
                });
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("{low_mhz:.6} – {high_mhz:.6} MHz"))
                        .monospace()
                        .color(TEXT),
                );
            });
        });

        for response in [spectrum, waterfall] {
            if let Some(position) = response
                .interact_pointer_pos()
                .filter(|_| response.clicked())
            {
                let fraction = (position.x - response.rect.center().x) / response.rect.width();
                let hz = f64::from(center_hz) + f64::from(fraction) * span_hz;
                return Some(hz.round() / 1e6);
            }
        }
        None
    }
}

fn level_y(plot: Rect, db: f32) -> f32 {
    plot.bottom() - ((db + 110.0) / 110.0).clamp(0.0, 1.0) * plot.height()
}

fn paint_trace(p: &egui::Painter, plot: Rect, bins: &[f32]) {
    let points: Vec<_> = bins
        .iter()
        .enumerate()
        .map(|(i, &db)| {
            let x = plot.left() + plot.width() * i as f32 / (bins.len() - 1) as f32;
            pos2(x, level_y(plot, db))
        })
        .collect();
    let mut mesh = egui::Mesh::default();
    for (i, &point) in points.iter().enumerate() {
        mesh.colored_vertex(point, Color32::from_rgb(33, 101, 146));
        mesh.colored_vertex(
            pos2(point.x, plot.bottom()),
            Color32::from_rgb(133, 177, 203),
        );
        if i > 0 {
            let j = 2 * i as u32;
            mesh.add_triangle(j - 2, j - 1, j);
            mesh.add_triangle(j, j - 1, j + 1);
        }
    }
    p.add(Shape::mesh(mesh));
    p.add(Shape::line(
        points,
        Stroke::new(0.9_f32, Color32::from_rgb(100, 190, 237)),
    ));
}

fn waterfall_color(db: f32) -> Color32 {
    const COLORS: [[u8; 3]; 7] = [
        [5, 10, 34],
        [12, 29, 163],
        [12, 86, 249],
        [0, 206, 244],
        [31, 218, 104],
        [245, 223, 71],
        [250, 71, 36],
    ];
    let t = ((db + 117.0) / 102.0).clamp(0.0, 1.0) * (COLORS.len() - 1) as f32;
    let i = (t as usize).min(COLORS.len() - 2);
    let mix = |channel: usize| {
        let a = f32::from(COLORS[i][channel]);
        let b = f32::from(COLORS[i + 1][channel]);
        (a + (b - a) * (t - i as f32)).round() as u8
    };
    Color32::from_rgb(mix(0), mix(1), mix(2))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_span_uses_the_nearest_zoom_at_the_current_sample_rate() {
        let mut scope = Scope::default();
        scope.set_span(960_000, 480_000);
        assert_eq!(scope.zoom, 2);
        scope.set_span(240_000, 480_000);
        assert_eq!(scope.zoom, 1);
        scope.set_span(240_000, 60_000);
        assert_eq!(scope.zoom, 4);
    }

    #[test]
    fn free_view_preserves_center_and_checks_visible_edges() {
        let mut scope = Scope::default();
        scope.set_fixed_center(Some(144_420_000));
        scope.set_span(240_000, 60_000);
        assert_eq!(scope.tuning_center(144_390_000, 240_000), Ok(144_420_000));
        assert_eq!(scope.tuning_center(144_450_000, 240_000), Ok(144_420_000));
        assert!(scope.tuning_center(144_450_001, 240_000).is_err());
        scope.set_fixed_center(None);
        assert_eq!(scope.tuning_center(144_450_001, 240_000), Ok(144_450_001));
    }

    #[test]
    fn waterfall_keeps_history_at_its_rf_frequency_until_a_new_session() {
        let mut waterfall = Waterfall::default();
        let mut band = Band {
            epoch: 1,
            center_hz: 446_000_000,
            span_hz: 240_000.0,
            width: 4,
        };
        let old = [-100.0, -100.0, -100.0, -100.0, -30.0, -30.0, -100.0, -100.0];
        assert!(waterfall.update(band, 1, &old));
        assert!(!waterfall.update(band, 1, &old));

        band.center_hz += 60_000;
        assert!(waterfall.update(band, 2, &[-70.0; 8]));
        assert_eq!(waterfall.rows.len(), 2);
        let image = waterfall.image().unwrap();
        assert_eq!(image.pixels[5], waterfall_color(-30.0));
        assert_eq!(image.pixels[7], BACKGROUND);

        band.epoch += 1;
        assert!(waterfall.update(band, 0, &old));
        assert!(waterfall.image().is_none());
    }
}
