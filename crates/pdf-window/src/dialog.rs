use eframe::egui;

use crate::format::{CONTROL_HEIGHT, quiet_icon_button};
use crate::icons::Icon;

const EDGE: f32 = 14.0;
const HEADER_ROOM: f32 = 96.0;
const FOOTER_ROOM: f32 = 70.0;
pub(crate) const FRAME: f32 = 32.0;
const TALL: f32 = 400.0;
const LABEL_WIDTH: f32 = 96.0;
const ROUND: u8 = 6;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Tone {
    Calm,
    Warning,
    Trouble,
}

pub(crate) struct Spec<'a> {
    pub(crate) id: &'a str,
    pub(crate) width: f32,
}

pub(crate) fn weak(ui: &egui::Ui) -> egui::Color32 {
    ui.visuals().weak_text_color()
}

fn tone_ink(ui: &egui::Ui, tone: Tone) -> egui::Color32 {
    match tone {
        Tone::Calm => weak(ui),
        Tone::Warning => ui.visuals().warn_fg_color,
        Tone::Trouble => ui.visuals().error_fg_color,
    }
}

pub(crate) fn panel<R>(
    ctx: &egui::Context,
    canvas: egui::Rect,
    spec: &Spec<'_>,
    content: impl FnOnce(&mut egui::Ui, f32) -> R,
) -> Option<R> {
    let room = (canvas.height() - 2.0 * EDGE - HEADER_ROOM - FOOTER_ROOM).max(120.0);
    egui::Window::new(spec.id)
        .id(egui::Id::new(spec.id))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .frame(egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(16)))
        .min_width(spec.width)
        .max_width(spec.width)
        .default_pos(opening_corner(ctx, canvas, spec.width))
        .default_size(egui::vec2(outer_width(ctx, spec.width), 100.0))
        .constrain_to(canvas)
        .show(ctx, |ui| {
            ui.set_width(spec.width);
            content(ui, room)
        })
        .and_then(|shown| shown.inner)
}

fn outer_width(ctx: &egui::Context, width: f32) -> f32 {
    width + FRAME + 2.0 * ctx.global_style().visuals.window_stroke.width
}

fn opening_corner(ctx: &egui::Context, canvas: egui::Rect, width: f32) -> egui::Pos2 {
    let outer = outer_width(ctx, width);
    let x = if canvas.width() - outer < 4.0 * EDGE {
        canvas.center().x - outer / 2.0
    } else {
        canvas.right() - EDGE - outer
    };
    egui::pos2(x, canvas.top() + EDGE)
}

pub(crate) fn beside<R>(
    ctx: &egui::Context,
    canvas: egui::Rect,
    (id, at): (egui::Id, egui::Pos2),
    width: f32,
    content: impl FnOnce(&mut egui::Ui, f32) -> R,
) -> Option<R> {
    let room = (canvas.height() - 2.0 * EDGE - HEADER_ROOM - FOOTER_ROOM).max(120.0);
    let measured = ctx
        .memory(|memory| memory.area_rect(id))
        .is_some_and(|rect| rect.height() > 60.0);
    let window = egui::Window::new("beside")
        .id(id)
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .frame(egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(16)))
        .min_width(width)
        .max_width(width)
        .default_pos(kept_inside(at, canvas, width + FRAME));
    let window = if measured {
        window.constrain_to(canvas)
    } else {
        window.constrain(false)
    };
    window
        .show(ctx, |ui| content(ui, room))
        .and_then(|shown| shown.inner)
}

fn kept_inside(at: egui::Pos2, canvas: egui::Rect, wide: f32) -> egui::Pos2 {
    let x =
        at.x.min(canvas.right() - wide - EDGE)
            .max(canvas.left() + EDGE);
    let y =
        at.y.min(canvas.bottom() - TALL - EDGE)
            .max(canvas.top() + EDGE);
    egui::pos2(x, y)
}

