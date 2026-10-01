use std::path::Path;

use eframe::egui;

use pdf_app::Editor;
use pdf_app::wording::{Command, Lang, Message, Tone};
use pdf_bytes::{ByteStore, SourceId};

use crate::app::name_of;
use crate::icons::Icon;
use crate::room;
use crate::window_state::{LeaveChoice, Leaving, Opened, Opening, Pointing, Tool, Window};

const MM_PER_POINT: f64 = 25.4 / 72.0;

pub(crate) const A4: [f64; 2] = [595.28, 841.89];

const PAPER_SIZES: &[(&str, [f64; 2])] = &[
    ("A3", [841.89, 1190.55]),
    ("A4", A4),
    ("A5", [419.53, 595.28]),
    ("B4", [708.66, 1000.63]),
    ("B5", [498.9, 708.66]),
    ("Letter", [612.0, 792.0]),
    ("Legal", [612.0, 1008.0]),
    ("Tabloid", [792.0, 1224.0]),
];

pub(crate) fn open_bytes(source: ByteStore, credential: &[u8]) -> Opened {
    if pdf_edit::info::lock(&source, credential) == pdf_edit::info::Lock::Refused {
        return Opened::Locked(source);
    }
    match Editor::open_with(source, credential) {
        Ok(mut editor) => {
            editor.set_clock(crate::moment::millis);
            Opened::Document(Box::new(editor))
        }
        Err(reason) => Opened::Refused(reason),
    }
}

impl Window {
    pub(crate) fn page_size_menu(&self, ui: &mut egui::Ui) -> Option<[f64; 2]> {
        let say = |command| Message::Command(command).say(self.lang);
        let turned = |[width, height]: [f64; 2]| {
            if self.landscape == (width > height) {
                [width, height]
            } else {
                [height, width]
            }
        };
        let points = |[width, height]: [f64; 2]| {
            format!(
                "{:.0} × {:.0} mm",
                width * MM_PER_POINT,
                height * MM_PER_POINT
            )
        };
        let mut chosen = None;
        let showing = self.has_document() && !self.home;
        if let Some(geometry) = showing.then(|| self.editor.geometry(self.focus)).flatten() {
            let [x0, y0, x1, y1] = geometry.media_box;
            let size = [(x1 - x0).abs(), (y1 - y0).abs()];
            let label = format!("{} ({})", say(Command::SameSizeAsThisPage), points(size));
            if ui.button(label).clicked() {
                chosen = Some(size);
            }
            ui.separator();
        }
        for (name, size) in PAPER_SIZES {
            let size = turned(*size);
            if ui.button(format!("{name} ({})", points(size))).clicked() {
                chosen = Some(size);
            }
        }
        chosen
    }

    pub(crate) fn selected(&self) -> bool {
        matches!(
            self.pointing,
            Pointing::Text { caret, .. } if caret.at != caret.anchor
        ) || matches!(self.pointing, Pointing::Block { .. })
            || (self.tool == Tool::Form && self.chosen_fields.is_some())
            || (self.tool == Tool::Link
                && self
                    .chosen_links
                    .as_ref()
                    .is_some_and(|chosen| !chosen.links.is_empty()))
            || self.pointing.object_on(self.focus).is_some()
    }

    pub(crate) fn delete_what_is_chosen(&mut self) {
        if self.remove_the_chosen_fields() {
            return;
        }
        if self.tool == Tool::Link && self.remove_the_chosen_link() {
            return;
        }
        self.delete();
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn toggle_the_assistant(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|input| input.time);
        if self.ai.open {
            self.ai.open = false;
            self.ai_flow = Some(room::Flow::new(self.ai.width, 0.0, now));
        } else {
            self.open_ai_panel(now);
        }
    }

