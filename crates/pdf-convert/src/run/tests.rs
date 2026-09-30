use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use super::random::{fresh_seed, from_the_hasher, owner_password};
use super::running::{Running, launch, panic_text};
use super::{Context, Failure, Input, Outcome, Progress, run, start};
use crate::catalogue::{Area, Choice, Setting, Tool, Value, Values};

fn hand_made_pdf(pages: &[&str]) -> Vec<u8> {
    let mut pdf: Vec<u8> = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    let mut object = |pdf: &mut Vec<u8>, body: &[u8]| {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", offsets.len()).as_bytes());
        pdf.extend_from_slice(body);
        pdf.extend_from_slice(b"\nendobj\n");
    };
    let kids: Vec<String> = (0..pages.len())
        .map(|at| format!("{} 0 R", 4 + 2 * at))
        .collect();
    object(&mut pdf, b"<< /Type /Catalog /Pages 2 0 R >>");
    object(
        &mut pdf,
        format!(
            "<< /Type /Pages /Kids [{}] /Count {} >>",
            kids.join(" "),
            pages.len()
        )
        .as_bytes(),
    );
    object(
        &mut pdf,
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
    );
    for (at, text) in pages.iter().enumerate() {
        object(
            &mut pdf,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents {} 0 R /Resources << /Font << /F1 3 0 R >> >> >>",
                5 + 2 * at
            )
            .as_bytes(),
        );
        let content = format!("BT /F1 24 Tf 72 700 Td ({text}) Tj ET");
        let mut body = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
        body.extend_from_slice(content.as_bytes());
        body.extend_from_slice(b"\nendstream");
        object(&mut pdf, &body);
    }
    let start = pdf.len();
    pdf.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes(),
    );
    for offset in &offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{start}\n%%EOF\n",
            offsets.len() + 1
        )
        .as_bytes(),
    );
    pdf
}

fn pdf_named(name: &str, pages: &[&str]) -> Input {
    Input::new(name, hand_made_pdf(pages))
}

fn run_plain(tool: Tool, inputs: Vec<Input>, values: &Values) -> Result<Outcome, Failure> {
    run(
        tool,
        inputs,
        values,
        &Context::default(),
        &mut |_| {},
        &AtomicBool::new(false),
    )
}

fn pages_in(pdf: &[u8], password: &[u8]) -> Result<usize, String> {
    let source = pdf_bytes::ByteStore::owning(pdf_bytes::SourceId::next_document(), pdf.to_vec());
    pdf_session::Session::new(source, password)
        .page_count()
        .map_err(|error| error.to_string())
}

fn page_sizes(pdf: &[u8]) -> Vec<(f64, f64)> {
    let source = pdf_bytes::ByteStore::owning(pdf_bytes::SourceId::next_document(), pdf.to_vec());
    pdf_session::Session::new(source, b"")
        .page_geometries()
        .unwrap()
        .iter()
        .map(pdf_content::PageGeometry::rotated_size)
        .collect()
}

fn text_of(outcome: &Outcome) -> String {
    String::from_utf8(outcome.files[0].1.clone()).unwrap()
}

fn holds(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|window| window == needle)
}

fn secret(text: &str) -> Value {
    Value::Secret(text.to_owned())
}

fn protected(name: &str, password: &str) -> Vec<u8> {
    let values = Values::new().with(Setting::NewPassword, secret(password));
    let made = run_plain(
        Tool::Protect,
        vec![pdf_named(name, &["Hello PanPDF"])],
        &values,
    )
    .unwrap();
    made.files[0].1.clone()
}

#[test]
fn a_page_of_text_comes_out_as_the_text_it_shows() {
    let hello = run_plain(
        Tool::PdfToText,
        vec![pdf_named("hello.pdf", &["Hello PanPDF"])],
        &Values::new(),
    )
    .unwrap();
    assert_eq!(hello.files.len(), 1);
    assert_eq!(hello.files[0].0, "hello.txt");
    assert!(
        text_of(&hello).contains("Hello PanPDF"),
        "{:?}",
        text_of(&hello)
    );
    let other = run_plain(
        Tool::PdfToText,
        vec![pdf_named("other.pdf", &["Goodbye"])],
        &Values::new(),
    )
    .unwrap();
    assert!(text_of(&other).contains("Goodbye"));
    assert!(!text_of(&other).contains("PanPDF"));
}

#[test]
fn only_the_pages_asked_for_are_read() {
    let values = Values::new().with(Setting::Pages, Value::Pages("2".into()));
    let out = run_plain(
        Tool::PdfToText,
        vec![pdf_named("three.pdf", &["First", "Second", "Third"])],
        &values,
    )
    .unwrap();
    let text = text_of(&out);
    assert!(text.contains("Second"), "{text:?}");
    assert!(
        !text.contains("First") && !text.contains("Third"),
        "{text:?}"
    );
    let all = run_plain(
        Tool::PdfToText,
        vec![pdf_named("three.pdf", &["First", "Second", "Third"])],
        &Values::new().with(Setting::Pages, Value::Pages("all".into())),
    )
    .unwrap();
    assert!(text_of(&all).contains("First") && text_of(&all).contains("Third"));
}

#[test]
fn a_page_the_file_does_not_have_is_bad_input() {
    let values = Values::new().with(Setting::Pages, Value::Pages("5".into()));
    let failure = run_plain(
        Tool::PdfToText,
        vec![pdf_named("one.pdf", &["Only"])],
        &values,
    )
    .unwrap_err();
    assert!(
        matches!(&failure, Failure::BadInput(why) if why.contains("page 5")),
        "{failure:?}"
    );
    let backwards = Values::new().with(Setting::Pages, Value::Pages("3-1".into()));
    let failure = run_plain(
        Tool::PdfToText,
        vec![pdf_named("one.pdf", &["Only"])],
        &backwards,
    )
    .unwrap_err();
    assert!(matches!(failure, Failure::BadInput(_)), "{failure:?}");
}

#[test]
fn a_page_of_text_comes_out_as_a_word_file() {
    let out = run_plain(
        Tool::PdfToWord,
        vec![pdf_named("letter.pdf", &["Hello PanPDF"])],
        &Values::new(),
    )
    .unwrap();
    assert_eq!(out.files.len(), 1);
    assert_eq!(out.files[0].0, "letter.docx");
    assert!(out.files[0].1.starts_with(b"PK"));
    assert!(holds(&out.files[0].1, b"word/document.xml"));
}