pub(crate) fn modal<R>(
    ctx: &egui::Context,
    spec: &Spec<'_>,
    content: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::ModalResponse<R> {
    egui::Modal::new(egui::Id::new(spec.id))
        .frame(egui::Frame::popup(&ctx.global_style()).inner_margin(egui::Margin::same(16)))
        .show(ctx, |ui| {
            ui.set_width(spec.width);
            content(ui)
        })
}

pub(crate) fn header(
    ui: &mut egui::Ui,
    title: &str,
    about: Option<&str>,
    close: Option<&str>,
) -> bool {
    let mut closed = false;
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(close) = close {
                closed = quiet_icon_button(ui, Icon::Close, close).clicked();
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(egui::Label::new(egui::RichText::new(title).size(15.0).strong()).truncate());
            });
        });
    });
    if let Some(about) = about {
        ui.add_space(2.0);
        ui.add(egui::Label::new(egui::RichText::new(about).size(12.0).color(weak(ui))).wrap());
    }
    ui.add_space(10.0);
    closed
}

pub(crate) fn tool_header(
    ui: &mut egui::Ui,
    title: &str,
    about: Option<&str>,
    close: Option<&str>,
    lang: pdf_app::wording::Lang,
) -> bool {
    let key = folded_key(ui);
    let folded = is_folded(ui);
    let mut closed = false;
    let mut flip = false;
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(close) = close {
                closed = quiet_icon_button(ui, Icon::Close, close).clicked();
            }
            let (icon, word) = if folded {
                (Icon::Expand, pdf_app::wording::Message::Unfold)
            } else {
                (Icon::Up, pdf_app::wording::Message::Fold)
            };
            flip = quiet_icon_button(ui, icon, &word.say(lang)).clicked();
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                let title = egui::RichText::new(title).size(15.0).strong();
                let label = egui::Label::new(title).truncate();
                let label = if folded {
                    label.sense(egui::Sense::click())
                } else {
                    label
                };
                if ui.add(label).clicked() {
                    flip = true;
                }
            });
        });
    });
    if flip {
        ui.data_mut(|data| data.insert_temp(key, !folded));
    }
    if closed {
        ui.data_mut(|data| data.remove::<bool>(key));
    }
    if !folded {
        if let Some(about) = about {
            ui.add_space(2.0);
            ui.add(egui::Label::new(egui::RichText::new(about).size(12.0).color(weak(ui))).wrap());
        }
        ui.add_space(10.0);
    }
    closed
}

pub(crate) fn body(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    if !is_folded(ui) {
        add(ui);
    }
}

fn folded_key(ui: &egui::Ui) -> egui::Id {
    ui.layer_id().id.with("folded")
}

pub(crate) fn is_folded(ui: &egui::Ui) -> bool {
    ui.data(|data| data.get_temp::<bool>(folded_key(ui)))
        .unwrap_or(false)
}

pub(crate) fn caption(ui: &mut egui::Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(
        egui::RichText::new(text)
            .size(12.0)
            .strong()
            .color(weak(ui)),
    );
    ui.add_space(4.0);
}

pub(crate) fn panel_caption(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(11.0).color(weak(ui)));
}

pub(crate) fn small(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(egui::RichText::new(text).size(11.5).color(weak(ui))).wrap());
}

pub(crate) fn hairline(ui: &mut egui::Ui) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        ui.visuals().widgets.noninteractive.bg_stroke,
    );
}

pub(crate) fn divide(ui: &mut egui::Ui) {
    ui.add_space(10.0);
    hairline(ui);
    ui.add_space(8.0);
}

