use std::time::{SystemTime, UNIX_EPOCH};

use eframe::egui;

use pdf_app::wording::{Command, Lang, Message, Refusal, StampWhy};
use pdf_edit::stamp::{Edge, Facts, Only, Side, Spot, Stamp};

use crate::canvas::box_on_screen;
use crate::dialog;
use crate::window_state::{Laid, StampDraft, Window};

const PANEL_WIDTH: f32 = 340.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StampKind {
    PageNumbers,
    HeaderFooter,
    Watermark,
}

const WATERMARK_GREY: [f32; 3] = [0.5, 0.5, 0.5];
const WATERMARK_OPACITY: u8 = 50;

const TOKENS: [(&str, Message); 4] = [
    ("{page}", Message::StampTokenPage),
    ("{pages}", Message::StampTokenPages),
    ("{file}", Message::StampTokenFile),
    ("{date}", Message::FieldDate),
];

pub(crate) fn draft_for(kind: StampKind) -> StampDraft {
    let (wording, spot, size, (colour, opacity)) = match kind {
        StampKind::PageNumbers => (
            "{page}",
            Spot::Along {
                edge: Edge::Footer,
                side: Side::Centre,
            },
            10.0,
            ([0.0; 3], 100),
        ),
        StampKind::HeaderFooter => (
            "{file}",
            Spot::Along {
                edge: Edge::Header,
                side: Side::Left,
            },
            9.0,
            ([0.0; 3], 100),
        ),
        StampKind::Watermark => (
            "DRAFT",
            Spot::Middle,
            60.0,
            (WATERMARK_GREY, WATERMARK_OPACITY),
        ),
    };
    StampDraft {
        door: match kind {
            StampKind::PageNumbers => Command::StampPageNumbers,
            StampKind::HeaderFooter => Command::StampHeaderFooter,
            StampKind::Watermark => Command::StampWatermark,
        },
        wording: wording.to_owned(),
        spot,
        family: crate::input::NEW_TEXT_FAMILY.to_owned(),
        size,
        bold: kind == StampKind::Watermark,
        colour,
        opacity,
        margin: 36.0,
        range: String::new(),
        only: Only::Every,
        start: 1,
        seen: std::collections::BTreeMap::new(),
    }
}

pub(crate) fn stamp_of(draft: &StampDraft) -> Stamp {
    Stamp {
        wording: draft.wording.clone(),
        spot: draft.spot,
        family: draft.family.clone(),
        size: draft.size,
        bold: draft.bold,
        italic: false,
        fill: Some(draft.colour.map(f64::from)),
        opacity: f64::from(draft.opacity.clamp(1, 100)) / 100.0,
        margin: draft.margin,
    }
}

pub(crate) fn facts_of(
    draft: &StampDraft,
    (page, first): (usize, usize),
    (count, name, today): (usize, &str, &str),
) -> Facts {
    let further = i64::try_from(page.saturating_sub(first)).unwrap_or(i64::MAX);
    Facts {
        number: draft.start.saturating_add(further),
        count: i64::try_from(count).unwrap_or(i64::MAX),
        name: name.to_owned(),
        today: today.to_owned(),
    }
}

fn today() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    pdf_app::dates::written_day(seconds)
}

fn range_refused(error: &pdf_edit::spike_move_text::SpikeError) -> Message {
    match error {
        pdf_edit::spike_move_text::SpikeError::RetypeUnsupported(reason) => StampWhy::of(reason)
            .map_or_else(
                || Message::Refused(error.to_string().into()),
                |why| Message::Refused(Refusal::Stamp(why)),
            ),
        _ => Message::Refused(error.to_string().into()),
    }
}

impl Window {
    pub(crate) fn open_the_stamp_panel(&mut self, kind: StampKind) {
        self.stamp_draft = Some(draft_for(kind));
    }

