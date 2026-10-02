use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;

use pdf_agent::connect::ToolCall;
use pdf_agent::tools::request::parse;
use pdf_edit::text_layer::{LayerWord, TextLayer};

use super::{Recognizing, Stock, the_languages};
use crate::ai_actions::Performed;
use crate::ai_actions::editing::tests::a_window_written;
use crate::ai_actions::tests::{land_what_was_sent, tool};
use crate::window_state::{OcrReading, PageRead, Window};

fn names(codes: &[&str]) -> Vec<String> {
    codes.iter().map(|code| (*code).to_owned()).collect()
}

#[test]
fn the_languages_asked_for_must_be_installed_and_in_one_place_or_the_call_says_which_is_not() {
    let (here, own, system, ticked) = (
        names(&["eng", "tha", "lao"]),
        names(&["tha", "lao"]),
        names(&["eng"]),
        names(&["eng"]),
    );
    let stock = Stock {
        here: &here,
        own: &own,
        system: &system,
        ticked: &ticked,
    };
    assert_eq!(
        the_languages(&[], &stock),
        Ok(names(&["eng"])),
        "the ones last used"
    );
    assert_eq!(
        the_languages(&names(&["tha", "lao"]), &stock),
        Ok(names(&["tha", "lao"]))
    );
    let missing = the_languages(&names(&["deu"]), &stock).expect_err("not installed");
    assert!(
        missing.contains("no model for deu")
            && missing.contains("these are installed: eng, tha, lao"),
        "{missing}"
    );
    let apart = the_languages(&names(&["eng", "tha"]), &stock).expect_err("in two places");
    assert!(
        apart.contains("all installed in one place") && apart.contains("eng"),
        "{apart}"
    );
    let nothing = Stock {
        here: &[],
        own: &[],
        system: &[],
        ticked: &[],
    };
    let none = the_languages(&[], &nothing).expect_err("none installed");
    assert!(none.contains("none is installed yet"), "{none}");
}

fn a_reading(window: &Window, words: &[(usize, &str)]) -> OcrReading {
    let read: BTreeMap<usize, PageRead> = words
        .iter()
        .map(|(page, text)| {
            (
                *page,
                PageRead::Read(pdf_ocr::Reading {
                    layer: TextLayer {
                        words: vec![LayerWord {
                            text: (*text).to_owned(),
                            frame: [72.0, 72.0, 160.0, 90.0],
                        }],
                    },
                    confidence: Some(88.0),
                }),
            )
        })
        .collect();
    let (_, answers) = channel();
    OcrReading {
        cancel: Arc::new(AtomicBool::new(false)),
        answers,
        workers: Vec::new(),
        pages: read.keys().copied().collect(),
        read,
        epoch: window.editor.epoch(),
    }
}