pub(crate) fn scrolling(ui: &mut egui::Ui, id: &str, room: f32, add: impl FnOnce(&mut egui::Ui)) {
    let known = ui.id().with(("natural height", id));
    let natural: f32 = ui.data(|data| data.get_temp(known)).unwrap_or(0.0);
    let shown = ui
        .scope(|ui| {
            ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
            egui::ScrollArea::vertical()
                .id_salt(id)
                .max_height(room)
                .min_scrolled_height(natural.min(room))
                .auto_shrink([false, true])
                .show(ui, add)
        })
        .inner;
    let now = shown.content_size.y;
    ui.data_mut(|data| data.insert_temp(known, now));
    if (now - natural).abs() > 0.5 {
        ui.ctx().request_repaint();
    }
}

pub(crate) fn footer(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    footer_with(ui, |_| {}, add);
}

pub(crate) fn footer_with(
    ui: &mut egui::Ui,
    left: impl FnOnce(&mut egui::Ui),
    right: impl FnOnce(&mut egui::Ui),
) {
    ui.add_space(12.0);
    hairline(ui);
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        left(ui);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
    });
}

pub(crate) fn primary(ui: &mut egui::Ui, text: &str, enabled: bool) -> egui::Response {
    let visuals = ui.visuals();
    let button = egui::Button::new(egui::RichText::new(text).color(egui::Color32::WHITE))
        .fill(visuals.selection.stroke.color)
        .corner_radius(ROUND)
        .min_size(egui::vec2(96.0, CONTROL_HEIGHT));
    ui.add_enabled(enabled, button)
}

pub(crate) fn secondary(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(text)
            .corner_radius(ROUND)
            .min_size(egui::vec2(72.0, CONTROL_HEIGHT)),
    )
}

pub(crate) fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let visuals = ui.visuals();
    egui::Frame::new()
        .fill(visuals.faint_bg_color)
        .stroke(visuals.widgets.noninteractive.bg_stroke)
        .corner_radius(8.0)
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

pub(crate) fn note(ui: &mut egui::Ui, tone: Tone, text: &str) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let ink = tone_ink(ui, tone);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 18.0), egui::Sense::hover());
        let mark = egui::Rect::from_center_size(rect.center(), egui::vec2(15.0, 15.0));
        Icon::Info.draw(ui.painter(), mark, ink);
        let words = egui::RichText::new(text).size(12.0);
        let words = if tone == Tone::Calm {
            words.color(ui.visuals().text_color())
        } else {
            words.color(ink)
        };
        ui.add(egui::Label::new(words).wrap());
    });
}

pub(crate) fn labelled(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(LABEL_WIDTH, CONTROL_HEIGHT),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_size(egui::vec2(LABEL_WIDTH, CONTROL_HEIGHT));
                ui.add(
                    egui::Label::new(egui::RichText::new(label).size(13.0).color(weak(ui)))
                        .truncate(),
                );
            },
        );
        add(ui);
    });
}

pub(crate) fn focus_ring(ui: &egui::Ui, response: &egui::Response, rect: egui::Rect, radius: f32) {
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(1.5),
            radius + 1.5,
            egui::Stroke::new(1.5, ui.visuals().selection.stroke.color),
            egui::StrokeKind::Outside,
        );
    }
}

