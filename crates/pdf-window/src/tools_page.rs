use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use eframe::egui;

use pdf_app::tools;
use pdf_app::wording::{Lang, Tools};
use pdf_convert::{Setting, Tool, Value, Values};

use crate::chooser::Chooser;
use crate::format::quiet_icon_button;
use crate::icons::Icon;
use crate::tools_marks;
use crate::tools_run::Working;
use crate::tools_screens::{self, Done, Failed, caption, note_box, primary_button, section};
use crate::tools_settings::{self, Draw};

pub(crate) const ACCENT: egui::Color32 = egui::Color32::from_rgb(37, 99, 235);
pub(crate) const ACCENT_HOVER: egui::Color32 = egui::Color32::from_rgb(29, 78, 216);

const CARD_HEIGHT: f32 = 56.0;
const LARGEST_PICTURE: u64 = 20 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Source {
    Document,
    File(PathBuf),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Held {
    pub(crate) source: Source,
    pub(crate) name: String,
    pub(crate) size: u64,
    pub(crate) pages: Option<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Wanted {
    Files { replacing: Option<usize> },
    SignaturePicture,
    SaveCopy,
}

pub(crate) struct Asking {
    pub(crate) chooser: Chooser,
    pub(crate) wanted: Wanted,
}

pub(crate) enum Stage {
    Settings,
    Running(Working),
    Done(Box<Done>),
    Failed(Box<Failed>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Act {
    Back,
    Run,
    Stop,
    Choose(Wanted),
    Remove(usize),
    Shift(usize, bool),
    StartOver,
    BackToSettings,
    TryAgain,
    OpenInPanPdf(PathBuf),
    OpenOutside(PathBuf),
    ShowInFolder(PathBuf),
    SaveCopy,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Ran {
    pub(crate) names: Vec<String>,
    pub(crate) original: PathBuf,
    pub(crate) base: PathBuf,
    pub(crate) from_bytes: Option<u64>,
    pub(crate) retried: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Facts {
    pub(crate) name: String,
    pub(crate) pages: usize,
    pub(crate) bytes: u64,
    pub(crate) unsaved: bool,
    pub(crate) page: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct Around {
    pub(crate) lang: Lang,
    pub(crate) document: Option<Facts>,
    pub(crate) busy: bool,
}

pub(crate) struct Page {
    pub(crate) tool: Tool,
    pub(crate) files: Vec<Held>,
    pub(crate) values: Values,
    pub(crate) drafts: BTreeMap<Setting, String>,
    pub(crate) repeat: String,
    pub(crate) pad: Vec<Vec<(f32, f32)>>,
    pub(crate) revealed: bool,
    pub(crate) stage: Stage,
    pub(crate) notices: Vec<String>,
    pub(crate) asking: Option<Asking>,
    pub(crate) ran: Option<Ran>,
}

impl Page {
    pub(crate) fn new(tool: Tool, around: &Around) -> Self {
        let mut files = Vec::new();
        if let Some(facts) = &around.document
            && tools::defaults_to_the_document(tool, true)
        {
            files.push(Held {
                source: Source::Document,
                name: facts.name.clone(),
                size: facts.bytes,
                pages: Some(facts.pages),
            });
        }
        Self {
            tool,
            files,
            values: Values::new(),
            drafts: BTreeMap::new(),
            repeat: String::new(),
            pad: Vec::new(),
            revealed: false,
            stage: Stage::Settings,
            notices: Vec::new(),
            asking: None,
            ran: None,
        }
    }

    pub(crate) fn names(&self) -> Vec<&str> {
        self.files.iter().map(|held| held.name.as_str()).collect()
    }

    pub(crate) fn add_files(&mut self, paths: &[PathBuf], replacing: Option<usize>, lang: Lang) {
        self.notices.clear();
        let names: Vec<String> = paths
            .iter()
            .map(|path| {
                path.file_name()
                    .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
            })
            .collect();
        let as_str: Vec<&str> = names.iter().map(String::as_str).collect();
        let several = tools::takes_several(self.tool);
        let room = if replacing.is_some() || !several {
            1
        } else {
            tools::room_for_more(self.tool, self.files.len())
        };
        let taken = tools::accept(self.tool, &as_str, room);
        let left_out: Vec<&str> = (0..names.len())
            .filter(|at| !taken.contains(at))
            .map(|at| names[at].as_str())
            .collect();
        if !left_out.is_empty() {
            self.notices
                .push(Tools::LeftOut(left_out.join(", ")).say(lang));
        }
        let mut fresh = Vec::new();
        for at in taken {
            let path = &paths[at];
            let Some(name) = names.get(at) else { continue };
            if self
                .files
                .iter()
                .any(|had| had.source == Source::File(path.clone()))
                || fresh
                    .iter()
                    .any(|had: &Held| had.source == Source::File(path.clone()))
            {
                continue;
            }
            match std::fs::metadata(path) {
                Ok(facts) if facts.is_file() => fresh.push(Held {
                    source: Source::File(path.clone()),
                    name: name.clone(),
                    size: facts.len(),
                    pages: None,
                }),
                Ok(_) => {}
                Err(error) => self.notices.push(
                    Tools::NotRead {
                        name: name.clone(),
                        why: error.to_string(),
                    }
                    .say(lang),
                ),
            }
        }
        if fresh.is_empty() {
            return;
        }
        match replacing {
            Some(slot) if slot < self.files.len() => {
                self.files[slot] = fresh.swap_remove(0);
            }
            _ if !several => {
                self.files.clear();
                self.files.push(fresh.swap_remove(0));
            }
            _ => self.files.extend(fresh),
        }
    }

    pub(crate) fn use_the_picture(&mut self, path: &Path, lang: Lang) {
        self.notices.clear();
        let name = path
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        let read = std::fs::metadata(path)
            .map_err(|error| error.to_string())
            .and_then(|facts| {
                if facts.len() > LARGEST_PICTURE {
                    Err("it is too large for a signature".to_owned())
                } else {
                    std::fs::read(path).map_err(|error| error.to_string())
                }
            });
        match read {
            Ok(bytes) => self
                .values
                .set(Setting::SignaturePicture, Value::File { name, bytes }),
            Err(why) => self.notices.push(Tools::NotRead { name, why }.say(lang)),
        }
    }

    pub(crate) fn remove(&mut self, at: usize) {
        if at < self.files.len() {
            self.files.remove(at);
        }
    }

    pub(crate) fn shift(&mut self, at: usize, earlier: bool) {
        let to = if earlier {
            at.checked_sub(1)
        } else {
            Some(at + 1)
        };
        if let Some(to) = to.filter(|to| *to < self.files.len() && at < self.files.len()) {
            self.files.swap(at, to);
        }
    }

    pub(crate) fn start_over(&mut self, around: &Around) {
        let fresh = Self::new(self.tool, around);
        *self = fresh;
    }

    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        around: &Around,
        hovering: bool,
    ) -> Option<Act> {
        let lang = around.lang;
        let tool = self.tool;
        match &mut self.stage {
            Stage::Settings => {}
            Stage::Running(working) => {
                let back = tools_screens::header(ui, tool, lang, false);
                return back.or_else(|| tools_screens::running(ui, working, tool, lang));
            }
            Stage::Done(done) => {
                let back = tools_screens::header(ui, tool, lang, true);
                return back.or_else(|| tools_screens::done(ui, done, lang));
            }
            Stage::Failed(failed) => {
                let back = tools_screens::header(ui, tool, lang, true);
                let password = tool.file_password();
                return back.or_else(|| tools_screens::failed(ui, failed, (tool, password), lang));
            }
        }
        let mut act = tools_screens::header(ui, tool, lang, true);
        let heading = if tools::takes_several(tool) || tool == Tool::Compare {
            Tools::FilesHeading
        } else {
            Tools::FileHeading
        };
        section(ui, &heading.say(lang));
        act = self.files_section(ui, around, hovering).or(act);
        for line in &self.notices {
            ui.add_space(6.0);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(line)
                        .size(12.0)
                        .color(ui.visuals().warn_fg_color),
                )
                .wrap(),
            );
        }
        act = self.settings_section(ui, around).or(act);
        if let Some(note) = Tools::Note(tool).said(lang) {
            ui.add_space(16.0);
            note_box(ui, &note);
        }
        ui.add_space(22.0);
        let missing = tools::ready(tool, &self.names(), &self.values, &self.repeat);
        let ready = missing.is_ok() && !around.busy;
        if primary_button(ui, &Tools::Go(tool).say(lang), ready, ui.available_width()) {
            act = Some(Act::Run);
        }
        ui.add_space(8.0);
        let said = match missing {
            Err(missing) => Tools::Missing(missing),
            Ok(()) => Tools::NothingUploaded,
        };
        caption(ui, &said.say(lang));
        act
    }

    fn settings_section(&mut self, ui: &mut egui::Ui, around: &Around) -> Option<Act> {
        let tool = self.tool;
        let (first, more) = tools::arranged(tool, &self.values);
        let mut wanted = None;
        let picture_name = match self.values.explicit(Setting::SignaturePicture) {
            Some(Value::File { name, .. }) => Some(name.clone()),
            _ => None,
        };
        let this_page = around
            .document
            .as_ref()
            .filter(|_| {
                self.files
                    .iter()
                    .all(|held| held.source == Source::Document)
                    && !self.files.is_empty()
            })
            .map(|facts| facts.page);
        let mut draw = Draw {
            values: &mut self.values,
            drafts: &mut self.drafts,
            pad: &mut self.pad,
            repeat: &mut self.repeat,
            revealed: &mut self.revealed,
            picture_name,
            lang: around.lang,
            this_page,
            wanted: &mut wanted,
        };
        if first.is_empty() && more.is_empty() {
            return None;
        }
        if first.is_empty() {
            ui.add_space(14.0);
        } else {
            section(ui, &Tools::SettingsHeading.say(around.lang));
        }
        if first.is_empty() {
            egui::CollapsingHeader::new(Tools::MoreSettings.say(around.lang))
                .id_salt(("more-settings", tool))
                .show(ui, |ui| {
                    for setting in more {
                        tools_settings::show(ui, &mut draw, setting);
                    }
                });
            return wanted.map(Act::Choose);
        }
        let visuals = ui.visuals();
        egui::Frame::new()
            .fill(visuals.panel_fill)
            .stroke(visuals.widgets.noninteractive.bg_stroke)
            .corner_radius(8.0)
            .inner_margin(egui::Margin::symmetric(16, 10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                for setting in first {
                    tools_settings::show(ui, &mut draw, setting);
                }
                if !more.is_empty() {
                    ui.add_space(8.0);
                    egui::CollapsingHeader::new(Tools::MoreSettings.say(around.lang))
                        .id_salt(("more-settings", tool))
                        .show(ui, |ui| {
                            for setting in more {
                                tools_settings::show(ui, &mut draw, setting);
                            }
                        });
                    ui.add_space(2.0);
                }
            });
        wanted.map(Act::Choose)
    }

    fn slot(&self, ui: &mut egui::Ui, around: &Around, hovering: bool, place: usize) -> Pressed {
        card(
            ui,
            &CardOf {
                place,
                held: self.files.get(place),
                around,
                tool: self.tool,
                hovering,
                row: None,
            },
        )
    }

    fn files_section(&mut self, ui: &mut egui::Ui, around: &Around, hovering: bool) -> Option<Act> {
        let lang = around.lang;
        let tool = self.tool;
        let mut act = None;
        if tool == Tool::Compare {
            for slot in 0..2 {
                let heading = if slot == 0 {
                    Tools::Older
                } else {
                    Tools::Newer
                };
                caption(ui, &heading.say(lang));
                ui.add_space(2.0);
                if self.slot(ui, around, hovering, slot) == Pressed::Change {
                    let replacing = (slot < self.files.len()).then_some(slot);
                    act = Some(Act::Choose(Wanted::Files { replacing }));
                }
                ui.add_space(6.0);
            }
            return act;
        }
        let several = tools::takes_several(tool) && !self.files.is_empty();
        if !several {
            if self.slot(ui, around, hovering, 0) == Pressed::Change {
                let replacing = (!self.files.is_empty()).then_some(0);
                act = Some(Act::Choose(Wanted::Files { replacing }));
            }
            return act;
        }
        let count = self.files.len();
        for (at, held) in self.files.iter().enumerate() {
            let row = Row {
                shift: tools::order_matters(tool).then_some((at > 0, at + 1 < count)),
            };
            let of = CardOf {
                place: at,
                held: Some(held),
                around,
                tool,
                hovering,
                row: Some(row),
            };
            match card(ui, &of) {
                Pressed::Remove => act = Some(Act::Remove(at)),
                Pressed::Earlier => act = Some(Act::Shift(at, true)),
                Pressed::Later => act = Some(Act::Shift(at, false)),
                Pressed::Change | Pressed::Nothing => {}
            }
            ui.add_space(6.0);
        }
        ui.horizontal(|ui| {
            let add = egui::Button::new(Tools::AddFiles.say(lang)).min_size(egui::vec2(0.0, 30.0));
            if ui.add(add).clicked() {
                act = Some(Act::Choose(Wanted::Files { replacing: None }));
            }
            ui.add_space(8.0);
            caption(ui, &Tools::DropHint.say(lang));
        });
        act
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Pressed {
    Nothing,
    Change,
    Remove,
    Earlier,
    Later,
}

struct CardOf<'a> {
    place: usize,
    held: Option<&'a Held>,
    around: &'a Around,
    tool: Tool,
    hovering: bool,
    row: Option<Row>,
}

struct Row {
    shift: Option<(bool, bool)>,
}

fn fitted(
    painter: &egui::Painter,
    text: &str,
    font: egui::FontId,
    colour: egui::Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, colour);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(20.0));
    painter.layout_job(job)
}

fn card(ui: &mut egui::Ui, of: &CardOf<'_>) -> Pressed {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), CARD_HEIGHT),
        egui::Sense::hover(),
    );
    let visuals = ui.visuals();
    let stroke = if of.hovering {
        egui::Stroke::new(1.5, visuals.selection.stroke.color)
    } else {
        visuals.widgets.noninteractive.bg_stroke
    };
    let painter = ui.painter().clone();
    painter.rect(
        rect,
        8.0,
        visuals.panel_fill,
        stroke,
        egui::StrokeKind::Inside,
    );
    card_face(ui, rect, of);
    card_buttons(ui, rect, of)
}

fn card_face(ui: &egui::Ui, rect: egui::Rect, of: &CardOf<'_>) {
    let lang = of.around.lang;
    let visuals = ui.visuals();
    let text = visuals.text_color();
    let faint = visuals.weak_text_color();
    let painter = ui.painter();
    let buttons_width = match &of.row {
        Some(Row { shift: Some(_) }) => 100.0,
        Some(Row { shift: None }) => 44.0,
        None => 104.0,
    };
    let room = rect.width() - 62.0 - buttons_width;
    let left = rect.left() + 56.0;
    let mark_at = egui::pos2(rect.left() + 14.0, rect.center().y - 15.0);
    let Some(held) = of.held else {
        let kind = of.tool.accepts()[0];
        Icon::Document.draw(
            painter,
            egui::Rect::from_min_size(mark_at, egui::vec2(30.0, 30.0)),
            faint,
        );
        painter.text(
            egui::pos2(left, rect.center().y - 8.0),
            egui::Align2::LEFT_CENTER,
            Tools::Choose(kind).say(lang),
            egui::FontId::proportional(14.0),
            text,
        );
        painter.text(
            egui::pos2(left, rect.center().y + 10.0),
            egui::Align2::LEFT_CENTER,
            Tools::DropHint.say(lang),
            egui::FontId::proportional(12.0),
            faint,
        );
        return;
    };
    tools_marks::mark_for_file(painter, mark_at, 30.0, &held.name);
    let galley = fitted(
        painter,
        &held.name,
        egui::FontId::proportional(14.0),
        text,
        room,
    );
    painter.galley(egui::pos2(left, rect.center().y - 17.0), galley, text);
    let unsaved = held.source == Source::Document
        && of
            .around
            .document
            .as_ref()
            .is_some_and(|facts| facts.unsaved);
    let detail = Tools::Detail {
        pages: held.pages,
        bytes: held.size,
        unsaved,
    }
    .say(lang);
    let galley = fitted(
        painter,
        &detail,
        egui::FontId::proportional(12.0),
        faint,
        room,
    );
    painter.galley(egui::pos2(left, rect.center().y + 3.0), galley, faint);
}

fn card_buttons(ui: &mut egui::Ui, rect: egui::Rect, of: &CardOf<'_>) -> Pressed {
    let lang = of.around.lang;
    let middle = rect.center().y;
    let at = |back: f32| {
        egui::Rect::from_center_size(
            egui::pos2(rect.right() - back, middle),
            egui::vec2(24.0, 24.0),
        )
    };
    let mut pressed = Pressed::Nothing;
    if let (Some(_), Some(row)) = (of.held, &of.row) {
        if let Some((can_earlier, can_later)) = row.shift {
            if chevron(ui, at(70.0), (of.place, true), can_earlier) {
                pressed = Pressed::Earlier;
            }
            if chevron(ui, at(42.0), (of.place, false), can_later) {
                pressed = Pressed::Later;
            }
        }
        let hover = Tools::RemoveFile.say(lang);
        if inside(ui, at(14.0), |ui| {
            quiet_icon_button(ui, Icon::Close, &hover)
        }) {
            pressed = Pressed::Remove;
        }
        return pressed;
    }
    let words = if of.held.is_some() {
        Tools::ChangeFile
    } else {
        Tools::ChooseFile
    };
    let button_rect = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 12.0 - 40.0, middle),
        egui::vec2(80.0, 30.0),
    );
    let button = egui::Button::new(words.say(lang));
    if inside(ui, button_rect, |ui| {
        ui.add_sized(button_rect.size(), button)
    }) {
        pressed = Pressed::Change;
    }
    pressed
}

fn inside(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    add: impl FnOnce(&mut egui::Ui) -> egui::Response,
) -> bool {
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    add(&mut child).clicked()
}

fn chevron(ui: &mut egui::Ui, rect: egui::Rect, (place, up): (usize, bool), enabled: bool) -> bool {
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let response = ui.interact(rect, ui.id().with(("shift-file", place, up)), sense);
    let visuals = ui.visuals();
    if enabled && response.hovered() {
        ui.painter()
            .rect_filled(rect, 4.0, visuals.widgets.hovered.weak_bg_fill);
    }
    let colour = if enabled {
        visuals.text_color()
    } else {
        visuals.weak_text_color().gamma_multiply(0.5)
    };
    let c = rect.center();
    let (near, far) = if up { (2.0, -2.0) } else { (-2.0, 2.0) };
    ui.painter().add(egui::Shape::line(
        vec![
            c + egui::vec2(-4.5, near),
            c + egui::vec2(0.0, far),
            c + egui::vec2(4.5, near),
        ],
        egui::Stroke::new(1.5, colour),
    ));
    enabled && response.clicked()
}

#[cfg(test)]
mod tests;
