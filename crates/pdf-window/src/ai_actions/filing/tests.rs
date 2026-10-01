use std::path::PathBuf;
use std::time::{Duration, Instant};

use pdf_agent::connect::{ToolCall, ToolResult};
use pdf_app::ai_permission::WrittenTo;

use crate::ai_actions::Performed;
use crate::ai_actions::editing::tests::{a_window_written, load_page};
use crate::ai_actions::tests::{a_blank_window, land_what_was_sent, tool};
use crate::window_state::{Leaving, Window};

fn folder(name: &str) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("panpdf-window-files-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("a folder");
    path
}

fn a_document_beside(name: &str, markdown: &str) -> (Window, PathBuf) {
    let folder = folder(name);
    let mut window = a_window_written(markdown);
    window.opened = folder.join("book.pdf");
    (window, folder)
}

fn pump(window: &mut Window) {
    let until = Instant::now() + Duration::from_secs(120);
    while window.ai.tools.files_in_flight() {
        window.keep_converting();
        window.keep_making();
        window.keep_recognizing();
        assert!(Instant::now() < until, "the job never finished");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn run_to_the_end(window: &mut Window, call: &ToolCall) -> ToolResult {
    window.ai.tools.take(std::slice::from_ref(call));
    window.ai.tools.revision_before = window.editor.revision();
    for _ in 0..50 {
        match window.perform(call) {
            Performed::Done(result) => {
                window.ai.tools.answer(result);
                break;
            }
            Performed::Sent => {
                land_what_was_sent(window);
                break;
            }
            Performed::Waiting => {
                pump(window);
                break;
            }
            Performed::NeedPages(pages) => {
                for page in pages {
                    load_page(window, page);
                }
            }
            Performed::Busy => panic!("{} found the editor busy", call.name),
        }
    }
    window
        .ai
        .tools
        .results
        .pop()
        .expect("the call was answered")
}

fn said(window: &mut Window, name: &str, arguments: &str) -> String {
    let result = run_to_the_end(window, &tool("t", name, arguments));
    assert!(!result.is_error, "{name} {arguments}: {}", result.text);
    result.text
}

fn refused(window: &mut Window, name: &str, arguments: &str) -> String {
    let result = run_to_the_end(window, &tool("t", name, arguments));
    assert!(
        result.is_error,
        "{name} {arguments} went ahead: {}",
        result.text
    );
    result.text
}

#[test]
fn converting_the_document_makes_a_new_file_beside_it_and_changes_nothing_in_it() {
    let (mut window, folder) = a_document_beside("convert", "Acme sells anvils from Bangkok.");
    let revision = window.editor.revision();
    let steps = window.ai.tools.run.steps();
    let text = said(
        &mut window,
        "convert",
        r#"{"document":"doc-1","tool":"pdf-to-text"}"#,
    );
    assert!(text.contains("book.txt"), "{text}");
    let made = std::fs::read_to_string(folder.join("book.txt")).expect("the text file");
    assert!(made.contains("Acme sells anvils from Bangkok."), "{made}");
    assert_eq!(
        window.editor.revision(),
        revision,
        "the document is as it was"
    );
    assert_eq!(
        window.ai.tools.run.steps(),
        steps,
        "no step of the assistant's"
    );
    assert!(!window.ai.tools.files_in_flight());
    assert!(window.ai.tools.queue.is_empty(), "the call is done with");
    assert!(
        matches!(
            window.editor.status(),
            pdf_app::wording::Message::SavedTo { name, .. } if name.ends_with("book.txt")
        ),
        "{:?}",
        window.editor.status()
    );
    let again = said(
        &mut window,
        "convert",
        r#"{"document":"doc-1","tool":"pdf-to-text"}"#,
    );
    assert!(
        again.contains("book-2.txt") && folder.join("book-2.txt").exists(),
        "no file is written over: {again}"
    );
    assert_eq!(
        std::fs::read_to_string(folder.join("book.txt")).expect("the first"),
        made
    );
}

#[test]
fn a_conversion_that_makes_several_files_puts_them_in_a_new_folder_and_one_file_goes_beside() {
    let (mut window, folder) = a_document_beside("pictures", "A page of words.");
    let one = said(
        &mut window,
        "convert",
        r#"{"document":"doc-1","tool":"pdf-to-jpg","options":{"format":"png","dpi":40}}"#,
    );
    assert!(
        one.contains("book-1.png") && folder.join("book-1.png").exists(),
        "one picture goes beside the document: {one}"
    );
    said(
        &mut window,
        "add_blank_page",
        r#"{"document":"doc-1","after_page":1}"#,
    );
    let two = said(
        &mut window,
        "convert",
        r#"{"document":"doc-1","tool":"pdf-to-jpg","options":{"format":"png","dpi":40}}"#,
    );
    assert!(two.contains("2 files in the new folder"), "{two}");
    let mut inside: Vec<String> = std::fs::read_dir(folder.join("book"))
        .expect("the new folder")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    inside.sort();
    assert_eq!(inside, ["book-1.png", "book-2.png"], "{two}");
    assert!(
        matches!(
            window.editor.status(),
            pdf_app::wording::Message::WrotePictures { files: 2, .. }
        ),
        "{:?}",
        window.editor.status()
    );
}

#[test]
fn a_conversion_that_is_refused_says_why_and_writes_nothing() {
    let (mut window, folder) = a_document_beside("convert-refused", "Words.");
    let wrong = refused(
        &mut window,
        "convert",
        r#"{"document":"doc-1","tool":"pdf-to-text","options":{"dpi":3}}"#,
    );
    assert!(wrong.contains("is not an option of pdf-to-text"), "{wrong}");
    let page = refused(
        &mut window,
        "convert",
        r#"{"document":"doc-1","tool":"pdf-to-text","options":{"pages":"9"}}"#,
    );
    assert!(page.starts_with("The converter"), "{page}");
    let missing = refused(
        &mut window,
        "convert",
        r#"{"document":"doc-1","tool":"word-to-pdf","files":["/no/such/letter.docx"]}"#,
    );
    assert!(!missing.is_empty(), "{missing}");
    assert!(
        std::fs::read_dir(&folder)
            .expect("a folder")
            .next()
            .is_none(),
        "nothing was written"
    );
}

#[test]
fn a_conversion_includes_the_changes_not_yet_saved_in_the_window() {
    let (mut window, folder) = a_document_beside("unsaved", "First words.");
    said(
        &mut window,
        "add_blank_page",
        r#"{"document":"doc-1","after_page":1}"#,
    );
    said(
        &mut window,
        "add_text",
        r#"{"document":"doc-1","page":2,"left":72,"top":72,"width":300,"text":"Second page words","font":"DejaVu Sans"}"#,
    );
    said(
        &mut window,
        "convert",
        r#"{"document":"doc-1","tool":"pdf-to-text"}"#,
    );
    let made = std::fs::read_to_string(folder.join("book.txt")).expect("the text file");
    assert!(
        made.contains("First words.") && made.contains("Second page words"),
        "{made}"
    );
}

#[test]
fn a_protected_copy_asks_for_its_password_and_a_file_that_needs_one_is_said_to_need_it() {
    let (mut window, folder) = a_document_beside("protect", "Secret anvils.");
    let text = said(
        &mut window,
        "protect_document",
        r#"{"document":"doc-1","password":"open sesame"}"#,
    );
    assert!(text.contains("book-protected.pdf"), "{text}");
    let protected = folder.join("book-protected.pdf");
    let bytes = std::fs::read(&protected).expect("the copy");
    let store = pdf_bytes::ByteStore::new(pdf_bytes::SourceId::new(1), bytes);
    assert_eq!(
        pdf_edit::info::lock(&store, b""),
        pdf_edit::info::Lock::Refused,
        "it does not open without the password"
    );
    assert_ne!(
        pdf_edit::info::lock(&store, b"open sesame"),
        pdf_edit::info::Lock::Refused
    );
    let without = refused(
        &mut window,
        "convert",
        &format!(
            r#"{{"document":"doc-1","tool":"pdf-to-text","files":["{}"]}}"#,
            protected.display()
        ),
    );
    assert!(without.contains("`password` option"), "{without}");
    let with = said(
        &mut window,
        "convert",
        &format!(
            r#"{{"document":"doc-1","tool":"pdf-to-text","files":["{}"],"options":{{"password":"open sesame"}}}}"#,
            protected.display()
        ),
    );
    assert!(with.contains("book-protected.txt"), "{with}");
    assert!(
        std::fs::read_to_string(folder.join("book-protected.txt"))
            .expect("the text")
            .contains("Secret anvils."),
        "beside the first file named, with its password"
    );
}

#[test]
fn a_conversion_the_person_stops_is_answered_and_leaves_nothing_in_flight() {
    let (mut window, _) = a_document_beside("stopped", "Words to be stopped.");
    let call = tool(
        "t",
        "convert",
        r#"{"document":"doc-1","tool":"pdf-to-text"}"#,
    );
    window.ai.tools.take(std::slice::from_ref(&call));
    assert!(matches!(window.perform(&call), Performed::Waiting));
    assert!(window.ai.tools.files_in_flight());
    assert!(
        window.ai.tools.stop().is_none(),
        "the stop waits for the work it cannot take back"
    );
    pump(&mut window);
    assert!(window.ai.tools.queue.is_empty());
    let answered = window
        .ai
        .tools
        .results
        .pop()
        .expect("the call was answered");
    assert!(
        answered.is_error || answered.text.contains("Made"),
        "either it was stopped or it had just finished: {}",
        answered.text
    );
    assert!(!window.ai.tools.files_in_flight());
}

#[test]
fn taking_pages_out_splitting_and_saving_pictures_write_new_files_beside_the_document() {
    let (mut window, folder) = a_document_beside("taking", "Page one words.");
    said(
        &mut window,
        "add_blank_page",
        r#"{"document":"doc-1","after_page":1}"#,
    );
    said(
        &mut window,
        "add_blank_page",
        r#"{"document":"doc-1","after_page":2}"#,
    );
    let steps = window.ai.tools.run.steps();
    let out = said(
        &mut window,
        "extract_pages",
        r#"{"document":"doc-1","pages":"1, 3"}"#,
    );
    assert!(
        out.contains("Took out 2 pages (pages 1, 3)") && out.contains("book-2pages.pdf"),
        "{out}"
    );
    let copy = std::fs::read(folder.join("book-2pages.pdf")).expect("the file");
    let store = pdf_bytes::ByteStore::new(pdf_bytes::SourceId::new(2), copy);
    assert_eq!(
        pdf_session::Session::new(store, b"")
            .page_count()
            .expect("pages"),
        2
    );
    let split = said(
        &mut window,
        "split_document",
        r#"{"document":"doc-1","every":2}"#,
    );
    assert!(split.contains("Split the document into 2 files"), "{split}");
    assert!(
        folder.join("book").join("book-1.pdf").exists()
            && folder.join("book").join("book-2.pdf").exists()
    );
    let pictures = said(
        &mut window,
        "export_page_pictures",
        r#"{"document":"doc-1","pages":"1-2","dpi":30}"#,
    );
    assert!(pictures.contains("as PNG pictures at 30 dpi"), "{pictures}");
    assert!(
        folder.join("book-2").join("book-p1.png").exists(),
        "a second folder, the first untouched: {pictures}"
    );
    assert_eq!(
        window.ai.tools.run.steps(),
        steps,
        "files written are not steps of the document"
    );
    let beyond = refused(
        &mut window,
        "extract_pages",
        r#"{"document":"doc-1","pages":"9"}"#,
    );
    assert!(
        beyond.contains("there is no page 9: the document has 3"),
        "{beyond}"
    );
    let once = refused(
        &mut window,
        "split_document",
        r#"{"document":"doc-1","every":9}"#,
    );
    assert!(once.contains("1 file, not several"), "{once}");
}

#[test]
fn a_copy_is_written_where_asked_or_beside_the_original_and_never_over_a_file() {
    let (mut window, folder) = a_document_beside("copy", "Words to copy.");
    let beside = said(&mut window, "save_copy", r#"{"document":"doc-1"}"#);
    assert!(beside.contains("book-edited.pdf"), "{beside}");
    assert!(folder.join("book-edited.pdf").exists());
    let again = said(&mut window, "save_copy", r#"{"document":"doc-1"}"#);
    assert!(
        again.contains("book-edited-2.pdf"),
        "numbered, not written over: {again}"
    );
    let wanted = folder.join("elsewhere.pdf");
    let named = said(
        &mut window,
        "save_copy",
        &format!(r#"{{"document":"doc-1","path":"{}"}}"#, wanted.display()),
    );
    assert!(
        named.contains("elsewhere.pdf") && wanted.exists(),
        "{named}"
    );
    let kept = std::fs::read(&wanted).expect("the copy");
    let over = refused(
        &mut window,
        "save_copy",
        &format!(r#"{{"document":"doc-1","path":"{}"}}"#, wanted.display()),
    );
    assert!(over.contains("No copy was written"), "{over}");
    assert_eq!(
        std::fs::read(&wanted).expect("still there"),
        kept,
        "never written over"
    );
    let original_path = window.opened.display().to_string();
    let original = refused(
        &mut window,
        "save_copy",
        &format!(r#"{{"document":"doc-1","path":"{original_path}"}}"#),
    );
    assert!(original.contains("No copy was written"), "{original}");
    assert!(
        !window.opened.exists(),
        "the original was never written, or written over"
    );
    let bytes = std::fs::read(&wanted).expect("a copy");
    let store = pdf_bytes::ByteStore::new(pdf_bytes::SourceId::new(3), bytes);
    assert_eq!(
        pdf_session::Session::new(store, b"")
            .page_count()
            .expect("pages"),
        1,
        "a whole document"
    );
    assert!(
        window.editor.can_undo(),
        "the window's own history is as it was"
    );
}

#[test]
fn a_document_with_no_file_of_its_own_needs_a_path_for_its_copy() {
    let mut window = a_blank_window();
    let why = refused(&mut window, "save_copy", r#"{"document":"doc-1"}"#);
    assert!(why.contains("no file of its own yet"), "{why}");
    assert_eq!(
        window.the_place_for(&pdf_agent::tools::request::Request::SaveCopy { path: None }),
        None
    );
}

#[test]
fn the_card_names_the_folder_or_the_file_a_call_would_write_to() {
    use pdf_agent::tools::request::parse;
    let (window, folder) = a_document_beside("place", "Words.");
    let place = |name: &str, arguments: &str| {
        let arguments = pdf_agent::json::Json::parse(arguments).expect("JSON");
        window.the_place_for(&parse(name, &arguments).expect("reads"))
    };
    let here = Some(WrittenTo::Folder(folder.display().to_string()));
    assert_eq!(place("convert", r#"{"tool":"pdf-to-word"}"#), here);
    assert_eq!(place("extract_pages", r#"{"pages":"1"}"#), here);
    assert_eq!(place("split_document", r#"{"every":1}"#), here);
    assert_eq!(place("export_page_pictures", "{}"), here);
    assert_eq!(
        place(
            "convert",
            r#"{"tool":"pdf-to-text","files":["/elsewhere/other.pdf"]}"#
        ),
        Some(WrittenTo::Folder("/elsewhere".to_owned())),
        "beside the first file named"
    );
    assert_eq!(
        place("save_copy", r#"{"path":"/tmp/x/copy.pdf"}"#),
        Some(WrittenTo::File("/tmp/x/copy.pdf".to_owned()))
    );
    assert_eq!(
        place("save_copy", "{}"),
        Some(WrittenTo::File(
            folder.join("book-edited.pdf").display().to_string()
        ))
    );
    assert_eq!(
        place("find_and_replace", r#"{"find":"a","replace_with":"b"}"#),
        None
    );
}

#[test]
fn a_pdf_made_is_opened_in_the_window_only_when_asked_and_when_no_other_call_waits() {
    let (mut window, folder) = a_document_beside("open-result", "Words to compress.");
    let text = said(
        &mut window,
        "convert",
        r#"{"document":"doc-1","tool":"compress-pdf","open_result":true}"#,
    );
    assert!(text.contains("The window is opening it now"), "{text}");
    let made = folder.join("book-compressed.pdf");
    assert!(
        matches!(&window.leaving, Some(Leaving::Open(path, 0)) if *path == made),
        "the document has changes, so the person is asked first: {:?}",
        window.leaving.is_some()
    );
    window.leaving = None;
    let (mut window, folder) = a_document_beside("open-result-shared", "Words to compress.");
    let first = tool(
        "a",
        "convert",
        r#"{"document":"doc-1","tool":"compress-pdf","open_result":true}"#,
    );
    let second = tool("b", "go_to_page", r#"{"document":"doc-1","page":1}"#);
    window.ai.tools.take(&[first.clone(), second]);
    window.ai.tools.revision_before = window.editor.revision();
    assert!(matches!(window.perform(&first), Performed::Waiting));
    pump(&mut window);
    let answered = window.ai.tools.results.pop().expect("answered");
    assert!(
        answered
            .text
            .contains("It was not opened in the window, because other calls"),
        "{}",
        answered.text
    );
    assert!(window.leaving.is_none());
    assert!(folder.join("book-compressed.pdf").exists());
    let (mut window, _) = a_document_beside("open-result-not", "Words to compress.");
    let plain = said(
        &mut window,
        "convert",
        r#"{"document":"doc-1","tool":"compress-pdf"}"#,
    );
    assert!(
        !plain.contains("opening it") && window.leaving.is_none(),
        "{plain}"
    );
}

#[test]
fn a_new_chat_or_a_dropped_queue_lets_go_of_work_in_flight() {
    let (mut window, _) = a_document_beside("dropped", "Words.");
    let call = tool(
        "t",
        "convert",
        r#"{"document":"doc-1","tool":"pdf-to-text"}"#,
    );
    window.ai.tools.take(std::slice::from_ref(&call));
    assert!(matches!(window.perform(&call), Performed::Waiting));
    assert!(window.ai.tools.files_in_flight());
    window.ai.tools.drop_the_queue();
    assert!(!window.ai.tools.files_in_flight() && window.ai.tools.queue.is_empty());
}
