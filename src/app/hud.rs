//! The on-screen controls: trait ledger (click to pin), audio deck and transport.

use super::DriftApp;
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Sense, Stroke};

const INK: Color32 = Color32::from_rgb(238, 240, 244);
const DIM: Color32 = Color32::from_rgba_premultiplied(124, 125, 127, 133);
const PANEL: Color32 = Color32::from_rgba_premultiplied(8, 8, 11, 205);
const LINE: Color32 = Color32::from_rgba_premultiplied(33, 33, 33, 33);
const WIDTH: f32 = 290.0;

fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

fn label(text: &str) -> egui::RichText {
    egui::RichText::new(text).font(mono(10.0)).color(INK)
}

fn pill(ui: &mut egui::Ui, text: &str, selected: bool) -> egui::Response {
    let fg = if selected { PANEL } else { INK };
    let bg = if selected {
        INK
    } else {
        Color32::from_white_alpha(14)
    };
    ui.add(
        egui::Button::new(
            egui::RichText::new(text.to_uppercase())
                .font(mono(9.5))
                .color(fg),
        )
        .fill(bg)
        .stroke(Stroke::new(1.0, LINE))
        .corner_radius(CornerRadius::same(12))
        .min_size(egui::vec2(0.0, 24.0)),
    )
}

fn rule(ui: &mut egui::Ui) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(WIDTH, 7.0), Sense::hover());
    ui.painter()
        .hline(r.x_range(), r.center().y, Stroke::new(1.0, LINE));
}

pub fn show(ctx: &egui::Context, app: &mut DriftApp) {
    if !app.hud_visible {
        return;
    }
    transport(ctx, app);

    let screen = ctx.content_rect();
    // Short windows drop the trait ledger so the audio controls always fit.
    let compact = screen.height() < 620.0;
    egui::Area::new(egui::Id::new("hud"))
        .pivot(Align2::LEFT_BOTTOM)
        .fixed_pos(egui::pos2(screen.left() + 16.0, screen.bottom() - 16.0))
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(PANEL)
                .stroke(Stroke::new(1.0, LINE))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(egui::Margin::symmetric(14, 12))
                .show(ui, |ui| {
                    ui.set_width(WIDTH);
                    panel(ui, app, compact);
                });
        });
}

fn transport(ctx: &egui::Context, app: &mut DriftApp) {
    let screen = ctx.content_rect();
    egui::Area::new(egui::Id::new("transport"))
        .pivot(Align2::RIGHT_TOP)
        .fixed_pos(egui::pos2(screen.right() - 14.0, screen.top() + 14.0))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if pill(ui, if app.paused { "Play" } else { "Pause" }, false).clicked() {
                    app.paused = !app.paused;
                }
                if pill(ui, "Shift", false).clicked() {
                    app.force_shift();
                }
                let fs = app.fullscreen;
                if pill(ui, if fs { "Window" } else { "Fullscreen" }, false).clicked() {
                    app.set_fullscreen(ctx, !fs);
                }
                if pill(ui, "Hide", false).clicked() {
                    app.hud_visible = false;
                }
            });
        });
}

