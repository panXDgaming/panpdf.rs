use std::path::{Path, PathBuf};
use std::time::Duration;

use eframe::egui;

use pdf_app::tools;
use pdf_app::wording::{Home, Lang, Message, Tools};
use pdf_convert::run::Failure;
use pdf_convert::{Group, Setting, Tool, Value};

use crate::app::name_of;
use crate::chooser::{Chooser, Chose, Offer};
use crate::tools_marks;
use crate::tools_page::{Act, Around, Asking, Facts, Page, Source, Stage, Wanted};
use crate::tools_run::{self, Order, Origin, Poll};
use crate::tools_screens::{Done, Failed, caption, weak};
use crate::window_state::{Window, desk};

const COLUMN: f32 = 760.0;
const TILE_HEIGHT: f32 = 78.0;
const GAP: f32 = 12.0;

pub(crate) enum Screen {
    List,
    Tool(Box<Page>),
}

pub(crate) struct Room {
    pub(crate) screen: Screen,
    query: String,
    chip: Option<Group>,
    focus_search: bool,
}

pub(crate) enum RoomAct {
    Leave,
    OpenTool(Tool),
    Page(Act),
}

impl Room {
    pub(crate) fn new() -> Self {
        Self {
            screen: Screen::List,
            query: String::new(),
            chip: None,
            focus_search: true,
        }
    }

    fn show(
        &mut self,
        ui: &mut egui::Ui,
        around: &Around,
        (hovering, behind): (bool, bool),
    ) -> Option<RoomAct> {
        match &mut self.screen {
            Screen::Tool(page) => page.show(ui, around, hovering).map(RoomAct::Page),
            Screen::List => self.list(ui, around.lang, behind),
        }
    }

    fn list(&mut self, ui: &mut egui::Ui, lang: Lang, behind: bool) -> Option<RoomAct> {
        let say = |sentence: Tools| sentence.say(lang);
        let mut act = None;
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(say(Tools::RoomTitle))
                    .size(26.0)
                    .strong(),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(say(Tools::ToolCount(Tool::ALL.len())))
                    .size(13.0)
                    .color(weak(ui)),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let words = if behind {
                    Message::Home(Home::BackToDocument).say(lang)
                } else {
                    say(Tools::BackToStart)
                };
                if ui.button(format!("\u{2190}  {words}")).clicked() {
                    act = Some(RoomAct::Leave);
                }
            });
        });
        ui.add_space(2.0);
        caption(ui, &say(Tools::RoomPromise));
        ui.add_space(16.0);
        let entered = self.search_row(ui, lang);
        let found = tools::found(&self.query, self.chip, lang);
        if entered && let [only] = found.as_slice() {
            return Some(RoomAct::OpenTool(*only));
        }
        if found.is_empty() {
            ui.add_space(24.0);
            ui.label(egui::RichText::new(say(Tools::NothingMatches)).color(weak(ui)));
            return act;
        }
        for group in Group::ALL {
            let these: Vec<Tool> = found
                .iter()
                .copied()
                .filter(|tool| tool.group() == group)
                .collect();
            if these.is_empty() {
                continue;
            }
            ui.add_space(22.0);
            ui.label(
                egui::RichText::new(say(Tools::GroupName(group)))
                    .size(13.0)
                    .strong()
                    .color(weak(ui)),
            );
            ui.add_space(8.0);
            if let Some(tool) = tiles(ui, lang, &these) {
                act = Some(RoomAct::OpenTool(tool));
            }
        }
        act
    }

    fn search_row(&mut self, ui: &mut egui::Ui, lang: Lang) -> bool {
        let mut entered = false;
        ui.horizontal_wrapped(|ui| {
            let field = egui::TextEdit::singleline(&mut self.query)
                .hint_text(Tools::SearchHint.say(lang))
                .desired_width(210.0)
                .margin(egui::Margin::symmetric(8, 5));
            let response = ui.add(field);
            if self.focus_search {
                response.request_focus();
                self.focus_search = false;
            }
            entered =
                response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            ui.add_space(10.0);
            let chips = std::iter::once((None, Tools::AllGroups)).chain(
                Group::ALL
                    .into_iter()
                    .map(|group| (Some(group), Tools::GroupChip(group))),
            );
            for (group, words) in chips {
                let chip = egui::Button::selectable(self.chip == group, words.say(lang))
                    .corner_radius(14.0)
                    .min_size(egui::vec2(40.0, 26.0));
                if ui.add(chip).clicked() {
                    self.chip = group;
                }
            }
        });
        entered
    }
}

