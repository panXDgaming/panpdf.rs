use pdf_convert::{Choice, Setting, Tool, Value};

use super::{Asked, Source, slugs, why};
use crate::desk::tests::with_paragraphs;
use crate::json::Json;
use crate::tools::request::{Request, parse};

fn read(name: &str, arguments: &str) -> Result<Asked, String> {
    let arguments = Json::parse(arguments).expect("the arguments in this test are JSON");
    match parse(name, &arguments)? {
        Request::Convert(asked) => Ok(asked),
        other => panic!("{name} read as {other:?}"),
    }
}

fn convert(arguments: &str) -> Result<Asked, String> {
    read("convert", arguments)
}

#[test]
fn the_tools_a_model_may_name_are_the_catalogue_less_protecting_which_is_its_own_tool() {
    let named = slugs();
    assert_eq!(named.len(), Tool::ALL.len() - 1);
    assert!(!named.contains(&"protect-pdf"));
    assert!(named.contains(&"pdf-to-jpg") && named.contains(&"compare-pdf"));
    let published = crate::tools::listed();
    let convert = published
        .as_list()
        .expect("a list")
        .iter()
        .find(|tool| tool.get("name").and_then(Json::as_str) == Some("convert"))
        .expect("the convert tool");
    let enumerated: Vec<&str> = convert
        .get("inputSchema")
        .and_then(|schema| schema.get("properties"))
        .and_then(|properties| properties.get("tool"))
        .and_then(|tool| tool.get("enum"))
        .and_then(Json::as_list)
        .expect("an enum")
        .iter()
        .filter_map(Json::as_str)
        .collect();
    assert_eq!(
        enumerated, named,
        "the schema offers what the catalogue has"
    );
}

fn options_named_in(description: &str) -> Vec<(Vec<String>, Vec<String>)> {
    let mut rows = Vec::new();
    for line in description.lines().filter(|line| line.starts_with("- ")) {
        let (tools, options) = line[2..].split_once(": ").expect("tools, a colon, options");
        let mut named = Vec::new();
        let (mut depth, mut word) = (0, String::new());
        for letter in options.trim_end_matches('.').chars() {
            match letter {
                '(' => depth += 1,
                ')' => depth -= 1,
                ',' if depth == 0 => {
                    named.push(std::mem::take(&mut word));
                    continue;
                }
                _ => {}
            }
            if depth == 0 && letter != ')' {
                word.push(letter);
            }
        }
        named.push(word);
        let keys = named
            .iter()
            .map(|key| key.trim().split(' ').next().unwrap_or_default().to_owned())
            .filter(|key| key != "no")
            .collect();
        rows.push((tools.split(", ").map(str::to_owned).collect(), keys));
    }
    rows
}

#[test]
fn every_option_the_description_names_is_an_option_of_that_tool() {
    let published = crate::tools::listed();
    let description = published
        .as_list()
        .expect("a list")
        .iter()
        .find(|tool| tool.get("name").and_then(Json::as_str) == Some("convert"))
        .and_then(|tool| {
            tool.get("description")
                .and_then(Json::as_str)
                .map(str::to_owned)
        })
        .expect("a description");
    let rows = options_named_in(&description);
    assert!(rows.len() >= 12, "{rows:?}");
    let mut covered = std::collections::BTreeSet::new();
    for (tools, keys) in rows {
        for slug in tools {
            let tool = Tool::from_slug(&slug).unwrap_or_else(|| panic!("{slug} is a tool"));
            covered.insert(tool);
            for key in &keys {
                assert!(
                    tool.settings().iter().any(|setting| setting.key() == key),
                    "{slug} is said to take {key}"
                );
            }
        }
    }
    for tool in Tool::ALL.into_iter().filter(|tool| *tool != Tool::Protect) {
        assert!(covered.contains(&tool), "{} is not described", tool.slug());
    }
    assert!(
        !options_named_in("- pdf-to-word: nonsense.")[0].1.is_empty(),
        "negative control: the reader finds what is written"
    );
}

