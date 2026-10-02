use std::path::{Path, PathBuf};
use std::sync::Arc;

use pdf_content::{FontProvider, FontRequest, GlyphProgram, SubstitutedFace};

use super::{BlockRange, Desk, parse_name};

#[derive(Debug)]
struct Packaged {
    latin: Arc<GlyphProgram>,
    thai: Arc<GlyphProgram>,
}

impl Packaged {
    fn face(&self, family: &str) -> Option<SubstitutedFace> {
        let program = match family {
            "DejaVu Sans" => &self.latin,
            "Noto Sans Thai" => &self.thai,
            _ => return None,
        };
        Some(SubstitutedFace {
            program: Arc::clone(program),
            identity: Arc::new(pdf_content::FaceIdentity {
                family: family.to_owned(),
                subfamily: "Regular".to_owned(),
                origin: format!("packaged:{family}"),
                sha256: family.to_owned(),
                face_index: 0,
                style: pdf_content::FontStyle::default(),
            }),
            reason: pdf_content::SubstitutionReason::ExactFamily,
        })
    }
}

impl FontProvider for Packaged {
    fn primary_face(&self, request: &FontRequest) -> Option<SubstitutedFace> {
        self.face(&request.family)
    }

    fn fallback_face(&self, _: &FontRequest, character: char) -> Option<SubstitutedFace> {
        let family = if ('\u{0E00}'..='\u{0E7F}').contains(&character) {
            "Noto Sans Thai"
        } else {
            "DejaVu Sans"
        };
        self.face(family)
    }

    fn description(&self) -> String {
        "packaged DejaVu Sans and Noto Sans Thai".to_owned()
    }
}

pub(crate) fn fonts() -> Arc<dyn FontProvider> {
    let parse = |bytes: &[u8]| Arc::new(GlyphProgram::parse(bytes.to_vec()).expect("parses"));
    Arc::new(Packaged {
        latin: parse(include_bytes!("../../../../fonts/packaged/DejaVuSans.ttf")),
        thai: parse(include_bytes!(
            "../../../../fonts/packaged/NotoSansThai-Regular.ttf"
        )),
    })
}

