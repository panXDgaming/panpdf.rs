use crate::ai_actions::editing::tests::{a_window_written, refused, said, text_of_page};
use crate::ai_actions::tests::undo_steps_left;
use crate::window_state::Window;

fn first_block(window: &mut Window) -> String {
    let read = text_of_page(window, 0);
    read.split('[')
        .nth(1)
        .and_then(|rest| rest.split(' ').next())
        .expect("a block name")
        .to_owned()
}

fn fields_of(window: &mut Window) -> Vec<(usize, String)> {
    let source = window.editor.source().cloned().expect("a document");
    pdf_edit::form::fields_of_document(&source, window.editor.credential())
        .expect("fields")
        .into_iter()
        .map(|(page, field)| (page, field.name))
        .collect()
}

#[test]
fn a_link_is_added_over_a_block_listed_by_name_and_removed_each_one_undo_step() {
    let mut window = a_window_written("Visit our site for anvils.");
    let block = first_block(&mut window);
    let none = said(
        &mut window,
        "links",
        r#"{"document":"doc-1","action":"list","page":1}"#,
    );
    assert_eq!(none, "Page 1 has no links.");
    let added = said(
        &mut window,
        "links",
        &format!(
            r#"{{"document":"doc-1","action":"add","block":"{block}","url":"https://example.org/anvils"}}"#
        ),
    );
    assert!(
        added.starts_with("Added a link on page 1 over [")
            && added.contains("https://example.org/anvils"),
        "{added}"
    );
    let listed = said(
        &mut window,
        "links",
        r#"{"document":"doc-1","action":"list","page":1}"#,
    );
    assert!(
        listed.starts_with("Page 1 has 1 link,")
            && listed.contains("p1-l1 [")
            && listed.contains("goes to https://example.org/anvils"),
        "{listed}"
    );
    let removed = said(
        &mut window,
        "links",
        r#"{"document":"doc-1","action":"remove","link":"p1-l1"}"#,
    );
    assert!(removed.starts_with("Removed p1-l1"), "{removed}");
    assert_eq!(
        said(
            &mut window,
            "links",
            r#"{"document":"doc-1","action":"list","page":1}"#
        ),
        "Page 1 has no links."
    );
    assert_eq!(
        window.ai.tools.run.steps(),
        3,
        "the write, the link, the removal"
    );
    assert_eq!(undo_steps_left(&mut window), 3);
}

#[test]
fn a_link_over_a_box_to_a_page_is_made_and_a_stale_name_is_refused() {
    let mut window = a_window_written("Words.");
    said(
        &mut window,
        "add_blank_page",
        r#"{"document":"doc-1","after_page":1}"#,
    );
    said(
        &mut window,
        "links",
        r#"{"document":"doc-1","action":"add","page":1,"left":300,"top":500,"right":400,"bottom":530,"to_page":2}"#,
    );
    let unlisted = refused(
        &mut window,
        "links",
        r#"{"document":"doc-1","action":"remove","link":"p1-l1"}"#,
    );
    assert!(unlisted.contains("has not been listed yet"), "{unlisted}");
    let listed = said(
        &mut window,
        "links",
        r#"{"document":"doc-1","action":"list","page":1}"#,
    );
    assert!(
        listed.contains("p1-l1 [300, 500, 400, 530] goes to page 2"),
        "{listed}"
    );
    said(
        &mut window,
        "links",
        r#"{"document":"doc-1","action":"add","page":1,"left":100,"top":600,"right":180,"bottom":620,"url":"mailto:a@example.org"}"#,
    );
    let stale = refused(
        &mut window,
        "links",
        r#"{"document":"doc-1","action":"remove","link":"p1-l1"}"#,
    );
    assert!(
        stale.contains("has not been listed yet"),
        "adding forgot the list: {stale}"
    );
    for (arguments, said) in [
        (
            r#"{"document":"doc-1","action":"add","page":1,"left":1,"top":1,"right":2,"bottom":2,"to_page":1}"#,
            "too small for a link",
        ),
        (
            r#"{"document":"doc-1","action":"add","page":1,"left":10,"top":10,"right":90,"bottom":30,"to_page":9}"#,
            "there is no page 9",
        ),
        (
            r#"{"document":"doc-1","action":"list","page":7}"#,
            "there is no page 7: the document has 2",
        ),
        (
            r#"{"document":"doc-1","action":"add","block":"p1-b99","url":"https://a.org"}"#,
            "p1-b99",
        ),
    ] {
        let why = refused(&mut window, "links", arguments);
        assert!(why.contains(said), "{arguments}: {why}");
    }
}