    pub(crate) fn toolbar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.add_space(3.0);
            ui.horizontal(|ui| {
                let room = ui.available_width();
                let idle = !self.editor.is_busy() && self.loading.is_none();
                let lang = self.lang;
                let slots = self.bar_slots(idle);

                let bar = room::Bar {
                    buttons: slots.iter().filter(|slot| slot.is_button()).count(),
                    rules: slots.iter().filter(|slot| slot.is_rule()).count(),
                    slack: self.toolbar_slack,
                };
                let labels = room::labels_fit(room, &bar, !self.toolbar_compact);
                self.toolbar_compact = !labels;

                let mut pieces: Vec<room::Piece> =
                    slots.iter().map(|slot| slot.piece(labels)).collect();
                pieces.push(room::Piece {
                    width: room::EDGE + self.toolbar_slack,
                    rank: 0,
                });
                let cut = room::shed_above(room, &pieces, room::OVERFLOW_WIDTH);

                let start = ui.cursor().min.x;
                ui.add_space(4.0);
                let mut pressed = None;
                let mut drawn = 0.0;
                let mut anything_yet = false;
                for slot in slots.iter().filter(|slot| slot.side == Side::Left) {
                    if let Some(command) =
                        self.draw_slot(ui, slot, cut, &mut anything_yet, &mut drawn)
                    {
                        pressed = Some(command);
                    }
                }

                let left_end = ui.cursor().min.x;
                let mut right_width = 0.0;
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.editor.is_busy() || self.loading.is_some() {
                        ui.spinner();
                    }
                    if cut != room::ALL
                        && let Some(command) = Self::overflow_menu(ui, &slots, cut, lang)
                    {
                        pressed = Some(command);
                    }
                    let mut yet = true;
                    for slot in slots.iter().filter(|slot| slot.side == Side::Right) {
                        if let Some(command) = self.draw_slot(ui, slot, cut, &mut yet, &mut drawn) {
                            pressed = Some(command);
                        }
                    }
                    right_width = ui.min_rect().width();
                });
                let measured = (left_end - start) + right_width;
                self.toolbar_slack = (measured - drawn).clamp(0.0, 80.0);
                if let Some(command) = pressed {
                    let ctx = ui.ctx().clone();
                    self.run_from_the_bar(&ctx, command);
                }
                self.let_go_of_text_the_tool_may_not_hold();
            });
            if self.finding.is_some() {
                self.find_bar(ui);
            }
            ui.add_space(3.0);
        });
    }

    pub(crate) fn tool_options(&mut self, ui: &mut egui::Ui) {
        let Some(command) = TOOLS
            .iter()
            .find(|(_, _, tool)| *tool == self.tool)
            .map(|(_, command, _)| *command)
            .filter(|_| self.tool_has_choices())
        else {
            return;
        };
        let name = Message::Command(command).say(self.lang);
        egui::Panel::top("tool options").show(ui, |ui| {
            egui::ScrollArea::horizontal()
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(name)
                                .size(11.0)
                                .color(ui.visuals().weak_text_color()),
                        );
                        crate::format::rule(ui);
                        self.tool_choices(ui);
                    });
                });
        });
    }

    fn bar_slots(&self, idle: bool) -> Vec<Slot> {
        let button = |icon, command, enabled, on, rank, side| Slot {
            side,
            rank,
            what: What::Button {
                icon,
                command,
                enabled,
                on,
            },
        };
        let rule = |rank, side| Slot {
            side,
            rank,
            what: What::Rule,
        };
        let mut slots = vec![
            button(
                Icon::Open,
                Command::Open,
                idle,
                false,
                OPEN_RANK,
                Side::Left,
            ),
            button(
                Icon::Save,
                Command::Save,
                idle,
                false,
                SAVE_RANK,
                Side::Left,
            ),
            rule(HISTORY_RANK, Side::Left),
            button(
                Icon::Undo,
                Command::Undo,
                idle && self.editor.can_undo(),
                false,
                HISTORY_RANK,
                Side::Left,
            ),
            button(
                Icon::Redo,
                Command::Redo,
                idle && self.editor.can_redo(),
                false,
                HISTORY_RANK,
                Side::Left,
            ),
            rule(0, Side::Left),
        ];
        for (icon, command, tool) in TOOLS {
            slots.push(button(
                icon,
                command,
                if tool == Tool::Select { true } else { idle },
                self.tool == tool,
                0,
                Side::Left,
            ));
        }
        slots.push(button(
            Icon::Delete,
            Command::Delete,
            idle && self.selected(),
            false,
            DELETE_RANK,
            Side::Left,
        ));
        slots.extend(self.view_slots());
        slots
    }

    fn view_slots(&self) -> Vec<Slot> {
        let button = |icon, command, enabled, on, rank| Slot {
            side: Side::Right,
            rank,
            what: What::Button {
                icon,
                command,
                enabled,
                on,
            },
        };
        let rule = |rank| Slot {
            side: Side::Right,
            rank,
            what: What::Rule,
        };
        let mut slots = Vec::new();
        #[cfg(not(target_arch = "wasm32"))]
        {
            slots.push(button(
                Icon::Assistant,
                Command::Assistant,
                true,
                self.ai.open,
                ASSISTANT_RANK,
            ));
            slots.push(rule(ASSISTANT_RANK));
        }
        slots.push(button(
            Icon::ZoomIn,
            Command::ZoomIn,
            true,
            false,
            ZOOM_RANK,
        ));
        slots.push(button(
            Icon::ZoomOut,
            Command::ZoomOut,
            true,
            false,
            ZOOM_RANK,
        ));
        slots.push(rule(PAGING_RANK));
        let pages = self.editor.page_count();
        slots.push(button(
            Icon::Next,
            Command::NextPage,
            self.focus + 1 < pages,
            false,
            PAGING_RANK,
        ));
        slots.push(button(
            Icon::Previous,
            Command::PreviousPage,
            self.focus > 0,
            false,
            PAGING_RANK,
        ));
        slots
    }

    fn draw_slot(
        &self,
        ui: &mut egui::Ui,
        slot: &Slot,
        cut: u8,
        anything_yet: &mut bool,
        drawn: &mut f32,
    ) -> Option<Command> {
        if slot.rank >= cut {
            return None;
        }
        *drawn += slot.piece(!self.toolbar_compact).width;
        match &slot.what {
            What::Rule => {
                if *anything_yet {
                    toolbar_separator(ui);
                }
                None
            }
            What::Button {
                icon,
                command,
                enabled,
                on,
            } => {
                *anything_yet = true;
                self.tool_button(ui, *icon, *command, *enabled, *on)
                    .then_some(*command)
            }
        }
    }

    fn overflow_menu(ui: &mut egui::Ui, slots: &[Slot], cut: u8, lang: Lang) -> Option<Command> {
        let name = Message::Command(Command::MoreForPage).say(lang);
        let opened = crate::format::icon_button(ui, Icon::More, &name, false, true);
        let mut pressed = None;
        let ctx = ui.ctx().clone();
        egui::Popup::menu(&opened).show(|ui| {
            ui.set_min_width(180.0);
            for slot in slots {
                if slot.rank < cut {
                    continue;
                }
                let What::Button {
                    command, enabled, ..
                } = slot.what
                else {
                    continue;
                };
                let button = crate::menus::menu_item(&ctx, lang, command);
                if ui.add_enabled(enabled, button).clicked() {
                    pressed = Some(command);
                    ui.close();
                }
            }
        });
        pressed
    }

    fn run_from_the_bar(&mut self, ctx: &egui::Context, command: Command) {
        match command {
            Command::Open => self.asking_to_open = true,
            Command::Save => {
                self.save();
            }
            Command::Undo => {
                self.walk_history(true);
            }
            Command::Redo => {
                self.walk_history(false);
            }
            Command::Delete => self.delete_what_is_chosen(),
            Command::Select => {
                self.tool = Tool::Select;
                self.pictures.clear();
                self.ink = None;
            }
            Command::Text => {
                self.pictures.clear();
                self.tool = Tool::Text;
            }
            Command::Pen => self.take_up(Tool::Pen),
            Command::Highlighter => self.take_up(Tool::Highlighter),
            Command::Shape => self.take_up(Tool::Shape),
            Command::Form => self.take_up_the_form_tool(),
            Command::Link => self.take_up_the_link_tool(),
            Command::Picture => {
                self.choosing_for = crate::page_actions::Choosing::Picture;
                self.asking_to_open = true;
            }
            #[cfg(not(target_arch = "wasm32"))]
            Command::Assistant => self.toggle_the_assistant(ctx),
            Command::ZoomIn => self.zoom_by(true, None),
            Command::ZoomOut => self.zoom_by(false, None),
            Command::NextPage => self.goto(self.focus + 1),
            Command::PreviousPage => self.goto(self.focus.saturating_sub(1)),
            _ => {}
        }
        #[cfg(target_arch = "wasm32")]
        let _ = ctx;
    }

    fn let_go_of_text_the_tool_may_not_hold(&mut self) {
        if self.tool.edits_text() {
            return;
        }
        let kept = self.pointing;
        self.point_at(kept);
        self.drop_the_text_draft();
    }

    fn tool_has_choices(&self) -> bool {
        self.tool.is_drawing() || matches!(self.tool, Tool::Form | Tool::Link)
    }

    fn tool_choices(&mut self, ui: &mut egui::Ui) {
        match self.tool {
            Tool::Form => self.form_tool_choices(ui),
            Tool::Link => self.link_tool_choices(ui),
            Tool::Shape => {
                self.shape_choices(ui);
                crate::format::rule(ui);
                self.pen_choices(ui);
            }
            Tool::Pen | Tool::Highlighter => self.pen_choices(ui),
            Tool::Select | Tool::Text | Tool::Picture => {}
        }
    }

    pub(crate) fn tool_hint(&self) -> Option<String> {
        match self.tool {
            Tool::Form => self.form_tool_hint(),
            Tool::Link => self.link_tool_hint(),
            Tool::Select
            | Tool::Text
            | Tool::Pen
            | Tool::Highlighter
            | Tool::Shape
            | Tool::Picture => None,
        }
    }

    fn hover_words(&self, ctx: &egui::Context, command: Command) -> String {
        let named = match command {
            Command::Assistant => Command::AiAssistant,
            other => other,
        };
        let name = Message::Command(named).say(self.lang);
        match crate::shortcuts::hint(ctx, command) {
            Some(keys) => format!("{name} ({keys})"),
            None => name,
        }
    }

    fn tool_button(
        &self,
        ui: &mut egui::Ui,
        icon: Icon,
        command: Command,
        enabled: bool,
        on: bool,
    ) -> bool {
        let name = Message::Command(caption_of(command)).say(self.lang);
        let width = room::tool_width(!self.toolbar_compact);
        let size = egui::vec2(width, room::TOOL_HEIGHT);
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
        let response = response.on_hover_text(self.hover_words(ui.ctx(), command));
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &name));
        let visuals = ui.visuals();
        let colour = if !enabled {
            visuals.weak_text_color()
        } else if on {
            visuals.selection.stroke.color
        } else {
            visuals.text_color()
        };
        if ui.is_rect_visible(rect) {
            if on {
                ui.painter()
                    .rect_filled(rect, 4.0, visuals.selection.bg_fill.gamma_multiply(0.35));
            } else if enabled && response.hovered() {
                ui.painter()
                    .rect_filled(rect, 4.0, visuals.widgets.hovered.bg_fill);
            }
            crate::dialog::focus_ring(ui, &response, rect, 4.0);
            let top = if self.toolbar_compact {
                rect.center().y - room::ICON_SIDE / 2.0
            } else {
                rect.top() + 4.0
            };
            let glyph = egui::Rect::from_min_size(
                egui::pos2(rect.center().x - room::ICON_SIDE / 2.0, top),
                egui::vec2(room::ICON_SIDE, room::ICON_SIDE),
            );
            icon.draw_tinted(ui.painter(), glyph, colour, enabled);
            #[cfg(not(target_arch = "wasm32"))]
            if command == Command::Assistant
                && !self.ai.open
                && let Some(badge) = self.ai.badge()
            {
                crate::ai_panel::paint_badge(ui, glyph.right_top() + egui::vec2(3.0, 2.0), badge);
            }
            if self.toolbar_compact {
                return enabled && response.clicked();
            }
            ui.painter().text(
                egui::pos2(rect.center().x, rect.bottom() - 3.0),
                egui::Align2::CENTER_BOTTOM,
                &name,
                egui::FontId::proportional(10.0),
                colour,
            );
        }
        enabled && response.clicked()
    }

    pub(crate) fn open(&mut self, path: &Path) {
        self.open_at(path, 0);
    }

    pub(crate) fn open_at(&mut self, path: &Path, page: usize) {
        if self.editor.is_busy() || self.loading.is_some() || self.unlocking.is_some() {
            return;
        }
        if *path == self.opened {
            self.home = false;
            self.goto(page.min(self.editor.page_count().saturating_sub(1)));
            return;
        }
        if self.unsaved() {
            self.leaving = Some(Leaving::Open(path.to_path_buf(), page));
            return;
        }
        self.open_now(path, page);
    }

    fn open_now(&mut self, path: &Path, page: usize) {
        #[cfg(not(target_arch = "wasm32"))]
        crate::reporting::say(
            pdf_app::trouble::Kind::Document,
            &format!("opening {}", path.display()),
        );
        self.remember_here();
        let wanted = path.to_path_buf();
        let reading = wanted.clone();
        let opening = Message::Opening(name_of(&wanted));
        self.editor.say(opening);
        self.loading = Some(Opening {
            path: wanted,
            page,
            changed_protection: false,
            tried_a_password: false,
            handle: std::thread::spawn(move || match std::fs::read(&reading) {
                Ok(bytes) => open_bytes(ByteStore::owning(SourceId::next_document(), bytes), b""),
                Err(error) => Opened::Refused(format!("{}: {error}", reading.display())),
            }),
        });
    }

    fn name_of_the_document(&self) -> String {
        if self.untitled {
            if self.destination.as_os_str().is_empty() {
                pdf_app::wording::Home::Untitled.say(self.lang)
            } else {
                name_of(&self.destination)
            }
        } else {
            name_of(&self.opened)
        }
    }

    pub(crate) fn window_title_now(&self) -> String {
        if self.home || !self.has_document() {
            return Message::ProgramName.say(self.lang);
        }
        Message::WindowTitle {
            name: self.name_of_the_document(),
            unsaved: self.unsaved(),
        }
        .say(self.lang)
    }

    pub(crate) fn keep_the_window_title(&mut self, ctx: &egui::Context) {
        let title = self.window_title_now();
        if title != self.system_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.system_title = title;
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn title_for_the_chat(&self, path: &std::path::Path) -> String {
        path.file_name().map_or_else(
            || pdf_app::wording::Home::Untitled.say(self.lang),
            |name| name.to_string_lossy().into_owned(),
        )
    }

    fn take_the_document(
        &mut self,
        editor: pdf_app::Editor,
        path: std::path::PathBuf,
        page: usize,
    ) {
        self.editor = editor;
        self.saved_epoch = self.editor.epoch();
        self.saved_digest = None;
        self.protection_changed = false;
        self.untitled = path.as_os_str().is_empty();
        self.destination = if self.untitled {
            std::path::PathBuf::new()
        } else {
            crate::save_file::unused_copy(&path)
        };
        #[cfg(not(target_arch = "wasm32"))]
        self.ai
            .document_arrived(self.title_for_the_chat(&path), place_of(&path));
        self.opened = path;
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.tools = None;
        }
        self.resume = None;
        self.input = pdf_app::draft::Input::default();
        self.restriction_answered = false;
        self.point_at(Pointing::Nothing);
        self.drag = None;
        self.landing = None;
        self.let_go_of_what_belonged_to_the_last_document();
        for (id, held, slot) in self.tiles.clear() {
            self.retire(id, held, slot);
        }
        self.scenes.clear();
        self.thumbs.clear();
        self.failed.clear();
        self.chosen_pages.clear();
        self.focus = 0;
        self.wanted_offset = Some(egui::Vec2::ZERO);
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(folder) = self.opened.parent() {
            self.library = crate::hub::pdfs_in(folder);
        }
        let last = self.editor.page_count().saturating_sub(1);
        if page > 0 {
            self.goto(page.min(last));
            self.focus = page.min(last);
        }
        self.home = false;
        self.remember_here();
        let returned = pdf_heap::give_back();
        #[cfg(not(target_arch = "wasm32"))]
        crate::reporting::say(
            pdf_app::trouble::Kind::Session,
            if returned {
                "memory: returned to the system after letting a document go"
            } else {
                "memory: nothing was returned after letting a document go"
            },
        );
        #[cfg(target_arch = "wasm32")]
        let _ = returned;
    }

    fn let_go_of_what_belonged_to_the_last_document(&mut self) {
        self.filling = None;
        self.save_after_the_field = false;
        self.saving_then_leaving = None;
        self.text_draft = None;
        self.stamp_draft = None;
        self.ocr_draft = None;
        self.link_draft = None;
        self.field_draft = None;
        self.field_draft_origin = None;
        self.naming_draft = None;
        self.properties = None;
        self.print_draft = None;
        self.splitting = None;
        self.exporting = None;
        self.chosen_fields = None;
        self.chosen_links = None;
        self.chosen_bookmark = None;
        self.renaming = None;
        self.pictures.clear();
        self.ink = None;
        self.search_the_document_again();
    }

    pub(crate) fn let_the_document_go(&mut self) {
        if self.editor.is_busy() || self.loading.is_some() {
            return;
        }
        if self.unsaved() {
            self.leaving = Some(Leaving::LetGo);
            return;
        }
        self.drop_the_document();
    }

    fn drop_the_document(&mut self) {
        match Editor::blank(A4) {
            Ok(editor) => {
                self.take_the_document(editor, std::path::PathBuf::new(), 0);
                self.untitled = false;
                self.home = true;
                self.editor
                    .say(Message::Home(pdf_app::wording::Home::DocumentLetGo));
            }
            Err(reason) => self.editor.say(Message::Plain(reason)),
        }
    }

    pub(crate) fn new_document(&mut self, size: [f64; 2]) {
        if self.editor.is_busy() || self.loading.is_some() {
            return;
        }
        if self.unsaved() {
            self.leaving = Some(Leaving::New(size));
            return;
        }
        self.start_a_new_document(size);
    }

    fn start_a_new_document(&mut self, size: [f64; 2]) {
        match Editor::blank(size) {
            Ok(editor) => {
                self.remember_here();
                self.take_the_document(editor, std::path::PathBuf::new(), 0);
                self.editor.say(Message::StartedANewDocument);
            }
            Err(reason) => self.editor.say(Message::Plain(reason)),
        }
    }

    pub(crate) fn collect_open(&mut self, ctx: &egui::Context) {
        let Some(loading) = &self.loading else { return };
        if !loading.handle.is_finished() {
            ctx.request_repaint();
            return;
        }
        let Some(loading) = self.loading.take() else {
            return;
        };
        match loading.handle.join() {
            Ok(Opened::Document(editor)) => {
                self.take_the_document(*editor, loading.path, loading.page);
                #[cfg(not(target_arch = "wasm32"))]
                if std::mem::take(&mut self.read_text_after_opening) {
                    self.open_the_ocr_panel();
                }
                if loading.changed_protection {
                    self.protection_changed = true;
                    self.editor.say(Message::Plain(
                        pdf_app::wording::Fact::ProtectionChanged.say(self.lang),
                    ));
                }
            }
            Ok(Opened::Locked(source)) => {
                self.editor.say(Message::Quiet);
                self.unlocking = Some(crate::unlock::Unlock {
                    path: loading.path,
                    page: loading.page,
                    source,
                    typed: String::new(),
                    tried: loading.tried_a_password,
                    shown: false,
                    focus: true,
                    for_pages: None,
                });
            }
            Ok(Opened::Refused(reason)) => self.editor.say(Message::Plain(reason)),
            Err(_) => {
                let failed = Message::OpeningFailed;
                self.editor.say(failed);
            }
        }
    }

    pub(crate) fn field_text_waits(&self) -> bool {
        self.filling
            .as_ref()
            .is_some_and(|filling| filling.text != filling.field.field.value.shown())
    }

    pub(crate) fn save(&mut self) -> bool {
        if self.input.pending() || self.input.draft().is_some() {
            self.editor.say(Message::ResolveDraftBeforeSaving);
            return false;
        }
        if self.field_text_waits() {
            self.finish_the_field();
            self.save_after_the_field = self.filling.is_none() && self.running.is_some();
            return false;
        }
        if self.untitled && self.destination.as_os_str().is_empty() {
            self.save_a_copy_as();
            return false;
        }
        match self.editor.export() {
            Ok(export) => match crate::save_file::save(
                &self.opened,
                &self.destination,
                &export.bytes,
                self.saved_digest.as_deref(),
            ) {
                Ok(digest) => {
                    self.saved_digest = Some(digest);
                    self.saved_epoch = self.editor.epoch();
                    self.protection_changed = false;
                    if self.untitled {
                        let saved = self.destination.clone();
                        self.remember_file(&saved);
                    }
                    let frame = self.frame;
                    let revision = self
                        .editor
                        .revision()
                        .map_or_else(|| "unknown".to_owned(), |revision| format!("r{revision}"));
                    let digest = pdf_content::sha256_hex(&export.bytes);
                    if let Some(trace) = self.trace.as_mut() {
                        trace.note(
                            frame,
                            "saved",
                            &format!(
                                "path={} bytes={} revision={revision} sha256={digest}",
                                self.destination.display(),
                                export.bytes.len()
                            ),
                        );
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    crate::reporting::say(
                        pdf_app::trouble::Kind::Document,
                        &format!(
                            "saved {} ({} bytes)",
                            self.destination.display(),
                            export.bytes.len()
                        ),
                    );
                    let said = Message::SavedTo {
                        name: name_of(&self.destination),
                        bytes: export.bytes.len() as u64,
                    };
                    self.editor.say(said);
                    #[cfg(not(target_arch = "wasm32"))]
                    self.ai.saved_as(place_of(&self.destination));
                    true
                }
                Err(error) => {
                    #[cfg(not(target_arch = "wasm32"))]
                    crate::reporting::say(
                        pdf_app::trouble::Kind::Failed,
                        &format!("could not save {}: {error}", self.destination.display()),
                    );
                    let said = Message::CouldNotSave {
                        name: self.destination.display().to_string(),
                        why: error.to_string(),
                    };
                    self.editor.say(said);
                    false
                }
            },
            Err(reason) => {
                self.editor.say(Message::Plain(reason));
                false
            }
        }
    }

    pub(crate) fn unsaved(&self) -> bool {
        self.editor.is_busy()
            || self.protection_changed
            || self.editor.epoch() != self.saved_epoch
            || self.input.pending()
            || self.input.draft().is_some()
            || self.field_text_waits()
    }

    pub(crate) fn guard_close(&mut self, ctx: &egui::Context) {
        if ctx.input(|input| input.viewport().close_requested())
            && !self.close_confirmed
            && (self.unsaved() || self.loading.is_some())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.leaving = Some(Leaving::Close);
        }
    }

    pub(crate) fn asks_about_restrictions(&self) -> bool {
        !self.restriction_answered
            && !self.home
            && self.leaving.is_none()
            && self.loading.is_none()
            && self.unlocking.is_none()
            && self.editor.editing_restricted()
    }

    pub(crate) fn warn_of_restrictions(&mut self, ctx: &egui::Context) {
        if !self.asks_about_restrictions() {
            return;
        }
        let idle = !self.editor.is_busy();
        let mut edit = false;
        let mut read = false;
        let modal = egui::Modal::new(egui::Id::new("editing-restricted")).show(ctx, |ui| {
            ui.set_max_width(460.0);
            ui.heading(Message::EditingRestricted.say(self.lang));
            ui.label(Message::EditingRestrictedWarning.say(self.lang));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                edit = ui
                    .add_enabled(idle, egui::Button::new(Message::EditAnyway.say(self.lang)))
                    .clicked();
                read = ui.button(Message::ReadOnly.say(self.lang)).clicked();
            });
        });
        if (edit && self.editor.set_aside_restrictions()) || read || modal.should_close() {
            self.restriction_answered = true;
        }
    }

    pub(crate) fn confirm_leaving(&mut self, ctx: &egui::Context) {
        if self.leaving.is_none() {
            return;
        }
        let busy = self.editor.is_busy();
        let loading = self.loading.is_some();
        let pending = self.input.pending();
        let draft = self.input.draft().is_some();
        let offer = leaving_offer(StillHolding {
            at_rest: !busy && !loading && !pending,
            draft,
        });
        let mut choice = None;
        egui::Modal::new(egui::Id::new("unsaved-document")).show(ctx, |ui| {
            ui.heading(Message::UnsavedChanges.say(self.lang));
            ui.label(Message::SaveBeforeLeaving.say(self.lang));
            if draft {
                ui.label(Message::ResolveDraftBeforeSaving.say(self.lang));
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        offer.save,
                        egui::Button::new(Message::Command(Command::Save).say(self.lang)),
                    )
                    .on_disabled_hover_text(
                        if busy {
                            Message::SaveWaitsForTheRunningEdit
                        } else if pending {
                            Message::SaveWaitsForTypingToLand
                        } else if draft {
                            Message::SaveWaitsForTheDraft
                        } else {
                            Message::SaveWaitsForTheDocumentToOpen
                        }
                        .say(self.lang),
                    )
                    .clicked()
                {
                    choice = Some(LeaveChoice::Save);
                }
                if ui
                    .add_enabled(
                        offer.discard,
                        egui::Button::new(Message::DiscardChanges.say(self.lang)),
                    )
                    .clicked()
                {
                    choice = Some(LeaveChoice::Discard);
                }
                if ui.button(Message::CancelLeaving.say(self.lang)).clicked() {
                    choice = Some(LeaveChoice::Cancel);
                }
            });
        });
        if let Some(choice) = choice {
            self.resolve_leaving(choice, ctx);
        }
    }

    pub(crate) fn resolve_leaving(&mut self, choice: LeaveChoice, ctx: &egui::Context) {
        match choice {
            LeaveChoice::Cancel => self.leaving = None,
            LeaveChoice::Save if !self.save() => {
                if self.chooser.is_some() || self.save_after_the_field {
                    self.saving_then_leaving = self.leaving.take();
                }
            }
            LeaveChoice::Save | LeaveChoice::Discard => {
                if matches!(choice, LeaveChoice::Discard) {
                    self.input.abandon();
                    self.filling = None;
                }
                let leaving = self.leaving.take();
                self.carry_on_leaving(leaving, ctx);
            }
        }
    }

    fn carry_on_leaving(&mut self, leaving: Option<Leaving>, ctx: &egui::Context) {
        match leaving {
            Some(Leaving::Open(path, page)) => self.open_now(&path, page),
            Some(Leaving::New(size)) => self.start_a_new_document(size),
            Some(Leaving::Close) => {
                self.close_confirmed = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Some(Leaving::LetGo) => self.drop_the_document(),
            None => {}
        }
    }

    pub(crate) fn carry_on_after_saving(&mut self, ctx: &egui::Context) {
        let Some(leaving) = self.saving_then_leaving.take() else {
            return;
        };
        if !self.unsaved() {
            self.carry_on_leaving(Some(leaving), ctx);
        }
    }

    fn is_showing_a_document(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        if self.tools.is_some() {
            return false;
        }
        self.has_document() && !self.home
    }

    pub(crate) fn status_bar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|input| input.time);
        let said = self.editor.status().clone();
        self.status_line.watch(&said, now);
        let strength = self.status_line.strength(now);
        if let Some(wait) = self.status_line.changes_after(now) {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
        }
        let lang = self.lang;
        let status = said.say(lang);
        let trouble = said.tone() == Tone::Trouble;
        let hint = self.tool_hint();
        let with_document = self.is_showing_a_document();
        let position = with_document.then(|| {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a zoom is between 25% and 400%"
            )]
            let percent = (self.zoom * 100.0).round() as u32;
            Message::PageAndZoom {
                page: self.focus + 1,
                count: self.editor.page_count(),
                percent,
            }
            .say(lang)
        });
        let unsaved = with_document && self.unsaved();
        let unsaved_words = Message::UnsavedChanges.say(lang);
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.set_min_height(ui.spacing().interact_size.y);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let weak = ui.visuals().weak_text_color();
                    if let Some(position) = position {
                        ui.label(egui::RichText::new(position).size(12.0).color(weak));
                    }
                    if unsaved {
                        unsaved_dot(ui).on_hover_text(&unsaved_words);
                    }
                    if let Some(hint) = hint {
                        ui.label(egui::RichText::new(hint).size(12.0).color(weak));
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        let colour = if trouble {
                            ui.visuals().warn_fg_color
                        } else {
                            weak.gamma_multiply(strength)
                        };
                        let words = egui::RichText::new(&status).size(12.0).color(colour);
                        ui.add(egui::Label::new(words).truncate())
                            .on_hover_text(&status);
                    });
                });
            });
        });
    }

    pub(crate) fn draft_bar(&mut self, ui: &mut egui::Ui) {
        let Some(draft) = self.input.draft() else {
            return;
        };
        let text = draft.text.clone();
        let reason = draft.reason.say(self.lang);
        let retryable = draft.can_retry(self.editor.epoch());
        let idle = !self.editor.is_busy() && self.resume.is_none();
        let preview: String = {
            let mut shown: String = text
                .chars()
                .take(40)
                .map(|character| match character {
                    '\n' => '⏎',
                    pdf_edit::LINE_BREAK => '↵',
                    other => other,
                })
                .collect();
            if text.chars().count() > 40 {
                shown.push('…');
            }
            shown
        };
        let lang = self.lang;
        let (mut retry, mut copy, mut discard) = (false, false, false);
        egui::Panel::top("draft").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(Message::DraftBarTitle.say(lang)).strong());
                ui.label(format!("“{preview}”")).on_hover_text(&text);
                retry = ui
                    .add_enabled(
                        retryable && idle,
                        egui::Button::new(Message::DraftRetry.say(lang)),
                    )
                    .on_disabled_hover_text(
                        if retryable {
                            Message::DraftWaitForTheEditToLand
                        } else {
                            Message::DraftPlaceNoLongerUsable
                        }
                        .say(lang),
                    )
                    .clicked();
                copy = ui.button(Message::DraftCopy.say(lang)).clicked();
                discard = ui.button(Message::DraftDiscard.say(lang)).clicked();
                ui.add(egui::Label::new(&reason).truncate())
                    .on_hover_text(&reason);
            });
        });
        if copy {
            ui.ctx().copy_text(text);
            self.editor.say(Message::DraftCopied);
        }
        if retry {
            self.retry_draft();
        }
        if discard {
            self.input.discard();
            let said = Message::DraftDiscarded;
            self.editor.say(said);
        }
    }
}

