// SPDX-License-Identifier: GPL-2.0-only

use eframe::egui::{
    self, pos2, vec2, Align2, Color32, FontId, Painter, Pos2, Rect, Sense, Shape, Stroke,
    StrokeKind, Ui, Vec2,
};

pub const AMBER: Color32 = Color32::from_rgb(249, 174, 83);
pub const INK: Color32 = Color32::from_rgb(216, 208, 191);
const DIM: Color32 = Color32::from_rgb(156, 136, 112);
const GLASS: Color32 = Color32::from_rgb(28, 21, 14);

pub fn readout(
    ui: &mut Ui,
    width: f32,
    frequency: f64,
    mode: &str,
    receiving: bool,
    editable: bool,
    edit: &mut Option<String>,
) -> Option<f64> {
    let id = ui.make_persistent_id("vfo_entry");
    let (rect, response) = ui.allocate_exact_size(
        vec2(width, 136.0),
        if editable && edit.is_none() {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 7.0, Color32::from_rgb(15, 14, 13));
    p.rect_stroke(
        rect,
        7.0,
        Stroke::new(1.0_f32, Color32::from_gray(92)),
        StrokeKind::Inside,
    );
    let inner = rect.shrink(6.0);
    p.rect_filled(inner, 4.0, GLASS);
    p.rect_stroke(
        inner,
        4.0,
        Stroke::new(1.0_f32, Color32::from_rgb(83, 54, 29)),
        StrokeKind::Inside,
    );
    p.hline(
        inner.x_range(),
        inner.top() + 28.0,
        Stroke::new(1.0_f32, Color32::from_rgb(61, 43, 27)),
    );
    p.text(
        inner.left_top() + vec2(14.0, 13.0),
        Align2::LEFT_CENTER,
        "VFO",
        FontId::monospace(13.0),
        DIM,
    );
    p.text(
        inner.center_top() + vec2(0.0, 13.0),
        Align2::CENTER_CENTER,
        mode,
        FontId::monospace(14.0),
        AMBER,
    );
    p.text(
        inner.right_top() + vec2(-14.0, 13.0),
        Align2::RIGHT_CENTER,
        if receiving { "RX" } else { "STBY" },
        FontId::monospace(13.0),
        if receiving { AMBER } else { DIM },
    );
    p.text(
        inner.right_bottom() + vec2(-16.0, -12.0),
        Align2::RIGHT_CENTER,
        "MHz",
        FontId::monospace(14.0),
        AMBER,
    );

    if !editable {
        *edit = None;
    }
    if let Some(text) = edit {
        let text_rect = Rect::from_center_size(
            inner.center() + vec2(0.0, 8.0),
            vec2(inner.width() - 38.0, 54.0),
        );
        let field = ui.put(
            text_rect,
            egui::TextEdit::singleline(text)
                .id(id)
                .font(FontId::monospace(33.0))
                .horizontal_align(egui::Align::Center)
                .text_color(AMBER),
        );
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            field.surrender_focus();
            *edit = None;
        } else if field.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let value = text.trim().replace(',', ".").parse().ok();
            field.surrender_focus();
            *edit = None;
            return value;
        }
    } else {
        let digits = format!("{frequency:.6}");
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                editable,
                crate::tr_format!("Frecuencia {digits} MHz", "Frequency {digits} MHz"),
            )
        });
        let mut remaining = digits.chars().filter(|&c| c != '.').count() as u32;
        let slots = remaining as f32 + 0.38;
        let cell = ((inner.width() - 54.0) / slots).min(43.0);
        let mut x = inner.center().x - cell * slots / 2.0;
        let y = inner.top() + 39.0;
        let mut hovered_place = None;
        let right_click = if editable && response.clicked_by(egui::PointerButton::Secondary) {
            response.interact_pointer_pos()
        } else {
            None
        };
        let mut zero_delta_hz = None;
        for c in digits.chars() {
            if c == '.' {
                p.circle_filled(pos2(x + cell * 0.12, y + 58.0), 3.0, AMBER);
                x += cell * 0.38;
            } else if let Some(digit) = c.to_digit(10) {
                remaining -= 1;
                let slot = Rect::from_min_size(pos2(x - 2.5, y - 5.0), vec2(cell, 72.0));
                let place = 10_u32.pow(remaining);
                if right_click.is_some_and(|pos| slot.contains(pos)) {
                    zero_delta_hz = Some(digit * place);
                }
                if editable && response.hover_pos().is_some_and(|pos| slot.contains(pos)) {
                    hovered_place = Some(place);
                    p.rect_filled(slot, 3.0, Color32::from_rgb(62, 41, 21));
                    p.hline(slot.x_range(), slot.bottom(), Stroke::new(1.5_f32, AMBER));
                }
                seven_segment(&p, pos2(x, y), cell - 5.0, 62.0, digit as usize);
                x += cell;
            }
        }
        if response.clicked() {
            *edit = Some(digits);
            ui.memory_mut(|memory| memory.request_focus(id));
        }
        if editable {
            response
                .on_hover_cursor(egui::CursorIcon::Text)
                .on_hover_text(crate::tr!(
                "Rueda: ajustar dígito · Clic derecho: poner en cero · Clic izquierdo: escribir",
                "Wheel: adjust digit · Right-click: set to zero · Left-click: type"
            ));
        }
        let wheel_id = id.with("digit_wheel");
        if let Some(delta_hz) = zero_delta_hz {
            ui.data_mut(|data| data.remove::<(u32, f32)>(wheel_id));
            let current_hz = (frequency * 1e6).round() as u32;
            return current_hz
                .checked_sub(delta_hz)
                .filter(|&hz| hz >= crate::MIN_FREQUENCY_HZ)
                .map(|hz| f64::from(hz) / 1e6);
        }
        if let Some(place) = hovered_place {
            let wheel = wheel_delta(ui);
            let steps = ui.data_mut(|data| {
                let (last_place, remainder) =
                    data.get_temp::<(u32, f32)>(wheel_id).unwrap_or_default();
                let delta = if last_place == place { remainder } else { 0.0 } + wheel;
                let steps = delta.trunc();
                data.insert_temp(wheel_id, (place, delta - steps));
                steps
            });
            if steps != 0.0 {
                let hz = (frequency * 1e6).round();
                return Some((hz + f64::from(steps) * f64::from(place)) / 1e6);
            }
        } else {
            ui.data_mut(|data| data.remove::<(u32, f32)>(wheel_id));
        }
    }
    None
}