#[test]
fn a_shape_is_drawn_listed_as_a_drawing_and_taken_back_in_one_step() {
    let mut window = a_window_written("Words to point at.");
    let said_so = said(
        &mut window,
        "draw_shape",
        r##"{"document":"doc-1","page":1,"shape":"arrow","left":200,"top":300,"right":400,"bottom":350,"color":"#cc0000","width":3}"##,
    );
    assert!(
        said_so.starts_with("Drew an arrow on page 1 over [200, 300, 400, 350]"),
        "{said_so}"
    );
    let objects = said(
        &mut window,
        "objects",
        r#"{"document":"doc-1","action":"list","page":1}"#,
    );
    assert!(
        objects.contains("p1-o1 drawing [200, 300, 400, 352]"),
        "the barbs of the head reach a little past the end of the shaft: {objects}"
    );
    assert!(
        matches!(
            window.editor.status(),
            pdf_app::wording::Message::Done(pdf_app::wording::Done::DrewShape)
        ),
        "{:?}",
        window.editor.status()
    );
    let off = refused(
        &mut window,
        "draw_shape",
        r#"{"document":"doc-1","page":1,"shape":"line","left":900,"top":900,"right":950,"bottom":950}"#,
    );
    assert!(off.contains("lies off the page"), "{off}");
    let far = refused(
        &mut window,
        "draw_shape",
        r#"{"document":"doc-1","page":3,"shape":"line","left":1,"top":1,"right":9,"bottom":9}"#,
    );
    assert!(far.contains("there is no page 3"), "{far}");
    assert_eq!(undo_steps_left(&mut window), 2, "the write and the arrow");
    let after = said(
        &mut window,
        "objects",
        r#"{"document":"doc-1","action":"list","page":1}"#,
    );
    assert!(!after.contains("p1-o1 drawing"), "{after}");
}

#[test]
fn a_form_field_is_added_filled_and_put_in_order_each_one_undo_step() {
    let mut window = a_window_written("A form to fill.");
    for (name, left, top) in [("Right", 300, 150), ("Left", 72, 150), ("Below", 72, 250)] {
        let added = said(
            &mut window,
            "add_field",
            &format!(
                r#"{{"document":"doc-1","page":1,"kind":"text","left":{left},"top":{top},"width":120,"height":22,"name":"{name}"}}"#
            ),
        );
        assert!(
            added.contains(&format!("named \u{201c}{name}\u{201d}")),
            "{added}"
        );
    }
    assert_eq!(
        fields_of(&mut window),
        [
            (0, "Right".to_owned()),
            (0, "Left".to_owned()),
            (0, "Below".to_owned())
        ]
    );
    let taken = refused(
        &mut window,
        "add_field",
        r#"{"document":"doc-1","page":1,"kind":"text","left":10,"top":10,"width":50,"height":20,"name":"Left"}"#,
    );
    assert!(taken.contains("already in this form"), "{taken}");
    let ordered = said(
        &mut window,
        "set_tab_order",
        r#"{"document":"doc-1","page":1,"order":"rows"}"#,
    );
    assert!(
        ordered.contains("tabs through its 3 fields in rows order"),
        "{ordered}"
    );
    assert_eq!(
        fields_of(&mut window)
            .iter()
            .map(|(_, name)| name.as_str())
            .collect::<Vec<_>>(),
        ["Left", "Right", "Below"]
    );
    said(
        &mut window,
        "fill_field",
        r#"{"document":"doc-1","name":"Left","value":"Kham"}"#,
    );
    let info = said(&mut window, "document_info", r#"{"document":"doc-1"}"#);
    assert!(info.contains("Left (text, page 1): Kham"), "{info}");
    let none = refused(
        &mut window,
        "set_tab_order",
        r#"{"document":"doc-1","page":2,"order":"rows"}"#,
    );
    assert!(none.contains("there is no page 2"), "{none}");
    assert_eq!(
        undo_steps_left(&mut window),
        6,
        "the write, three fields, the order and the fill: one each"
    );
    assert!(fields_of(&mut window).is_empty());
}

#[test]
fn a_page_with_no_form_fields_has_no_tab_order_to_set() {
    let mut window = a_window_written("No form here.");
    let why = refused(
        &mut window,
        "set_tab_order",
        r#"{"document":"doc-1","page":1,"order":"columns"}"#,
    );
    assert!(why.contains("page 1 has no form fields"), "{why}");
}