fn folder(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("panpdf-agent-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("a folder");
    path
}

fn document(folder: &Path, name: &str, text: &str) -> PathBuf {
    let path = folder.join(name);
    std::fs::write(
        &path,
        pdf_session::blank_document([595.0, 842.0]).expect("a blank page"),
    )
    .expect("written");
    let mut desk = Desk::with_fonts(Some(fonts()));
    let opened = desk.open(&path, "", false).expect("opens");
    desk.place_text(
        &opened.handle,
        0,
        [72.0, 72.0, 400.0, 200.0],
        text,
        ("DejaVu Sans", 12.0, false, false, None),
    )
    .expect("the text is placed");
    desk.save(&opened.handle, &path, true)
        .expect("saved over the blank");
    path
}

pub(crate) fn with_paragraphs(name: &str, paragraphs: &[&str]) -> (Desk, String) {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let number = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let folder = folder(&format!("{name}-{number}"));
    let path = folder.join("one.pdf");
    std::fs::write(
        &path,
        pdf_session::blank_document([595.0, 842.0]).expect("a blank page"),
    )
    .expect("written");
    let mut desk = Desk::with_fonts(Some(fonts()));
    let handle = desk.open(&path, "", false).expect("opens").handle;
    for (at, words) in paragraphs.iter().enumerate() {
        #[expect(clippy::cast_precision_loss, reason = "a few paragraphs")]
        let top = 72.0 + 120.0 * at as f64;
        desk.place_text(
            &handle,
            0,
            [72.0, top, 400.0, top + 60.0],
            words,
            ("DejaVu Sans", 12.0, false, false, None),
        )
        .expect("the paragraph is placed");
    }
    (desk, handle)
}

pub(crate) fn view_of_page(desk: &mut Desk, handle: &str, page: usize) -> pdf_session::PageView {
    let (source, credential) = desk.source(handle).expect("a source");
    pdf_session::interpret_page_fully(&source, page, &credential, None, desk.fonts())
        .expect("the page reads")
}

#[test]
fn a_page_reads_as_named_blocks() {
    let folder = folder("reads");
    let path = document(&folder, "one.pdf", "Hello world, from an agent.");
    let mut desk = Desk::with_fonts(Some(fonts()));
    let handle = desk.open(&path, "", false).expect("opens").handle;
    let blocks = desk.blocks(&handle, 0).expect("the page reads");
    assert_eq!(blocks.len(), 1, "{blocks:?}");
    let block = &blocks[0];
    assert_eq!(block.name(), "p1-b1");
    assert_eq!(block.text, "Hello world, from an agent.");
    assert!((block.size - 12.0).abs() < 0.5, "{}", block.size);
    assert!((block.area[0] - 72.0).abs() < 1.0, "{:?}", block.area);
    assert!(
        block.area[1] > 72.0 && block.area[1] < 90.0,
        "{:?}",
        block.area
    );
    assert!(block.fixed.is_none());
}

#[test]
fn a_word_or_a_whole_block_is_replaced() {
    let folder = folder("replaces");
    let path = document(&folder, "one.pdf", "Hello world, from an agent.");
    let mut desk = Desk::with_fonts(Some(fonts()));
    let handle = desk.open(&path, "", false).expect("opens").handle;
    desk.blocks(&handle, 0).expect("read");
    let now = desk
        .rewrite(&handle, "p1-b1", Some("world"), "there")
        .expect("one word is replaced")
        .expect("and the block reads back");
    assert_eq!(now.text, "Hello there, from an agent.");
    let now = desk
        .rewrite(&handle, &now.name(), None, "สวัสดีครับ")
        .expect("the whole block is replaced, in Thai")
        .expect("and reads back");
    assert_eq!(now.text, "สวัสดีครับ");
}

#[test]
fn a_name_finds_its_block_again_or_is_refused() {
    let folder = folder("names");
    let path = document(&folder, "one.pdf", "First paragraph.");
    let mut desk = Desk::with_fonts(Some(fonts()));
    let handle = desk.open(&path, "", false).expect("opens").handle;
    desk.place_text(
        &handle,
        0,
        [72.0, 400.0, 400.0, 500.0],
        "Second paragraph.",
        ("DejaVu Sans", 12.0, false, false, None),
    )
    .expect("a second paragraph, far below the first");
    let blocks = desk.blocks(&handle, 0).expect("read");
    assert_eq!(blocks.len(), 2, "{blocks:?}");
    let named = |text: &str| {
        blocks
            .iter()
            .find(|block| block.text.starts_with(text))
            .expect("the paragraph")
            .name()
    };
    let (first, second) = (named("First"), named("Second"));

    desk.rewrite(&handle, &first, Some("First"), "Opening")
        .expect("the first is edited");
    let again = desk
        .rewrite(&handle, &second, Some("Second"), "Closing")
        .expect("the second is found by the name read before the edit")
        .expect("and reads back");
    assert_eq!(again.text, "Closing paragraph.");

    desk.walk(&handle, true).expect("undo the second");
    desk.walk(&handle, true).expect("undo the first");
    let refused = desk
        .rewrite(&handle, &first, None, "x")
        .expect_err("the block changed since its name was given");
    assert!(refused.contains("read_text page 1 again"), "{refused}");

    let never = desk
        .rewrite(&handle, "p1-b9", None, "x")
        .expect_err("never read");
    assert!(never.contains("has not been read"), "{never}");
}

#[test]
fn a_find_that_is_not_one_place_is_refused() {
    let folder = folder("find");
    let path = document(&folder, "one.pdf", "one two one");
    let mut desk = Desk::with_fonts(Some(fonts()));
    let handle = desk.open(&path, "", false).expect("opens").handle;
    desk.blocks(&handle, 0).expect("read");
    let absent = desk
        .rewrite(&handle, "p1-b1", Some("three"), "x")
        .expect_err("absent");
    assert!(absent.contains("is not in the block"), "{absent}");
    let twice = desk
        .rewrite(&handle, "p1-b1", Some("one"), "x")
        .expect_err("twice");
    assert!(twice.contains("2 times"), "{twice}");
    let empty = desk
        .rewrite(&handle, "p1-b1", Some(""), "x")
        .expect_err("empty");
    assert!(empty.contains("empty"), "{empty}");
}

#[test]
fn undo_and_redo_walk_the_edits() {
    let folder = folder("undo");
    let path = document(&folder, "one.pdf", "Keep this.");
    let mut desk = Desk::with_fonts(Some(fonts()));
    let handle = desk.open(&path, "", false).expect("opens").handle;
    assert!(
        !desk.walk(&handle, true).expect("undo"),
        "nothing to undo yet"
    );
    desk.blocks(&handle, 0).expect("read");
    desk.rewrite(&handle, "p1-b1", None, "Changed.")
        .expect("edited");
    assert!(desk.walk(&handle, true).expect("undo"));
    assert_eq!(desk.blocks(&handle, 0).expect("read")[0].text, "Keep this.");
    assert!(desk.walk(&handle, false).expect("redo"));
    assert_eq!(desk.blocks(&handle, 0).expect("read")[0].text, "Changed.");
}

#[test]
fn saving_keeps_what_it_was_not_told_to_replace() {
    let folder = folder("save");
    let path = document(&folder, "one.pdf", "Before.");
    let mut desk = Desk::with_fonts(Some(fonts()));
    let handle = desk.open(&path, "", false).expect("opens").handle;
    desk.blocks(&handle, 0).expect("read");
    desk.rewrite(&handle, "p1-b1", None, "After.")
        .expect("edited");

    let refused = desk.save(&handle, &path, false).expect_err("exists");
    assert!(refused.contains("already exists"), "{refused}");

    let copy = folder.join("copy.pdf");
    desk.save(&handle, &copy, false)
        .expect("a new file is written");
    let mut other = Desk::with_fonts(Some(fonts()));
    let reopened = other.open(&copy, "", false).expect("reopens").handle;
    assert_eq!(other.blocks(&reopened, 0).expect("read")[0].text, "After.");

    let mut changed = std::fs::read(&path).expect("read");
    changed.extend_from_slice(b"\n% changed by someone else\n");
    std::fs::write(&path, &changed).expect("written");
    let refused = desk
        .save(&handle, &path, true)
        .expect_err("changed on disk");
    assert!(refused.contains("changed on disk"), "{refused}");
    assert_eq!(
        std::fs::read(&path).expect("read"),
        changed,
        "left as it was"
    );

    let left: Vec<_> = std::fs::read_dir(&folder)
        .expect("listed")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".tmp"))
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

