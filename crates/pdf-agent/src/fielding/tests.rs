use pdf_edit::new_field::NewFieldKind;

use super::{Asked, Order, kind_name, kind_of, said_added, tab_command};
use crate::desk::Desk;
use crate::desk::tests::with_paragraphs;
use crate::json::Json;
use crate::tools::request::{Request, parse};

fn read(name: &str, arguments: &str) -> Result<Request, String> {
    parse(
        name,
        &Json::parse(arguments).expect("the arguments in this test are JSON"),
    )
}

fn field(arguments: &str) -> Result<Asked, String> {
    match read("add_field", arguments)? {
        Request::AddField(asked) => Ok(asked),
        other => panic!("add_field read as {other:?}"),
    }
}

#[test]
fn every_kind_of_field_has_a_word_that_reads_back_as_itself() {
    for word in [
        "text",
        "paragraph",
        "checkbox",
        "radio",
        "dropdown",
        "list",
        "date",
        "signature",
        "button",
    ] {
        assert_eq!(kind_of(word).map(kind_name), Some(word), "{word}");
    }
    assert_eq!(kind_of("combo"), Some(NewFieldKind::Dropdown));
    assert_eq!(kind_of("scribble"), None);
}

#[test]
#[expect(
    clippy::float_cmp,
    reason = "the numbers are read straight from the text, not computed"
)]
fn a_field_is_read_with_its_box_name_and_choices() {
    let asked = field(
        r#"{"document":"doc-1","page":2,"kind":"dropdown","left":100,"top":200,"width":150,"height":24,"name":" Country ","options":["Thailand","Laos"]}"#,
    )
    .expect("reads");
    assert_eq!(asked.page, 1);
    assert_eq!(asked.kind, NewFieldKind::Dropdown);
    assert_eq!(asked.area, [100.0, 200.0, 250.0, 224.0]);
    assert_eq!(asked.name.as_deref(), Some("Country"));
    assert_eq!(asked.options, ["Thailand", "Laos"]);
    let nameless = field(
        r#"{"document":"doc-1","page":1,"kind":"text","left":10,"top":10,"width":100,"height":20}"#,
    )
    .expect("reads");
    assert!(nameless.name.is_none() && nameless.options.is_empty());
}

