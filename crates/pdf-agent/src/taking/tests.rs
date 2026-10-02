use std::path::Path;
use std::sync::Arc;

use super::{
    Split, extract_outcome, page_list, pages_named, picture_names, said_extracted, said_pictures,
    said_split, split_outcome,
};
use crate::desk::tests::with_paragraphs;
use crate::json::Json;
use crate::tools::request::{Request, parse};

fn read(name: &str, arguments: &str) -> Result<Request, String> {
    parse(
        name,
        &Json::parse(arguments).expect("the arguments in this test are JSON"),
    )
}

fn three_pages() -> (crate::desk::Desk, String) {
    let (mut desk, handle) = with_paragraphs("taking", &["First page words."]);
    for after in 1..=2 {
        desk.command(
            &handle,
            &pdf_edit::Command::AddBlankPage {
                beside: after - 1,
                before: false,
                size: [595.0, 842.0],
            },
        )
        .expect("a page");
    }
    assert_eq!(desk.page_count(&handle), Ok(3));
    (desk, handle)
}

#[test]
fn pages_are_named_like_a_person_would_type_them_and_a_page_that_is_not_there_is_refused() {
    assert_eq!(pages_named("1-3, 5", 6), Ok(vec![0, 1, 2, 4]));
    assert_eq!(
        pages_named("5, 1", 6),
        Ok(vec![0, 4]),
        "in page order, once each"
    );
    assert_eq!(pages_named("", 3), Ok(vec![0, 1, 2]), "none named is all");
    assert_eq!(
        pages_named("9", 6),
        Err("there is no page 9: the document has 6".to_owned())
    );
    assert_eq!(
        pages_named("2-12", 6),
        Err("there is no page 12: the document has 6".to_owned())
    );
    assert_eq!(pages_named("0", 6), Err("pages count from 1".to_owned()));
    assert!(pages_named("chapter two", 6).is_err());
    assert!(pages_named("3-1", 6).is_err(), "backwards");
}

#[test]
fn a_run_of_pages_is_listed_the_short_way() {
    assert_eq!(page_list(&[0, 1, 2, 5, 7, 8]), "1-3, 6, 8-9");
    assert_eq!(page_list(&[4]), "5");
    assert_eq!(page_list(&[]), "");
}