#[test]
fn a_page_of_text_comes_out_as_html_markdown_slides_and_a_workbook_each_by_its_own_tool() {
    for (tool, name, inside) in [
        (Tool::PdfToHtml, "hello.html", "Hello PanPDF"),
        (Tool::PdfToMarkdown, "hello.md", "Hello PanPDF"),
        (Tool::PdfToPowerPoint, "hello.pptx", "ppt/presentation.xml"),
        (Tool::PdfToExcel, "hello.xlsx", "xl/workbook.xml"),
        (Tool::PdfToWord, "hello.docx", "word/document.xml"),
    ] {
        let out = run_plain(
            tool,
            vec![pdf_named("hello.pdf", &["Hello PanPDF"])],
            &Values::new(),
        )
        .unwrap();
        assert_eq!(out.files[0].0, name, "{tool:?}");
        assert!(holds(&out.files[0].1, inside.as_bytes()), "{tool:?}");
    }
}

#[test]
fn two_files_of_the_same_name_do_not_overwrite_each_other() {
    let out = run_plain(
        Tool::PdfToText,
        vec![
            pdf_named("same.pdf", &["Alpha"]),
            pdf_named("dir/same.pdf", &["Beta"]),
            pdf_named("SAME.pdf", &["Gamma"]),
        ],
        &Values::new(),
    )
    .unwrap();
    let names: Vec<&str> = out.files.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["same.txt", "same-2.txt", "SAME-3.txt"]);
}

#[test]
fn a_file_that_fails_among_files_that_work_is_named_in_the_notes() {
    let out = run_plain(
        Tool::PdfToText,
        vec![
            pdf_named("good.pdf", &["Fine"]),
            Input::new("broken.pdf", b"this is not a pdf".to_vec()),
        ],
        &Values::new(),
    )
    .unwrap();
    assert_eq!(out.files.len(), 1);
    assert!(
        out.notes
            .iter()
            .any(|note| note.starts_with("broken.pdf: ")),
        "{:?}",
        out.notes
    );
}

#[test]
fn when_every_file_fails_the_failure_is_returned() {
    let failure = run_plain(
        Tool::PdfToText,
        vec![Input::new("broken.pdf", b"this is not a pdf".to_vec())],
        &Values::new(),
    )
    .unwrap_err();
    assert!(matches!(failure, Failure::BadInput(_)), "{failure:?}");
}

#[test]
fn a_protected_file_asks_for_its_password() {
    let locked = protected("hello.pdf", "open sesame");
    let asked = |tool: Tool, values: &Values| {
        run_plain(tool, vec![Input::new("locked.pdf", locked.clone())], values).unwrap_err()
    };
    for tool in [
        Tool::PdfToText,
        Tool::PdfToWord,
        Tool::PdfToImage,
        Tool::Compress,
        Tool::Repair,
        Tool::Unlock,
        Tool::PdfToPdfA,
        Tool::Redact,
        Tool::Sign,
    ] {
        let values = match tool {
            Tool::Sign => Values::new().with(
                Setting::Drawing,
                Value::Strokes(vec![vec![(0.1, 0.1), (0.9, 0.9)]]),
            ),
            Tool::Redact => Values::new().with(Setting::Search, Value::Terms(vec!["Hello".into()])),
            _ => Values::new(),
        };
        assert_eq!(asked(tool, &values), Failure::NeedsPassword, "{tool:?}");
    }
}

#[test]
fn a_wrong_password_asks_for_the_password_again() {
    let locked = protected("hello.pdf", "open sesame");
    for tool in [Tool::Unlock, Tool::PdfToText, Tool::Compress] {
        let values = Values::new().with(Setting::Password, secret("not it"));
        let failure = run_plain(
            tool,
            vec![Input::new("locked.pdf", locked.clone())],
            &values,
        )
        .unwrap_err();
        assert_eq!(failure, Failure::NeedsPassword, "{tool:?}");
    }
}

#[test]
fn protecting_then_unlocking_gives_the_pages_back() {
    let locked = protected("hello.pdf", "open sesame");
    assert!(holds(&locked, b"/Encrypt"));
    assert!(pages_in(&locked, b"").is_err());
    assert_eq!(pages_in(&locked, b"open sesame"), Ok(1));
    let values = Values::new().with(Setting::Password, secret("open sesame"));
    let opened = run_plain(
        Tool::Unlock,
        vec![Input::new("locked.pdf", locked.clone())],
        &values,
    )
    .unwrap();
    assert_eq!(opened.files.len(), 1);
    assert_eq!(opened.files[0].0, "locked-unlocked.pdf");
    assert!(opened.files[0].1.starts_with(b"%PDF"));
    assert!(!holds(&opened.files[0].1, b"/Encrypt"));
    assert_eq!(pages_in(&opened.files[0].1, b""), Ok(1));
    let text = run_plain(
        Tool::PdfToText,
        vec![Input::new("locked.pdf", locked)],
        &values,
    )
    .unwrap();
    assert!(text_of(&text).contains("Hello PanPDF"));
}

#[test]
fn a_password_with_spaces_at_its_edges_is_kept_as_typed() {
    let locked = protected("hello.pdf", "  spaced  ");
    let right = Values::new().with(Setting::Password, secret("  spaced  "));
    let opened = run_plain(
        Tool::Unlock,
        vec![Input::new("l.pdf", locked.clone())],
        &right,
    );
    assert!(opened.is_ok(), "{opened:?}");
    let trimmed = Values::new().with(Setting::Password, secret("spaced"));
    let refused = run_plain(Tool::Unlock, vec![Input::new("l.pdf", locked)], &trimmed);
    assert_eq!(refused.unwrap_err(), Failure::NeedsPassword);
}

#[test]
fn a_protected_file_can_be_protected_again_with_its_own_password_given() {
    let locked = protected("hello.pdf", "first");
    let values = Values::new()
        .with(Setting::FilePassword, secret("first"))
        .with(Setting::NewPassword, secret("second"));
    let again = run_plain(Tool::Protect, vec![Input::new("l.pdf", locked)], &values).unwrap();
    assert_eq!(pages_in(&again.files[0].1, b"second"), Ok(1));
    assert!(pages_in(&again.files[0].1, b"first").is_err());
}

#[test]
fn a_limit_without_an_owner_password_gets_a_random_one_that_binds() {
    let values = Values::new().with(Setting::Forbid, Value::Choices(vec![Choice::Copy]));
    let made = run_plain(
        Tool::Protect,
        vec![pdf_named("hello.pdf", &["Hello PanPDF"])],
        &values,
    )
    .unwrap();
    assert_eq!(pages_in(&made.files[0].1, b""), Ok(1));
    assert!(
        made.notes
            .iter()
            .any(|note| note.contains("made at random")),
        "{:?}",
        made.notes
    );
    assert!(
        !made
            .notes
            .iter()
            .any(|note| note.contains("permissions bind only")),
        "{:?}",
        made.notes
    );
}

