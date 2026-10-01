use std::path::PathBuf;

use pdf_app::Editor;
use pdf_bytes::{ByteStore, SourceId};
use pdf_convert::run::Failure;
use pdf_convert::{Setting, Tool, Value, Values};

use super::{Screen, apply_the_passwords, ending_of, give_the_credential};
use crate::tools_page::{Page, Source, Stage};
use crate::window_state::Window;

fn window_on(path: PathBuf) -> Window {
    let source = ByteStore::new(
        SourceId::new(0),
        &include_bytes!("../../../pdf-edit/tests/data/modifiable-r3.pdf")[..],
    );
    let editor = Editor::open_with(source, b"view").unwrap();
    Window::new(editor, path, Vec::new())
}

fn blank_window() -> Window {
    Window::new(
        Editor::stand_in().expect("the stand-in document opens"),
        PathBuf::new(),
        Vec::new(),
    )
}

fn page(window: &mut Window) -> &mut Page {
    window.the_page().expect("a tool's page is open")
}

fn folder(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("panpdf-room-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn wait_for_the_end(window: &mut Window) {
    let ctx = eframe::egui::Context::default();
    let until = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while matches!(
        window.tools.as_ref().map(|room| &room.screen),
        Some(Screen::Tool(page)) if matches!(page.stage, Stage::Running(_))
    ) {
        window.keep_the_tool_going(&ctx);
        assert!(std::time::Instant::now() < until, "the job did not finish");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn the_list_is_shown_first_and_a_tool_opens_its_page_with_the_open_document() {
    let mut window = window_on(PathBuf::from("/somewhere/original.pdf"));
    assert!(window.tools.is_none());
    window.open_the_tools(None);
    assert!(matches!(
        window.tools.as_ref().map(|room| &room.screen),
        Some(Screen::List)
    ));
    window.open_the_tools(Some(Tool::Compress));
    let held = page(&mut window).files.clone();
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].name, "original.pdf");
    assert_eq!(held[0].source, Source::Document);
    window.open_the_tools(Some(Tool::WordToPdf));
    assert!(page(&mut window).files.is_empty());
}

#[test]
fn the_room_can_be_opened_with_no_document_and_a_tool_starts_empty() {
    let mut window = blank_window();
    window.open_the_tools(Some(Tool::PdfToWord));
    assert!(page(&mut window).files.is_empty());
}

#[test]
fn recognising_text_opens_the_text_panel_of_the_document_and_leaves_the_room() {
    let mut window = window_on(PathBuf::from("/somewhere/original.pdf"));
    window.open_the_tools(None);
    window.open_the_tools(Some(Tool::Ocr));
    assert!(window.tools.is_none());
    assert!(window.ocr_draft.is_some());
    assert!(!window.home);
}

#[test]
fn recognising_text_with_no_document_asks_for_one_and_opens_the_panel_when_it_arrives() {
    let mut window = blank_window();
    window.open_the_tools(None);
    window.open_the_tools(Some(Tool::Ocr));
    assert!(window.tools.is_none());
    assert!(window.asking_to_open);
    assert!(window.read_text_after_opening);
    assert!(window.ocr_draft.is_none());
}

#[test]
fn the_password_of_a_document_is_given_to_the_tool_unless_one_was_typed() {
    let mut values = Values::new();
    give_the_credential(Tool::Compress, 0, b"view", &mut values);
    assert_eq!(values.text(Setting::Password).as_deref(), Some("view"));
    let mut typed = Values::new().with(Setting::Password, Value::Secret("mine".to_owned()));
    give_the_credential(Tool::Compress, 0, b"view", &mut typed);
    assert_eq!(typed.text(Setting::Password).as_deref(), Some("mine"));
    let mut none = Values::new();
    give_the_credential(Tool::Compress, 0, b"", &mut none);
    assert!(!none.is_set(Setting::Password));
}

#[test]
fn the_password_of_a_document_goes_where_that_tool_wants_it() {
    let mut protect = Values::new();
    give_the_credential(Tool::Protect, 0, b"view", &mut protect);
    assert_eq!(protect.text(Setting::FilePassword).as_deref(), Some("view"));
    assert!(!protect.is_set(Setting::NewPassword));
    let mut newer = Values::new();
    give_the_credential(Tool::Compare, 1, b"view", &mut newer);
    assert_eq!(newer.text(Setting::SecondPassword).as_deref(), Some("view"));
    let mut elsewhere = Values::new();
    give_the_credential(Tool::Sign, 1, b"view", &mut elsewhere);
    assert!(!elsewhere.is_set(Setting::Password));
}

#[test]
fn passwords_typed_after_a_failure_are_the_files_and_for_a_comparison_the_newer_files_too() {
    let around = window_on(PathBuf::from("/a.pdf")).tools_around();
    let mut page = Page::new(Tool::Compare, &around);
    apply_the_passwords(&mut page, "old", "new");
    assert_eq!(page.values.text(Setting::Password).as_deref(), Some("old"));
    assert_eq!(
        page.values.text(Setting::SecondPassword).as_deref(),
        Some("new")
    );
    let mut unlock = Page::new(Tool::Unlock, &around);
    apply_the_passwords(&mut unlock, "sesame", "ignored");
    assert_eq!(
        unlock.values.text(Setting::Password).as_deref(),
        Some("sesame")
    );
    assert!(!unlock.values.is_set(Setting::SecondPassword));
}

#[test]
fn a_result_is_offered_to_be_saved_under_the_ending_it_has() {
    assert_eq!(ending_of("a.docx"), "docx");
    assert_eq!(ending_of("A.XLSX"), "xlsx");
    assert_eq!(ending_of("deck.pptx"), "pptx");
    assert_eq!(ending_of("page.html"), "html");
    assert_eq!(ending_of("notes.md"), "md");
    assert_eq!(ending_of("words.txt"), "txt");
    assert_eq!(ending_of("one.jpeg"), "jpg");
    assert_eq!(ending_of("one.png"), "png");
    assert_eq!(ending_of("x.pdf"), "pdf");
    assert_eq!(ending_of("noending"), "pdf");
}

#[test]
fn a_document_is_converted_as_it_is_on_screen_and_the_result_is_saved_beside_it() {
    let folder = folder("convert");
    let mut window = window_on(folder.join("original.pdf"));
    window.open_the_tools(Some(Tool::PdfToText));
    window.begin_the_tool(false);
    assert!(matches!(page(&mut window).stage, Stage::Running(_)));
    wait_for_the_end(&mut window);
    let Stage::Done(done) = &page(&mut window).stage else {
        panic!("the conversion did not finish");
    };
    assert_eq!(done.outcome.files[0].0, "original.txt");
    let saved = done.saved.as_ref().expect("the result is saved");
    assert_eq!(saved.files, vec![folder.join("original.txt")]);
    assert!(
        std::fs::metadata(folder.join("original.txt"))
            .unwrap()
            .len()
            > 0
    );
    assert!(
        !folder.join("original.pdf").exists(),
        "the original is not written"
    );
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_document_that_asks_for_its_password_gives_it_to_the_tool_without_being_asked() {
    let folder = folder("password");
    let mut window = window_on(folder.join("locked.pdf"));
    assert_eq!(window.editor.credential(), b"view");
    window.open_the_tools(Some(Tool::Compress));
    window.begin_the_tool(false);
    wait_for_the_end(&mut window);
    let stage = &page(&mut window).stage;
    assert!(
        matches!(stage, Stage::Done(_)),
        "{}",
        match stage {
            Stage::Failed(failed) => format!("{:?}", failed.failure),
            _ => "not done".to_owned(),
        }
    );
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_failure_with_no_password_asks_for_it_and_try_again_runs_with_what_was_typed() {
    let folder = folder("again");
    let locked = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../pdf-edit/tests/data/modifiable-r3.pdf"),
    )
    .unwrap();
    std::fs::write(folder.join("locked.pdf"), locked).unwrap();
    let mut window = blank_window();
    window.open_the_tools(Some(Tool::Unlock));
    page(&mut window).add_files(
        &[folder.join("locked.pdf")],
        None,
        pdf_app::wording::Lang::English,
    );
    window.begin_the_tool(false);
    wait_for_the_end(&mut window);
    {
        let Stage::Failed(failed) = &mut page(&mut window).stage else {
            panic!("a locked file without its password should fail");
        };
        assert_eq!(failed.failure, Failure::NeedsPassword);
        failed.typed = "view".to_owned();
    }
    window.begin_the_tool(true);
    wait_for_the_end(&mut window);
    let stage = &page(&mut window).stage;
    assert!(
        matches!(stage, Stage::Done(_)),
        "{}",
        match stage {
            Stage::Failed(failed) => format!("{:?}", failed.failure),
            _ => "not done".to_owned(),
        }
    );
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_file_dropped_while_a_tools_page_is_open_goes_to_the_page_and_opens_no_document() {
    let folder = folder("drop");
    std::fs::write(folder.join("dropped.pdf"), b"%PDF-1.4").unwrap();
    let mut window = blank_window();
    window.open_the_tools(Some(Tool::Compress));
    let ctx = eframe::egui::Context::default();
    let raw = eframe::egui::RawInput {
        dropped_files: vec![eframe::egui::DroppedFile {
            path: Some(folder.join("dropped.pdf")),
            ..Default::default()
        }],
        ..Default::default()
    };
    let _ = ctx.run_ui(raw, |ui| {
        assert!(window.tools_take_the_drop(ui.ctx()));
    });
    assert_eq!(page(&mut window).names(), ["dropped.pdf"]);
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn files_dropped_on_the_list_are_not_taken_by_the_room() {
    let mut window = blank_window();
    window.open_the_tools(None);
    let ctx = eframe::egui::Context::default();
    let raw = eframe::egui::RawInput {
        dropped_files: vec![eframe::egui::DroppedFile {
            path: Some(PathBuf::from("/x/y.pdf")),
            ..Default::default()
        }],
        ..Default::default()
    };
    let _ = ctx.run_ui(raw, |ui| {
        assert!(!window.tools_take_the_drop(ui.ctx()));
    });
}

#[test]
fn opening_a_document_shuts_the_room() {
    let mut window = window_on(PathBuf::from("/somewhere/original.pdf"));
    window.open_the_tools(None);
    assert!(window.tools.is_some());
    window.new_document([595.0, 842.0]);
    assert!(window.tools.is_none());
}