pub(crate) struct LeavingOffer {
    pub(crate) save: bool,
    pub(crate) discard: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct StillHolding {
    pub(crate) at_rest: bool,
    pub(crate) draft: bool,
}

pub(crate) fn leaving_offer(holding: StillHolding) -> LeavingOffer {
    LeavingOffer {
        save: holding.at_rest && !holding.draft,
        discard: true,
    }
}

const fn caption_of(command: Command) -> Command {
    match command {
        Command::PreviousPage => Command::Previous,
        Command::NextPage => Command::Next,
        other => other,
    }
}

fn toolbar_separator(ui: &mut egui::Ui) {
    ui.add_space(4.0);
    ui.separator();
    ui.add_space(4.0);
}

const OPEN_RANK: u8 = 5;
const ZOOM_RANK: u8 = 5;
const SAVE_RANK: u8 = 4;
const PAGING_RANK: u8 = 4;
const DELETE_RANK: u8 = 3;
#[cfg_attr(
    target_arch = "wasm32",
    expect(
        dead_code,
        reason = "the assistant button is not built for the browser"
    )
)]
const ASSISTANT_RANK: u8 = 3;
const HISTORY_RANK: u8 = 2;

const TOOLS: [(Icon, Command, Tool); 8] = [
    (Icon::Select, Command::Select, Tool::Select),
    (Icon::Text, Command::Text, Tool::Text),
    (Icon::Pen, Command::Pen, Tool::Pen),
    (Icon::Highlighter, Command::Highlighter, Tool::Highlighter),
    (Icon::Shape, Command::Shape, Tool::Shape),
    (Icon::Form, Command::Form, Tool::Form),
    (Icon::Link, Command::Link, Tool::Link),
    (Icon::Picture, Command::Picture, Tool::Picture),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Side {
    Left,
    Right,
}

#[derive(Clone, Debug)]
enum What {
    Button {
        icon: Icon,
        command: Command,
        enabled: bool,
        on: bool,
    },
    Rule,
}

#[derive(Clone, Debug)]
struct Slot {
    side: Side,
    rank: u8,
    what: What,
}

impl Slot {
    fn is_button(&self) -> bool {
        matches!(self.what, What::Button { .. })
    }

    fn is_rule(&self) -> bool {
        matches!(self.what, What::Rule)
    }

    fn piece(&self, labels: bool) -> room::Piece {
        let width = match self.what {
            What::Button { .. } => room::tool_width(labels) + room::GAP,
            What::Rule => room::RULE_WIDTH,
        };
        room::Piece {
            width,
            rank: self.rank,
        }
    }
}

fn unsaved_dot(ui: &mut egui::Ui) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
    ui.painter()
        .circle_filled(rect.center(), 3.5, ui.visuals().warn_fg_color);
    response
}