#[test]
fn protecting_with_nothing_to_lock_it_with_is_bad_input() {
    let failure = run_plain(
        Tool::Protect,
        vec![pdf_named("hello.pdf", &["Hello PanPDF"])],
        &Values::new(),
    )
    .unwrap_err();
    assert!(matches!(failure, Failure::BadInput(_)), "{failure:?}");
}

#[test]
fn compressing_gives_a_pdf_that_still_opens() {
    let out = run_plain(
        Tool::Compress,
        vec![pdf_named("hello.pdf", &["Hello PanPDF", "Second page"])],
        &Values::new(),
    )
    .unwrap();
    assert_eq!(out.files.len(), 1);
    assert_eq!(out.files[0].0, "hello-compressed.pdf");
    assert!(out.files[0].1.starts_with(b"%PDF"));
    assert_eq!(pages_in(&out.files[0].1, b""), Ok(2));
    let low = run_plain(
        Tool::Compress,
        vec![pdf_named("hello.pdf", &["Hello PanPDF"])],
        &Values::new().with(Setting::Level, Value::Choice(Choice::Low)),
    )
    .unwrap();
    assert_eq!(pages_in(&low.files[0].1, b""), Ok(1));
}

#[test]
fn compressing_several_files_gives_one_file_each() {
    let out = run_plain(
        Tool::Compress,
        vec![pdf_named("a.pdf", &["A"]), pdf_named("b.pdf", &["B", "B2"])],
        &Values::new(),
    )
    .unwrap();
    let names: Vec<&str> = out.files.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["a-compressed.pdf", "b-compressed.pdf"]);
    assert_eq!(pages_in(&out.files[1].1, b""), Ok(2));
}

#[test]
fn a_protected_file_stays_protected_when_compressed() {
    let locked = protected("hello.pdf", "open sesame");
    let values = Values::new().with(Setting::Password, secret("open sesame"));
    let out = run_plain(
        Tool::Compress,
        vec![Input::new("locked.pdf", locked)],
        &values,
    )
    .unwrap();
    assert_eq!(pages_in(&out.files[0].1, b"open sesame"), Ok(1));
    assert!(pages_in(&out.files[0].1, b"").is_err());
}

fn small_pictures() -> Values {
    Values::new().with(Setting::Resolution, Value::Number(20.0))
}

#[test]
fn every_page_becomes_one_jpg() {
    let out = run_plain(
        Tool::PdfToImage,
        vec![pdf_named("two.pdf", &["First", "Second"])],
        &small_pictures(),
    )
    .unwrap();
    let names: Vec<&str> = out.files.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["two-1.jpg", "two-2.jpg"]);
    for (name, bytes) in &out.files {
        assert!(bytes.starts_with(&[0xFF, 0xD8, 0xFF]), "{name}");
    }
}

#[test]
fn a_picture_format_of_png_gives_png_files() {
    let values = small_pictures().with(Setting::PictureFormat, Value::Choice(Choice::Png));
    let out = run_plain(
        Tool::PdfToImage,
        vec![pdf_named("one.pdf", &["Only"])],
        &values,
    )
    .unwrap();
    assert_eq!(out.files[0].0, "one-1.png");
    assert!(out.files[0].1.starts_with(b"\x89PNG"));
}

#[test]
fn a_file_with_no_pictures_inside_makes_nothing_and_says_so() {
    let values = Values::new().with(Setting::WhatToTake, Value::Choice(Choice::PicturesInside));
    let out = run_plain(
        Tool::PdfToImage,
        vec![pdf_named("text.pdf", &["Only text"])],
        &values,
    )
    .unwrap();
    assert!(out.made_nothing());
    assert!(
        out.notes
            .iter()
            .any(|note| note.contains("no pictures were found")),
        "{:?}",
        out.notes
    );
}

#[test]
fn a_resolution_out_of_range_is_bad_input_before_anything_runs() {
    let values = Values::new().with(Setting::Resolution, Value::Number(5.0));
    let mut heard = Vec::new();
    let failure = run(
        Tool::PdfToImage,
        vec![pdf_named("one.pdf", &["Only"])],
        &values,
        &Context::default(),
        &mut |progress| heard.push(progress),
        &AtomicBool::new(false),
    )
    .unwrap_err();
    assert_eq!(
        failure,
        Failure::BadInput("dpi: 5 is not between 10 and 1200".to_owned())
    );
    assert_eq!(heard, [Progress::Starting]);
}

#[test]
fn a_cancel_set_before_the_start_gives_cancelled_and_does_nothing() {
    let mut heard = Vec::new();
    let failure = run(
        Tool::PdfToText,
        vec![pdf_named("one.pdf", &["Only"])],
        &Values::new(),
        &Context::default(),
        &mut |progress| heard.push(progress),
        &AtomicBool::new(true),
    )
    .unwrap_err();
    assert_eq!(failure, Failure::Cancelled);
    assert!(heard.is_empty(), "{heard:?}");
}

#[test]
fn a_cancel_between_pages_stops_the_pictures_there() {
    let cancel = AtomicBool::new(false);
    let mut steps = 0;
    let failure = run(
        Tool::PdfToImage,
        vec![pdf_named("three.pdf", &["A", "B", "C"])],
        &small_pictures(),
        &Context::default(),
        &mut |progress| {
            if let Progress::Step { done, .. } = progress {
                steps += 1;
                if done == 1 {
                    cancel.store(true, Ordering::Relaxed);
                }
            }
        },
        &cancel,
    )
    .unwrap_err();
    assert_eq!(failure, Failure::Cancelled);
    assert_eq!(steps, 2);
}

#[test]
fn a_cancel_between_pages_of_a_text_read_gives_cancelled() {
    let cancel = AtomicBool::new(false);
    let failure = run(
        Tool::PdfToText,
        vec![pdf_named("three.pdf", &["A", "B", "C"])],
        &Values::new(),
        &Context::default(),
        &mut |progress| {
            if matches!(progress, Progress::Step { done: 1, .. }) {
                cancel.store(true, Ordering::Relaxed);
            }
        },
        &cancel,
    )
    .unwrap_err();
    assert_eq!(failure, Failure::Cancelled);
}