fn seven_segment(p: &Painter, origin: Pos2, width: f32, height: f32, digit: usize) {
    const DIGITS: [u8; 10] = [0x3f, 0x06, 0x5b, 0x4f, 0x66, 0x6d, 0x7d, 0x07, 0x7f, 0x6f];
    let edge = 4.0;
    let right = width - edge;
    let mid = height / 2.0;
    let bottom = height - edge;
    let pairs = [
        (vec2(edge + 3.0, edge), vec2(right - 3.0, edge)),
        (vec2(right, edge + 4.0), vec2(right, mid - 4.0)),
        (vec2(right, mid + 4.0), vec2(right, bottom - 4.0)),
        (vec2(edge + 3.0, bottom), vec2(right - 3.0, bottom)),
        (vec2(edge, mid + 4.0), vec2(edge, bottom - 4.0)),
        (vec2(edge, edge + 4.0), vec2(edge, mid - 4.0)),
        (vec2(edge + 3.0, mid), vec2(right - 3.0, mid)),
    ];
    for (index, (start, end)) in pairs.into_iter().enumerate() {
        let axis = (end - start).normalized() * 2.6;
        let normal = vec2(-axis.y, axis.x);
        let a = origin + start;
        let b = origin + end;
        let active = DIGITS[digit] & (1 << index) != 0;
        p.add(Shape::convex_polygon(
            vec![
                a - axis,
                a + normal,
                b + normal,
                b + axis,
                b - normal,
                a - normal,
            ],
            if active {
                AMBER
            } else {
                Color32::from_rgb(52, 37, 22)
            },
            Stroke::NONE,
        ));
    }
}

pub fn meter(ui: &mut Ui, db: f32, per_bin: bool, receiving: bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(174.0, 102.0), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 5.0, Color32::from_rgb(17, 16, 14));
    p.rect_stroke(
        rect,
        5.0,
        Stroke::new(1.0_f32, Color32::from_gray(79)),
        StrokeKind::Inside,
    );
    let glass = rect.shrink(5.0);
    p.rect_filled(glass, 3.0, Color32::from_rgb(57, 39, 22));
    let pivot = rect.center_bottom() - vec2(0.0, 14.0);
    let point = |fraction: f32, radius: f32| {
        let angle = -2.48 + 1.82 * fraction;
        pivot + Vec2::angled(angle) * radius
    };
    let scale: Vec<_> = (0..=40)
        .map(|tick| point(tick as f32 / 40.0, 69.0))
        .collect();
    p.add(Shape::line(scale, Stroke::new(1.0_f32, DIM)));
    for tick in 0..=20 {
        let t = tick as f32 / 20.0;
        let major = tick % 5 == 0;
        p.line_segment(
            [point(t, 69.0), point(t, if major { 61.0 } else { 65.0 })],
            Stroke::new(1.0_f32, if major { INK } else { DIM }),
        );
        if major {
            p.text(
                point(t, 78.0),
                Align2::CENTER_CENTER,
                format!("{}", -100 + tick * 5),
                FontId::monospace(9.0),
                INK,
            );
        }
    }
    let level = if receiving {
        ((db + 100.0) / 100.0).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.line_segment([pivot, point(level, 67.0)], Stroke::new(1.8_f32, AMBER));
    p.circle_filled(pivot, 3.5, Color32::from_rgb(151, 111, 65));
    p.text(
        pos2(rect.center().x, rect.bottom() - 24.0),
        Align2::CENTER_CENTER,
        if per_bin { "dBFS/bin" } else { "dBFS" },
        FontId::monospace(10.0),
        INK,
    );
}