    fn document_name(&self) -> String {
        self.opened
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    fn stamped_pages(&self, draft: &StampDraft) -> Result<Vec<usize>, Message> {
        pdf_edit::stamp::pages_of(&draft.range, self.editor.page_count(), draft.only)
            .map_err(|error| range_refused(&error))
    }

    pub(crate) fn stamp_panel(&mut self, ctx: &egui::Context) {
        if self.stamp_draft.is_none() || !self.has_document() {
            return;
        }
        self.see_the_stamp();
        let lang = self.lang;
        let busy = self.editor.is_busy();
        let canvas = self.canvas;
        let pages = self
            .stamp_draft
            .as_ref()
            .map(|draft| self.stamped_pages(draft));
        let shown: Option<(usize, Result<String, Message>)> =
            self.stamp_draft.as_ref().and_then(|draft| {
                let page = match pages.as_ref()?.as_ref() {
                    Ok(pages) if pages.contains(&self.focus) => self.focus,
                    Ok(pages) => *pages.first()?,
                    Err(_) => return None,
                };
                let said = draft
                    .seen
                    .get(&page)
                    .map_or(Err(Message::StampPreviewNotShown { page }), |(_, seen)| {
                        seen.clone().map(|(line, _)| line)
                    });
                Some((page, said))
            });
        let Some(draft) = self.stamp_draft.as_mut() else {
            return;
        };
        let mut apply = false;
        let mut close = false;
        let title = Message::Command(draft.door).say(lang);
        let why = Message::StampWhy.say(lang);
        let shut = Message::Close.say(lang);
        let spec = dialog::Spec {
            id: "stamp-panel",
            width: PANEL_WIDTH,
        };
        dialog::panel(ctx, canvas, &spec, |ui, room| {
            close = dialog::header(
                ui,
                title.trim_end_matches('\u{2026}'),
                Some(&why),
                Some(&shut),
            );
            dialog::scrolling(ui, "stamp-body", room, |ui| {
                wording_box(ui, draft, lang);
                dialog::divide(ui);
                where_it_goes(ui, draft, lang);
                dialog::divide(ui);
                how_it_looks(ui, draft, lang);
                dialog::divide(ui);
                which_pages(ui, draft, lang);
            });
            match &shown {
                Some((page, Ok(line))) => {
                    ui.add_space(8.0);
                    let said = Message::StampPreview {
                        page: *page,
                        line: line.clone(),
                    };
                    dialog::note(ui, dialog::Tone::Calm, &said.say(lang));
                }
                Some((_, Err(why))) => {
                    ui.add_space(8.0);
                    dialog::note(ui, dialog::Tone::Warning, &why.say(lang));
                }
                None => {}
            }
            if let Some(Err(why)) = &pages {
                ui.add_space(8.0);
                dialog::note(ui, dialog::Tone::Trouble, &why.say(lang));
            }
            let count = pages
                .as_ref()
                .and_then(|pages| pages.as_ref().ok())
                .map_or(0, Vec::len);
            let refused = matches!(shown, Some((_, Err(Message::Refused(_)))));
            dialog::footer(ui, |ui| {
                let ready = count > 0 && !busy && !refused;
                apply = dialog::primary(ui, &Message::StampApply(count).say(lang), ready).clicked();
                if dialog::secondary(ui, &Message::Close.say(lang)).clicked() {
                    close = true;
                }
            });
        });
        if close {
            self.stamp_draft = None;
            return;
        }
        if apply {
            self.stamp_the_range();
        }
    }

    fn see_the_stamp(&mut self) {
        let Some(draft) = self.stamp_draft.as_ref() else {
            return;
        };
        let Ok(pages) = self.stamped_pages(draft) else {
            return;
        };
        let stamp = stamp_of(draft);
        let (name, today, count) = (self.document_name(), today(), self.editor.page_count());
        let first = pages.first().copied().unwrap_or(0);
        let mut worked = Vec::new();
        for laid in &self.laid {
            if pages.binary_search(&laid.page).is_err() {
                continue;
            }
            let facts = facts_of(draft, (laid.page, first), (count, &name, &today));
            let key = format!("{stamp:?} {facts:?} {}", self.editor.epoch());
            if draft
                .seen
                .get(&laid.page)
                .is_some_and(|(held, _)| *held == key)
            {
                continue;
            }
            let seen = self.editor.stamp_landing(laid.page, &stamp, &facts);
            if matches!(seen, Err(Message::StampPreviewNotShown { .. })) {
                continue;
            }
            worked.push((
                laid.page,
                key,
                seen.map(|(landing, pixels)| (landing.line, pixels)),
            ));
        }
        if let Some(draft) = self.stamp_draft.as_mut() {
            draft
                .seen
                .retain(|page, _| pages.binary_search(page).is_ok());
            for (page, key, seen) in worked {
                draft.seen.insert(page, (key, seen));
            }
        }
    }

    pub(crate) fn stamp_on_the_page(&self, glass: &egui::Painter, laid: Laid) {
        let Some(draft) = self.stamp_draft.as_ref() else {
            return;
        };
        let Some((_, Ok((line, Some(pixels))))) = draft.seen.get(&laid.page) else {
            return;
        };
        let area = box_on_screen(laid.placed, *pixels);
        let blue = egui::Color32::from_rgb(0, 90, 200);
        glass.rect_filled(
            area.expand(2.0),
            2.0,
            egui::Color32::from_rgba_unmultiplied(0, 90, 200, 28),
        );
        glass.rect_stroke(
            area.expand(2.0),
            2.0,
            egui::Stroke::new(1.0, blue),
            egui::StrokeKind::Outside,
        );
        let [red, green, blue_part] = draft.colour.map(|part| {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let byte = (part.clamp(0.0, 1.0) * 255.0).round() as u8;
            byte
        });
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let alpha = (f32::from(draft.opacity.clamp(1, 100)) * 2.55).round() as u8;
        glass.text(
            egui::pos2(area.left(), area.bottom()),
            egui::Align2::LEFT_BOTTOM,
            line,
            egui::FontId::proportional(area.height().max(4.0)),
            egui::Color32::from_rgba_unmultiplied(red, green, blue_part, alpha),
        );
    }

    fn stamp_the_range(&mut self) {
        let Some(draft) = self.stamp_draft.as_ref() else {
            return;
        };
        let pages = match self.stamped_pages(draft) {
            Ok(pages) => pages,
            Err(why) => {
                self.editor.say(why);
                return;
            }
        };
        let stamp = stamp_of(draft);
        let start = draft.start;
        let job = self
            .editor
            .begin_stamp(pages, stamp, (start, self.document_name(), today()));
        if job.is_none() {
            self.editor.say(Message::AnotherEditIsRunning);
            return;
        }
        self.stamp_draft = None;
        self.send(job);
    }
}

fn wording_box(ui: &mut egui::Ui, draft: &mut StampDraft, lang: Lang) {
    dialog::caption(ui, &Message::FieldText.say(lang));
    ui.add(egui::TextEdit::singleline(&mut draft.wording).desired_width(f32::INFINITY));
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 6.0);
        dialog::small(ui, &Message::StampInsert.say(lang));
        for (token, word) in &TOKENS {
            if ui
                .add(
                    egui::Button::new(egui::RichText::new(word.say(lang)).size(11.5))
                        .corner_radius(11.0)
                        .small()
                        .min_size(egui::vec2(0.0, 22.0)),
                )
                .clicked()
            {
                draft.wording.push_str(token);
            }
        }
    });
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Place {
    Header,
    Footer,
    Middle,
}