#[test]
fn the_calls_that_take_pages_out_read_or_refuse_what_they_were_given() {
    assert_eq!(
        read("extract_pages", r#"{"document":"doc-1","pages":" 2-4 "}"#),
        Ok(Request::ExtractPages(super::Extract {
            pages: "2-4".to_owned()
        }))
    );
    for bad in [
        r#"{"document":"doc-1"}"#,
        r#"{"document":"doc-1","pages":""}"#,
        r#"{"document":"doc-1","pages":"all"}"#,
    ] {
        assert!(read("extract_pages", bad).is_err(), "{bad}");
    }
    assert_eq!(
        read("split_document", r#"{"document":"doc-1","every":3}"#),
        Ok(Request::SplitDocument(Split::Every(3)))
    );
    assert_eq!(
        read("split_document", r#"{"document":"doc-1","at":"5, 12"}"#),
        Ok(Request::SplitDocument(Split::At("5, 12".to_owned())))
    );
    for bad in [
        r#"{"document":"doc-1"}"#,
        r#"{"document":"doc-1","every":2,"at":"3"}"#,
        r#"{"document":"doc-1","every":0}"#,
        r#"{"document":"doc-1","every":2.5}"#,
        r#"{"document":"doc-1","at":"  "}"#,
    ] {
        assert!(read("split_document", bad).is_err(), "{bad}");
    }
    assert_eq!(
        read("export_page_pictures", r#"{"document":"doc-1"}"#),
        Ok(Request::ExportPictures(super::Pictures {
            pages: String::new(),
            dpi: 150.0
        }))
    );
    for bad in [
        r#"{"document":"doc-1","dpi":5}"#,
        r#"{"document":"doc-1","dpi":601}"#,
    ] {
        assert!(
            read("export_page_pictures", bad)
                .expect_err("refused")
                .contains("`dpi` is a number from 20 to 600"),
            "{bad}"
        );
    }
}

#[test]
fn a_split_says_how_many_files_it_makes_and_refuses_one() {
    assert_eq!(
        Split::Every(2).groups(5),
        Ok(vec![vec![0, 1], vec![2, 3], vec![4]])
    );
    assert_eq!(
        Split::At("3, 5".to_owned()).groups(6),
        Ok(vec![vec![0, 1], vec![2, 3], vec![4, 5]])
    );
    let one = Split::Every(9).groups(5).expect_err("one file is no split");
    assert!(one.contains("1 file, not several"), "{one}");
    assert!(Split::At("99".to_owned()).groups(5).is_err());
    let many = Split::Every(1).groups(600).expect_err("too many");
    assert!(many.contains("500"), "{many}");
}

#[test]
fn taking_pages_out_names_the_file_after_them_and_holds_only_them() {
    let (mut desk, handle) = three_pages();
    let path = desk.handles()[0].1.clone();
    let said = desk.extract(&handle, "1, 3").expect("taken out");
    assert!(said.contains("Took out 2 pages (pages 1, 3)"), "{said}");
    let made = path.with_file_name("one-2pages.pdf");
    assert!(made.exists(), "{said}");
    let mut other = crate::desk::Desk::with_fonts(Some(crate::desk::tests::fonts()));
    let opened = other.open(&made, "", false).expect("it opens");
    assert_eq!(opened.pages, 2);
    let words = other.blocks(&opened.handle, 0).expect("page one");
    assert!(
        words
            .iter()
            .any(|block| block.text.contains("First page words")),
        "page 1 came along"
    );
    let again = desk.extract(&handle, "1, 3").expect("again");
    assert!(
        again.contains("one-2pages-2.pdf"),
        "never written over: {again}"
    );
    assert!(desk.extract(&handle, "7").is_err());
}

#[test]
fn a_split_writes_numbered_files_into_a_new_folder_and_every_page_lands_in_one() {
    let (mut desk, handle) = three_pages();
    let path = desk.handles()[0].1.clone();
    let said = desk.split(&handle, &Split::Every(2)).expect("split");
    assert!(
        said.contains("Split the document into 2 files (file 1: pages 1-2; file 2: page 3)"),
        "{said}"
    );
    let folder = path.with_file_name("one");
    assert!(folder.is_dir(), "{said}");
    let mut names: Vec<String> = std::fs::read_dir(&folder)
        .expect("a folder")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    assert_eq!(names, ["one-1.pdf", "one-2.pdf"]);
    let mut other = crate::desk::Desk::with_fonts(Some(crate::desk::tests::fonts()));
    let pages: Vec<usize> = names
        .iter()
        .map(|name| {
            other
                .open(&folder.join(name), "", false)
                .expect("opens")
                .pages
        })
        .collect();
    assert_eq!(pages, [2, 1]);
    let again = desk.split(&handle, &Split::Every(2)).expect("again");
    assert!(
        path.with_file_name("one-2").is_dir() && again.contains("one-2"),
        "a second folder, the first untouched: {again}"
    );
}

#[test]
fn pages_are_drawn_as_png_files_of_the_size_the_resolution_says() {
    let (mut desk, handle) = three_pages();
    let path = desk.handles()[0].1.clone();
    let said = desk
        .page_pictures(
            &handle,
            &super::Pictures {
                pages: "1-2".to_owned(),
                dpi: 72.0,
            },
        )
        .expect("drawn");
    assert!(
        said.contains("Drew 2 pages (pages 1-2) as PNG pictures at 72 dpi"),
        "{said}"
    );
    let folder = path.with_file_name("one");
    let first = std::fs::read(folder.join("one-p1.png")).expect("a picture");
    assert_eq!(&first[1..4], b"PNG");
    let wide = u32::from_be_bytes([first[16], first[17], first[18], first[19]]);
    let high = u32::from_be_bytes([first[20], first[21], first[22], first[23]]);
    assert_eq!((wide, high), (595, 842), "72 dpi is one pixel to a point");
    assert!(folder.join("one-p2.png").exists());
    assert!(!folder.join("one-p3.png").exists());
    let big = desk
        .page_pictures(
            &handle,
            &super::Pictures {
                pages: "1".to_owned(),
                dpi: 300.0,
            },
        )
        .expect("a fine one");
    assert!(big.contains("300 dpi"), "{big}");
    let fine = std::fs::read(path.with_file_name("one-p1.png")).expect("a picture beside it");
    let wide = u32::from_be_bytes([fine[16], fine[17], fine[18], fine[19]]);
    assert_eq!(wide, 2480, "300 dpi is not cut down to 2400 pixels");
}

#[test]
fn the_outcome_of_each_is_files_named_after_the_document() {
    let (mut desk, handle) = three_pages();
    let path = Path::new("/docs/book.pdf");
    let (store, credential) = desk.source(&handle).expect("a source");
    let bytes: Arc<[u8]> = Arc::from(store.to_vec());
    let one = extract_outcome(path, &bytes, &credential, &[1, 2]).expect("one file");
    assert_eq!(one.files.len(), 1);
    assert_eq!(one.files[0].0, "book-p2-3.pdf");
    assert!(extract_outcome(path, &bytes, &credential, &[]).is_err());
    let many = split_outcome(path, (&bytes, &credential), &[vec![0], vec![1], vec![2]])
        .expect("three files");
    let names: Vec<&str> = many.files.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["book-1.pdf", "book-2.pdf", "book-3.pdf"]);
    assert_eq!(
        picture_names(path, &[0, 2, 4]),
        ["book-p1.png", "book-p3.png", "book-p5.png"],
        "each is named for its page"
    );
    let twelve: Vec<usize> = (0..12).collect();
    assert_eq!(
        picture_names(path, &twelve)[0],
        "book-p01.png",
        "padded so they sort"
    );
    assert_eq!(picture_names(path, &[6]), ["book-p7.png"]);
    assert!(said_extracted(&[1], "Made x.").contains("Made x."));
    assert!(said_split(&[vec![0], vec![1]], "Made y.").contains("Made y."));
    assert!(said_pictures(&[0], 150.0, "Made z.").contains("150 dpi"));
}