#[test]
fn a_cancel_before_a_whole_file_tool_runs_discards_its_result() {
    let cancel = AtomicBool::new(false);
    let failure = run(
        Tool::Repair,
        vec![pdf_named("one.pdf", &["Only"])],
        &Values::new(),
        &Context::default(),
        &mut |progress| {
            if progress == Progress::Busy {
                cancel.store(true, Ordering::Relaxed);
            }
        },
        &cancel,
    )
    .unwrap_err();
    assert_eq!(failure, Failure::Cancelled);
}

#[test]
fn no_files_is_bad_input() {
    for tool in Tool::ALL {
        let failure = run_plain(tool, Vec::new(), &Values::new()).unwrap_err();
        assert_eq!(
            failure,
            Failure::BadInput("no file was given".to_owned()),
            "{tool:?}"
        );
    }
}

#[test]
fn the_wrong_number_of_files_is_bad_input_that_says_how_many() {
    let one = vec![pdf_named("a.pdf", &["A"])];
    let failure = run_plain(Tool::Compare, one.clone(), &Values::new()).unwrap_err();
    assert_eq!(
        failure,
        Failure::BadInput("compare-pdf takes exactly 2 files, and 1 was given".to_owned())
    );
    let two = vec![pdf_named("a.pdf", &["A"]), pdf_named("b.pdf", &["B"])];
    let failure = run_plain(Tool::Sign, two, &Values::new()).unwrap_err();
    assert_eq!(
        failure,
        Failure::BadInput("sign-pdf takes exactly 1 file, and 2 were given".to_owned())
    );
}

#[test]
fn a_progress_run_reports_each_file_and_page_and_then_the_writing() {
    let mut heard = Vec::new();
    run(
        Tool::PdfToText,
        vec![pdf_named("a.pdf", &["A", "B"]), pdf_named("b.pdf", &["C"])],
        &Values::new(),
        &Context::default(),
        &mut |progress| heard.push(progress),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(
        heard,
        [
            Progress::Starting,
            Progress::File { index: 0, of: 2 },
            Progress::Step { done: 1, total: 2 },
            Progress::Step { done: 2, total: 2 },
            Progress::Writing,
            Progress::File { index: 1, of: 2 },
            Progress::Step { done: 1, total: 1 },
            Progress::Writing,
        ]
    );
}

#[test]
fn a_running_job_reports_and_hands_over_its_result_once() {
    let running = start(
        Tool::PdfToText,
        vec![pdf_named("hello.pdf", &["Hello PanPDF"])],
        Values::new(),
        Context::default(),
    );
    let result = wait_for(&running);
    let outcome = result.unwrap();
    assert!(text_of(&outcome).contains("Hello PanPDF"));
    assert!(running.finished().is_none());
    assert_eq!(running.progress(), Progress::Writing);
    assert!(running.fraction().is_some_and(|f| (f - 1.0).abs() < 1e-9));
}

#[test]
fn a_running_job_that_cannot_work_ends_with_its_failure() {
    let running = start(
        Tool::PdfToText,
        Vec::new(),
        Values::new(),
        Context::default(),
    );
    let result = wait_for(&running);
    assert_eq!(
        result.unwrap_err(),
        Failure::BadInput("no file was given".to_owned())
    );
}

fn wait_for(running: &Running) -> Result<Outcome, Failure> {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(result) = running.finished() {
            return result;
        }
        assert!(Instant::now() < deadline, "the job never finished");
        thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn cancelling_a_running_job_reaches_the_worker_that_is_waiting_for_it() {
    let running = launch("waits", |_, cancel| {
        while !cancel.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(1));
        }
        Err(Failure::Cancelled)
    });
    assert!(running.finished().is_none());
    running.cancel();
    assert_eq!(wait_for(&running), Err(Failure::Cancelled));
}

#[test]
fn dropping_a_running_job_cancels_it() {
    let stopped = Arc::new(AtomicBool::new(false));
    let seen = Arc::clone(&stopped);
    let running = launch("waits", move |_, cancel| {
        while !cancel.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(1));
        }
        seen.store(true, Ordering::Relaxed);
        Err(Failure::Cancelled)
    });
    drop(running);
    let deadline = Instant::now() + Duration::from_secs(60);
    while !stopped.load(Ordering::Relaxed) {
        assert!(Instant::now() < deadline, "the worker was never told");
        thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn a_worker_that_panics_ends_the_job_with_the_reason_and_the_others_carry_on() {
    let broken = launch("panics", |_, _| panic!("the reader broke"));
    assert_eq!(
        wait_for(&broken),
        Err(Failure::Panicked("the reader broke".to_owned()))
    );
    let fine = start(
        Tool::PdfToText,
        vec![pdf_named("hello.pdf", &["Hello PanPDF"])],
        Values::new(),
        Context::default(),
    );
    assert!(wait_for(&fine).is_ok());
}

#[test]
fn a_running_job_shows_where_it_has_got_to_as_a_fraction() {
    let stop = Arc::new(AtomicBool::new(false));
    let hold = Arc::clone(&stop);
    let running = launch("halfway", move |progress, _| {
        progress(Progress::File { index: 1, of: 4 });
        progress(Progress::Step { done: 1, total: 2 });
        while !hold.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(1));
        }
        Ok(Outcome::default())
    });
    let deadline = Instant::now() + Duration::from_secs(60);
    while !matches!(running.progress(), Progress::Step { .. }) {
        assert!(Instant::now() < deadline, "no progress was ever reported");
        thread::sleep(Duration::from_millis(1));
    }
    let fraction = running.fraction().unwrap();
    assert!((fraction - 0.375).abs() < 1e-9, "{fraction}");
    stop.store(true, Ordering::Relaxed);
    assert!(wait_for(&running).unwrap().made_nothing());
}

#[test]
fn a_panic_is_reported_with_the_words_it_was_raised_with() {
    let text = std::panic::catch_unwind(|| panic!("the reader broke")).unwrap_err();
    assert_eq!(panic_text(text.as_ref()), "the reader broke");
    let owned = std::panic::catch_unwind(|| panic!("{}", String::from("formatted"))).unwrap_err();
    assert_eq!(panic_text(owned.as_ref()), "formatted");
    let odd = std::panic::catch_unwind(|| std::panic::panic_any(7_u8)).unwrap_err();
    assert_eq!(panic_text(odd.as_ref()), "no reason was given");
}