pub(crate) fn segments<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    id: &str,
    now: &mut T,
    options: &[(T, String)],
) -> bool {
    let count = f32::from(u8::try_from(options.len()).unwrap_or(u8::MAX).max(1));
    let (whole, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), CONTROL_HEIGHT),
        egui::Sense::hover(),
    );
    let each = whole.width() / count;
    let enabled = ui.is_enabled();
    let visuals = ui.visuals().clone();
    let edge = visuals.widgets.noninteractive.bg_stroke;
    let mut changed = false;
    for (at, (value, name)) in options.iter().enumerate() {
        let step = f32::from(u8::try_from(at).unwrap_or(u8::MAX));
        let cell = egui::Rect::from_min_size(
            whole.min + egui::vec2(each * step, 0.0),
            egui::vec2(each, CONTROL_HEIGHT),
        );
        let sense = if enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        };
        let response = ui.interact(cell, ui.id().with((id, at)), sense);
        let on = *now == *value;
        let inner = cell.shrink(2.0);
        if on {
            ui.painter()
                .rect_filled(inner, 4.0, visuals.selection.bg_fill.gamma_multiply(0.45));
        } else if enabled && response.hovered() {
            ui.painter()
                .rect_filled(inner, 4.0, visuals.widgets.hovered.weak_bg_fill);
        }
        let ink = if !enabled {
            visuals.weak_text_color()
        } else if on {
            visuals.selection.stroke.color
        } else {
            visuals.text_color()
        };
        ui.painter().text(
            cell.center(),
            egui::Align2::CENTER_CENTER,
            name,
            egui::FontId::proportional(13.0),
            ink,
        );
        focus_ring(ui, &response, inner, 4.0);
        if enabled && response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if response.clicked() && !on {
            *now = *value;
            changed = true;
        }
    }
    ui.painter()
        .rect_stroke(whole, 6.0, edge, egui::StrokeKind::Inside);
    changed
}

pub(crate) fn foldable(
    ui: &mut egui::Ui,
    id: egui::Id,
    title: &str,
    body: impl FnOnce(&mut egui::Ui),
) {
    let mut open = ui.data(|data| data.get_temp::<bool>(id)).unwrap_or(false);
    let header = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 16.0), egui::Sense::hover());
        let icon = if open { Icon::Expand } else { Icon::Next };
        icon.draw(
            ui.painter(),
            egui::Rect::from_center_size(rect.center(), egui::vec2(10.0, 10.0)),
            weak(ui),
        );
        ui.label(egui::RichText::new(title).size(12.0).color(weak(ui)));
    });
    let toggle = ui.interact(
        header.response.rect.expand2(egui::vec2(0.0, 2.0)),
        id.with("header"),
        egui::Sense::click(),
    );
    focus_ring(ui, &toggle, toggle.rect, 3.0);
    if toggle.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if toggle.clicked() {
        open = !open;
    }
    ui.data_mut(|data| data.insert_temp(id, open));
    if open {
        ui.add_space(4.0);
        body(ui);
    }
}

pub(crate) fn spinner(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(60));
    let radius = rect.width() / 2.0 - 1.5;
    let time = ui.input(|input| input.time);
    let start = time * std::f64::consts::TAU;
    let points: Vec<egui::Pos2> = (0..=14_u32)
        .map(|at| {
            let angle = start + 4.2 * f64::from(at) / 14.0;
            let (sin, cos) = angle.sin_cos();
            #[expect(clippy::cast_possible_truncation, reason = "a point on the screen")]
            let offset = egui::vec2(cos as f32, sin as f32);
            rect.center() + radius * offset
        })
        .collect();
    ui.painter().add(egui::Shape::line(
        points,
        egui::Stroke::new(1.6, ui.visuals().weak_text_color()),
    ));
}

pub(crate) fn bar(ui: &mut egui::Ui, fraction: Option<f32>) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 8.0), egui::Sense::hover());
    let visuals = ui.visuals();
    let ink = visuals.selection.stroke.color;
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, visuals.widgets.inactive.weak_bg_fill);
    if let Some(share) = fraction {
        let mut filled = rect;
        filled.set_right(rect.left() + rect.width() * share.clamp(0.03, 1.0));
        painter.rect_filled(filled, 4.0, ink);
        return;
    }
    let time = ui.input(|input| input.time);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a position along a bar, between nothing and the whole of it"
    )]
    let along = ((time * 0.7).fract() * 1.4 - 0.2) as f32;
    let span = rect.width() * 0.3;
    let left = rect.left() + rect.width() * along;
    let pill = egui::Rect::from_min_max(
        egui::pos2(left, rect.top()),
        egui::pos2(left + span, rect.bottom()),
    );
    painter.rect_filled(pill, 4.0, ink);
    ui.ctx().request_repaint();
}