#[test]
fn the_options_of_a_tool_are_read_by_the_catalogues_own_keys_and_choices() {
    let asked = convert(
        r#"{"document":"doc-1","tool":"pdf-to-jpg","options":{"format":"png","dpi":200,"pages":"1-2","mode":"extract"}}"#,
    )
    .expect("reads");
    assert_eq!(asked.tool, Tool::PdfToImage);
    assert_eq!(
        asked.values.choice(Setting::PictureFormat),
        Some(Choice::Png)
    );
    assert_eq!(
        asked.values.choice(Setting::WhatToTake),
        Some(Choice::PicturesInside)
    );
    assert_eq!(asked.values.number(Setting::Resolution), Some(200.0));
    assert_eq!(asked.values.text(Setting::Pages).as_deref(), Some("1-2"));
    assert!(asked.files.is_empty() && !asked.open_result);
    let by_alias = convert(r#"{"document":"doc-1","tool":"pdf-to-image"}"#).expect("an alias");
    assert_eq!(by_alias.tool, Tool::PdfToImage);
    let nulls = convert(
        r#"{"document":"doc-1","tool":"compress-pdf","options":{"level":null},"files":null}"#,
    )
    .expect("a null is a thing left out");
    assert!(!nulls.values.is_set(Setting::Level));
}

#[test]
fn an_option_the_tool_does_not_take_is_refused_naming_the_ones_it_does() {
    let why = convert(r#"{"document":"doc-1","tool":"compress-pdf","options":{"dpi":300}}"#)
        .expect_err("refused");
    assert!(
        why.contains("`dpi` is not an option of compress-pdf") && why.contains("level, password"),
        "{why}"
    );
    let none = convert(
        r#"{"document":"doc-1","tool":"word-to-pdf","files":["/a/b.docx"],"options":{"x":1}}"#,
    )
    .expect_err("refused");
    assert!(none.contains("no options"), "{none}");
}

#[test]
fn a_choice_a_number_or_a_kind_of_value_that_is_wrong_is_refused_in_the_options_words() {
    let bad_choice =
        convert(r#"{"document":"doc-1","tool":"pdf-to-jpg","options":{"format":"gif"}}"#)
            .expect_err("refused");
    assert!(bad_choice.contains("jpg, png"), "{bad_choice}");
    let too_fine = convert(r#"{"document":"doc-1","tool":"pdf-to-jpg","options":{"dpi":5000}}"#)
        .expect_err("refused");
    assert!(
        too_fine.contains("dpi") && too_fine.contains("5000"),
        "{too_fine}"
    );
    let text_for_flag =
        convert(r#"{"document":"doc-1","tool":"pdf-to-pdfa","options":{"password":3}}"#)
            .expect_err("refused");
    assert!(
        text_for_flag.contains("`password` is text"),
        "{text_for_flag}"
    );
    let not_object = convert(r#"{"document":"doc-1","tool":"pdf-to-text","options":["pages"]}"#)
        .expect_err("refused");
    assert!(
        not_object.contains("`options` is an object"),
        "{not_object}"
    );
    assert!(
        convert(r#"{"document":"doc-1","tool":"pdf-to-nothing"}"#)
            .expect_err("refused")
            .contains("is not a tool")
    );
    assert!(
        convert(r#"{"document":"doc-1"}"#)
            .expect_err("refused")
            .contains("`tool` is needed")
    );
}

#[test]
fn a_drawing_or_a_set_of_boxes_cannot_be_given_in_words() {
    let drawn = convert(
        r#"{"document":"doc-1","tool":"sign-pdf","options":{"how":"draw","draw":[[[0,0],[1,1]]]}}"#,
    )
    .expect_err("refused");
    assert!(drawn.contains("sign with how: type"), "{drawn}");
    let boxes =
        convert(r#"{"document":"doc-1","tool":"redact-pdf","options":{"areas":[{"page":1}]}}"#)
            .expect_err("refused");
    assert!(boxes.contains("`search`"), "{boxes}");
}

#[test]
fn redacting_needs_words_or_the_files_own_marks_and_is_the_destructive_conversion() {
    let nothing = convert(r#"{"document":"doc-1","tool":"redact-pdf"}"#).expect_err("refused");
    assert!(nothing.contains("needs `search`"), "{nothing}");
    let words = read(
        "convert",
        r#"{"document":"doc-1","tool":"redact-pdf","options":{"search":["Acme","Bangkok"],"case":true}}"#,
    )
    .expect("reads");
    assert_eq!(
        words.values.explicit(Setting::Search),
        Some(&Value::Terms(vec!["Acme".to_owned(), "Bangkok".to_owned()]))
    );
    assert!(words.destroys());
    let commas =
        convert(r#"{"document":"doc-1","tool":"redact-pdf","options":{"search":"Acme, Bangkok"}}"#)
            .expect("words may be separated by commas");
    assert_eq!(
        commas.values.explicit(Setting::Search),
        Some(&Value::Terms(vec!["Acme".to_owned(), "Bangkok".to_owned()]))
    );
    let marks =
        convert(r#"{"document":"doc-1","tool":"redact-pdf","options":{"annotations":true}}"#)
            .expect("the file's own marks are enough");
    assert!(marks.destroys());
    let harmless = convert(r#"{"document":"doc-1","tool":"compress-pdf"}"#).expect("reads");
    assert!(
        !harmless.destroys(),
        "negative control: only redaction destroys"
    );
}

#[test]
fn signing_needs_a_typed_name_or_a_picture_file_and_never_a_drawing() {
    let typed = convert(
        r#"{"document":"doc-1","tool":"sign-pdf","options":{"text":"Alice Example","position":"top-left"}}"#,
    )
    .expect("a name is enough");
    assert_eq!(typed.values.choice(Setting::Signature), Some(Choice::Typed));
    assert_eq!(typed.values.choice(Setting::Place), Some(Choice::TopLeft));
    let picture = convert(
        r#"{"document":"doc-1","tool":"sign-pdf","options":{"image":"/tmp/signature.png"}}"#,
    )
    .expect("a picture is enough");
    assert_eq!(
        picture.values.choice(Setting::Signature),
        Some(Choice::FromPicture)
    );
    assert_eq!(picture.pictures.len(), 1);
    assert!(
        convert(r#"{"document":"doc-1","tool":"sign-pdf"}"#)
            .expect_err("refused")
            .contains("typed name")
    );
    assert!(
        convert(r#"{"document":"doc-1","tool":"sign-pdf","options":{"how":"type"}}"#)
            .expect_err("refused")
            .contains("needs `text`")
    );
    assert!(
        convert(r#"{"document":"doc-1","tool":"sign-pdf","options":{"how":"draw"}}"#)
            .expect_err("refused")
            .contains("typed name")
    );
}

#[test]
fn protecting_is_its_own_tool_that_convert_refuses_to_do() {
    let refused =
        convert(r#"{"document":"doc-1","tool":"protect-pdf","options":{"password":"x"}}"#)
            .expect_err("refused");
    assert!(refused.contains("protect_document"), "{refused}");
    let protect = read(
        "protect_document",
        r#"{"document":"doc-1","password":"open sesame","owner_password":"boss","deny":["print","copy"]}"#,
    )
    .expect("reads");
    assert_eq!(protect.tool, Tool::Protect);
    assert_eq!(
        protect.values.text(Setting::NewPassword).as_deref(),
        Some("open sesame")
    );
    assert_eq!(
        protect.values.text(Setting::OwnerPassword).as_deref(),
        Some("boss")
    );
    assert_eq!(
        protect.values.explicit(Setting::Forbid),
        Some(&Value::Choices(vec![Choice::Print, Choice::Copy]))
    );
    assert!(!protect.destroys());
    let shown = format!("{protect:?}");
    assert!(
        !shown.contains("open sesame") && !shown.contains("boss"),
        "a password never reaches a log: {shown}"
    );
    assert!(
        read("protect_document", r#"{"document":"doc-1","password":""}"#)
            .expect_err("refused")
            .contains("is empty")
    );
    assert!(
        read("protect_document", r#"{"document":"doc-1"}"#)
            .expect_err("refused")
            .contains("`password`")
    );
    assert!(
        read(
            "protect_document",
            r#"{"document":"doc-1","password":"x","deny":["sing"]}"#
        )
        .expect_err("refused")
        .contains("print-high")
    );
}

#[test]
fn the_open_document_is_the_file_unless_others_are_named() {
    let own = convert(r#"{"document":"doc-1","tool":"pdf-to-text"}"#).expect("reads");
    assert_eq!(own.sources(), Ok(vec![Source::Document]));
    let named =
        convert(r#"{"document":"doc-1","tool":"pdf-to-text","files":["/a/one.pdf","/a/two.pdf"]}"#)
            .expect("reads");
    assert_eq!(
        named.sources(),
        Ok(vec![
            Source::File("/a/one.pdf".into()),
            Source::File("/a/two.pdf".into())
        ])
    );
    let compare = convert(r#"{"document":"doc-1","tool":"compare-pdf","files":["/a/newer.pdf"]}"#)
        .expect("reads");
    assert_eq!(
        compare.sources(),
        Ok(vec![Source::Document, Source::File("/a/newer.pdf".into())])
    );
    assert!(
        convert(r#"{"document":"doc-1","tool":"compare-pdf"}"#)
            .expect_err("refused")
            .contains("compares two files")
    );
    let one_only = convert(
        r#"{"document":"doc-1","tool":"redact-pdf","files":["/a.pdf","/b.pdf"],"options":{"search":["x"]}}"#,
    )
    .expect_err("redact takes one file");
    assert!(one_only.contains("does not take 2 files"), "{one_only}");
}

#[test]
fn files_of_the_wrong_kind_are_refused_for_the_tool_and_a_converter_to_pdf_needs_files() {
    let wrong = convert(r#"{"document":"doc-1","tool":"word-to-pdf","files":["/a/letter.pdf"]}"#)
        .expect_err("refused");
    assert!(wrong.contains("takes .docx"), "{wrong}");
    let none = convert(r#"{"document":"doc-1","tool":"word-to-pdf"}"#).expect_err("refused");
    assert!(none.contains("name them in `files`"), "{none}");
    let right = convert(r#"{"document":"doc-1","tool":"word-to-pdf","files":["/a/letter.DOCX"]}"#)
        .expect("an upper-case ending is the same");
    assert_eq!(right.files.len(), 1);
    let many: Vec<String> = (0..21).map(|at| format!("\"/a/{at}.pdf\"")).collect();
    let too_many = convert(&format!(
        r#"{{"document":"doc-1","tool":"pdf-to-text","files":[{}]}}"#,
        many.join(",")
    ))
    .expect_err("refused");
    assert!(
        too_many.contains("the most one call takes is 20"),
        "{too_many}"
    );
    let tilde =
        convert(r#"{"document":"doc-1","tool":"pdf-to-text","files":["~/x.pdf"]}"#).expect("reads");
    assert!(
        !tilde.files[0].starts_with("~"),
        "a path that starts with ~ is taken from the home folder: {:?}",
        tilde.files
    );
}

#[test]
fn what_went_wrong_is_said_in_plain_words_for_the_model() {
    use pdf_convert::run::Failure;
    assert!(why(&Failure::NeedsPassword).contains("`password` option"));
    assert!(why(&Failure::Cancelled).contains("nothing was written"));
    assert!(why(&Failure::BadInput("no pdf was given".to_owned())).contains("no pdf was given"));
    assert!(why(&Failure::Refused("too big".to_owned())).starts_with("The converter refused"));
    assert!(why(&Failure::Panicked("oops".to_owned())).contains("stopped unexpectedly"));
}

#[test]
fn a_conversion_of_the_open_document_makes_a_new_file_beside_it_and_changes_nothing() {
    let (mut desk, handle) = with_paragraphs("convert-text", &["Anvils from Bangkok."]);
    let path = desk.handles()[0].1.clone();
    let before = std::fs::read(&path).expect("the file");
    let asked = convert(r#"{"document":"doc-1","tool":"pdf-to-text"}"#).expect("reads");
    let said = desk.convert(&handle, &asked).expect("converted");
    let made = path.with_file_name("one.txt");
    assert!(said.contains("Made") && said.contains("one.txt"), "{said}");
    let words = std::fs::read_to_string(&made).expect("the text file");
    assert!(words.contains("Anvils from Bangkok."), "{words}");
    assert_eq!(
        std::fs::read(&path).expect("the file"),
        before,
        "the original is untouched"
    );
    let again = desk.convert(&handle, &asked).expect("converted again");
    assert!(
        again.contains("one-2.txt") && path.with_file_name("one-2.txt").exists(),
        "the second one has a name no file had: {again}"
    );
    assert_eq!(
        std::fs::read_to_string(&made).expect("the first"),
        words,
        "the first is not written over"
    );
}

#[test]
fn a_conversion_includes_changes_not_yet_saved_and_comparing_takes_the_other_file() {
    let (mut desk, handle) = with_paragraphs("convert-unsaved", &["Old words only."]);
    let path = desk.handles()[0].1.clone();
    let other = path.with_file_name("other.pdf");
    std::fs::copy(&path, &other).expect("a copy");
    desk.blocks(&handle, 0).expect("read");
    desk.rewrite(&handle, "p1-b1", None, "New words only.")
        .expect("rewritten");
    let text = desk
        .convert(
            &handle,
            &convert(r#"{"document":"doc-1","tool":"pdf-to-text"}"#).expect("reads"),
        )
        .expect("converted");
    assert!(text.contains("one.txt"), "{text}");
    let words = std::fs::read_to_string(path.with_file_name("one.txt")).expect("the text");
    assert!(
        words.contains("New words only.") && !words.contains("Old words"),
        "the unsaved change is in it: {words}"
    );
    let report = desk
        .convert(
            &handle,
            &convert(&format!(
                r#"{{"document":"doc-1","tool":"compare-pdf","files":["{}"],"options":{{"format":"txt"}}}}"#,
                other.display()
            ))
            .expect("reads"),
        )
        .expect("compared");
    assert!(report.contains("one-comparison.txt"), "{report}");
    let compared =
        std::fs::read_to_string(path.with_file_name("one-comparison.txt")).expect("a report");
    assert!(!compared.trim().is_empty());
}

#[test]
fn a_converter_that_refuses_says_why_and_writes_nothing() {
    let (mut desk, handle) = with_paragraphs("convert-refused", &["Words."]);
    let path = desk.handles()[0].1.clone();
    let asked = convert(r#"{"document":"doc-1","tool":"pdf-to-text","options":{"pages":"9"}}"#)
        .expect("reads");
    let why = desk
        .convert(&handle, &asked)
        .expect_err("page 9 is not there");
    assert!(why.starts_with("The converter"), "{why}");
    assert!(
        !path.with_file_name("one.txt").exists(),
        "nothing was written"
    );
    let missing =
        convert(r#"{"document":"doc-1","tool":"word-to-pdf","files":["/no/such/letter.docx"]}"#)
            .expect("reads");
    let why = desk.convert(&handle, &missing).expect_err("no such file");
    assert!(why.contains("cannot be read"), "{why}");
}

#[test]
fn a_request_to_open_the_result_is_answered_honestly_where_there_is_no_window() {
    let (mut desk, handle) = with_paragraphs("convert-open", &["Words."]);
    let asked =
        convert(r#"{"document":"doc-1","tool":"pdf-to-text","open_result":true}"#).expect("reads");
    assert!(asked.open_result);
    let said = desk.convert(&handle, &asked).expect("converted");
    assert!(
        said.ends_with("It was not opened: there is no window here to open it in."),
        "{said}"
    );
    let quiet = desk
        .convert(
            &handle,
            &convert(r#"{"document":"doc-1","tool":"pdf-to-text"}"#).expect("reads"),
        )
        .expect("converted");
    assert!(!quiet.contains("not opened"), "{quiet}");
}