#[test]
fn a_seed_from_the_hasher_is_full_length_and_differs_each_time() {
    let one = from_the_hasher(1);
    let two = from_the_hasher(2);
    assert_eq!(one.len(), 64);
    assert_ne!(one, two);
    assert_ne!(one, from_the_hasher(1));
    assert!(one.iter().any(|byte| *byte != one[0]));
    let first_words: Vec<&[u8]> = one.chunks(8).collect();
    for (at, word) in first_words.iter().enumerate() {
        assert!(!first_words[at + 1..].contains(word), "word {at} repeats");
    }
}

#[test]
fn a_fresh_seed_is_never_one_byte_repeated_and_never_the_same_twice() {
    let one = fresh_seed();
    let two = fresh_seed();
    assert_ne!(one, two);
    assert!(pdf_seeds_it(&one));
    assert!(pdf_seeds_it(&from_the_hasher(9)));
    assert!(!pdf_seeds_it(&[7; 64]));
    assert!(!pdf_seeds_it(&one[..47]));
}

fn pdf_seeds_it(seed: &[u8]) -> bool {
    convert_pdfdoc::seed_random(seed).is_ok()
}

#[test]
fn an_owner_password_is_thirty_two_hex_digits_and_new_each_time() {
    let one = owner_password();
    let two = owner_password();
    assert_eq!(one.len(), 32);
    assert!(one.chars().all(|c| c.is_ascii_hexdigit()));
    assert_ne!(one, two);
}

#[test]
fn a_signature_drawn_with_strokes_is_placed_on_the_last_page() {
    let strokes = vec![vec![(0.1, 0.5), (0.4, 0.9), (0.6, 0.1), (0.9, 0.5)]];
    let values = Values::new().with(Setting::Drawing, Value::Strokes(strokes));
    let original = hand_made_pdf(&["First", "Second"]);
    let out = run_plain(
        Tool::Sign,
        vec![Input::new("contract.pdf", original.clone())],
        &values,
    )
    .unwrap();
    assert_eq!(out.files[0].0, "contract-signed.pdf");
    assert_eq!(pages_in(&out.files[0].1, b""), Ok(2));
    assert!(out.files[0].1.len() > original.len());
    assert!(
        out.notes.iter().any(|note| note.contains("signed 1 page")),
        "{:?}",
        out.notes
    );
}

#[test]
fn a_signature_goes_on_the_pages_asked_for_and_every_page_means_every_page() {
    let strokes = vec![vec![(0.1, 0.5), (0.4, 0.9), (0.6, 0.1), (0.9, 0.5)]];
    let signed = |pages: Option<&str>| {
        let mut values = Values::new().with(Setting::Drawing, Value::Strokes(strokes.clone()));
        if let Some(pages) = pages {
            values.set(Setting::SignPages, Value::Pages(pages.to_owned()));
        }
        run_plain(
            Tool::Sign,
            vec![pdf_named("c.pdf", &["One", "Two", "Three"])],
            &values,
        )
        .unwrap()
        .notes
        .join("|")
    };
    assert!(signed(None).contains("signed 1 page ("), "{}", signed(None));
    assert!(
        signed(Some("all")).contains("signed 3 pages"),
        "{}",
        signed(Some("all"))
    );
    assert!(signed(Some("ALL")).contains("signed 3 pages"));
    assert!(
        signed(Some("1")).contains("signed 1 page ("),
        "{}",
        signed(Some("1"))
    );
    assert!(
        signed(Some("1,3")).contains("signed 2 pages"),
        "{}",
        signed(Some("1,3"))
    );
    assert!(signed(Some("last")).contains("signed 1 page ("));
}

#[test]
fn a_signature_that_is_a_picture_is_read_for_its_size() {
    let picture = convert_raster::Image::filled(40, 20, 3, 30)
        .to_jpeg(80)
        .unwrap();
    let values = Values::new().with(
        Setting::SignaturePicture,
        Value::File {
            name: "me.jpg".into(),
            bytes: picture,
        },
    );
    let out = run_plain(Tool::Sign, vec![pdf_named("c.pdf", &["Text"])], &values).unwrap();
    assert_eq!(pages_in(&out.files[0].1, b""), Ok(1));
}

#[test]
fn signing_without_a_signature_says_what_is_missing() {
    let failure = run_plain(
        Tool::Sign,
        vec![pdf_named("c.pdf", &["Text"])],
        &Values::new(),
    )
    .unwrap_err();
    assert_eq!(
        failure,
        Failure::BadInput("draw the signature first".to_owned())
    );
    let typed = Values::new().with(Setting::Signature, Value::Choice(Choice::Typed));
    let failure = run_plain(Tool::Sign, vec![pdf_named("c.pdf", &["Text"])], &typed).unwrap_err();
    assert_eq!(
        failure,
        Failure::BadInput("type the name to sign with".to_owned())
    );
    let picture = Values::new().with(Setting::Signature, Value::Choice(Choice::FromPicture));
    let failure = run_plain(Tool::Sign, vec![pdf_named("c.pdf", &["Text"])], &picture).unwrap_err();
    assert_eq!(
        failure,
        Failure::BadInput("choose the picture of the signature".to_owned())
    );
}

#[test]
fn redacting_a_word_removes_it_from_the_text_and_not_just_from_sight() {
    let original = pdf_named("secret.pdf", &["Account 12345 belongs to Alice"]);
    let before = run_plain(Tool::PdfToText, vec![original.clone()], &Values::new()).unwrap();
    assert!(text_of(&before).contains("12345"));
    let values = Values::new().with(Setting::Search, Value::Terms(vec!["12345".into()]));
    let redacted = run_plain(Tool::Redact, vec![original], &values).unwrap();
    assert_eq!(redacted.files[0].0, "secret-redacted.pdf");
    let after = run_plain(
        Tool::PdfToText,
        vec![Input::new("redacted.pdf", redacted.files[0].1.clone())],
        &Values::new(),
    )
    .unwrap();
    let text = text_of(&after);
    assert!(!text.contains("12345"), "{text:?}");
    assert!(text.contains("Alice"), "{text:?}");
    assert!(
        redacted.notes.iter().any(|note| note.contains("1 matches")),
        "{:?}",
        redacted.notes
    );
}

#[test]
fn redacting_an_area_of_a_page_is_read_from_the_typed_area() {
    let area = Area {
        page: 1,
        x0: 60.0,
        y0: 690.0,
        x1: 560.0,
        y1: 730.0,
    };
    let values = Values::new().with(Setting::Areas, Value::Areas(vec![area]));
    let redacted = run_plain(
        Tool::Redact,
        vec![pdf_named("secret.pdf", &["Account 12345"])],
        &values,
    )
    .unwrap();
    let after = run_plain(
        Tool::PdfToText,
        vec![Input::new("r.pdf", redacted.files[0].1.clone())],
        &Values::new(),
    )
    .unwrap();
    assert!(!text_of(&after).contains("12345"), "{:?}", text_of(&after));
}

