use eframe::egui;

use crate::format::CONTROL_HEIGHT;
use crate::icons::Icon;

pub(super) use crate::dialog::{hairline, spinner};

pub(super) const CAPTION: f32 = 11.0;

pub(super) const ROUND: u8 = 6;

pub(super) fn caption(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).size(CAPTION).weak()
}

pub(super) fn small(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).size(CAPTION)
}

pub(super) fn accent_fill(ui: &egui::Ui) -> egui::Color32 {
    ui.visuals().selection.bg_fill
}

pub(super) fn accent_ink(ui: &egui::Ui) -> egui::Color32 {
    ui.visuals().selection.stroke.color
}

pub(super) fn primary_button(ui: &mut egui::Ui, text: &str, enabled: bool) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(egui::RichText::new(text).color(accent_ink(ui)))
            .fill(accent_fill(ui))
            .corner_radius(ROUND)
            .min_size(egui::vec2(0.0, CONTROL_HEIGHT)),
    )
}

pub(super) fn secondary_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(egui::Button::new(text).min_size(egui::vec2(0.0, CONTROL_HEIGHT)))
}

pub(super) fn framed_button(ui: &mut egui::Ui, text: egui::RichText) -> egui::Response {
    let visuals = ui.visuals();
    ui.add(
        egui::Button::new(text)
            .fill(visuals.extreme_bg_color)
            .stroke(visuals.widgets.noninteractive.bg_stroke)
            .corner_radius(ROUND)
            .min_size(egui::vec2(0.0, 24.0)),
    )
}

pub(super) fn link(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(egui::Link::new(small(text)))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Dot {
    Offline,
    Ready,
    Working,
    NeedsYou,
}

pub(super) fn status_dot(ui: &mut egui::Ui, state: Dot) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(10.0, 16.0), egui::Sense::hover());
    let visuals = ui.visuals();
    let centre = rect.center();
    match state {
        Dot::Offline => {
            ui.painter().circle_stroke(
                centre,
                3.5,
                egui::Stroke::new(1.3, visuals.weak_text_color()),
            );
        }
        Dot::Ready => {
            ui.painter()
                .circle_filled(centre, 4.0, visuals.selection.stroke.color);
        }
        Dot::Working => {
            let time = ui.input(|input| input.time);
            let beat = 0.5 + 0.5 * (time * std::f64::consts::TAU * 0.9).sin();
            #[expect(clippy::cast_possible_truncation, reason = "a share of one")]
            let share = (0.4 + 0.6 * beat) as f32;
            ui.painter().circle_filled(
                centre,
                4.0,
                visuals.selection.stroke.color.gamma_multiply(share),
            );
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(60));
        }
        Dot::NeedsYou => {
            ui.painter()
                .circle_filled(centre, 4.0, visuals.warn_fg_color);
            ui.painter().circle_stroke(
                centre,
                6.0,
                egui::Stroke::new(1.0, visuals.warn_fg_color.gamma_multiply(0.4)),
            );
        }
    }
    response
}

pub(super) struct Chip<'a> {
    pub(super) icon: Option<Icon>,
    pub(super) text: &'a str,
    pub(super) trailing: Option<Icon>,
    pub(super) on: bool,
    pub(super) ink: Option<egui::Color32>,
    pub(super) dashed: bool,
}

impl<'a> Chip<'a> {
    pub(super) const fn new(text: &'a str) -> Self {
        Self {
            icon: None,
            text,
            trailing: None,
            on: false,
            ink: None,
            dashed: false,
        }
    }
}

const CHIP_HEIGHT: f32 = 26.0;
const CHIP_ROUND: u8 = 13;
const CHIP_ICON: f32 = 14.0;
const CHIP_PAD: f32 = 8.0;

pub(super) fn chip(ui: &mut egui::Ui, chip: &Chip<'_>) -> egui::Response {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let ink = chip.ink.unwrap_or_else(|| ui.visuals().text_color());
    let galley = ui.painter().layout_no_wrap(chip.text.to_owned(), font, ink);
    let mut width = CHIP_PAD * 2.0 + galley.size().x;
    if chip.icon.is_some() {
        width += CHIP_ICON + 5.0;
    }
    if chip.trailing.is_some() {
        width += 12.0 + 4.0;
    }
    let width = width.min(ui.available_width().max(CHIP_HEIGHT));
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, CHIP_HEIGHT), egui::Sense::click());
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let visuals = ui.visuals();
    let round = egui::CornerRadius::same(CHIP_ROUND);
    if chip.on {
        ui.painter()
            .rect_filled(rect, round, visuals.selection.bg_fill.gamma_multiply(0.35));
    } else if response.hovered() {
        ui.painter()
            .rect_filled(rect, round, visuals.widgets.hovered.weak_bg_fill);
    }
    let edge = if chip.on {
        visuals.selection.stroke.color.gamma_multiply(0.6)
    } else {
        visuals.widgets.noninteractive.bg_stroke.color
    };
    if chip.dashed && !chip.on {
        let edge = egui::Stroke::new(1.0, edge);
        ui.painter()
            .add(egui::Shape::dashed_line(&outline(rect), edge, 3.0, 3.0));
    } else {
        ui.painter().rect_stroke(
            rect,
            round,
            egui::Stroke::new(1.0, edge),
            egui::StrokeKind::Inside,
        );
    }
    let mut at = rect.left() + CHIP_PAD;
    if let Some(icon) = chip.icon {
        let slot = egui::Rect::from_center_size(
            egui::pos2(at + CHIP_ICON / 2.0, rect.center().y),
            egui::vec2(CHIP_ICON, CHIP_ICON),
        );
        icon.draw(ui.painter(), slot, ink);
        at += CHIP_ICON + 5.0;
    }
    ui.painter().galley(
        egui::pos2(at, rect.center().y - galley.size().y / 2.0),
        galley,
        ink,
    );
    if let Some(icon) = chip.trailing {
        let slot = egui::Rect::from_center_size(
            egui::pos2(rect.right() - CHIP_PAD - 6.0 + 2.0, rect.center().y),
            egui::vec2(12.0, 12.0),
        );
        icon.draw(ui.painter(), slot, visuals.weak_text_color());
    }
    response
}

