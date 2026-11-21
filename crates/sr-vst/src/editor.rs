//! Janela do plugin Surround Rescue — mesmo visual do app (azul-noite + âmbar, Bahnschrift).
//! Controles desenhados à mão: palco com caixas arrastáveis, sliders, chaves e EQ vertical.

use crate::SrParams;
use nih_plug::prelude::*;
use nih_plug_egui::egui::{self, Align, Align2, Color32, FontFamily, FontId, Layout, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui, Vec2};
use std::sync::Arc;

const NIGHT: Color32 = Color32::from_rgb(0x14, 0x1a, 0x33);
const DEEP: Color32 = Color32::from_rgb(0x0e, 0x13, 0x28);
const SURFACE: Color32 = Color32::from_rgb(0x1d, 0x25, 0x47);
const LINE: Color32 = Color32::from_rgb(0x2c, 0x36, 0x64);
const INK: Color32 = Color32::from_rgb(0xe9, 0xec, 0xfb);
const SOFT: Color32 = Color32::from_rgb(0x9a, 0xa3, 0xcc);
const AMBER: Color32 = Color32::from_rgb(0xff, 0xb5, 0x47);
const AMBER_INK: Color32 = Color32::from_rgb(0x2a, 0x1a, 0x00);

pub const SIZE: (u32, u32) = (620, 940);

fn display(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("display".into()))
}

fn body(size: f32) -> FontId {
    FontId::proportional(size)
}

/// Fontes do Windows (Bahnschrift para títulos e números, Segoe UI no resto) e cores.
pub fn setup(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let fdir = std::path::Path::new(r"C:\Windows\Fonts");
    let mut display_family = vec![];
    if let Ok(b) = std::fs::read(fdir.join("bahnschrift.ttf")) {
        fonts.font_data.insert("bahnschrift".into(), Arc::new(egui::FontData::from_owned(b)));
        display_family.push("bahnschrift".to_string());
    }
    if let Ok(b) = std::fs::read(fdir.join("segoeui.ttf")) {
        fonts.font_data.insert("segoe".into(), Arc::new(egui::FontData::from_owned(b)));
        fonts.families.get_mut(&FontFamily::Proportional).unwrap().insert(0, "segoe".into());
    }
    display_family.extend(fonts.families[&FontFamily::Proportional].iter().cloned());
    fonts.families.insert(FontFamily::Name("display".into()), display_family);
    ctx.set_fonts(fonts);

    let mut v = egui::Visuals::dark();
    v.panel_fill = NIGHT;
    v.window_fill = NIGHT;
    v.override_text_color = Some(INK);
    v.selection.bg_fill = AMBER;
    ctx.set_visuals(v);
}

fn card(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui)) {
    egui::Frame::default().fill(SURFACE).corner_radius(12.0).inner_margin(16.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(title).font(display(16.0)).color(INK));
        ui.add_space(8.0);
        add(ui);
    });
}

fn set_plain(setter: &ParamSetter, p: &FloatParam, v: f32) {
    setter.begin_set_parameter(p);
    setter.set_parameter(p, v);
    setter.end_set_parameter(p);
}

/// Slider horizontal: arraste ou clique; duplo clique volta ao padrão.
fn slider(ui: &mut Ui, setter: &ParamSetter, p: &FloatParam, label: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).font(body(13.5)).color(INK));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(format!("{p}")).font(display(15.0)).color(AMBER));
        });
    });
    let (rect, r) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::click_and_drag());
    let pad = 10.0;
    let x_of = |t: f32| rect.left() + pad + t * (rect.width() - 2.0 * pad);
    if r.drag_started() {
        setter.begin_set_parameter(p);
    }
    if let (true, Some(pos)) = (r.dragged() || r.clicked(), r.interact_pointer_pos()) {
        let t = ((pos.x - rect.left() - pad) / (rect.width() - 2.0 * pad)).clamp(0.0, 1.0);
        if r.clicked() {
            setter.begin_set_parameter(p);
        }
        setter.set_parameter_normalized(p, t);
        if r.clicked() {
            setter.end_set_parameter(p);
        }
    }
    if r.drag_stopped() {
        setter.end_set_parameter(p);
    }
    if r.double_clicked() {
        set_plain(setter, p, p.default_plain_value());
    }
    let t = p.unmodulated_normalized_value();
    let painter = ui.painter();
    let y = rect.center().y;
    let track = Rect::from_min_max(Pos2::new(x_of(0.0), y - 3.0), Pos2::new(x_of(1.0), y + 3.0));
    painter.rect_filled(track, 3.0, LINE);
    painter.rect_filled(Rect::from_min_max(track.min, Pos2::new(x_of(t), track.max.y)), 3.0, AMBER);
    let knob = Pos2::new(x_of(t), y);
    painter.circle_filled(knob, if r.hovered() || r.dragged() { 9.5 } else { 8.5 }, AMBER);
    painter.circle_filled(knob, 4.5, DEEP);
    ui.add_space(6.0);
}