fn tiles(ui: &mut egui::Ui, lang: Lang, these: &[Tool]) -> Option<Tool> {
    let across = ui.available_width();
    let columns = if across >= 620.0 {
        3
    } else if across >= 400.0 {
        2
    } else {
        1
    };
    #[expect(clippy::cast_precision_loss, reason = "one, two or three columns")]
    let wide = (across - GAP * (columns - 1) as f32) / columns as f32;
    let mut opened = None;
    for row in these.chunks(columns) {
        let (area, _) =
            ui.allocate_exact_size(egui::vec2(across, TILE_HEIGHT), egui::Sense::hover());
        for (at, tool) in row.iter().enumerate() {
            #[expect(clippy::cast_precision_loss, reason = "at most three tiles in a row")]
            let left = area.left() + at as f32 * (wide + GAP);
            let tile = egui::Rect::from_min_size(
                egui::pos2(left, area.top()),
                egui::vec2(wide, TILE_HEIGHT),
            );
            if tile_of(ui, tile, *tool, lang) {
                opened = Some(*tool);
            }
        }
        ui.add_space(GAP - ui.spacing().item_spacing.y);
    }
    opened
}

fn tile_of(ui: &mut egui::Ui, tile: egui::Rect, tool: Tool, lang: Lang) -> bool {
    let response = ui.interact(
        tile,
        ui.id().with(("tool-tile", tool)),
        egui::Sense::click(),
    );
    if ui.is_rect_visible(tile) {
        let visuals = ui.visuals();
        let hovered = response.hovered();
        let fill = if hovered {
            visuals.widgets.hovered.weak_bg_fill
        } else {
            visuals.panel_fill
        };
        let border = if hovered {
            visuals.selection.stroke.color
        } else {
            visuals.widgets.noninteractive.bg_stroke.color
        };
        let text = visuals.text_color();
        let faint = visuals.weak_text_color();
        let painter = ui.painter().with_clip_rect(tile.intersect(ui.clip_rect()));
        painter.rect(
            tile,
            8.0,
            fill,
            egui::Stroke::new(1.0, border),
            egui::StrokeKind::Inside,
        );
        tools_marks::mark(
            &painter,
            egui::pos2(tile.left() + 14.0, tile.center().y - 17.0),
            34.0,
            tool,
        );
        let left = tile.left() + 56.0;
        let room = tile.width() - 56.0 - 12.0;
        let name = single_line(&painter, &Tools::Name(tool).say(lang), 15.0, text, room);
        painter.galley(egui::pos2(left, tile.top() + 11.0), name, text);
        let mut job = egui::text::LayoutJob::simple(
            Tools::Blurb(tool).say(lang),
            egui::FontId::proportional(12.0),
            faint,
            room,
        );
        job.wrap.max_rows = 2;
        job.wrap.overflow_character = Some('\u{2026}');
        let blurb = painter.layout_job(job);
        painter.galley(egui::pos2(left, tile.top() + 33.0), blurb, faint);
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response.clicked()
}

fn single_line(
    painter: &egui::Painter,
    text: &str,
    size: f32,
    colour: egui::Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(
        text.to_owned(),
        egui::FontId::proportional(size),
        colour,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(20.0));
    painter.layout_job(job)
}

impl Window {
    pub(crate) fn open_the_tools(&mut self, tool: Option<Tool>) {
        if tool.is_some_and(tools::opens_the_text_panel) {
            self.read_text_with_the_panel();
            return;
        }
        if self.tools.as_ref().is_some_and(|room| {
            matches!(&room.screen, Screen::Tool(page) if matches!(page.stage, Stage::Running(_)))
        }) {
            return;
        }
        let around = self.tools_around();
        let mut room = self.tools.take().unwrap_or_else(Room::new);
        room.screen = match tool {
            Some(tool) => Screen::Tool(Box::new(Page::new(tool, &around))),
            None => Screen::List,
        };
        self.tools = Some(room);
    }

    fn read_text_with_the_panel(&mut self) {
        self.tools = None;
        if self.has_document() {
            self.home = false;
            self.open_the_ocr_panel();
        } else {
            self.read_text_after_opening = true;
            self.asking_to_open = true;
        }
    }

    pub(crate) fn tools_around(&self) -> Around {
        let document = self.has_document().then(|| Facts {
            name: self.document_file_name(),
            pages: self.editor.page_count(),
            bytes: self
                .editor
                .source()
                .map_or(0, |source| u64::try_from(source.len()).unwrap_or(u64::MAX)),
            unsaved: self.unsaved(),
            page: self.focus,
        });
        Around {
            lang: self.lang,
            document,
            busy: self.editor.is_busy() || self.loading.is_some(),
        }
    }

    fn document_file_name(&self) -> String {
        if self.untitled && self.destination.as_os_str().is_empty() {
            format!("{}.pdf", Home::Untitled.say(self.lang))
        } else if self.untitled {
            name_of(&self.destination)
        } else {
            name_of(&self.opened)
        }
    }

    pub(crate) fn tools_screen(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.keep_the_tool_going(&ctx);
        self.status_bar(ui);
        let around = self.tools_around();
        let hovering = ctx.input(|input| !input.raw.hovered_files.is_empty());
        let behind = self.has_document() && !self.home;
        let dark = self.dark;
        let Some(mut room) = self.tools.take() else {
            return;
        };
        let mut act = None;
        let ground = egui::Frame::NONE.fill(desk(dark));
        egui::CentralPanel::no_frame().frame(ground).show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let across = ui.available_width();
                    let side = ((across - COLUMN) / 2.0).max(16.0);
                    let wide = (across - side - 16.0).clamp(200.0, COLUMN - 32.0);
                    ui.add_space(36.0);
                    ui.horizontal(|ui| {
                        ui.add_space(side);
                        ui.vertical(|ui| {
                            ui.set_width(wide);
                            act = room.show(ui, &around, (hovering, behind));
                        });
                    });
                    ui.add_space(36.0);
                });
        });
        self.tools = Some(room);
        if let Some(act) = act {
            self.tools_act(&ctx, act);
        }
        self.the_tools_chooser(&ctx);
    }

    fn tools_act(&mut self, ctx: &egui::Context, act: RoomAct) {
        match act {
            RoomAct::Leave => self.tools = None,
            RoomAct::OpenTool(tool) => self.open_the_tools(Some(tool)),
            RoomAct::Page(act) => self.page_act(ctx, act),
        }
    }

    fn the_page(&mut self) -> Option<&mut Page> {
        match &mut self.tools.as_mut()?.screen {
            Screen::Tool(page) => Some(page),
            Screen::List => None,
        }
    }

    fn page_act(&mut self, ctx: &egui::Context, act: Act) {
        let around = self.tools_around();
        match act {
            Act::Back => {
                if let Some(room) = self.tools.as_mut() {
                    room.screen = Screen::List;
                }
            }
            Act::Run => self.begin_the_tool(false),
            Act::TryAgain => self.begin_the_tool(true),
            Act::Stop => {
                if let Some(page) = self.the_page()
                    && let Stage::Running(working) = &mut page.stage
                {
                    working.stop();
                }
            }
            Act::Choose(wanted) => self.ask_for_a_file(wanted),
            Act::Remove(at) => {
                if let Some(page) = self.the_page() {
                    page.remove(at);
                }
            }
            Act::Shift(at, earlier) => {
                if let Some(page) = self.the_page() {
                    page.shift(at, earlier);
                }
            }
            Act::StartOver => {
                if let Some(page) = self.the_page() {
                    page.start_over(&around);
                }
            }
            Act::BackToSettings => {
                if let Some(page) = self.the_page() {
                    page.stage = Stage::Settings;
                }
            }
            Act::OpenInPanPdf(path) => self.open_at(&path, 0),
            Act::OpenOutside(path) => self.open_out(&path.to_string_lossy()),
            Act::ShowInFolder(path) => {
                if let Err(error) = tools_run::show_in_folder(&path) {
                    self.editor.say(Message::CouldNotOpen {
                        uri: path.display().to_string(),
                        why: error.to_string(),
                    });
                }
            }
            Act::SaveCopy => self.ask_for_a_file(Wanted::SaveCopy),
        }
        ctx.request_repaint();
    }

    fn start_folder(&self) -> Option<PathBuf> {
        let there = |path: &Path| path.parent().map(Path::to_path_buf);
        let page_file = self.tools.as_ref().and_then(|room| match &room.screen {
            Screen::Tool(page) => page.files.iter().find_map(|held| match &held.source {
                Source::File(path) => there(path),
                Source::Document => None,
            }),
            Screen::List => None,
        });
        page_file.or_else(|| {
            (self.has_document() && !self.untitled)
                .then(|| there(&self.opened))
                .flatten()
        })
    }

    fn ask_for_a_file(&mut self, wanted: Wanted) {
        let folder = self.start_folder();
        let Some(page) = self.the_page() else { return };
        let tool = page.tool;
        let chooser = match wanted {
            Wanted::Files { replacing } => {
                let offer = Offer::Extensions(tools::extensions(tool));
                let room = if replacing.is_some() {
                    1
                } else {
                    tools::room_for_more(tool, page.files.len())
                };
                if room > 1 {
                    Chooser::several_of(folder.as_deref(), offer)
                } else {
                    Chooser::at(folder.as_deref(), offer)
                }
            }
            Wanted::SignaturePicture => Chooser::at(folder.as_deref(), Offer::Pictures),
            Wanted::SaveCopy => {
                let Stage::Done(done) = &page.stage else {
                    return;
                };
                let Some((name, _)) = done.outcome.files.first() else {
                    return;
                };
                let saved = done
                    .saved
                    .as_ref()
                    .ok()
                    .and_then(|saved| saved.files.first());
                let shown = saved.map_or_else(|| name.clone(), |path| name_of(path));
                let place = saved.and_then(|path| path.parent()).map(Path::to_path_buf);
                Chooser::saving_as(
                    place.as_deref().or(folder.as_deref()),
                    &tools::stem_of(&shown),
                    1,
                    ending_of(name),
                )
            }
        };
        page.asking = Some(Asking { chooser, wanted });
    }

    fn the_tools_chooser(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        let Some(page) = self.the_page() else { return };
        let Some(mut asking) = page.asking.take() else {
            return;
        };
        let title = match asking.wanted {
            Wanted::Files { .. } => Tools::ChooseFilesTitle,
            Wanted::SignaturePicture => Tools::ChooseSignatureTitle,
            Wanted::SaveCopy => Tools::SaveResultTitle,
        }
        .say(lang);
        match asking.chooser.show(ctx, lang, &title) {
            Chose::Nothing => page.asking = Some(asking),
            Chose::Cancelled => {}
            Chose::Open(path) => self.the_tool_was_given(asking.wanted, &[path]),
            Chose::Several(paths) => self.the_tool_was_given(asking.wanted, &paths),
            Chose::Save(path) => self.save_a_copy_of_the_result(&path),
        }
    }

    fn the_tool_was_given(&mut self, wanted: Wanted, paths: &[PathBuf]) {
        let lang = self.lang;
        let Some(page) = self.the_page() else { return };
        match wanted {
            Wanted::Files { replacing } => page.add_files(paths, replacing, lang),
            Wanted::SignaturePicture => {
                if let Some(path) = paths.first() {
                    page.use_the_picture(path, lang);
                }
            }
            Wanted::SaveCopy => {}
        }
    }

    fn save_a_copy_of_the_result(&mut self, path: &Path) {
        let Some(page) = self.the_page() else { return };
        let Stage::Done(done) = &mut page.stage else {
            return;
        };
        let Some((_, bytes)) = done.outcome.files.first() else {
            return;
        };
        let size = bytes.len() as u64;
        let name = path.display().to_string();
        let written = tools_run::write_a_copy(&done.original, path, bytes);
        let said = match written {
            Ok(saved) => {
                done.saved = Ok(saved);
                Message::SavedTo { name, bytes: size }
            }
            Err(why) => Message::CouldNotSave { name, why },
        };
        self.editor.say(said);
    }

    fn base_folder(&self, files: &[crate::tools_page::Held]) -> (PathBuf, PathBuf) {
        let there = |path: &Path| path.parent().map(Path::to_path_buf);
        let from_the_document = || {
            (!self.untitled && !self.opened.as_os_str().is_empty())
                .then(|| (self.opened.clone(), there(&self.opened)))
        };
        let first = files.first();
        let chosen = match first.map(|held| &held.source) {
            Some(Source::File(path)) => Some((path.clone(), there(path))),
            Some(Source::Document) => from_the_document(),
            None => None,
        };
        let (original, folder) = chosen.unwrap_or_default();
        let folder = folder
            .filter(|folder| !folder.as_os_str().is_empty())
            .or_else(tools_run::documents_folder)
            .unwrap_or_else(|| PathBuf::from("."));
        (original, folder)
    }

    fn begin_the_tool(&mut self, retry: bool) {
        let around = self.tools_around();
        if around.busy {
            return;
        }
        let credential = self.editor.credential().to_vec();
        let wants_document = self.the_page().is_some_and(|page| {
            page.files
                .iter()
                .any(|held| held.source == Source::Document)
        });
        let exported = if wants_document {
            match self.editor.export() {
                Ok(export) => Some(export.bytes),
                Err(why) => {
                    self.tool_failed(Failure::Refused(why));
                    return;
                }
            }
        } else {
            None
        };
        let held = self
            .the_page()
            .map(|page| page.files.clone())
            .unwrap_or_default();
        let (original, base) = self.base_folder(&held);
        let Some(page) = self.the_page() else { return };
        if retry && let Stage::Failed(failed) = &page.stage {
            let (older, newer) = (failed.typed.clone(), failed.typed_newer.clone());
            apply_the_passwords(page, &older, &newer);
        }
        let tool = page.tool;
        let mut values = page.values.clone();
        let mut exported = exported;
        let mut sources = Vec::with_capacity(page.files.len());
        for (at, held) in page.files.iter().enumerate() {
            let origin = match &held.source {
                Source::Document => {
                    values.give_the_credential(tool, at, &credential);
                    Origin::Document(exported.take().unwrap_or_default())
                }
                Source::File(path) => Origin::File(path.clone()),
            };
            sources.push((held.name.clone(), origin));
        }
        let names: Vec<String> = page.files.iter().map(|held| held.name.clone()).collect();
        let from_bytes =
            (tool == Tool::Compress && page.files.len() == 1).then(|| page.files[0].size);
        page.ran = Some(crate::tools_page::Ran {
            names,
            original,
            base,
            from_bytes,
            retried: retry,
        });
        page.stage = Stage::Running(tools_run::begin(Order {
            tool,
            values,
            sources,
        }));
    }

    fn tool_failed(&mut self, failure: Failure) {
        if let Some(page) = self.the_page() {
            page.stage = Stage::Failed(Box::new(Failed {
                failure,
                typed: String::new(),
                typed_newer: String::new(),
                tried: false,
            }));
        }
    }

    fn keep_the_tool_going(&mut self, ctx: &egui::Context) {
        let Some(page) = self.the_page() else { return };
        let Stage::Running(working) = &mut page.stage else {
            return;
        };
        ctx.request_repaint_after(Duration::from_millis(50));
        let Poll::Finished(result) = working.poll() else {
            return;
        };
        let tenths = working.seconds_tenths();
        let ran = page.ran.take().unwrap_or_default();
        let tool = page.tool;
        page.stage = Stage::Settings;
        let said = match result {
            Ok(outcome) if outcome.made_nothing() => {
                let why = outcome.notes.join("; ");
                page.stage = failed_stage(Failure::Refused(why), ran.retried);
                None
            }
            Err(Failure::Cancelled) => None,
            Err(failure) => {
                page.stage = failed_stage(failure, ran.retried);
                None
            }
            Ok(outcome) => {
                let names: Vec<&str> = ran.names.iter().map(String::as_str).collect();
                let saved =
                    tools_run::save_beside(tool, &outcome, &names, (&ran.original, &ran.base));
                let said = saved.as_ref().ok().and_then(|saved| {
                    let first = saved.files.first()?;
                    let bytes = outcome.files.first().map(|(_, bytes)| bytes.len() as u64)?;
                    Some(Message::SavedTo {
                        name: first.display().to_string(),
                        bytes,
                    })
                });
                page.stage = Stage::Done(Box::new(Done {
                    tool,
                    outcome,
                    saved,
                    tenths,
                    from_bytes: ran.from_bytes,
                    original: ran.original,
                }));
                said
            }
        };
        if let Some(said) = said {
            self.editor.say(said);
        }
        ctx.request_repaint();
    }

    pub(crate) fn tools_take_the_drop(&mut self, ctx: &egui::Context) -> bool {
        let lang = self.lang;
        let around = self.tools_around();
        let Some(page) = self.the_page() else {
            return false;
        };
        let dropped: Vec<PathBuf> = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .filter_map(|file| file.path.clone())
                .collect()
        });
        if dropped.is_empty() || page.asking.is_some() {
            return true;
        }
        match page.stage {
            Stage::Running(_) => {}
            Stage::Done(_) | Stage::Failed(_) => {
                page.start_over(&around);
                page.add_files(&dropped, None, lang);
            }
            Stage::Settings => page.add_files(&dropped, None, lang),
        }
        true
    }
}

fn failed_stage(failure: Failure, retried: bool) -> Stage {
    Stage::Failed(Box::new(Failed {
        tried: retried && failure == Failure::NeedsPassword,
        failure,
        typed: String::new(),
        typed_newer: String::new(),
    }))
}

fn apply_the_passwords(page: &mut Page, older: &str, newer: &str) {
    let tool = page.tool;
    if !older.is_empty()
        && let Some(setting) = tool.file_password()
    {
        page.values.set(setting, Value::Secret(older.to_owned()));
    }
    if !newer.is_empty() && tool == Tool::Compare {
        page.values
            .set(Setting::SecondPassword, Value::Secret(newer.to_owned()));
    }
}

pub(crate) fn ending_of(name: &str) -> &'static str {
    match name
        .rsplit_once('.')
        .map(|(_, ending)| ending.to_ascii_lowercase())
        .as_deref()
    {
        Some("docx") => "docx",
        Some("xlsx") => "xlsx",
        Some("pptx") => "pptx",
        Some("html") => "html",
        Some("md") => "md",
        Some("txt") => "txt",
        Some("jpg" | "jpeg") => "jpg",
        Some("png") => "png",
        _ => "pdf",
    }
}

#[cfg(test)]
mod tests;
