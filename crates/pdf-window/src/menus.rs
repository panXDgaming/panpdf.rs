use eframe::egui;
use egui::containers::menu::SubMenuButton;

use pdf_app::wording::{Command, Lang, Message};

use crate::window_state::{Tool, Window, set_dark};

const FILE_MENU_WITH_KEYS: [Command; 7] = [
    Command::NewDocument,
    Command::OpenDocument,
    Command::Save,
    Command::SaveAs,
    Command::Print,
    Command::DocumentProperties,
    Command::Documents,
];

const EDIT_MENU_WITH_KEYS: [Command; 8] = [
    Command::Undo,
    Command::Redo,
    Command::Cut,
    Command::Copy,
    Command::Paste,
    Command::PasteInPlace,
    Command::Delete,
    Command::Find,
];

const VIEW_MENU_WITH_KEYS: [Command; 3] = [Command::ZoomIn, Command::ZoomOut, Command::ShowFrames];

pub(crate) fn menus_shown(home: bool) -> Vec<Command> {
    let mut shown = vec![Command::File];
    if !home {
        shown.extend([Command::Edit, Command::Insert, Command::Page]);
    }
    shown.extend([Command::Tools, Command::View]);
    #[cfg(not(target_arch = "wasm32"))]
    shown.push(Command::Help);
    shown
}

pub(crate) fn menu_item(
    ctx: &egui::Context,
    lang: Lang,
    command: Command,
) -> egui::Button<'static> {
    let button = egui::Button::new(Message::Command(command).say(lang));
    match crate::shortcuts::hint(ctx, command) {
        Some(keys) => button.shortcut_text(keys),
        None => button,
    }
}

const KEYS_APART: f32 = 24.0;

fn make_room_for_the_keys(ui: &mut egui::Ui, lang: Lang, commands: &[Command]) {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let measured = |text: String| {
        ui.painter()
            .layout_no_wrap(text, font.clone(), egui::Color32::PLACEHOLDER)
            .size()
            .x
    };
    let widest = commands
        .iter()
        .map(|command| {
            let name = measured(Message::Command(*command).say(lang));
            let keys = crate::shortcuts::hint(ui.ctx(), *command)
                .map_or(0.0, |keys| measured(keys) + KEYS_APART);
            name + keys
        })
        .fold(0.0, f32::max);
    ui.set_min_width(widest + 2.0 * ui.spacing().button_padding.x + ui.spacing().item_spacing.x);
}

fn submenu_button(ctx: &egui::Context, lang: Lang, command: Command) -> SubMenuButton<'static> {
    let button = menu_item(ctx, lang, command).right_text(SubMenuButton::RIGHT_ARROW);
    SubMenuButton::from_button(button)
}