#[test]
fn block_names_read_as_page_and_block() {
    assert_eq!(parse_name("p3-b12"), Ok((2, 11)));
    for bad in ["p0-b1", "p1-b0", "3-12", "p3b12", "p-b", "pX-b1", ""] {
        assert!(parse_name(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn a_find_across_a_break_is_a_range_across_it() {
    let folder = folder("across");
    let path = document(&folder, "one.pdf", "ab\ncd efgh ijkl mnop");
    let mut desk = Desk::with_fonts(Some(fonts()));
    let handle = desk.open(&path, "", false).expect("opens").handle;
    let blocks = desk.blocks(&handle, 0).expect("read");
    assert_eq!(blocks.len(), 1, "{blocks:?}");
    assert_eq!(blocks[0].text, "ab\ncd efgh ijkl mnop");
    let open = desk.open.get_mut(&handle).expect("open");
    let view = super::view_of(&mut open.session, 0).expect("page");
    let (_, parts) = super::read(&view, 0, 0).expect("the block");
    assert_eq!(
        super::range_within(&parts.reading, "b\nc"),
        Ok(BlockRange::Between {
            from: (0, 1),
            to: (1, 1)
        })
    );
}

#[test]
fn a_name_from_before_the_pages_moved_is_refused() {
    let folder = folder("moved");
    let path = folder.join("three.pdf");
    std::fs::write(
        &path,
        pdf_session::blank_document([595.0, 842.0]).expect("a blank page"),
    )
    .expect("written");
    let mut desk = Desk::with_fonts(Some(fonts()));
    let handle = desk.open(&path, "", false).expect("opens").handle;
    for _ in 0..2 {
        desk.command(
            &handle,
            &pdf_edit::Command::AddBlankPage {
                beside: 0,
                before: false,
                size: [595.0, 842.0],
            },
        )
        .expect("another page");
    }
    for page in 0..3 {
        desk.place_text(
            &handle,
            page,
            [72.0, 700.0, 400.0, 60.0],
            "Company confidential.",
            ("DejaVu Sans", 10.0, false, false, None),
        )
        .expect("the same footer on every page");
        desk.blocks(&handle, page).expect("the page reads");
    }

    desk.command(&handle, &pdf_edit::Command::RemovePages { pages: vec![0] })
        .expect("the first page goes");

    let refused = desk
        .rewrite(&handle, "p2-b1", None, "Public.")
        .expect_err("a name from before the pages moved means nothing now");
    assert!(
        refused.contains("before the pages were moved") && refused.contains("read_text"),
        "{refused}"
    );
    for page in 0..2 {
        let footer = &desk.blocks(&handle, page).expect("what is left reads")[0];
        assert_eq!(
            footer.text,
            "Company confidential.",
            "the footer of page {} says what it said",
            page + 1
        );
    }
}

#[test]
fn a_block_deleted_by_giving_it_no_text_is_a_success_with_nothing_to_read_back() {
    let folder = folder("delete-block");
    let path = document(&folder, "one.pdf", "Gone soon.");
    let mut desk = Desk::with_fonts(Some(fonts()));
    let handle = desk.open(&path, "", false).expect("opens").handle;
    desk.blocks(&handle, 0).expect("read");
    let now = desk
        .rewrite(&handle, "p1-b1", None, "")
        .expect("the deletion is not an error");
    assert!(now.is_none(), "{now:?}");
    assert!(desk.blocks(&handle, 0).expect("read").is_empty());
    let kept = desk
        .rewrite(&handle, "p1-b1", None, "x")
        .expect_err("the block is gone, so its name is too");
    assert!(kept.contains("no longer on the page"), "{kept}");
}

#[test]
fn the_server_says_a_block_was_deleted_and_the_window_is_not_told_it_was_replaced() {
    let folder = folder("delete-said");
    let path = document(&folder, "one.pdf", "Gone soon.");
    let mut desk = Desk::with_fonts(Some(fonts()));
    let handle = desk.open(&path, "", false).expect("opens").handle;
    let call = |desk: &mut Desk, name: &str, rest: &str| {
        let mut arguments = crate::json::Json::parse(rest).expect("JSON");
        if let crate::json::Json::Object(members) = &mut arguments {
            members.insert(
                "document".to_owned(),
                crate::json::Json::text(handle.clone()),
            );
        }
        crate::tools::call(desk, name, &arguments)
    };
    call(&mut desk, "read_text", "{}").expect("read");
    let said = call(&mut desk, "replace_text", r#"{"block":"p1-b1","text":""}"#)
        .expect("a deletion is not an error");
    assert!(said.text.starts_with("Deleted p1-b1."), "{}", said.text);
    assert_eq!(
        said.data.get("deleted"),
        Some(&crate::json::Json::Bool(true))
    );
}

#[test]
fn a_document_saved_under_another_name_is_no_longer_reported_as_having_changes() {
    let folder = folder("saved-as");
    let path = document(&folder, "one.pdf", "Before.");
    let mut desk = Desk::with_fonts(Some(fonts()));
    let handle = desk.open(&path, "", false).expect("opens").handle;
    let changed = |desk: &Desk| desk.handles()[0].3;
    assert!(!changed(&desk), "known answer: nothing was done yet");
    desk.blocks(&handle, 0).expect("read");
    desk.rewrite(&handle, "p1-b1", None, "After.")
        .expect("edited");
    assert!(changed(&desk), "an edit is a change");
    desk.save(&handle, &folder.join("copy.pdf"), false)
        .expect("saved as a copy");
    assert!(
        !changed(&desk),
        "what was saved is not unsaved, whatever it was saved as"
    );
    desk.walk(&handle, true).expect("undone");
    assert!(changed(&desk), "an edit after the save is a change again");
    let lost = desk.close(&handle).expect("closed");
    assert!(lost, "and closing says so");
    let handle = desk.open(&path, "", false).expect("opens again").handle;
    desk.save(&handle, &folder.join("again.pdf"), false)
        .expect("saved");
    assert!(!desk.close(&handle).expect("closed"), "nothing was lost");
}

#[test]
fn a_folder_or_a_file_that_is_not_there_is_not_opened_and_says_so() {
    let mut desk = Desk::with_fonts(Some(fonts()));
    let folder = folder("not-a-file");
    let why = desk.open(&folder, "", false).expect_err("a folder");
    assert!(why.contains("is not a file"), "{why}");
    let why = desk
        .open(&folder.join("missing.pdf"), "", false)
        .expect_err("missing");
    assert!(why.contains("cannot be read"), "{why}");
    #[cfg(unix)]
    {
        let why = desk
            .open(std::path::Path::new("/dev/zero"), "", false)
            .expect_err("a device is read without end");
        assert!(why.contains("is not a file"), "{why}");
    }
}

#[cfg(unix)]
#[test]
fn saving_over_a_private_file_keeps_it_private_and_a_link_stays_a_link() {
    use std::os::unix::fs::PermissionsExt as _;
    let folder = folder("private");
    let path = document(&folder, "one.pdf", "Secret.");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("private");
    let mut desk = Desk::with_fonts(Some(fonts()));
    let handle = desk.open(&path, "", false).expect("opens").handle;
    desk.blocks(&handle, 0).expect("read");
    desk.rewrite(&handle, "p1-b1", None, "Still secret.")
        .expect("edited");
    desk.save(&handle, &path, true).expect("saved over");
    let mode = std::fs::metadata(&path)
        .expect("facts")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        mode, 0o600,
        "the new file is as private as the one it replaced: {mode:o}"
    );

    let link = folder.join("link.pdf");
    std::os::unix::fs::symlink(&path, &link).expect("a link");
    desk.walk(&handle, true).expect("undo");
    desk.blocks(&handle, 0).expect("read");
    desk.rewrite(&handle, "p1-b1", None, "Through the link.")
        .expect("edited");
    desk.save(&handle, &link, true)
        .expect("saved through the link");
    assert!(
        std::fs::symlink_metadata(&link)
            .expect("facts")
            .file_type()
            .is_symlink(),
        "the link was not replaced by a file"
    );
    let mut other = Desk::with_fonts(Some(fonts()));
    let other_handle = other.open(&path, "", false).expect("opens").handle;
    assert_eq!(
        other.blocks(&other_handle, 0).expect("read")[0].text,
        "Through the link."
    );
}

#[test]
fn new_text_on_a_page_shown_turned_is_refused_in_words_and_the_page_is_left_alone() {
    let folder = folder("turned");
    let path = document(&folder, "one.pdf", "Upright.");
    let mut desk = Desk::with_fonts(Some(fonts()));
    let handle = desk.open(&path, "", false).expect("opens").handle;
    let before = desk.source(&handle).expect("a source").0.len();
    desk.command(
        &handle,
        &pdf_edit::Command::RotatePages {
            pages: vec![0],
            quarter_turns: 1,
        },
    )
    .expect("turned");
    let turned = desk.source(&handle).expect("a source").0.len();
    assert_ne!(before, turned);
    let why = desk
        .place_text(
            &handle,
            0,
            [72.0, 300.0, 372.0, 320.0],
            "Approved",
            ("DejaVu Sans", 12.0, false, false, None),
        )
        .expect_err("a turned page");
    assert!(why.contains("shown turned"), "{why}");
    assert_eq!(
        desk.source(&handle).expect("a source").0.len(),
        turned,
        "nothing was written"
    );
    let answer = crate::tools::call(
        &mut desk,
        "write_pages",
        &crate::json::Json::object([
            ("document", crate::json::Json::text(handle.clone())),
            ("markdown", crate::json::Json::text("A paragraph.")),
            ("font", crate::json::Json::text("DejaVu Sans")),
        ]),
    );
    let why = answer
        .err()
        .expect("a whole document cannot be written on a turned page either");
    assert!(why.contains("shown turned"), "{why}");
    assert_eq!(
        desk.source(&handle).expect("a source").0.len(),
        turned,
        "and not half of it"
    );
    let (_, width, height) = desk
        .look_closer(&handle, 0, [0.0, 0.0, 200.0, 100.0], 72.0)
        .expect("looking at a turned page is not writing on it");
    assert!(width > 0 && height > 0);
}

#[test]
fn text_in_a_script_no_installed_font_draws_is_refused_and_not_written_as_empty_boxes() {
    let folder = folder("no-font-for-script");
    let path = document(&folder, "one.pdf", "Some words.");
    let mut desk = Desk::with_fonts(Some(fonts()));
    let handle = desk.open(&path, "", false).expect("opens").handle;
    let before = desk.source(&handle).expect("a source").0.len();
    let asked = |text: &str| {
        crate::json::Json::object([
            ("document", crate::json::Json::text(handle.clone())),
            ("page", crate::json::Json::count(1)),
            ("left", crate::json::Json::count(72)),
            ("top", crate::json::Json::count(300)),
            ("width", crate::json::Json::count(300)),
            ("text", crate::json::Json::text(text)),
        ])
    };
    let why = crate::tools::call(
        &mut desk,
        "add_text",
        &asked("\u{928}\u{92e}\u{938}\u{94d}\u{924}\u{947}"),
    )
    .err()
    .expect("Devanagari has no face here");
    assert!(why.contains("cannot"), "{why}");
    assert_eq!(
        desk.source(&handle).expect("a source").0.len(),
        before,
        "nothing was written"
    );
    let said = crate::tools::call(&mut desk, "add_text", &asked("Plain Latin words."))
        .expect("and a face that draws it is used");
    assert!(said.text.starts_with("Written on page 1"), "{}", said.text);
}

#[test]
fn a_drawing_block_is_drawn_on_the_page() {
    let folder = folder("drawing");
    let path = document(&folder, "one.pdf", "Words.");
    let mut desk = Desk::with_fonts(Some(fonts()));
    let handle = desk.open(&path, "", false).expect("opens").handle;
    let markdown = "```draw\nsize 300 120\ncircle 60 60 40 fill=#00ff00\nheart 150 20 80 80 fill=#ff0000\n```\n";
    crate::tools::call(
        &mut desk,
        "write_pages",
        &crate::json::Json::object([
            ("document", crate::json::Json::text(handle.clone())),
            ("markdown", crate::json::Json::text(markdown)),
            ("font", crate::json::Json::text("DejaVu Sans")),
            ("theme", crate::json::Json::text("plain")),
        ]),
    )
    .expect("written");
    let (store, _) = desk.source(&handle).expect("a source");
    let (canvas, _) = pdf_cli::render_page_strict(&store, 0, 1.0).expect("drawn");
    let rgb = canvas.to_rgb8();
    let count = |want: [u8; 3]| rgb.chunks_exact(3).filter(|pixel| *pixel == want).count();
    let green = count([0, 255, 0]);
    assert!((4500..=5500).contains(&green), "{green} green pixels");
    assert!(count([255, 0, 0]) > 2000, "the heart");
}