#[test]
fn comparing_two_files_names_the_words_that_changed() {
    let old = pdf_named("old.pdf", &["Hello PanPDF"]);
    let new = pdf_named("new.pdf", &["Hello World"]);
    let html = run_plain(
        Tool::Compare,
        vec![old.clone(), new.clone()],
        &Values::new(),
    )
    .unwrap();
    assert_eq!(html.files.len(), 1);
    assert_eq!(html.files[0].0, "comparison.html");
    let page = String::from_utf8(html.files[0].1.clone()).unwrap();
    assert!(page.contains("PanPDF") && page.contains("World"), "{page}");
    assert!(
        html.notes
            .iter()
            .any(|note| note.contains("1 pages compared, 1 differ")),
        "{:?}",
        html.notes
    );
    let text = run_plain(
        Tool::Compare,
        vec![old.clone(), new],
        &Values::new().with(Setting::ReportAs, Value::Choice(Choice::TextReport)),
    )
    .unwrap();
    assert_eq!(text.files[0].0, "comparison.txt");
    let same = run_plain(Tool::Compare, vec![old.clone(), old], &Values::new()).unwrap();
    assert!(
        same.notes.iter().any(|note| note.contains("0 differ")),
        "{:?}",
        same.notes
    );
}

#[test]
fn repairing_a_file_with_no_index_gives_a_clean_file_that_opens() {
    let whole = hand_made_pdf(&["Hello PanPDF"]);
    let cut = whole
        .windows(4)
        .position(|window| window == b"xref")
        .unwrap();
    let damaged = whole[..cut].to_vec();
    assert!(!holds(&damaged, b"startxref"));
    let out = run_plain(
        Tool::Repair,
        vec![Input::new("damaged.pdf", damaged)],
        &Values::new(),
    )
    .unwrap();
    assert_eq!(out.files[0].0, "damaged-repaired.pdf");
    assert!(holds(&out.files[0].1, b"startxref"));
    assert_eq!(pages_in(&out.files[0].1, b""), Ok(1));
    let text = run_plain(
        Tool::PdfToText,
        vec![Input::new("r.pdf", out.files[0].1.clone())],
        &Values::new(),
    )
    .unwrap();
    assert!(text_of(&text).contains("Hello PanPDF"));
}

#[test]
fn an_archive_copy_carries_the_archive_identification() {
    let out = run_plain(
        Tool::PdfToPdfA,
        vec![pdf_named("hello.pdf", &["Hello PanPDF"])],
        &Values::new(),
    )
    .unwrap();
    assert_eq!(out.files[0].0, "hello-pdfa.pdf");
    assert!(holds(&out.files[0].1, b"pdfaid:part"));
    assert_eq!(pages_in(&out.files[0].1, b""), Ok(1));
}

#[test]
fn pictures_become_the_pages_of_one_pdf_named_for_the_first() {
    let picture = |value: u8| {
        convert_raster::Image::filled(60, 40, 3, value)
            .to_jpeg(80)
            .unwrap()
    };
    let inputs = vec![
        Input::new("first.jpg", picture(40)),
        Input::new("second.jpg", picture(200)),
    ];
    let one = run_plain(Tool::ImageToPdf, inputs.clone(), &Values::new()).unwrap();
    assert_eq!(one.files.len(), 1);
    assert_eq!(one.files[0].0, "first.pdf");
    assert_eq!(pages_in(&one.files[0].1, b""), Ok(2));
    let apart = run_plain(
        Tool::ImageToPdf,
        inputs,
        &Values::new().with(Setting::Merge, Value::Flag(false)),
    )
    .unwrap();
    let names: Vec<&str> = apart.files.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["first.pdf", "second.pdf"]);
}

#[test]
fn the_page_size_of_a_picture_pdf_defaults_to_the_picture_as_the_website_does() {
    let picture = convert_raster::Image::filled(300, 100, 3, 90)
        .to_jpeg(80)
        .unwrap();
    let sized = |values: &Values| {
        let out = run_plain(
            Tool::ImageToPdf,
            vec![Input::new("wide.jpg", picture.clone())],
            values,
        )
        .unwrap();
        page_sizes(&out.files[0].1)
    };
    let same = sized(&Values::new());
    assert_eq!(same.len(), 1);
    assert!(same[0].0 > same[0].1 * 2.5, "{same:?}");
    assert!(same[0].0 < 400.0, "{same:?}");
    let a4 = sized(&Values::new().with(Setting::PageSize, Value::Choice(Choice::A4)));
    assert!(
        (a4[0].0 - 841.89).abs() < 0.5 && (a4[0].1 - 595.28).abs() < 0.5,
        "{a4:?}"
    );
    let letter = sized(&Values::new().with(Setting::PageSize, Value::Choice(Choice::Letter)));
    assert!(
        (letter[0].0 - 792.0).abs() < 0.5 && (letter[0].1 - 612.0).abs() < 0.5,
        "{letter:?}"
    );
    let upright = sized(
        &Values::new()
            .with(Setting::PageSize, Value::Choice(Choice::A4))
            .with(Setting::Orientation, Value::Choice(Choice::Portrait)),
    );
    assert!((upright[0].0 - 595.28).abs() < 0.5, "{upright:?}");
}

#[test]
fn a_photograph_becomes_a_scanned_page() {
    let photo = convert_raster::Image::filled(200, 300, 3, 180)
        .to_jpeg(80)
        .unwrap();
    let out = run_plain(
        Tool::ScanToPdf,
        vec![Input::new("photo.jpg", photo)],
        &Values::new(),
    )
    .unwrap();
    assert_eq!(out.files[0].0, "photo.pdf");
    assert_eq!(pages_in(&out.files[0].1, b""), Ok(1));
}

#[test]
fn a_web_page_cannot_be_laid_out_without_fonts_and_says_so() {
    let page = Input::new(
        "page.html",
        b"<html><body><p>Hello</p></body></html>".to_vec(),
    );
    let failure = run_plain(Tool::HtmlToPdf, vec![page], &Values::new()).unwrap_err();
    assert!(
        matches!(&failure, Failure::Refused(why) if why.contains("no fonts")),
        "{failure:?}"
    );
    let asset = Input::new("logo.png", vec![1, 2, 3]);
    let failure = run_plain(Tool::HtmlToPdf, vec![asset], &Values::new()).unwrap_err();
    assert!(matches!(failure, Failure::BadInput(_)), "{failure:?}");
}