/// Chave liga/desliga.
fn toggle(ui: &mut Ui, setter: &ParamSetter, p: &BoolParam, label: &str) {
    ui.horizontal(|ui| {
        let (rect, r) = ui.allocate_exact_size(Vec2::new(40.0, 22.0), Sense::click());
        let on = p.value();
        if r.clicked() {
            setter.begin_set_parameter(p);
            setter.set_parameter(p, !on);
            setter.end_set_parameter(p);
        }
        let painter = ui.painter();
        painter.rect_filled(rect, 11.0, if on { AMBER } else { LINE });
        let cx = if on { rect.right() - 11.0 } else { rect.left() + 11.0 };
        painter.circle_filled(Pos2::new(cx, rect.center().y), 8.0, if on { AMBER_INK } else { INK });
        ui.label(RichText::new(label).font(body(14.0)).color(INK).strong());
    });
}

fn pos_on(c: Pos2, r: f32, deg: f32) -> Pos2 {
    let t = deg.to_radians();
    Pos2::new(c.x + t.sin() * r, c.y - t.cos() * r)
}

/// Palco: a cabeça vista de cima e as caixas; arraste FL/FR para mudar a abertura.
fn stage(ui: &mut Ui, setter: &ParamSetter, params: &SrParams) {
    let side = ui.available_width().min(260.0);
    let (rect, r) = ui.allocate_exact_size(Vec2::new(side, side + 26.0), Sense::click_and_drag());
    let c = Pos2::new(rect.center().x, rect.top() + side / 2.0);
    let radius = side * 0.38;
    let p = &params.front_angle;
    if r.drag_started() {
        setter.begin_set_parameter(p);
    }
    if r.dragged() {
        if let Some(pos) = r.interact_pointer_pos() {
            let d = pos - c;
            let deg = d.x.atan2(-d.y).to_degrees().abs().clamp(10.0, 90.0).round();
            setter.set_parameter(p, deg);
        }
    }
    if r.drag_stopped() {
        setter.end_set_parameter(p);
    }
    let on = params.enabled.value();
    let a = p.value();
    let painter = ui.painter();
    painter.circle_filled(c, radius + 30.0, SURFACE);
    painter.circle_stroke(c, radius * 0.45, Stroke::new(1.0, LINE));
    for d in (0..360).step_by(6) {
        painter.circle_filled(pos_on(c, radius, d as f32), 1.2, LINE);
    }
    for d in (0..360).step_by(15) {
        let long = d % 45 == 0;
        painter.line_segment([pos_on(c, radius + 20.0, d as f32), pos_on(c, radius + if long { 28.0 } else { 24.0 }, d as f32)], Stroke::new(1.0, LINE));
    }
    // feixe entre as caixas frontais
    if on {
        let mut pts = vec![c];
        let steps = 24;
        for i in 0..=steps {
            pts.push(pos_on(c, radius, -a + 2.0 * a * i as f32 / steps as f32));
        }
        painter.add(Shape::convex_polygon(pts, Color32::from_rgba_unmultiplied(0xff, 0xb5, 0x47, 34), Stroke::NONE));
    }
    let speakers = [("C", 0.0, false), ("FL", -a, true), ("FR", a, true), ("SL", -90.0, false), ("SR", 90.0, false), ("RL", -150.0, false), ("RR", 150.0, false)];
    for (name, deg, active) in speakers {
        let pos = pos_on(c, radius, deg);
        let lit = active && on;
        let box_ = Rect::from_center_size(pos, Vec2::splat(if active { 20.0 } else { 16.0 }));
        painter.rect_filled(box_, 5.0, if lit { AMBER } else if active { LINE } else { DEEP });
        if !active {
            painter.rect_stroke(box_, 5.0, Stroke::new(1.0, LINE), egui::StrokeKind::Inside);
        }
        painter.text(pos_on(c, radius - 24.0, deg), Align2::CENTER_CENTER, name, display(11.0), if lit { INK } else { SOFT });
    }
    // cabeça
    painter.circle_filled(c + Vec2::new(-17.0, 2.0), 5.0, INK);
    painter.circle_filled(c + Vec2::new(17.0, 2.0), 5.0, INK);
    painter.circle_filled(c, 16.0, INK);
    painter.add(Shape::convex_polygon(vec![c + Vec2::new(-5.0, -15.0), c + Vec2::new(0.0, -24.0), c + Vec2::new(5.0, -15.0)], AMBER, Stroke::NONE));
    painter.text(Pos2::new(c.x, rect.bottom() - 8.0), Align2::CENTER_CENTER, format!("{:.0}° de abertura", a * 2.0), display(14.0), AMBER);
}