fn place_of(spot: Spot) -> Place {
    match spot {
        Spot::Along {
            edge: Edge::Header, ..
        } => Place::Header,
        Spot::Along {
            edge: Edge::Footer, ..
        } => Place::Footer,
        Spot::Middle => Place::Middle,
    }
}

fn spot_for(place: Place, side: Side) -> Spot {
    match place {
        Place::Header => Spot::Along {
            edge: Edge::Header,
            side,
        },
        Place::Footer => Spot::Along {
            edge: Edge::Footer,
            side,
        },
        Place::Middle => Spot::Middle,
    }
}

fn side_of(spot: Spot) -> Side {
    match spot {
        Spot::Along { side, .. } => side,
        Spot::Middle => Side::Centre,
    }
}

fn where_it_goes(ui: &mut egui::Ui, draft: &mut StampDraft, lang: Lang) {
    dialog::caption(ui, &Message::StampWhere.say(lang));
    let mut place = place_of(draft.spot);
    let places = [
        (Place::Header, Message::StampHeader.say(lang)),
        (Place::Footer, Message::StampFooter.say(lang)),
        (Place::Middle, Message::StampMiddle.say(lang)),
    ];
    let mut side = side_of(draft.spot);
    let moved = dialog::segments(ui, "stamp-place", &mut place, &places);
    if place != Place::Middle {
        ui.add_space(6.0);
        let sides = [
            (Side::Left, Message::AlignLeft.say(lang)),
            (Side::Centre, Message::AlignCentre.say(lang)),
            (Side::Right, Message::AlignRight.say(lang)),
        ];
        dialog::segments(ui, "stamp-side", &mut side, &sides);
    }
    if moved || side != side_of(draft.spot) {
        draft.spot = spot_for(place, side);
    }
}