fn panel(ui: &mut egui::Ui, app: &mut DriftApp, compact: bool) {
    ui.spacing_mut().item_spacing = egui::vec2(6.0, 2.0);
    let active = app.pulse.active();

    // Header
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("DRIFTFIELD")
                .font(mono(13.0))
                .strong()
                .color(INK),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let meta = if app.paused {
                "paused".to_string()
            } else if active {
                format!("on the beat · {}", app.beats_to_shift)
            } else {
                format!("next shift {:.1}s", (app.next_at - app.sim).max(0.0))
            };
            ui.label(egui::RichText::new(meta).font(mono(9.5)).color(DIM));
        });
    });
    rule(ui);

    // Trait ledger
    if compact {
        ui.label(
            egui::RichText::new("Make the window taller to see and pin traits.")
                .font(mono(9.0))
                .color(DIM),
        );
    } else {
        for i in 0..app.traits.defs.len() {
            trait_row(ui, app, i, active);
        }
    }
    rule(ui);

    // Audio deck
    let a = app.last_audio.clone();
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("AUDIO").font(mono(9.0)).color(DIM));
        ui.add_space(30.0);
        let status = app.audio.status(a.levels.present);
        ui.add(egui::Label::new(label(&status)).truncate());
    });
    ui.horizontal(|ui| {
        let bpm = if a.levels.bpm > 0.0 {
            format!("{:.0} bpm", a.levels.bpm)
        } else {
            "— bpm".into()
        };
        ui.label(egui::RichText::new(bpm).font(mono(9.5)).color(DIM));
    });
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("VIBE").font(mono(9.0)).color(DIM));
        ui.add_space(37.0);
        let text = if app.pulse.active() {
            app.vibe.describe()
        } else {
            "—".into()
        };
        ui.add(egui::Label::new(label(&text)).truncate());
    });
    meters(ui, &a.levels, &app.pulse);
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        let demo = app.audio.demo_on();
        if pill(ui, if demo { "Stop demo" } else { "Demo beat" }, demo).clicked() {
            app.audio.set_demo(!demo);
        }
        if pill(ui, "Reduce motion", app.reduced_motion).clicked() {
            app.reduced_motion = !app.reduced_motion;
        }
        if !app.audio.system_ok() && pill(ui, "Retry system audio", false).clicked() {
            app.audio.start_system();
        }
    });
    rule(ui);
    ui.add(
        egui::Label::new(
            egui::RichText::new(
                "Moves to whatever your computer is playing. Arrows change a trait, the dot pins it. \
                 Space pauses, N shifts, H hides, F fullscreen, D demo beat.",
            )
            .font(mono(9.0))
            .color(DIM),
        )
        .wrap(),
    );
}

/// Draws a small solid triangle pointing left (`dir < 0`) or right.
fn arrow(p: &egui::Painter, center: egui::Pos2, dir: f32, color: Color32) {
    let (w, h) = (3.5 * dir, 4.0);
    p.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(center.x + w, center.y),
            egui::pos2(center.x - w, center.y - h),
            egui::pos2(center.x - w, center.y + h),
        ],
        color,
        Stroke::NONE,
    ));
}

