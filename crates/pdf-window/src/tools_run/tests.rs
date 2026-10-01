use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use pdf_app::wording::Lang;
use pdf_convert::run::{Failure, Outcome, Progress, is_inside};
use pdf_convert::{Setting, Tool, Value, Values};

use super::{Order, Origin, Poll, Saved, begin, progress_line, save_beside, write_a_copy};

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

struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("panpdf-tools-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn finished(mut working: super::Working) -> Result<Outcome, Failure> {
    let until = Instant::now() + Duration::from_secs(120);
    loop {
        if let Poll::Finished(result) = working.poll() {
            return result;
        }
        assert!(Instant::now() < until, "the job did not finish");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn line(progress: Progress, fraction: Option<f64>, files: usize, pictures: bool) -> String {
    progress_line(progress, fraction, (files, pictures), Lang::English)
}

#[test]
fn a_job_that_has_not_begun_says_so() {
    assert_eq!(
        line(Progress::Starting, Some(0.0), 1, false),
        "Starting\u{2026}"
    );
    assert_eq!(
        line(Progress::Writing, Some(1.0), 3, false),
        "Writing the result\u{2026}"
    );
    assert_eq!(
        line(Progress::Busy, None, 3, false),
        "Working on it\u{2026}"
    );
}

#[test]
fn one_file_shows_its_page_and_how_far_it_is() {
    assert_eq!(
        line(Progress::Step { done: 4, total: 10 }, Some(0.4), 1, false),
        "Page 4 of 10 \u{00b7} 40%"
    );
    assert_eq!(
        line(
            Progress::Step { done: 2, total: 3 },
            Some(2.0 / 3.0),
            1,
            true
        ),
        "Picture 2 of 3 \u{00b7} 67%"
    );
}

#[test]
#[expect(clippy::cast_precision_loss, reason = "a few files and a few pages")]
fn several_files_show_which_one_is_being_read_as_the_website_does() {
    let step = |index: usize, done: usize, total: usize| {
        let fraction = (index as f64 + done as f64 / total as f64) / 3.0;
        line(Progress::Step { done, total }, Some(fraction), 3, false)
    };
    assert_eq!(
        step(0, 1, 2),
        "File 1 of 3 \u{00b7} Page 1 of 2 \u{00b7} 17%"
    );
    assert_eq!(
        step(1, 0, 4),
        "File 2 of 3 \u{00b7} Page 0 of 4 \u{00b7} 33%"
    );
    assert_eq!(
        step(1, 4, 4),
        "File 2 of 3 \u{00b7} Page 4 of 4 \u{00b7} 67%"
    );
    assert_eq!(
        step(2, 10, 10),
        "File 3 of 3 \u{00b7} Page 10 of 10 \u{00b7} 100%"
    );
    assert_eq!(
        line(
            Progress::File { index: 1, of: 3 },
            Some(1.0 / 3.0),
            3,
            false
        ),
        "File 2 of 3 \u{00b7} 33%"
    );
}

#[test]
fn a_file_the_tool_names_cannot_climb_out_of_the_folder_it_is_written_in() {
    for fine in ["a.docx", "folder/a.png", "a_files/image1.png"] {
        assert!(is_inside(fine), "{fine}");
    }
    for bad in ["", "../a.docx", "/etc/passwd", "a/../../b", "..\\a", "a\\b"] {
        assert!(!is_inside(bad), "{bad:?}");
    }
}

fn outcome(files: &[(&str, &[u8])]) -> Outcome {
    Outcome {
        files: files
            .iter()
            .map(|(name, bytes)| ((*name).to_owned(), bytes.to_vec()))
            .collect(),
        notes: Vec::new(),
    }
}

#[test]
fn one_file_is_written_beside_the_original_and_never_over_what_is_there() {
    let folder = Folder::new("beside");
    let original = folder.path().join("report.pdf");
    std::fs::write(&original, b"%PDF").unwrap();
    let made = outcome(&[("report.docx", b"word")]);
    let saved = save_beside(
        Tool::PdfToWord,
        &made,
        &["report.pdf"],
        (&original, folder.path()),
    )
    .unwrap();
    assert_eq!(saved.files, vec![folder.path().join("report.docx")]);
    assert!(!saved.tucked_in);
    assert_eq!(std::fs::read(&saved.files[0]).unwrap(), b"word");
    let again = save_beside(
        Tool::PdfToWord,
        &outcome(&[("report.docx", b"new")]),
        &["report.pdf"],
        (&original, folder.path()),
    )
    .unwrap();
    assert_eq!(again.files, vec![folder.path().join("report-2.docx")]);
    assert_eq!(
        std::fs::read(folder.path().join("report.docx")).unwrap(),
        b"word",
        "the first result is untouched"
    );
    assert_eq!(std::fs::read(&original).unwrap(), b"%PDF");
}

#[test]
fn several_files_go_into_a_folder_of_their_own_with_their_own_folders_kept() {
    let folder = Folder::new("several");
    let made = outcome(&[
        ("report.md", b"# hello"),
        ("report_files/image1.png", b"one"),
        ("report_files/image2.png", b"two"),
    ]);
    let original = folder.path().join("report.pdf");
    let saved = save_beside(
        Tool::PdfToMarkdown,
        &made,
        &["report.pdf"],
        (&original, folder.path()),
    )
    .unwrap();
    assert!(saved.tucked_in);
    assert_eq!(saved.folder, folder.path().join("report"));
    assert_eq!(saved.files.len(), 3);
    assert_eq!(
        std::fs::read(folder.path().join("report/report_files/image2.png")).unwrap(),
        b"two"
    );
    let second = save_beside(
        Tool::PdfToMarkdown,
        &made,
        &["report.pdf"],
        (&original, folder.path()),
    )
    .unwrap();
    assert_eq!(second.folder, folder.path().join("report-2"));
}

#[test]
fn a_comparison_is_written_under_the_name_of_the_older_file() {
    let folder = Folder::new("compare");
    let made = outcome(&[("comparison.html", b"<html>")]);
    let saved = save_beside(
        Tool::Compare,
        &made,
        &["v1.pdf", "v2.pdf"],
        (&folder.path().join("v1.pdf"), folder.path()),
    )
    .unwrap();
    assert_eq!(saved.files, vec![folder.path().join("v1-comparison.html")]);
}

#[test]
fn a_name_that_would_leave_the_folder_is_refused_before_anything_is_written() {
    let folder = Folder::new("refused");
    let made = outcome(&[("../escape.txt", b"x")]);
    let refused = save_beside(
        Tool::PdfToText,
        &made,
        &["a.pdf"],
        (Path::new(""), folder.path()),
    );
    assert!(refused.is_err());
    assert!(!folder.path().parent().unwrap().join("escape.txt").exists());
}

#[test]
fn a_copy_saved_by_name_does_not_overwrite_a_file_of_that_name() {
    let folder = Folder::new("copy");
    let target = folder.path().join("made.docx");
    let saved = write_a_copy(Path::new(""), &target, b"one").unwrap();
    assert_eq!(
        saved,
        Saved {
            folder: folder.path().to_path_buf(),
            files: vec![target.clone()],
            tucked_in: false,
        }
    );
    assert!(write_a_copy(Path::new(""), &target, b"two").is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"one");
}

#[test]
fn the_text_of_a_pdf_is_made_through_the_runner_the_window_uses() {
    let order = Order {
        tool: Tool::PdfToText,
        values: Values::new(),
        sources: vec![(
            "letter.pdf".to_owned(),
            Origin::Document(hand_made_pdf(&["Dear reader", "Yours truly"])),
        )],
    };
    let made = finished(begin(order)).unwrap();
    assert_eq!(made.files.len(), 1);
    assert_eq!(made.files[0].0, "letter.txt");
    let text = String::from_utf8_lossy(&made.files[0].1).into_owned();
    assert!(text.contains("Dear reader"), "{text}");
    assert!(text.contains("Yours truly"), "{text}");
}

#[test]
fn a_pdf_is_compressed_through_the_runner_the_window_uses_and_stays_a_pdf() {
    let original = hand_made_pdf(&["Page one", "Page two", "Page three"]);
    let order = Order {
        tool: Tool::Compress,
        values: Values::new(),
        sources: vec![("deed.pdf".to_owned(), Origin::Document(original))],
    };
    let made = finished(begin(order)).unwrap();
    assert_eq!(made.files[0].0, "deed-compressed.pdf");
    assert!(made.files[0].1.starts_with(b"%PDF"));
    assert!(!made.notes.is_empty());
}

#[test]
fn a_file_read_from_disk_is_converted_and_one_that_is_gone_is_a_plain_failure() {
    let folder = Folder::new("disk");
    let file = folder.path().join("scan.pdf");
    std::fs::write(&file, hand_made_pdf(&["Read me"])).unwrap();
    let order = |path: PathBuf| Order {
        tool: Tool::PdfToText,
        values: Values::new(),
        sources: vec![("scan.pdf".to_owned(), Origin::File(path))],
    };
    let made = finished(begin(order(file))).unwrap();
    assert!(String::from_utf8_lossy(&made.files[0].1).contains("Read me"));
    let gone = finished(begin(order(folder.path().join("missing.pdf"))));
    assert!(matches!(gone, Err(Failure::BadInput(_))), "{gone:?}");
}

#[test]
fn a_job_stopped_before_it_starts_is_cancelled_and_made_nothing() {
    let order = Order {
        tool: Tool::PdfToText,
        values: Values::new(),
        sources: vec![(
            "a.pdf".to_owned(),
            Origin::Document(hand_made_pdf(&["One"])),
        )],
    };
    let mut working = begin(order);
    working.stop();
    assert!(working.stopping);
    let result = finished(working);
    assert_eq!(result.unwrap_err(), Failure::Cancelled);
}

#[test]
fn a_protected_file_asks_for_its_password_and_opens_with_it() {
    let original = hand_made_pdf(&["Secret"]);
    let lock = Order {
        tool: Tool::Protect,
        values: Values::new().with(Setting::NewPassword, Value::Secret("sesame".to_owned())),
        sources: vec![("secret.pdf".to_owned(), Origin::Document(original))],
    };
    let locked = finished(begin(lock)).unwrap();
    let bytes = locked.files[0].1.clone();
    let unlock = |password: Option<&str>| {
        let mut values = Values::new();
        if let Some(password) = password {
            values.set(Setting::Password, Value::Secret(password.to_owned()));
        }
        finished(begin(Order {
            tool: Tool::Unlock,
            values,
            sources: vec![(
                "secret-protected.pdf".to_owned(),
                Origin::Document(bytes.clone()),
            )],
        }))
    };
    assert_eq!(unlock(None).unwrap_err(), Failure::NeedsPassword);
    assert_eq!(unlock(Some("wrong")).unwrap_err(), Failure::NeedsPassword);
    assert!(unlock(Some("sesame")).is_ok());
}
