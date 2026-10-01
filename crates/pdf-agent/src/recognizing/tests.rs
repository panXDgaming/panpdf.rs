use super::{
    Asked, is_a_language_code, said_no_engine, said_no_language, said_not_in_one_place,
    said_nothing_to_read, said_read,
};
use crate::json::Json;
use crate::tools::request::{Request, parse};

fn read(arguments: &str) -> Result<Asked, String> {
    match parse(
        "ocr_pages",
        &Json::parse(arguments).expect("the arguments in this test are JSON"),
    )? {
        Request::OcrPages(asked) => Ok(asked),
        other => panic!("ocr_pages read as {other:?}"),
    }
}

#[test]
fn the_pages_the_languages_and_whether_to_leave_pages_with_text_are_read() {
    assert_eq!(
        read(r#"{"document":"doc-1"}"#),
        Ok(Asked {
            pages: String::new(),
            languages: Vec::new(),
            skip_text: true
        })
    );
    assert_eq!(
        read(
            r#"{"document":"doc-1","pages":" 2-3, 7 ","languages":["tha","eng"],"skip_pages_with_text":false}"#
        ),
        Ok(Asked {
            pages: "2-3, 7".to_owned(),
            languages: vec!["tha".to_owned(), "eng".to_owned()],
            skip_text: false
        })
    );
    assert_eq!(
        read(r#"{"document":"doc-1","languages":"tha+eng"}"#)
            .expect("reads")
            .languages,
        ["tha", "eng"],
        "a list written the way the recogniser writes it"
    );
    assert!(
        read(r#"{"document":"doc-1","pages":null,"languages":null}"#)
            .expect("a null is left out")
            .languages
            .is_empty()
    );
}

#[test]
fn a_language_that_is_not_a_code_is_refused_before_anything_is_read() {
    for bad in [
        "../eng",
        "english language",
        "",
        "e".repeat(40).as_str(),
        "en g",
    ] {
        assert!(!is_a_language_code(bad), "{bad:?}");
        let why = read(&format!(
            r#"{{"document":"doc-1","languages":[{}]}}"#,
            Json::text(bad).write()
        ))
        .expect_err(bad);
        assert!(why.contains("is not a language code"), "{bad}: {why}");
    }
    for good in ["eng", "tha", "chi_sim", "lao"] {
        assert!(is_a_language_code(good), "{good}");
    }
    assert!(
        read(r#"{"document":"doc-1","languages":[3]}"#)
            .expect_err("refused")
            .contains("list of words")
    );
    assert!(
        read(r#"{"document":"doc-1","skip_pages_with_text":"no"}"#)
            .expect_err("refused")
            .contains("true or false")
    );
}

#[test]
fn a_missing_recogniser_or_language_is_said_clearly_and_says_nothing_changed() {
    let engine = said_no_engine("Install tesseract-ocr.");
    assert!(
        engine.contains("not installed")
            && engine.contains("Install tesseract-ocr.")
            && engine.contains("Nothing in the document changed"),
        "{engine}"
    );
    let language = said_no_language(&["lao".to_owned()], &["eng".to_owned(), "tha".to_owned()]);
    assert!(
        language.contains("no model for lao")
            && language.contains("these are installed: eng, tha")
            && language.contains("Nothing in the document changed"),
        "{language}"
    );
    let none = said_no_language(&["tha".to_owned()], &[]);
    assert!(none.contains("none is installed yet"), "{none}");
    let apart = said_not_in_one_place(&["lao".to_owned()]);
    assert!(
        apart.contains("all installed in one place") && apart.contains("lao"),
        "{apart}"
    );
}

#[test]
fn what_was_read_is_told_in_pages_and_in_how_sure_the_recogniser_was() {
    let said = said_read(
        &[0, 1, 4],
        (87, 2, 1),
        &["tha".to_owned(), "eng".to_owned()],
    );
    assert!(
        said.starts_with("Made pages 1, 2, 5 searchable in tha+eng (the recogniser was 87 % sure)"),
        "{said}"
    );
    assert!(
        said.contains("2 pages already had text and were left alone")
            && said.contains("1 page could not be read")
            && said.contains("undo takes back"),
        "{said}"
    );
    let one = said_read(&[2], (50, 0, 0), &["eng".to_owned()]);
    assert!(one.starts_with("Made page 3 searchable"), "{one}");
    assert!(!one.contains("already had text"), "{one}");
    let many: Vec<usize> = (0..40).collect();
    assert!(
        said_read(&many, (90, 0, 0), &["eng".to_owned()]).starts_with("Made 40 pages searchable")
    );
}

#[test]
fn nothing_read_says_why_without_calling_it_a_success() {
    let had = said_nothing_to_read(3, 0, None);
    assert!(
        had.contains("already has text (3 pages)") && had.contains("skip_pages_with_text: false"),
        "{had}"
    );
    assert!(said_nothing_to_read(1, 0, None).contains("(1 page)"));
    let blank = said_nothing_to_read(0, 0, None);
    assert!(blank.contains("found no words"), "{blank}");
    let failed = said_nothing_to_read(0, 2, Some("the engine stopped"));
    assert!(
        failed.contains("the engine stopped") && failed.contains("2 pages failed"),
        "{failed}"
    );
}