/// Equalizador: 10 barras verticais que crescem a partir do zero.
fn eq(ui: &mut Ui, setter: &ParamSetter, bands: [(&FloatParam, &str); 10]) {
    let w = ui.available_width() / 10.0;
    let h = 150.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (p, f) in bands {
            let (rect, r) = ui.allocate_exact_size(Vec2::new(w, h + 44.0), Sense::click_and_drag());
            let top = rect.top() + 22.0;
            let bottom = top + h;
            if r.drag_started() {
                setter.begin_set_parameter(p);
            }
            if r.dragged() {
                if let Some(pos) = r.interact_pointer_pos() {
                    setter.set_parameter_normalized(p, (1.0 - (pos.y - top) / h).clamp(0.0, 1.0));
                }
            }
            if r.drag_stopped() {
                setter.end_set_parameter(p);
            }
            if r.double_clicked() {
                set_plain(setter, p, 0.0);
            }
            let t = p.unmodulated_normalized_value();
            let x = rect.center().x;
            let y_of = |t: f32| bottom - t * h;
            let painter = ui.painter();
            painter.rect_filled(Rect::from_min_max(Pos2::new(x - 3.0, top), Pos2::new(x + 3.0, bottom)), 3.0, LINE);
            let (y0, y1) = (y_of(0.5), y_of(t));
            painter.rect_filled(Rect::from_min_max(Pos2::new(x - 3.0, y0.min(y1)), Pos2::new(x + 3.0, y0.max(y1))), 3.0, AMBER);
            painter.circle_filled(Pos2::new(x, y1), if r.hovered() || r.dragged() { 9.0 } else { 8.0 }, AMBER);
            painter.circle_filled(Pos2::new(x, y1), 4.0, DEEP);
            let v = p.value();
            painter.text(Pos2::new(x, rect.top() + 9.0), Align2::CENTER_CENTER, format!("{}{:.1}", if v > 0.0 { "+" } else { "" }, v), display(12.0), AMBER);
            painter.text(Pos2::new(x, bottom + 14.0), Align2::CENTER_CENTER, f, display(12.0), SOFT);
        }
    });
    // linha do 0 dB
    let _ = ui;
}

pub fn draw(ctx: &egui::Context, setter: &ParamSetter, params: &Arc<SrParams>) {
    egui::CentralPanel::default().frame(egui::Frame::default().fill(NIGHT).inner_margin(18.0)).show(ctx, |ui| {
        egui::ScrollArea::vertical().scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 10.0;
            ui.horizontal(|ui| {
                ui.label(RichText::new("Surround Rescue").font(display(24.0)).color(INK));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let on = params.enabled.value();
                    ui.label(RichText::new(if on { "Ligado" } else { "Desligado" }).font(display(14.0)).color(if on { AMBER } else { SOFT }));
                    toggle(ui, setter, &params.enabled, "");
                });
            });
            ui.columns(2, |cols| {
                stage(&mut cols[0], setter, params);
                card(&mut cols[1], "Campo de som", |ui| {
                    slider(ui, setter, &params.front_angle, "Abertura das caixas");
                    slider(ui, setter, &params.spatial, "Intensidade do surround");
                    ui.label(RichText::new("Arraste as caixas da frente no círculo. Duplo clique num controle volta ao padrão.").font(body(12.0)).color(SOFT));
                });
            });
            card(ui, "Áudio", |ui| {
                ui.columns(2, |cols| {
                    toggle(&mut cols[0], setter, &params.stereo_enhanced, "Estéreo melhorado");
                    slider(&mut cols[0], setter, &params.bass, "Reforço de graves");
                    slider(&mut cols[0], setter, &params.normalization, "Normalização");
                    slider(&mut cols[1], setter, &params.presence, "Clareza: presença");
                    slider(&mut cols[1], setter, &params.voice, "Clareza: volume das vozes");
                    slider(&mut cols[1], setter, &params.volume, "Volume");
                });
            });
            card(ui, "Equalizador", |ui| {
                eq(
                    ui,
                    setter,
                    [
                        (&params.eq0, "31"), (&params.eq1, "63"), (&params.eq2, "125"), (&params.eq3, "250"), (&params.eq4, "500"),
                        (&params.eq5, "1K"), (&params.eq6, "2K"), (&params.eq7, "4K"), (&params.eq8, "8K"), (&params.eq9, "16K"),
                    ],
                );
            });
        });
    });
}