fn how_it_looks(ui: &mut egui::Ui, draft: &mut StampDraft, lang: Lang) {
    dialog::caption(ui, &Message::StampLook.say(lang));
    dialog::labelled(ui, &Message::Font.say(lang), |ui| {
        egui::ComboBox::from_id_salt("stamp-font")
            .selected_text(draft.family.clone())
            .width(ui.available_width() - 8.0)
            .show_ui(ui, |ui| {
                for family in pdf_cli::font_families() {
                    ui.selectable_value(&mut draft.family, family.clone(), family);
                }
            });
    });
    dialog::labelled(ui, &Message::Size.say(lang), |ui| {
        ui.add(
            egui::DragValue::new(&mut draft.size)
                .range(4.0..=200.0)
                .speed(0.5)
                .suffix(" pt"),
        );
        ui.add_space(10.0);
        ui.checkbox(&mut draft.bold, Message::Bold.say(lang));
    });
    dialog::labelled(ui, &Message::StampColour.say(lang), |ui| {
        ui.color_edit_button_rgb(&mut draft.colour);
    });
    dialog::labelled(ui, &Message::StampOpacity.say(lang), |ui| {
        ui.spacing_mut().slider_width = ui.available_width() - 64.0;
        ui.add(egui::Slider::new(&mut draft.opacity, 1..=100).suffix(" %"));
    });
    dialog::labelled(ui, &Message::StampMargin.say(lang), |ui| {
        ui.add(
            egui::DragValue::new(&mut draft.margin)
                .range(0.0..=288.0)
                .speed(0.5),
        );
    });
}

fn which_pages(ui: &mut egui::Ui, draft: &mut StampDraft, lang: Lang) {
    dialog::caption(ui, &Message::StampPages.say(lang));
    ui.add(
        egui::TextEdit::singleline(&mut draft.range)
            .hint_text(Message::StampPagesHint.say(lang))
            .desired_width(f32::INFINITY),
    );
    ui.add_space(6.0);
    let options =
        [Only::Every, Only::Odd, Only::Even].map(|only| (only, Message::StampOnly(only).say(lang)));
    dialog::segments(ui, "stamp-only", &mut draft.only, &options);
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(Message::StampStart.say(lang))
                .size(13.0)
                .color(dialog::weak(ui)),
        );
        ui.add_space(6.0);
        ui.add(egui::DragValue::new(&mut draft.start).range(-9999..=99_999));
    });
}

#[cfg(test)]
mod tests {
    use pdf_edit::stamp::{Edge, Only, Side, Spot};

    use super::{StampKind, draft_for, facts_of, stamp_of};

    #[test]
    fn each_door_opens_where_its_job_starts() {
        let numbers = draft_for(StampKind::PageNumbers);
        assert_eq!(numbers.wording, "{page}");
        assert_eq!(
            numbers.spot,
            Spot::Along {
                edge: Edge::Footer,
                side: Side::Centre
            }
        );
        assert_eq!(stamp_of(&numbers).fill, Some([0.0; 3]));
        let header = draft_for(StampKind::HeaderFooter);
        assert_eq!(
            header.spot,
            Spot::Along {
                edge: Edge::Header,
                side: Side::Left
            }
        );
        let watermark = draft_for(StampKind::Watermark);
        assert_eq!(watermark.spot, Spot::Middle);
        assert_eq!(stamp_of(&watermark).fill, Some([0.5, 0.5, 0.5]));
        assert!(
            (stamp_of(&watermark).opacity - 0.5).abs() < f64::EPSILON,
            "a watermark lets the page show through"
        );
        assert!((stamp_of(&numbers).opacity - 1.0).abs() < f64::EPSILON);
        assert_eq!(numbers.only, Only::Every);
        assert!(numbers.range.is_empty(), "every page, until a person says");
    }

    #[test]
    fn a_page_counts_on_from_the_first_page_of_the_range() {
        let mut draft = draft_for(StampKind::PageNumbers);
        draft.start = 1;
        let facts = facts_of(&draft, (4, 2), (12, "book.pdf", "18/09/2026"));
        assert_eq!(facts.number, 3);
        assert_eq!(facts.count, 12);
        assert_eq!(facts.name, "book.pdf");
        draft.start = 10;
        assert_eq!(
            facts_of(&draft, (2, 2), (12, "", "")).number,
            10,
            "the first page shows the start"
        );
    }
}