fn outline(rect: egui::Rect) -> Vec<egui::Pos2> {
    let radius = rect.height() / 2.0;
    let mut points = Vec::new();
    for step in 0..=12_u8 {
        let angle = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * f32::from(step) / 12.0;
        points.push(
            egui::pos2(rect.right() - radius, rect.center().y)
                + radius * egui::vec2(angle.cos(), angle.sin()),
        );
    }
    for step in 0..=12_u8 {
        let angle = std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * f32::from(step) / 12.0;
        points.push(
            egui::pos2(rect.left() + radius, rect.center().y)
                + radius * egui::vec2(angle.cos(), angle.sin()),
        );
    }
    points.push(points[0]);
    points
}

pub(super) fn round_button(
    ui: &mut egui::Ui,
    icon: Icon,
    hover: &str,
    enabled: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(CONTROL_HEIGHT, CONTROL_HEIGHT),
        if enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    if ui.is_rect_visible(rect) {
        let visuals = ui.visuals();
        let (fill, ink) = if enabled {
            let fill = if response.hovered() {
                visuals.selection.bg_fill.gamma_multiply(1.25)
            } else {
                visuals.selection.bg_fill
            };
            (fill, visuals.selection.stroke.color)
        } else {
            (
                visuals.widgets.noninteractive.weak_bg_fill,
                visuals.weak_text_color().gamma_multiply(0.7),
            )
        };
        ui.painter()
            .circle_filled(rect.center(), rect.width() / 2.0, fill);
        icon.draw(ui.painter(), rect.shrink(7.0), ink);
    }
    response.on_hover_text(hover)
}

pub(super) fn disclosure(ui: &mut egui::Ui, open: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 14.0), egui::Sense::hover());
    let icon = if open { Icon::Expand } else { Icon::Next };
    icon.draw(
        ui.painter(),
        egui::Rect::from_center_size(rect.center(), egui::vec2(10.0, 10.0)),
        ui.visuals().weak_text_color(),
    );
}

pub(super) fn tick_mark(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 14.0), egui::Sense::hover());
    Icon::Check.draw(
        ui.painter(),
        egui::Rect::from_center_size(rect.center(), egui::vec2(11.0, 11.0)),
        ui.visuals().selection.stroke.color,
    );
}

pub(super) fn field(
    ui: &mut egui::Ui,
    id: egui::Id,
    add: impl FnOnce(&mut egui::Ui) -> egui::Response,
) -> egui::Response {
    let focused = ui.memory(|memory| memory.has_focus(id));
    let visuals = ui.visuals();
    let edge = if focused {
        visuals.selection.stroke.color.gamma_multiply(0.8)
    } else {
        visuals.widgets.noninteractive.bg_stroke.color
    };
    egui::Frame::new()
        .fill(visuals.extreme_bg_color)
        .stroke(egui::Stroke::new(1.0, edge))
        .corner_radius(ROUND)
        .inner_margin(egui::Margin::symmetric(8, 5))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

pub(super) fn foldable(
    ui: &mut egui::Ui,
    id: egui::Id,
    title: &str,
    open_by_default: bool,
    body: impl FnOnce(&mut egui::Ui),
) {
    let mut open = ui
        .data(|data| data.get_temp::<bool>(id))
        .unwrap_or(open_by_default);
    let header = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        disclosure(ui, open);
        ui.label(small(title));
    });
    let toggle = ui.interact(
        header.response.rect.expand2(egui::vec2(0.0, 2.0)),
        id.with("header"),
        egui::Sense::click(),
    );
    if toggle.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if toggle.clicked() {
        open = !open;
    }
    ui.data_mut(|data| data.insert_temp(id, open));
    if open {
        body(ui);
    }
}

pub(super) fn clipped(text: &str, most: usize) -> String {
    if text.chars().count() <= most {
        return text.to_owned();
    }
    let mut kept: String = text.chars().take(most.saturating_sub(1)).collect();
    kept.push('\u{2026}');
    kept
}

pub(super) fn like_a_field(ui: &mut egui::Ui) {
    let visuals = ui.visuals().clone();
    let widgets = &mut ui.visuals_mut().widgets;
    widgets.inactive.weak_bg_fill = visuals.extreme_bg_color;
    widgets.inactive.bg_stroke = visuals.widgets.noninteractive.bg_stroke;
    widgets.hovered.weak_bg_fill = visuals.extreme_bg_color;
    widgets.open.weak_bg_fill = visuals.extreme_bg_color;
}