/// One trait: ◀ LABEL value ▶ (pin). The arrows step through the options; only the
/// dot on the right pins.
fn trait_row(ui: &mut egui::Ui, app: &mut DriftApp, i: usize, active: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(WIDTH, 20.0), Sense::hover());
    let st = app.traits.state[i].clone();
    let def_label = app.traits.defs[i].label;
    let p = ui.painter_at(rect);
    let y = rect.center().y;

    let age = (app.sim - app.flashed[i]) as f32;
    if (0.0..1.0).contains(&age) {
        let a = ((1.0 - age) * 46.0) as u8;
        p.rect_filled(rect, CornerRadius::same(4), Color32::from_white_alpha(a));
    }

    // Hit areas
    let left = egui::Rect::from_min_size(rect.min, egui::vec2(18.0, rect.height()));
    let pin = egui::Rect::from_min_size(
        egui::pos2(rect.right() - 20.0, rect.top()),
        egui::vec2(20.0, rect.height()),
    );
    let right = egui::Rect::from_min_size(
        egui::pos2(pin.left() - 18.0, rect.top()),
        egui::vec2(18.0, rect.height()),
    );
    let id = ui.id().with(("trait", i));
    let l_resp = ui.interact(left, id.with("prev"), Sense::click());
    let r_resp = ui.interact(right, id.with("next"), Sense::click());
    let p_resp = ui.interact(pin, id.with("pin"), Sense::click());

    for (resp, r) in [(&l_resp, left), (&r_resp, right), (&p_resp, pin)] {
        if resp.hovered() {
            p.rect_filled(
                r.shrink(1.0),
                CornerRadius::same(4),
                Color32::from_white_alpha(22),
            );
        }
    }
    let arrow_color = |hovered: bool| if hovered { Color32::WHITE } else { DIM };
    arrow(&p, left.center(), -1.0, arrow_color(l_resp.hovered()));
    arrow(&p, right.center(), 1.0, arrow_color(r_resp.hovered()));

    let key_color = if st.locked { INK } else { DIM };
    p.text(
        egui::pos2(left.right() + 2.0, y),
        Align2::LEFT_CENTER,
        def_label.to_uppercase(),
        mono(9.0),
        key_color,
    );
    let desc = app.traits.describe(i, active);
    let idle = desc.ends_with("needs audio");
    let val_color = if st.moving() {
        Color32::WHITE
    } else if idle {
        DIM
    } else {
        INK
    };
    let value_rect = egui::Rect::from_min_max(
        egui::pos2(left.right() + 80.0, rect.top()),
        egui::pos2(right.left() - 2.0, rect.bottom()),
    );
    ui.painter_at(value_rect).text(
        egui::pos2(value_rect.left(), y),
        Align2::LEFT_CENTER,
        desc,
        mono(10.5),
        val_color,
    );

    let dot = pin.center();
    if st.locked {
        p.circle_filled(dot, 3.5, INK);
    } else {
        let c = if p_resp.hovered() { INK } else { DIM };
        p.circle_stroke(dot, 3.5, Stroke::new(1.0, c));
    }
    if st.moving() {
        let prog = app.traits.progress(i, app.sim);
        let x0 = left.right() + 2.0;
        let x1 = x0 + (right.left() - 2.0 - x0) * prog;
        p.hline(
            x0..=x1,
            rect.bottom() - 1.0,
            Stroke::new(1.0, Color32::WHITE),
        );
    }

    if l_resp.clicked() {
        app.step_trait(i, -1);
    }
    if r_resp.clicked() {
        app.step_trait(i, 1);
    }
    if p_resp.clicked() {
        app.traits.toggle_lock(i);
    }
    l_resp.on_hover_text("Previous");
    r_resp.on_hover_text("Next");
    p_resp.on_hover_text(if st.locked {
        "Pinned. Click to let it change."
    } else {
        "Pin: keep this one"
    });
}

fn meters(
    ui: &mut egui::Ui,
    l: &driftfield::analysis::Levels,
    pulse: &driftfield::reactions::Pulse,
) {
    let gate = pulse.gate;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(WIDTH, 24.0), Sense::hover());
    let p = ui.painter_at(rect);
    let bands = [("LOW", l.low), ("MID", l.mid), ("HIGH", l.high)];
    let dots_w = 66.0;
    let w = (WIDTH - dots_w) / 3.0 - 6.0;
    for (k, (name, v)) in bands.iter().enumerate() {
        let x = rect.left() + k as f32 * (w + 6.0);
        p.text(
            egui::pos2(x, rect.top() + 6.0),
            Align2::LEFT_CENTER,
            *name,
            mono(8.0),
            DIM,
        );
        let track = egui::Rect::from_min_size(egui::pos2(x, rect.top() + 15.0), egui::vec2(w, 3.0));
        p.rect_filled(track, CornerRadius::same(2), Color32::from_white_alpha(26));
        let mut fill = track;
        fill.set_width(w * (v * gate).clamp(0.0, 1.0));
        p.rect_filled(fill, CornerRadius::same(2), INK);
    }
    // One dot per instrument: kick, snare, hats.
    let hits = [
        ("K", pulse.kick),
        ("S", pulse.snare_env),
        ("H", pulse.hat_env),
    ];
    for (k, (name, v)) in hits.iter().enumerate() {
        let cx = rect.right() - dots_w + 10.0 + k as f32 * 22.0;
        p.text(
            egui::pos2(cx, rect.top() + 6.0),
            Align2::CENTER_CENTER,
            *name,
            mono(8.0),
            DIM,
        );
        let alpha = ((0.12 + 0.88 * (v * gate).clamp(0.0, 1.0)) * 255.0) as u8;
        p.circle_filled(
            egui::pos2(cx, rect.top() + 16.0),
            4.0,
            Color32::from_white_alpha(alpha),
        );
    }
}