#[test]
fn a_word_file_that_is_not_one_is_bad_input_that_says_so() {
    let notes = Input::new("notes.docx", b"just some words".to_vec());
    let failure = run_plain(Tool::WordToPdf, vec![notes], &Values::new()).unwrap_err();
    assert!(
        matches!(&failure, Failure::BadInput(why) if why.contains("notes.docx: not a Word document")),
        "{failure:?}"
    );
}

#[test]
fn a_workbook_and_a_deck_come_out_as_pdf_pages_each_by_its_own_tool() {
    let workbook = include_bytes!("../../../../convert/tools/excel-to-pdf/tests/data/rtl.xlsx");
    let deck = include_bytes!("../../../../convert/tools/powerpoint-to-pdf/tests/data/rtl.pptx");
    let sheets = run_plain(
        Tool::ExcelToPdf,
        vec![Input::new("dir/book.xlsx", workbook.to_vec())],
        &Values::new(),
    )
    .unwrap();
    assert_eq!(sheets.files[0].0, "book.pdf");
    assert!(pages_in(&sheets.files[0].1, b"").is_ok_and(|pages| pages >= 1));
    assert!(
        sheets
            .notes
            .iter()
            .any(|note| note.contains("no fonts were given")),
        "{:?}",
        sheets.notes
    );
    let slides = run_plain(
        Tool::PowerPointToPdf,
        vec![Input::new("deck.pptx", deck.to_vec())],
        &Values::new(),
    )
    .unwrap();
    assert_eq!(slides.files[0].0, "deck.pdf");
    assert!(pages_in(&slides.files[0].1, b"").is_ok_and(|pages| pages >= 1));
    let crossed = run_plain(
        Tool::PowerPointToPdf,
        vec![Input::new("book.xlsx", workbook.to_vec())],
        &Values::new(),
    );
    assert!(crossed.is_err(), "a workbook is not a deck: {crossed:?}");
}

#[test]
fn a_workbook_prints_by_the_choices_of_fit_paper_and_orientation() {
    let workbook = include_bytes!("../../../../convert/tools/excel-to-pdf/tests/data/rtl.xlsx");
    let printed = |values: &Values| {
        let out = run_plain(
            Tool::ExcelToPdf,
            vec![Input::new("book.xlsx", workbook.to_vec())],
            values,
        )
        .unwrap();
        page_sizes(&out.files[0].1)
    };
    let letter = printed(
        &Values::new()
            .with(Setting::Paper, Value::Choice(Choice::Letter))
            .with(Setting::SheetOrientation, Value::Choice(Choice::Portrait)),
    );
    let a4 = printed(
        &Values::new()
            .with(Setting::Paper, Value::Choice(Choice::A4))
            .with(Setting::SheetOrientation, Value::Choice(Choice::Portrait)),
    );
    assert!(
        (letter[0].0 - 612.0).abs() < 1.0 && (letter[0].1 - 792.0).abs() < 1.0,
        "{letter:?}"
    );
    assert!(
        (a4[0].0 - 595.0).abs() < 1.0 && (a4[0].1 - 842.0).abs() < 1.0,
        "{a4:?}"
    );
    let sideways = printed(
        &Values::new()
            .with(Setting::Paper, Value::Choice(Choice::A4))
            .with(Setting::SheetOrientation, Value::Choice(Choice::Landscape)),
    );
    assert!((sideways[0].0 - 842.0).abs() < 1.0, "{sideways:?}");
}

#[test]
fn a_page_that_already_has_text_is_not_read_by_ocr_and_a_stray_name_changes_nothing() {
    for name in ["scan.pdf", "page-1.txt", "words.tsv", "scan"] {
        let failure = run_plain(
            Tool::Ocr,
            vec![pdf_named(name, &["Already text"])],
            &Values::new(),
        )
        .unwrap_err();
        let reason = failure.to_string();
        assert!(
            matches!(failure, Failure::Refused(_))
                && (reason.contains("Tesseract") || reason.contains("already had text")),
            "{name}: {reason}"
        );
    }
}

#[test]
fn a_text_setting_reaches_the_engine_as_the_text_typed() {
    let values = Values::new().with(Setting::Languages, Value::Text(String::new()));
    let pairs = super::encode::pairs(Tool::Ocr, &values);
    assert!(
        pairs.contains(&("languages", "eng".to_owned())),
        "{pairs:?}"
    );
    let two = Values::new().with(Setting::Languages, Value::Text("lao+eng".into()));
    let pairs = super::encode::pairs(Tool::Ocr, &two);
    assert!(
        pairs.contains(&("languages", "lao+eng".to_owned())),
        "{pairs:?}"
    );
}

#[test]
fn flags_and_numbers_are_sent_as_the_engine_reads_them() {
    let values = Values::new()
        .with(Setting::Resolution, Value::Number(300.0))
        .with(Setting::Pages, Value::Pages(" 1,3-5 ".into()));
    let pairs = super::encode::pairs(Tool::PdfToImage, &values);
    let get = |key: &str| {
        pairs
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.as_str())
    };
    assert_eq!(get("dpi"), Some("300"));
    assert_eq!(get("pages"), Some("1,3-5"));
    assert_eq!(get("mode"), Some("pages"));
    assert_eq!(get("format"), Some("jpg"));
    assert_eq!(get("quality"), Some("85"));
    assert_eq!(get("password"), None);
    let merge = super::encode::pairs(Tool::ImageToPdf, &Values::new());
    let get = |key: &str| {
        merge
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.as_str())
    };
    assert_eq!(get("merge"), Some("true"));
    assert_eq!(get("size"), Some("fit"));
    assert_eq!(get("orientation"), Some("auto"));
    assert_eq!(get("margin"), Some("none"));
}

#[test]
fn a_setting_that_waits_on_another_is_left_out_when_the_other_says_no() {
    let values = Values::new().with(Setting::WhatToTake, Value::Choice(Choice::PicturesInside));
    let pairs = super::encode::pairs(Tool::PdfToImage, &values);
    assert!(pairs.iter().all(|(key, _)| *key != "dpi"), "{pairs:?}");
    let png = values.with(Setting::PictureFormat, Value::Choice(Choice::Png));
    let pairs = super::encode::pairs(Tool::PdfToImage, &png);
    assert!(pairs.iter().all(|(key, _)| *key != "quality"), "{pairs:?}");
}