impl Window {
    pub(crate) fn menu_bar(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        let say = |command| Message::Command(command).say(lang);
        let idle = !self.editor.is_busy() && self.loading.is_none();
        let with_document = self.has_document() && !self.home;
        let working = idle && with_document;
        egui::Panel::top("menu").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                for menu in menus_shown(self.home) {
                    ui.menu_button(say(menu), |ui| match menu {
                        Command::File => self.file_menu(ui, (idle, working, with_document)),
                        Command::Edit => self.edit_menu(ui, working),
                        Command::Insert => self.insert_menu(ui, working),
                        Command::Page => self.page_menu(ui, working),
                        Command::Tools => self.tools_menu(ui, with_document),
                        Command::View => self.view_menu(ui, (working, with_document)),
                        #[cfg(not(target_arch = "wasm32"))]
                        Command::Help => self.help_menu(ui),
                        _ => {}
                    });
                }
            });
        });
    }

    fn file_menu(&mut self, ui: &mut egui::Ui, (idle, working, with_document): (bool, bool, bool)) {
        let ctx = ui.ctx().clone();
        let lang = self.lang;
        let say = |command| Message::Command(command).say(lang);
        let item = |command| menu_item(&ctx, lang, command);
        make_room_for_the_keys(ui, lang, &FILE_MENU_WITH_KEYS);
        ui.add_enabled_ui(idle, |ui| {
            submenu_button(&ctx, lang, Command::NewDocument).ui(ui, |ui| {
                if let Some(size) = self.page_size_menu(ui) {
                    self.new_document(size);
                    ui.close();
                }
            });
        });
        if ui.add_enabled(idle, item(Command::OpenDocument)).clicked() {
            self.asking_to_open = true;
            ui.close();
        }
        ui.add_enabled_ui(self.library.len() > 1, |ui| {
            ui.menu_button(say(Command::Documents), |ui| {
                for path in self.library.clone() {
                    let open = path == self.opened;
                    let button = egui::Button::selectable(open, crate::app::name_of(&path));
                    if ui.add_enabled(idle || open, button).clicked() {
                        self.open(&path);
                        ui.close();
                    }
                }
            });
        });
        ui.separator();
        if ui.add_enabled(working, item(Command::Save)).clicked() {
            self.save();
            ui.close();
        }
        if ui.add_enabled(working, item(Command::SaveAs)).clicked() {
            self.save_a_copy_as();
            ui.close();
        }
        ui.separator();
        ui.add_enabled_ui(working, |ui| {
            ui.menu_button(say(Command::Export), |ui| {
                if ui.button(say(Command::PagesToPictures)).clicked() {
                    self.open_the_export_panel();
                    ui.close();
                }
            });
        });
        ui.add_enabled_ui(idle, |ui| {
            ui.menu_button(say(Command::CreateAPdf), |ui| {
                if ui.button(say(Command::PicturesToPdf)).clicked() {
                    self.choose_pictures(None);
                    ui.close();
                }
            });
        });
        ui.separator();
        if ui.add_enabled(working, item(Command::Print)).clicked() {
            self.open_the_print_dialog();
            ui.close();
        }
        ui.separator();
        if ui
            .add_enabled(working, item(Command::DocumentProperties))
            .clicked()
        {
            self.open_the_properties();
            ui.close();
        }
        ui.separator();
        let close = egui::Button::new(say(Command::CloseDocument));
        if ui.add_enabled(with_document, close).clicked() {
            self.go_home();
            ui.close();
        }
    }

    fn edit_menu(&mut self, ui: &mut egui::Ui, working: bool) {
        let ctx = ui.ctx().clone();
        let lang = self.lang;
        let say = |command| Message::Command(command).say(lang);
        let item = |command| menu_item(&ctx, lang, command);
        make_room_for_the_keys(ui, lang, &EDIT_MENU_WITH_KEYS);
        if ui
            .add_enabled(working && self.editor.can_undo(), item(Command::Undo))
            .clicked()
        {
            self.walk_history(true);
            ui.close();
        }
        if ui
            .add_enabled(working && self.editor.can_redo(), item(Command::Redo))
            .clicked()
        {
            self.walk_history(false);
            ui.close();
        }
        ui.separator();
        let chosen = working && self.selected();
        if ui.add_enabled(chosen, item(Command::Cut)).clicked() {
            let in_text = self.pointing.editing();
            self.cut(&ctx, in_text);
            ui.close();
        }
        if ui.add_enabled(chosen, item(Command::Copy)).clicked() {
            let in_text = self.pointing.editing();
            self.copy(&ctx, in_text);
            ui.close();
        }
        let holding = working && self.clipboard.is_some();
        if ui.add_enabled(holding, item(Command::Paste)).clicked() {
            self.paste_the_clipboard(&ctx, false);
            ui.close();
        }
        if ui
            .add_enabled(holding, item(Command::PasteInPlace))
            .clicked()
        {
            self.paste_the_clipboard(&ctx, true);
            ui.close();
        }
        if ui.add_enabled(chosen, item(Command::Delete)).clicked() {
            self.delete_what_is_chosen();
            ui.close();
        }
        ui.separator();
        let ordering = working && self.can_order();
        ui.add_enabled_ui(ordering, |ui| {
            ui.menu_button(say(Command::Arrange), |ui| {
                for (command, order) in [
                    (Command::BringToFront, pdf_edit::Stacking::ToFront),
                    (Command::BringForward, pdf_edit::Stacking::Forward),
                    (Command::SendBackward, pdf_edit::Stacking::Backward),
                    (Command::SendToBack, pdf_edit::Stacking::ToBack),
                ] {
                    if ui.button(say(command)).clicked() {
                        self.put_in_order(order);
                        ui.close();
                    }
                }
            });
        });
        ui.separator();
        if ui.add_enabled(working, item(Command::Find)).clicked() {
            self.open_the_find_bar();
            ui.close();
        }
        if self.editor.editing_restricted() {
            ui.separator();
            let allow = egui::Button::new(say(Command::AllowEditing));
            if ui.add_enabled(working, allow).clicked() {
                self.restriction_answered = false;
                ui.close();
            }
        }
    }

    fn insert_menu(&mut self, ui: &mut egui::Ui, working: bool) {
        let lang = self.lang;
        let say = |command| Message::Command(command).say(lang);
        ui.add_enabled_ui(working, |ui| {
            if ui.button(say(Command::InsertText)).clicked() {
                self.pictures.clear();
                self.tool = Tool::Text;
                ui.close();
            }
            if ui.button(say(Command::InsertPictures)).clicked() {
                self.choosing_for = crate::page_actions::Choosing::Picture;
                self.asking_to_open = true;
                ui.close();
            }
            if ui.button(say(Command::Shape)).clicked() {
                self.take_up(Tool::Shape);
                ui.close();
            }
            if ui.button(say(Command::Link)).clicked() {
                self.take_up_the_link_tool();
                ui.close();
            }
            if ui.button(say(Command::InsertField)).clicked() {
                self.take_up_the_form_tool();
                ui.close();
            }
            ui.separator();
            for (command, tool) in [
                (Command::Pen, Tool::Pen),
                (Command::Highlighter, Tool::Highlighter),
            ] {
                if ui.button(say(command)).clicked() {
                    self.take_up(tool);
                    ui.close();
                }
            }
            ui.separator();
            for (command, kind) in [
                (
                    Command::StampPageNumbers,
                    crate::stamp_tool::StampKind::PageNumbers,
                ),
                (
                    Command::StampHeaderFooter,
                    crate::stamp_tool::StampKind::HeaderFooter,
                ),
                (
                    Command::StampWatermark,
                    crate::stamp_tool::StampKind::Watermark,
                ),
            ] {
                if ui.button(say(command)).clicked() {
                    self.open_the_stamp_panel(kind);
                    ui.close();
                }
            }
        });
    }

    fn tools_menu(&mut self, ui: &mut egui::Ui, with_document: bool) {
        let ctx = ui.ctx().clone();
        let lang = self.lang;
        let say = |command| Message::Command(command).say(lang);
        #[cfg(not(target_arch = "wasm32"))]
        {
            let assistant = menu_item(&ctx, lang, Command::AiAssistant).selected(self.ai.open);
            if ui.add_enabled(with_document, assistant).clicked() {
                self.toggle_the_assistant(&ctx);
                ui.close();
            }
        }
        if ui
            .add_enabled(
                with_document,
                egui::Button::new(say(Command::RecognizeText)),
            )
            .clicked()
        {
            self.open_the_ocr_panel();
            ui.close();
        }
        #[cfg(not(target_arch = "wasm32"))]
        if ui.button(say(Command::ConnectAgents)).clicked() {
            self.open_the_agents_window();
            ui.close();
        }
        #[cfg(target_arch = "wasm32")]
        let _ = (&ctx, with_document);
    }

    fn view_menu(&mut self, ui: &mut egui::Ui, (working, with_document): (bool, bool)) {
        let ctx = ui.ctx().clone();
        let lang = self.lang;
        let say = |command| Message::Command(command).say(lang);
        let item = |command| menu_item(&ctx, lang, command);
        make_room_for_the_keys(ui, lang, &VIEW_MENU_WITH_KEYS);
        if ui.add_enabled(working, item(Command::ZoomIn)).clicked() {
            self.zoom_by(true, None);
        }
        if ui.add_enabled(working, item(Command::ZoomOut)).clicked() {
            self.zoom_by(false, None);
        }
        ui.separator();
        let mut pages = !self.pages_folded;
        if ui.checkbox(&mut pages, say(Command::Pages)).changed() {
            let now = ui.input(|input| input.time);
            self.fold_the_pages(!pages, now);
        }
        let _ = ui.checkbox(&mut self.show_contents, say(Command::Contents));
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut assistant = self.ai.open;
            let shown = egui::Checkbox::new(&mut assistant, say(Command::AiAssistant));
            if ui.add_enabled(with_document, shown).changed() {
                self.toggle_the_assistant(&ctx);
            }
        }
        #[cfg(target_arch = "wasm32")]
        let _ = with_document;
        ui.separator();
        submenu_button(&ctx, lang, Command::Frames).ui(ui, |ui| {
            let all = item(Command::ShowFrames).selected(self.show_frames);
            if ui.add(all).clicked() {
                self.show_frames = !self.show_frames;
            }
            ui.separator();
            ui.add_enabled_ui(self.show_frames, |ui| {
                let _ = ui.checkbox(&mut self.framed.text, say(Command::FramesOfText));
                let _ = ui.checkbox(&mut self.framed.pictures, say(Command::FramesOfPictures));
                let _ = ui.checkbox(&mut self.framed.drawings, say(Command::FramesOfDrawings));
            });
        });
        if ui
            .checkbox(&mut self.dark, say(Command::DarkMode))
            .changed()
        {
            set_dark(&ctx, self.dark);
            self.dark_chosen = true;
            self.remember_the_view();
        }
        if crate::chrome::language_rows().len() > 1 {
            ui.menu_button(say(Command::Language), |ui| {
                for (language, named) in crate::chrome::language_rows() {
                    let _ = ui.radio_value(&mut self.lang, language, named);
                }
            });
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn help_menu(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        let say = |command| Message::Command(command).say(lang);
        if ui.button(say(Command::ReportAProblem)).clicked() {
            self.open_out(&crate::reporting::report_link());
            ui.close();
        }
        let folder = crate::reporting::log_folder();
        let show = egui::Button::new(say(Command::ShowTheLog));
        if ui.add_enabled(folder.is_some(), show).clicked() {
            if let Some(folder) = folder {
                self.open_out(&folder.to_string_lossy());
            }
            ui.close();
        }
        ui.separator();
        let _ = ui.checkbox(&mut self.show_speed, say(Command::ShowDrawingSpeed));
    }
}