pub fn dial(ui: &mut Ui, position: f32, remainder: &mut f32) -> f32 {
    let (rect, response) = ui.allocate_exact_size(vec2(152.0, 154.0), Sense::click_and_drag());
    let response = response
        .on_hover_cursor(egui::CursorIcon::Grab)
        .on_hover_text(crate::tr!(
            "Arrastra el dial o gira la rueda para sintonizar",
            "Drag the dial or scroll to tune"
        ));
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            response.enabled(),
            crate::tr!("Dial de sintonía", "Tuning dial"),
        )
    });
    let p = ui.painter_at(rect);
    let center = rect.center_top() + vec2(0.0, 72.0);
    for i in 0..36 {
        let ray = Vec2::angled(std::f32::consts::TAU * i as f32 / 36.0);
        let radius = if i % 3 == 0 { 69.0 } else { 66.0 };
        p.line_segment(
            [center + ray * 62.0, center + ray * radius],
            Stroke::new(1.0_f32, if i % 3 == 0 { INK } else { DIM }),
        );
    }
    p.circle_filled(center + vec2(1.0, 3.0), 60.0, Color32::from_gray(13));
    p.circle_filled(center, 58.5, Color32::from_gray(26));
    p.circle_stroke(center, 57.0, Stroke::new(1.0_f32, Color32::from_gray(100)));
    for i in 0..48 {
        let angle = position + std::f32::consts::TAU * i as f32 / 48.0;
        let ray = Vec2::angled(angle);
        p.line_segment(
            [center + ray * 51.0, center + ray * 56.0],
            Stroke::new(
                1.2_f32,
                Color32::from_gray(if i % 2 == 0 { 71 } else { 45 }),
            ),
        );
    }
    p.circle_filled(center, 50.0, Color32::from_gray(36));
    p.circle_stroke(center, 48.5, Stroke::new(1.0_f32, Color32::from_gray(76)));
    p.circle_filled(center - vec2(0.0, 1.0), 43.0, Color32::from_gray(31));
    let marker = Vec2::angled(position - std::f32::consts::FRAC_PI_2);
    p.line_segment(
        [center + marker * 40.0, center + marker * 49.0],
        Stroke::new(3.0_f32, AMBER),
    );
    let dimple = center + marker.rot90() * 22.0;
    p.circle_filled(dimple + vec2(0.0, 1.0), 8.5, Color32::from_gray(68));
    p.circle_filled(dimple, 8.0, Color32::from_gray(21));
    p.text(
        rect.center_bottom(),
        Align2::CENTER_BOTTOM,
        crate::tr!("SINTONÍA", "TUNING"),
        FontId::monospace(11.0),
        INK,
    );

    if !response.enabled() {
        return 0.0;
    }
    if response.drag_started() {
        *remainder = 0.0;
    }
    let delta = response.drag_delta();
    let wheel = if response.hovered() {
        wheel_delta(ui)
    } else {
        0.0
    };
    *remainder += (delta.x - delta.y) / 7.0 + wheel;
    let steps = remainder.trunc();
    *remainder -= steps;
    let key = if response.has_focus() {
        ui.input(|i| {
            f32::from(u8::from(i.key_pressed(egui::Key::ArrowRight)))
                - f32::from(u8::from(i.key_pressed(egui::Key::ArrowLeft)))
        })
    } else {
        0.0
    };
    steps + key
}

fn wheel_delta(ui: &Ui) -> f32 {
    ui.input(|i| {
        i.events
            .iter()
            .filter_map(|event| match event {
                egui::Event::MouseWheel { delta, unit, .. } => Some(match unit {
                    egui::MouseWheelUnit::Point => delta.y / 40.0,
                    egui::MouseWheelUnit::Line | egui::MouseWheelUnit::Page => delta.y,
                }),
                _ => None,
            })
            .sum()
    })
}