fn waiting_on(window: &mut Window, reading: OcrReading) -> ToolCall {
    let call = tool("ocr", "ocr_pages", r#"{"document":"doc-1"}"#);
    let request = parse("ocr_pages", &call.arguments).expect("reads");
    window.ai.tools.take(std::slice::from_ref(&call));
    window.ai.tools.revision_before = window.editor.revision();
    window.ai.tools.recognizing = Some(Recognizing {
        call: call.clone(),
        request,
        reading,
        languages: names(&["eng"]),
    });
    call
}

#[test]
fn what_the_recogniser_read_is_written_as_one_step_the_person_can_undo_and_the_call_says_so() {
    let mut window = a_window_written("A scanned page with words already.");
    let before = window.editor.revision();
    let reading = a_reading(&window, &[(0, "recognised")]);
    waiting_on(&mut window, reading);
    window.keep_recognizing();
    assert!(
        window.ai.tools.sent.is_some(),
        "the layer is on its way to the document"
    );
    land_what_was_sent(&mut window);
    let answered = window.ai.tools.results.pop().expect("answered");
    assert!(!answered.is_error, "{}", answered.text);
    assert!(
        answered
            .text
            .starts_with("Made page 1 searchable in eng (the recogniser was 88 % sure)")
            && answered.text.contains("undo takes back"),
        "{}",
        answered.text
    );
    assert_ne!(window.editor.revision(), before);
    assert!(
        matches!(
            window.editor.status(),
            pdf_app::wording::Message::Done(pdf_app::wording::Done::Recognized { pages: 1, .. })
        ),
        "{:?}",
        window.editor.status()
    );
    assert!(window.ai.tools.recognizing.is_none());
    assert_eq!(
        window.ai.tools.run.steps(),
        2,
        "the write and the layer: one step each, not one per word"
    );
    assert_eq!(
        crate::ai_actions::tests::undo_steps_left(&mut window),
        2,
        "the person's own undo walks the same two"
    );
}

#[test]
fn a_reading_that_found_nothing_or_failed_changes_nothing_and_says_why() {
    let mut window = a_window_written("Words.");
    let before = window.editor.revision();
    let (_, answers) = channel();
    let had_text = OcrReading {
        cancel: Arc::new(AtomicBool::new(false)),
        answers,
        workers: Vec::new(),
        pages: vec![0],
        read: BTreeMap::from([(0, PageRead::HadText)]),
        epoch: window.editor.epoch(),
    };
    waiting_on(&mut window, had_text);
    window.keep_recognizing();
    let said = window.ai.tools.results.pop().expect("answered");
    assert!(
        !said.is_error
            && said.text.contains("already has text")
            && said.text.contains("skip_pages_with_text: false"),
        "{}",
        said.text
    );
    let (_, answers) = channel();
    let broken = OcrReading {
        cancel: Arc::new(AtomicBool::new(false)),
        answers,
        workers: Vec::new(),
        pages: vec![0],
        read: BTreeMap::from([(0, PageRead::Failed("the engine stopped".to_owned()))]),
        epoch: window.editor.epoch(),
    };
    window.ai.tools.queue.clear();
    waiting_on(&mut window, broken);
    window.keep_recognizing();
    let failed = window.ai.tools.results.pop().expect("answered");
    assert!(
        failed.is_error && failed.text.contains("the engine stopped"),
        "{}",
        failed.text
    );
    assert_eq!(window.editor.revision(), before, "nothing was written");
    assert!(window.ai.tools.sent.is_none());
}

#[test]
fn a_document_that_changed_while_it_was_read_is_not_written_to() {
    let mut window = a_window_written("Words.");
    let mut reading = a_reading(&window, &[(0, "late")]);
    reading.epoch = window.editor.epoch().wrapping_add(1);
    let before = window.editor.revision();
    waiting_on(&mut window, reading);
    window.keep_recognizing();
    let answered = window.ai.tools.results.pop().expect("answered");
    assert!(
        answered.is_error && answered.text.contains("changed while it was being read"),
        "{}",
        answered.text
    );
    assert_eq!(window.editor.revision(), before);
}

#[test]
fn a_reading_the_person_stops_is_cancelled_and_answered_and_a_reading_still_going_is_left_alone() {
    let mut window = a_window_written("Words.");
    let still = a_reading(&window, &[(0, "x")]);
    let mut going = still;
    let (send, answers) = channel::<(usize, PageRead)>();
    going.answers = answers;
    going.pages = vec![0, 1];
    going.read.remove(&1);
    waiting_on(&mut window, going);
    window.keep_recognizing();
    assert!(
        window.ai.tools.recognizing.is_some() && window.ai.tools.results.is_empty(),
        "one page of two is read: it goes on"
    );
    assert!(
        window.ai.tools.stop().is_none(),
        "a stop waits for the reading to wind down"
    );
    let cancel = Arc::clone(
        &window
            .ai
            .tools
            .recognizing
            .as_ref()
            .expect("held")
            .reading
            .cancel,
    );
    assert!(
        cancel.load(Ordering::Relaxed),
        "the workers are told to stop"
    );
    window.keep_recognizing();
    let answered = window.ai.tools.results.pop().expect("answered");
    assert!(
        answered.is_error && answered.text.contains("The person stopped the reading"),
        "{}",
        answered.text
    );
    assert!(window.ai.tools.recognizing.is_none());
    drop(send);
}

#[test]
fn on_a_machine_without_the_recogniser_the_call_says_so_and_changes_nothing() {
    if pdf_ocr::Tesseract::locate().is_ok() {
        return;
    }
    let mut window = a_window_written("Words.");
    let before = window.editor.revision();
    let call = tool(
        "o",
        "ocr_pages",
        r#"{"document":"doc-1","languages":["eng"]}"#,
    );
    window.ai.tools.take(std::slice::from_ref(&call));
    let Performed::Done(result) = window.perform(&call) else {
        panic!("it answers at once");
    };
    assert!(
        result.is_error
            && result.text.contains("not installed")
            && result.text.contains("Nothing in the document changed"),
        "{}",
        result.text
    );
    assert_eq!(window.editor.revision(), before);
    assert!(window.ai.tools.recognizing.is_none());
    let bad = tool("p", "ocr_pages", r#"{"document":"doc-1","pages":"9"}"#);
    let Performed::Done(result) = window.perform(&bad) else {
        panic!("it answers at once");
    };
    assert!(
        result
            .text
            .contains("there is no page 9: the document has 1"),
        "{}",
        result.text
    );
}