#[cfg(test)]
mod tests {
    use pdf_app::wording::Command;

    use super::menus_shown;

    #[test]
    fn the_home_screen_shows_only_the_menus_that_can_work_without_a_document() {
        let home = menus_shown(true);
        for gone in [Command::Edit, Command::Insert, Command::Page] {
            assert!(!home.contains(&gone), "{gone:?} on the home screen");
        }
        for stays in [Command::File, Command::Tools, Command::View] {
            assert!(
                home.contains(&stays),
                "{stays:?} missing from the home screen"
            );
        }
    }

    #[test]
    fn a_document_shows_every_menu_in_the_order_a_person_reads_them() {
        let shown = menus_shown(false);
        let wanted = [
            Command::File,
            Command::Edit,
            Command::Insert,
            Command::Page,
            Command::Tools,
            Command::View,
        ];
        assert_eq!(&shown[..wanted.len()], &wanted);
        #[cfg(not(target_arch = "wasm32"))]
        assert_eq!(shown.last(), Some(&Command::Help));
    }

    #[test]
    fn the_home_screen_keeps_the_order_of_the_menus_it_shares_with_a_document() {
        let home = menus_shown(true);
        let document = menus_shown(false);
        let shared: Vec<Command> = document
            .iter()
            .copied()
            .filter(|menu| home.contains(menu))
            .collect();
        assert_eq!(shared, home);
    }
}
