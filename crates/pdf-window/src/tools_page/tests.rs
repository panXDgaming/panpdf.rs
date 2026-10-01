use std::path::{Path, PathBuf};

use pdf_app::wording::Lang;
use pdf_convert::{Setting, Tool, Value};

use super::{Around, Facts, Page, Source, Stage};

fn around(document: bool) -> Around {
    Around {
        lang: Lang::English,
        document: document.then(|| Facts {
            name: "report.pdf".to_owned(),
            pages: 12,
            bytes: 90_000,
            unsaved: false,
            page: 3,
        }),
        busy: false,
    }
}

struct Folder(PathBuf);

impl Folder {
    fn with(files: &[(&str, usize)]) -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "panpdf-tools-page-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        for (name, size) in files {
            std::fs::write(path.join(name), vec![0_u8; *size]).unwrap();
        }
        Self(path)
    }

    fn at(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn names(page: &Page) -> Vec<&str> {
    page.names()
}

#[test]
fn the_open_document_is_the_first_file_of_a_tool_that_starts_from_a_pdf() {
    let page = Page::new(Tool::Compress, &around(true));
    assert_eq!(names(&page), ["report.pdf"]);
    assert_eq!(page.files[0].source, Source::Document);
    assert_eq!(page.files[0].pages, Some(12));
    assert_eq!(page.files[0].size, 90_000);
}

#[test]
fn a_tool_that_does_not_start_from_a_pdf_starts_with_no_file() {
    for tool in [
        Tool::WordToPdf,
        Tool::ExcelToPdf,
        Tool::PowerPointToPdf,
        Tool::ImageToPdf,
        Tool::ScanToPdf,
        Tool::HtmlToPdf,
    ] {
        assert!(Page::new(tool, &around(true)).files.is_empty(), "{tool:?}");
    }
    assert!(Page::new(Tool::Compress, &around(false)).files.is_empty());
}

#[test]
fn the_text_panel_is_not_a_page_so_no_page_holds_the_document_for_it() {
    assert!(Page::new(Tool::Ocr, &around(true)).files.is_empty());
}

#[test]
fn comparing_puts_the_open_document_first_and_the_next_file_chosen_second() {
    let folder = Folder::with(&[("new.pdf", 10), ("newer.pdf", 10), ("third.pdf", 10)]);
    let mut page = Page::new(Tool::Compare, &around(true));
    assert_eq!(names(&page), ["report.pdf"]);
    page.add_files(&[folder.at("new.pdf")], None, Lang::English);
    assert_eq!(names(&page), ["report.pdf", "new.pdf"]);
    page.add_files(&[folder.at("third.pdf")], None, Lang::English);
    assert_eq!(
        names(&page),
        ["report.pdf", "new.pdf"],
        "only two are compared"
    );
    page.add_files(&[folder.at("newer.pdf")], Some(1), Lang::English);
    assert_eq!(names(&page), ["report.pdf", "newer.pdf"]);
    page.add_files(&[folder.at("third.pdf")], Some(0), Lang::English);
    assert_eq!(names(&page), ["third.pdf", "newer.pdf"]);
}

#[test]
fn a_tool_that_takes_one_file_has_it_replaced_and_keeps_only_the_first_chosen() {
    let folder = Folder::with(&[("a.pdf", 5), ("b.pdf", 6), ("c.pdf", 7), ("d.pdf", 8)]);
    let mut page = Page::new(Tool::Sign, &around(true));
    page.add_files(
        &[folder.at("a.pdf"), folder.at("b.pdf")],
        Some(0),
        Lang::English,
    );
    assert_eq!(names(&page), ["a.pdf"]);
    page.add_files(&[folder.at("c.pdf")], None, Lang::English);
    assert_eq!(names(&page), ["c.pdf"]);
    assert_eq!(page.files[0].size, 7);
}

#[test]
fn a_tool_that_takes_many_files_adds_them_in_the_order_chosen_and_not_twice() {
    let folder = Folder::with(&[("one.png", 1), ("two.png", 2), ("three.jpg", 3)]);
    let mut page = Page::new(Tool::ImageToPdf, &around(false));
    page.add_files(
        &[folder.at("two.png"), folder.at("one.png")],
        None,
        Lang::English,
    );
    assert_eq!(names(&page), ["two.png", "one.png"]);
    page.add_files(
        &[folder.at("one.png"), folder.at("three.jpg")],
        None,
        Lang::English,
    );
    assert_eq!(names(&page), ["two.png", "one.png", "three.jpg"]);
}

#[test]
fn files_of_the_wrong_kind_are_left_out_and_named() {
    let folder = Folder::with(&[("a.docx", 1), ("b.txt", 1), ("c.pdf", 1)]);
    let mut page = Page::new(Tool::WordToPdf, &around(false));
    page.add_files(
        &[folder.at("a.docx"), folder.at("b.txt"), folder.at("c.pdf")],
        None,
        Lang::English,
    );
    assert_eq!(names(&page), ["a.docx"]);
    assert_eq!(page.notices.len(), 1);
    assert!(page.notices[0].contains("b.txt"), "{:?}", page.notices);
    assert!(page.notices[0].contains("c.pdf"), "{:?}", page.notices);
    assert!(!page.notices[0].contains("a.docx"), "{:?}", page.notices);
}

#[test]
fn a_file_that_is_not_there_is_said_not_to_be_readable() {
    let mut page = Page::new(Tool::Compress, &around(false));
    page.add_files(
        &[Path::new("/no/such/folder/ghost.pdf").to_path_buf()],
        None,
        Lang::English,
    );
    assert!(page.files.is_empty());
    assert_eq!(page.notices.len(), 1);
    assert!(page.notices[0].starts_with("ghost.pdf could not be read"));
}

#[test]
fn a_web_page_takes_its_pictures_and_styles_with_it() {
    let folder = Folder::with(&[
        ("page.html", 1),
        ("logo.png", 1),
        ("style.css", 1),
        ("n.txt", 1),
    ]);
    let mut page = Page::new(Tool::HtmlToPdf, &around(false));
    page.add_files(
        &[
            folder.at("page.html"),
            folder.at("logo.png"),
            folder.at("style.css"),
            folder.at("n.txt"),
        ],
        None,
        Lang::English,
    );
    assert_eq!(names(&page), ["page.html", "logo.png", "style.css"]);
}

#[test]
fn a_file_is_moved_earlier_or_later_and_cannot_leave_the_list() {
    let folder = Folder::with(&[("a.png", 1), ("b.png", 1), ("c.png", 1)]);
    let mut page = Page::new(Tool::ScanToPdf, &around(false));
    page.add_files(
        &[folder.at("a.png"), folder.at("b.png"), folder.at("c.png")],
        None,
        Lang::English,
    );
    page.shift(0, true);
    assert_eq!(names(&page), ["a.png", "b.png", "c.png"]);
    page.shift(2, false);
    assert_eq!(names(&page), ["a.png", "b.png", "c.png"]);
    page.shift(1, true);
    assert_eq!(names(&page), ["b.png", "a.png", "c.png"]);
    page.shift(1, false);
    assert_eq!(names(&page), ["b.png", "c.png", "a.png"]);
    page.remove(0);
    assert_eq!(names(&page), ["c.png", "a.png"]);
    page.remove(9);
    assert_eq!(names(&page), ["c.png", "a.png"]);
}

#[test]
fn starting_over_keeps_the_tool_and_forgets_the_settings_and_what_was_made() {
    let mut page = Page::new(Tool::Compress, &around(true));
    page.values
        .set(Setting::Password, Value::Secret("sesame".to_owned()));
    page.repeat.push_str("again");
    page.notices.push("something".to_owned());
    page.stage = Stage::Settings;
    page.start_over(&around(true));
    assert_eq!(page.tool, Tool::Compress);
    assert!(!page.values.is_set(Setting::Password));
    assert!(page.repeat.is_empty());
    assert!(page.notices.is_empty());
    assert_eq!(names(&page), ["report.pdf"]);
}

#[test]
fn a_signature_picture_is_read_whole_and_a_missing_one_is_said_to_be_unreadable() {
    let folder = Folder::with(&[("me.png", 40)]);
    let mut page = Page::new(Tool::Sign, &around(true));
    page.use_the_picture(&folder.at("me.png"), Lang::English);
    match page.values.explicit(Setting::SignaturePicture) {
        Some(Value::File { name, bytes }) => {
            assert_eq!(name, "me.png");
            assert_eq!(bytes.len(), 40);
        }
        other => panic!("the picture was not kept: {other:?}"),
    }
    assert!(page.notices.is_empty());
    page.use_the_picture(&folder.at("gone.png"), Lang::English);
    assert_eq!(page.notices.len(), 1);
    assert!(page.notices[0].starts_with("gone.png could not be read"));
}