pub(crate) fn language_rows() -> Vec<(Lang, &'static str)> {
    Lang::ALL
        .iter()
        .map(|language| (*language, language.endonym()))
        .collect()
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn place_of(path: &std::path::Path) -> String {
    if path.as_os_str().is_empty() {
        return String::new();
    }
    std::fs::canonicalize(path)
        .or_else(|_| std::path::absolute(path))
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::{Lang, StillHolding, language_rows, leaving_offer};

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_tools_room_shows_no_page_and_zoom_of_the_document_behind_it() {
        let mut window = crate::window_state::Window::new(
            pdf_app::Editor::stand_in().expect("the stand-in document opens"),
            std::path::PathBuf::from("/missing/original.pdf"),
            Vec::new(),
        );
        assert!(window.is_showing_a_document());
        window.open_the_tools(None);
        assert!(
            !window.is_showing_a_document(),
            "the status bar has nothing to say about a page that is not on screen"
        );
    }

    #[test]
    fn the_picker_lists_exactly_the_languages_the_window_speaks() {
        let rows = language_rows();
        assert_eq!(rows.len(), Lang::ALL.len());
        for (at, (language, named)) in rows.iter().enumerate() {
            assert_eq!(*language, Lang::ALL[at]);
            assert_eq!(*named, language.endonym());
            assert!(!named.is_empty());
            assert_eq!(Lang::of_tag(language.tag()), Some(*language));
        }
        let english = rows
            .iter()
            .find(|(language, _)| *language == Lang::English)
            .expect("the window speaks English");
        assert_eq!(english.1, "English");
    }

    fn old_buggy_offer(holding: StillHolding) -> (bool, bool) {
        (holding.at_rest && !holding.draft, holding.at_rest)
    }

    #[test]
    fn the_old_rule_was_a_dead_end_which_is_the_control() {
        let (save, discard) = old_buggy_offer(StillHolding::default());
        assert!(
            !save && !discard,
            "known answer: a busy editor alone used to disable both ways out"
        );
    }

    #[test]
    fn the_leaving_dialogue_always_offers_a_way_out() {
        let bools = [false, true];
        let mut raised_the_dialogue_at_least_once = false;
        for busy in bools {
            for pending in bools {
                for draft in bools {
                    for loading in bools {
                        for protection_changed in bools {
                            for epoch_moved in bools {
                                let unsaved =
                                    busy || protection_changed || epoch_moved || pending || draft;
                                if !(unsaved || loading) {
                                    continue;
                                }
                                raised_the_dialogue_at_least_once = true;
                                let offer = leaving_offer(StillHolding {
                                    at_rest: !busy && !loading && !pending,
                                    draft,
                                });
                                assert!(
                                    offer.discard,
                                    "Discard must never be disabled: busy={busy} \
                                     pending={pending} draft={draft} loading={loading}"
                                );
                                assert!(
                                    offer.save || offer.discard,
                                    "no way out at all: busy={busy} pending={pending} \
                                     draft={draft} loading={loading} \
                                     protection_changed={protection_changed} \
                                     epoch_moved={epoch_moved}"
                                );
                            }
                        }
                    }
                }
            }
        }
        assert!(raised_the_dialogue_at_least_once);
    }
}