#[test]
fn the_workbook_choices_that_mean_as_the_workbook_are_not_sent() {
    let pairs = super::encode::pairs(Tool::ExcelToPdf, &Values::new());
    assert_eq!(pairs, [("grid", "false".to_owned())]);
    let values = Values::new()
        .with(Setting::Fit, Value::Choice(Choice::ColumnsOnOnePage))
        .with(Setting::Paper, Value::Choice(Choice::Letter))
        .with(Setting::Grid, Value::Flag(true))
        .with(Setting::SheetOrientation, Value::Choice(Choice::Landscape));
    let pairs = super::encode::pairs(Tool::ExcelToPdf, &values);
    assert_eq!(
        pairs,
        [
            ("fit", "width".to_owned()),
            ("paper", "letter".to_owned()),
            ("grid", "true".to_owned()),
            ("orientation", "landscape".to_owned()),
        ]
    );
}

#[test]
fn search_words_areas_and_strokes_are_written_the_way_the_tools_read_them() {
    let values = Values::new()
        .with(
            Setting::Search,
            Value::Terms(vec!["Smith, John".into(), " a|b ".into(), String::new()]),
        )
        .with(
            Setting::Areas,
            Value::Areas(vec![
                Area {
                    page: 2,
                    x0: 1.0,
                    y0: 2.5,
                    x1: 30.0,
                    y1: 40.0,
                },
                Area {
                    page: 1,
                    x0: 0.0,
                    y0: 0.0,
                    x1: 5.0,
                    y1: 6.0,
                },
            ]),
        );
    let pairs = super::encode::pairs(Tool::Redact, &values);
    let get = |key: &str| {
        pairs
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.as_str())
    };
    assert_eq!(get("search"), Some("Smith, John|a|b"));
    assert_eq!(get("areas"), Some("2:1,2.5,30,40; 1:0,0,5,6"));
    assert_eq!(get("case"), Some("false"));
    assert_eq!(get("color"), Some("0,0,0"));
    let strokes = Values::new().with(
        Setting::Drawing,
        Value::Strokes(vec![
            vec![(0.0, 0.5), (1.0, 0.25)],
            Vec::new(),
            vec![(0.5, 0.5)],
        ]),
    );
    let signed = super::encode::pairs(Tool::Sign, &strokes);
    let get = |key: &str| {
        signed
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.as_str())
    };
    assert_eq!(get("draw"), Some("0,0.5 1,0.25; 0.5,0.5"));
    assert_eq!(get("pages"), Some("last"));
    assert_eq!(get("position"), Some("bottom-right"));
    assert_eq!(get("width"), Some("160"));
    assert_eq!(get("how"), None);
}

#[test]
fn the_engines_own_words_are_told_apart_by_what_they_say() {
    use super::failure::from_engine;
    assert_eq!(from_engine("cancelled"), Failure::Cancelled);
    assert_eq!(
        from_engine("that password does not open the file"),
        Failure::NeedsPassword
    );
    assert_eq!(
        from_engine(
            "the file cannot be read as a PDF: password does not authenticate this document"
        ),
        Failure::NeedsPassword
    );
    assert_eq!(
        from_engine("the password is wrong, or the file needs one"),
        Failure::NeedsPassword
    );
    assert_eq!(
        from_engine("the file needs its password to be opened: give it with --password"),
        Failure::NeedsPassword
    );
    assert_eq!(
        from_engine("give a password (--password), or an owner password"),
        Failure::Refused("give a password (--password), or an owner password".to_owned())
    );
    assert!(matches!(
        from_engine("page 9 was asked for and the file has 2 pages"),
        Failure::BadInput(_)
    ));
    assert!(matches!(
        from_engine("no fonts were given"),
        Failure::Refused(_)
    ));
}

fn packaged_fonts() -> Context {
    let dir =
        std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fonts/packaged"));
    let provider = pdf_content::SystemFontProvider::discover_in(&[dir]);
    Context {
        fonts: Some(std::sync::Arc::new(provider)),
    }
}

#[test]
fn a_web_page_is_laid_out_with_the_fonts_the_context_hands_over() {
    let page = Input::new(
        "page.html",
        b"<html><body><h1>Hello</h1><p>Written in DejaVu</p></body></html>".to_vec(),
    );
    let out = run(
        Tool::HtmlToPdf,
        vec![page],
        &Values::new(),
        &packaged_fonts(),
        &mut |_| {},
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(out.files[0].0, "page.pdf");
    assert_eq!(pages_in(&out.files[0].1, b""), Ok(1));
    let text = run_plain(
        Tool::PdfToText,
        vec![Input::new("page.pdf", out.files[0].1.clone())],
        &Values::new(),
    )
    .unwrap();
    assert!(
        text_of(&text).contains("Written in"),
        "{:?}",
        text_of(&text)
    );
}

#[test]
fn a_page_needs_only_the_assets_it_is_given_and_never_reads_the_disk() {
    let page = Input::new(
        "page.html",
        b"<html><body><p>Hello</p><img src=\"/etc/hostname\"><img src=\"../secret.png\"></body></html>"
            .to_vec(),
    );
    let out = run(
        Tool::HtmlToPdf,
        vec![page],
        &Values::new(),
        &packaged_fonts(),
        &mut |_| {},
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(pages_in(&out.files[0].1, b""), Ok(1));
    assert!(
        out.notes.iter().any(|note| note.contains("/etc/hostname")),
        "{:?}",
        out.notes
    );
    assert!(
        out.notes.iter().any(|note| note.contains("secret.png")),
        "{:?}",
        out.notes
    );
}

#[test]
fn a_typed_signature_is_drawn_with_the_fonts_the_context_hands_over() {
    let values = Values::new()
        .with(Setting::Signature, Value::Choice(Choice::Typed))
        .with(Setting::TypedName, Value::Text("Alice Example".into()))
        .with(Setting::TypeFace, Value::Text("DejaVu Sans".into()));
    let original = hand_made_pdf(&["Contract"]);
    let out = run(
        Tool::Sign,
        vec![Input::new("contract.pdf", original.clone())],
        &values,
        &packaged_fonts(),
        &mut |_| {},
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(pages_in(&out.files[0].1, b""), Ok(1));
    let text = run_plain(
        Tool::PdfToText,
        vec![Input::new("signed.pdf", out.files[0].1.clone())],
        &Values::new(),
    )
    .unwrap();
    assert!(
        text_of(&text).contains("Alice Example"),
        "{:?}",
        text_of(&text)
    );
}