#[test]
fn a_field_that_is_too_small_or_has_choices_it_cannot_hold_or_none_it_needs_is_refused() {
    for (bad, said) in [
        (
            r#"{"document":"doc-1","page":1,"kind":"text","left":10,"top":10,"width":3,"height":20}"#,
            "too small for a field",
        ),
        (
            r#"{"document":"doc-1","page":1,"kind":"dropdown","left":10,"top":10,"width":90,"height":20}"#,
            "needs `options`",
        ),
        (
            r#"{"document":"doc-1","page":1,"kind":"text","left":10,"top":10,"width":90,"height":20,"options":["a"]}"#,
            "takes no `options`",
        ),
        (
            r#"{"document":"doc-1","page":1,"kind":"button","left":10,"top":10,"width":90,"height":20,"options":["a","b"]}"#,
            "one caption",
        ),
        (
            r#"{"document":"doc-1","page":1,"kind":"knob","left":10,"top":10,"width":90,"height":20}"#,
            "`kind` is text",
        ),
        (
            r#"{"document":"doc-1","page":1,"left":10,"top":10,"width":90,"height":20}"#,
            "`kind` is needed",
        ),
        (
            r#"{"document":"doc-1","page":1,"kind":"text","left":10,"top":10,"height":20}"#,
            "`width` is needed",
        ),
        (
            r#"{"document":"doc-1","page":1,"kind":"text","left":10,"top":10,"width":90,"height":20,"name":" "}"#,
            "`name` is empty",
        ),
        (
            r#"{"document":"doc-1","page":1,"kind":"list","left":10,"top":10,"width":90,"height":20,"options":[1]}"#,
            "list of text",
        ),
    ] {
        let why = field(bad).expect_err(bad);
        assert!(why.contains(said), "{bad}: {why}");
    }
    assert!(
        field(r#"{"document":"doc-1","page":1,"kind":"button","left":10,"top":10,"width":90,"height":20,"options":["Send"]}"#)
            .is_ok(),
        "negative control: a button takes its caption"
    );
}

#[test]
fn a_tab_order_names_a_page_and_one_of_three_orders() {
    assert_eq!(
        read(
            "set_tab_order",
            r#"{"document":"doc-1","page":2,"order":"columns"}"#
        ),
        Ok(Request::SetTabOrder {
            page: 1,
            order: Order::Columns
        })
    );
    for bad in [
        r#"{"document":"doc-1","page":2}"#,
        r#"{"document":"doc-1","order":"rows"}"#,
        r#"{"document":"doc-1","page":1,"order":"random"}"#,
    ] {
        assert!(read("set_tab_order", bad).is_err(), "{bad}");
    }
}

fn a_form() -> (Desk, String) {
    let (mut desk, handle) = with_paragraphs("fielding", &["A form."]);
    for (name, left, top) in [
        ("Right", 300.0, 100.0),
        ("Left", 72.0, 100.0),
        ("Below", 72.0, 200.0),
    ] {
        desk.add_field(
            &handle,
            &Asked {
                page: 0,
                kind: NewFieldKind::Text,
                area: [left, top, left + 120.0, top + 20.0],
                name: Some(name.to_owned()),
                options: Vec::new(),
            },
        )
        .expect("a field");
    }
    (desk, handle)
}

fn field_names(desk: &mut Desk, handle: &str) -> Vec<String> {
    let (source, credential) = desk.source(handle).expect("a source");
    pdf_edit::form::fields_of_document(&source, &credential)
        .expect("fields")
        .into_iter()
        .map(|(_, field)| field.name)
        .collect()
}

#[test]
fn a_field_added_is_in_the_form_by_its_name_at_the_place_given_and_is_one_undo_step() {
    let (mut desk, handle) = with_paragraphs("fielding-add", &["A form."]);
    let asked = field(
        r#"{"document":"doc-1","page":1,"kind":"checkbox","left":72,"top":100,"width":16,"height":16,"name":"Agree"}"#,
    )
    .expect("reads");
    let said = desk.add_field(&handle, &asked).expect("added");
    assert_eq!(said, said_added(&asked));
    assert!(
        said.contains("Added a checkbox field named \u{201c}Agree\u{201d} on page 1"),
        "{said}"
    );
    let (source, credential) = desk.source(&handle).expect("a source");
    let fields = pdf_edit::form::fields_of_document(&source, &credential).expect("fields");
    assert_eq!(fields.len(), 1);
    let (page, found) = &fields[0];
    assert_eq!((*page, found.name.as_str()), (0, "Agree"));
    assert!(
        (found.rect[1] - (842.0 - 116.0)).abs() < 0.01 && (found.rect[0] - 72.0).abs() < 0.01,
        "the top is 100 points down the page: {:?}",
        found.rect
    );
    let twice = desk.add_field(&handle, &asked).expect_err("a name taken");
    assert!(twice.contains("already in this form"), "{twice}");
    assert!(desk.walk(&handle, true).expect("undone"));
    assert!(
        field_names(&mut desk, &handle).is_empty(),
        "one undo takes the field away"
    );
    let far = desk
        .add_field(&handle, &Asked { page: 3, ..asked })
        .expect_err("refused");
    assert!(far.contains("there is no page 4"), "{far}");
}

#[test]
fn the_tab_order_follows_rows_or_columns_and_is_one_undo_step() {
    let (mut desk, handle) = a_form();
    assert_eq!(field_names(&mut desk, &handle), ["Right", "Left", "Below"]);
    let rows = desk.tab_order(&handle, 0, Order::Rows).expect("ordered");
    assert!(
        rows.contains("tabs through its 3 fields in rows order"),
        "{rows}"
    );
    assert_eq!(field_names(&mut desk, &handle), ["Left", "Right", "Below"]);
    desk.tab_order(&handle, 0, Order::Columns).expect("ordered");
    assert_eq!(field_names(&mut desk, &handle), ["Left", "Below", "Right"]);
    assert!(desk.walk(&handle, true).expect("undone"));
    assert_eq!(
        field_names(&mut desk, &handle),
        ["Left", "Right", "Below"],
        "one undo takes back the last order"
    );
    desk.tab_order(&handle, 0, Order::Structure)
        .expect("the order of the contents");
    let empty = desk
        .tab_order(&handle, 1, Order::Rows)
        .expect_err("no page two");
    assert!(empty.contains("there is no page 2"), "{empty}");
}

#[test]
fn a_page_without_fields_has_no_tab_order_to_set() {
    let (mut desk, handle) = with_paragraphs("fielding-none", &["No form here."]);
    let why = desk
        .tab_order(&handle, 0, Order::Rows)
        .expect_err("refused");
    assert!(why.contains("page 1 has no form fields"), "{why}");
    assert!(tab_command(0, &[], Order::Structure).is_err());
}