#[cfg(test)]
mod tests {
    use super::{Spec, body, panel, tool_header};
    use eframe::egui;

    #[test]
    fn a_tool_window_folds_to_its_header_and_opens_again() {
        let ctx = egui::Context::default();
        let canvas = egui::Rect::from_min_size(egui::pos2(0.0, 40.0), egui::vec2(1200.0, 760.0));
        let frame = |events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 800.0),
                )),
                events,
                ..egui::RawInput::default()
            };
            let mut drew_the_body = false;
            let _ = ctx.run_ui(input, |ui| {
                let spec = Spec {
                    id: "fold-probe",
                    width: 300.0,
                };
                panel(ui.ctx(), canvas, &spec, |ui, _| {
                    let _ = tool_header(
                        ui,
                        "Watermark",
                        Some("One line on every page."),
                        Some("Close"),
                        pdf_app::wording::Lang::English,
                    );
                    body(ui, |ui| {
                        ui.add_space(300.0);
                        drew_the_body = true;
                    });
                });
            });
            let placed = ctx
                .memory(|memory| memory.area_rect(egui::Id::new("fold-probe")))
                .unwrap_or(egui::Rect::NOTHING);
            (placed, drew_the_body)
        };
        let buttons = || {
            let window = egui::LayerId::new(egui::Order::Middle, egui::Id::new("fold-probe"));
            let mut found: Vec<egui::Rect> = ctx.viewport(|viewport| {
                viewport
                    .prev_pass
                    .widgets
                    .get_layer(window)
                    .filter(|widget| widget.sense.senses_click() && widget.rect.width() < 40.0)
                    .map(|widget| widget.rect)
                    .collect()
            });
            found.sort_by(|a, b| b.right().total_cmp(&a.right()));
            found
        };
        let click = |at: egui::Pos2| {
            let press = |pressed| egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            let _ = frame(vec![egui::Event::PointerMoved(at)]);
            let _ = frame(vec![press(true)]);
            let _ = frame(vec![press(false)]);
            frame(Vec::new())
        };
        let _ = frame(Vec::new());
        let (open, drawn) = frame(Vec::new());
        assert!(drawn && open.height() > 300.0, "{open:?}");
        let found = buttons();
        assert!(found.len() >= 2, "{found:?}");
        let (folded, drawn) = click(found[1].center());
        assert!(!drawn, "a folded window leaves its body out");
        assert!(folded.height() < 80.0, "folded to {folded:?}");
        assert!(
            (folded.top() - open.top()).abs() < 1.0,
            "{open:?} -> {folded:?}"
        );
        let (again, drawn) = click(buttons()[1].center());
        assert!(drawn, "unfolded, the body is back");
        assert!(
            (again.height() - open.height()).abs() < 1.0,
            "{open:?} -> {again:?}"
        );
    }

    fn frame_of(canvas: egui::Rect, width: f32) -> egui::Rect {
        let ctx = egui::Context::default();
        let mut seen = egui::Rect::NOTHING;
        for _ in 0..3 {
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                let spec = Spec { id: "probe", width };
                panel(ui.ctx(), canvas, &spec, |ui, _| {
                    ui.label("hello");
                    seen = ui.min_rect();
                });
            });
        }
        seen
    }

    #[test]
    fn a_panel_opens_at_the_top_right_and_can_be_dragged() {
        let ctx = egui::Context::default();
        let canvas = egui::Rect::from_min_size(egui::pos2(0.0, 40.0), egui::vec2(1200.0, 760.0));
        let frame = |events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 800.0),
                )),
                events,
                ..egui::RawInput::default()
            };
            let mut placed = egui::Rect::NOTHING;
            let _ = ctx.run_ui(input, |ui| {
                let spec = Spec {
                    id: "drag-probe",
                    width: 300.0,
                };
                panel(ui.ctx(), canvas, &spec, |ui, _| {
                    ui.add_space(120.0);
                });
                if let Some(rect) = ui
                    .ctx()
                    .memory(|memory| memory.area_rect(egui::Id::new("drag-probe")))
                {
                    placed = rect;
                }
            });
            placed
        };
        let press = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(Vec::new());
        let at = frame(Vec::new());
        assert!(
            (at.right() - (canvas.right() - super::EDGE)).abs() < 1.0,
            "{at:?}"
        );
        assert!(
            (at.top() - (canvas.top() + super::EDGE)).abs() < 1.0,
            "{at:?}"
        );

        let grab = egui::pos2(at.center().x, at.top() + 6.0);
        let to = grab + egui::vec2(-400.0, 200.0);
        frame(vec![egui::Event::PointerMoved(grab)]);
        frame(vec![press(grab, true)]);
        for step in 1..=10_u8 {
            let t = f32::from(step) / 10.0;
            frame(vec![egui::Event::PointerMoved(grab + (to - grab) * t)]);
        }
        frame(vec![press(to, false)]);
        let moved = frame(Vec::new());
        assert!(
            (moved.left() - (at.left() - 400.0)).abs() < 1.0,
            "{at:?} -> {moved:?}"
        );
        assert!(
            (moved.top() - (at.top() + 200.0)).abs() < 1.0,
            "{at:?} -> {moved:?}"
        );
    }

    #[test]
    fn a_segment_is_chosen_by_clicking_it_and_only_once() {
        let ctx = egui::Context::default();
        let mut now = 'a';
        let options = [
            ('a', "A".to_owned()),
            ('b', "B".to_owned()),
            ('c', "C".to_owned()),
        ];
        let mut times = 0;
        let step = |ctx: &egui::Context, events: Vec<egui::Event>, chosen: &mut char| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(600.0, 200.0),
                )),
                events,
                ..egui::RawInput::default()
            };
            let mut moved = false;
            let _ = ctx.run_ui(input, |ui| {
                moved = super::segments(ui, "probe", chosen, &options);
            });
            moved
        };
        let _ = step(&ctx, Vec::new(), &mut now);
        let inside_c = egui::pos2(560.0, 18.0);
        let press = |pressed| egui::Event::PointerButton {
            pos: inside_c,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let _ = step(&ctx, vec![egui::Event::PointerMoved(inside_c)], &mut now);
        let _ = step(&ctx, vec![press(true)], &mut now);
        times += usize::from(step(&ctx, vec![press(false)], &mut now));
        assert_eq!(now, 'c', "the last third of the row is the last choice");
        times += usize::from(step(&ctx, Vec::new(), &mut now));
        assert_eq!(times, 1, "it reported the change once");
    }

    #[test]
    fn a_window_beside_its_target_is_kept_inside_the_canvas() {
        let canvas = egui::Rect::from_min_size(egui::pos2(160.0, 100.0), egui::vec2(1200.0, 870.0));
        let wide = 352.0;
        let low = super::kept_inside(egui::pos2(1300.0, 900.0), canvas, wide);
        assert!(low.x + wide <= canvas.right(), "{low:?}");
        assert!(low.y + super::TALL <= canvas.bottom(), "{low:?}");
        let fits = egui::pos2(300.0, 200.0);
        assert_eq!(super::kept_inside(fits, canvas, wide), fits);
    }

    #[test]
    fn a_panel_sits_inside_the_canvas_whatever_else_is_on_screen() {
        let canvas = egui::Rect::from_min_max(egui::pos2(170.0, 80.0), egui::pos2(820.0, 700.0));
        let seen = frame_of(canvas, 340.0);
        assert!(
            canvas.contains_rect(seen),
            "{seen:?} is not inside {canvas:?}"
        );
        assert!(
            seen.right() <= canvas.right() - 10.0,
            "it keeps clear of the canvas edge: {seen:?}"
        );
    }
}
