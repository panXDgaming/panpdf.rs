use pdf_edit::link::{Arrival, Target};
use pdf_syntax::Reference;

use super::{
    Action, Goes, Place, add_command, anchor_of, goes_in_words, is_an_address, link_name, listing,
    parse_name, remove_command, target_of, the_link,
};
use crate::desk::tests::with_paragraphs;
use crate::desk::{Desk, device_of};
use crate::json::Json;
use crate::tools::request::{Request, parse};

fn read(arguments: &str) -> Result<Action, String> {
    match parse(
        "links",
        &Json::parse(arguments).expect("the arguments in this test are JSON"),
    )? {
        Request::Links(action) => Ok(action),
        other => panic!("links read as {other:?}"),
    }
}

#[test]
fn a_link_is_named_by_its_page_and_its_place_in_that_page_and_the_name_reads_back() {
    assert_eq!(link_name(2, 1), "p3-l2");
    assert_eq!(parse_name("p3-l2"), Ok((2, 1)));
    assert_eq!(parse_name(" p12-l7 "), Ok((11, 6)));
    for bad in [
        "", "p0-l1", "p1-l0", "p1-b1", "l1", "p1-l", "p-l1", "3-l1", "p1l1",
    ] {
        let why = parse_name(bad).expect_err(bad);
        assert!(why.contains("is not a link name"), "{bad}: {why}");
    }
}

#[test]
fn only_addresses_a_window_opens_are_links_a_model_may_make() {
    for good in [
        "https://example.org/a?b=1",
        "http://example.org",
        "mailto:someone@example.org",
        "HTTPS://EXAMPLE.ORG",
    ] {
        assert!(is_an_address(good), "{good}");
    }
    for bad in [
        "",
        "example.org",
        "https:",
        "https://a b",
        "javascript:alert(1)",
        "file:///etc/passwd",
        "ftp://example.org",
        "https://a\nb",
    ] {
        assert!(!is_an_address(bad), "{bad:?}");
    }
    assert!(!is_an_address(&format!("https://{}", "a".repeat(5_000))));
}

#[test]
fn the_calls_name_a_block_or_a_box_and_a_place_to_go_and_refuse_the_rest() {
    assert_eq!(
        read(r#"{"document":"doc-1","action":"list","page":3}"#),
        Ok(Action::List { page: 2 })
    );
    assert_eq!(
        read(r#"{"document":"doc-1","action":"add","block":"p4-b2","url":"https://example.org"}"#),
        Ok(Action::Add {
            page: 3,
            place: Place::Block("p4-b2".to_owned()),
            goes: Goes::Address("https://example.org".to_owned()),
        })
    );
    assert_eq!(
        read(
            r#"{"document":"doc-1","action":"add","page":1,"left":10,"top":20,"right":110,"bottom":40,"to_page":5}"#
        ),
        Ok(Action::Add {
            page: 0,
            place: Place::Area([10.0, 20.0, 110.0, 40.0]),
            goes: Goes::Page(4),
        })
    );
    assert_eq!(
        read(r#"{"document":"doc-1","action":"remove","link":"p1-l2"}"#),
        Ok(Action::Remove {
            link: "p1-l2".to_owned()
        })
    );
    for (bad, said) in [
        (r#"{"document":"doc-1"}"#, "`action` is needed"),
        (
            r#"{"document":"doc-1","action":"edit"}"#,
            "list, add or remove",
        ),
        (
            r#"{"document":"doc-1","action":"list"}"#,
            "`page` is needed",
        ),
        (
            r#"{"document":"doc-1","action":"remove","link":"p1-b2"}"#,
            "not a link name",
        ),
        (r#"{"document":"doc-1","action":"remove"}"#, "`link`"),
        (
            r#"{"document":"doc-1","action":"add","block":"p1-b1"}"#,
            "`url` or `to_page`",
        ),
        (
            r#"{"document":"doc-1","action":"add","block":"p1-b1","url":"https://a.org","to_page":2}"#,
            "not both",
        ),
        (
            r#"{"document":"doc-1","action":"add","block":"p1-b1","url":"javascript:x"}"#,
            "not an address a link can open",
        ),
        (
            r#"{"document":"doc-1","action":"add","url":"https://a.org"}"#,
            "`block` or a box",
        ),
        (
            r#"{"document":"doc-1","action":"add","block":"p1-b1","left":1,"url":"https://a.org"}"#,
            "`block` or the box, not both",
        ),
        (
            r#"{"document":"doc-1","action":"add","block":"p2-b1","page":1,"url":"https://a.org"}"#,
            "is on page 2, not page 1",
        ),
        (
            r#"{"document":"doc-1","action":"add","left":1,"top":2,"url":"https://a.org","page":1}"#,
            "`right` is needed",
        ),
        (
            r#"{"document":"doc-1","action":"add","left":1,"top":2,"right":9,"bottom":9,"url":"https://a.org"}"#,
            "`page` is needed with a box",
        ),
    ] {
        let why = read(bad).expect_err(bad);
        assert!(why.contains(said), "{bad}: {why}");
    }
}

#[test]
fn listing_and_deleting_are_told_apart_by_what_they_do_to_the_document() {
    let list = read(r#"{"document":"doc-1","action":"list","page":1}"#).expect("reads");
    assert!(list.only_reads() && !list.is_destructive());
    let remove = read(r#"{"document":"doc-1","action":"remove","link":"p1-l1"}"#).expect("reads");
    assert!(remove.is_destructive() && !remove.only_reads());
    let add = read(r#"{"document":"doc-1","action":"add","block":"p1-b1","url":"https://a.org"}"#)
        .expect("reads");
    assert!(!add.is_destructive() && !add.only_reads());
}

#[test]
fn a_link_that_goes_to_a_page_that_is_not_there_is_refused_and_a_small_area_too() {
    assert_eq!(
        target_of(&Goes::Page(2), 3),
        Ok(Target::Page(2, Arrival::InheritZoom))
    );
    assert!(
        target_of(&Goes::Page(3), 3)
            .expect_err("refused")
            .contains("there is no page 4: the document has 3")
    );
    let (mut desk, handle) = with_paragraphs("linking-small", &["x"]);
    let geometry = desk.page_geometries(&handle).expect("measured")[0];
    let matrix = device_of(&geometry).expect("a device").matrix;
    let small = add_command(0, &matrix, ([10.0, 10.0, 11.0, 40.0], &Goes::Page(0)), 1)
        .expect_err("refused");
    assert!(small.contains("too small for a link"), "{small}");
    let fine = add_command(0, &matrix, ([10.0, 10.0, 60.0, 40.0], &Goes::Page(0)), 1)
        .expect("a link fits");
    let pdf_edit::Command::AddLink { rect, .. } = fine else {
        panic!("an AddLink");
    };
    assert!(
        (rect[3] - rect[1] - 30.0).abs() < 1e-6,
        "30 shown points are 30 user points on an upright page: {rect:?}"
    );
}

#[test]
fn the_words_for_where_a_link_goes_cover_every_kind_of_target() {
    assert_eq!(
        goes_in_words(Some(&Target::Page(4, Arrival::FitPage))),
        "goes to page 5"
    );
    assert_eq!(
        goes_in_words(Some(&Target::Address("https://a.org".to_owned()))),
        "goes to https://a.org"
    );
    assert!(goes_in_words(Some(&Target::Name("Intro".to_owned()))).contains("named place"));
    assert!(
        goes_in_words(Some(&Target::Document {
            file: "other.pdf".to_owned(),
            page: 1,
            arrival: Arrival::FitPage
        }))
        .contains("page 2 of the file other.pdf")
    );
    assert!(goes_in_words(None).contains("nowhere"));
}

fn made_one(desk: &mut Desk, handle: &str) -> String {
    desk.blocks(handle, 0).expect("read");
    desk.links(
        handle,
        &Action::Add {
            page: 0,
            place: Place::Block("p1-b1".to_owned()),
            goes: Goes::Address("https://example.org/anvils".to_owned()),
        },
    )
    .expect("added")
}

#[test]
fn a_link_added_over_a_block_is_listed_by_name_with_its_box_and_where_it_goes() {
    let (mut desk, handle) = with_paragraphs("linking-add", &["Anvils from Bangkok."]);
    let before = desk
        .links(&handle, &Action::List { page: 0 })
        .expect("listed");
    assert_eq!(before, "Page 1 has no links.");
    let said = made_one(&mut desk, &handle);
    assert!(said.starts_with("Added a link on page 1 over ["), "{said}");
    assert!(said.contains("https://example.org/anvils"), "{said}");
    let listed = desk
        .links(&handle, &Action::List { page: 0 })
        .expect("listed");
    assert!(listed.starts_with("Page 1 has 1 link,"), "{listed}");
    assert!(
        listed.contains("p1-l1 [72, ") && listed.contains("goes to https://example.org/anvils"),
        "{listed}"
    );
    let boxed = desk
        .links(
            &handle,
            &Action::Add {
                page: 0,
                place: Place::Area([300.0, 500.0, 400.0, 530.0]),
                goes: Goes::Page(0),
            },
        )
        .expect("a box");
    assert!(boxed.contains("that goes to page 1"), "{boxed}");
    let two = desk
        .links(&handle, &Action::List { page: 0 })
        .expect("listed");
    assert!(
        two.starts_with("Page 1 has 2 links,")
            && two.contains("p1-l2 [300, 500, 400, 530] goes to page 1"),
        "{two}"
    );
    assert!(
        desk.walk(&handle, true).expect("undone"),
        "adding a link is one undo step"
    );
    let undone = desk
        .links(&handle, &Action::List { page: 0 })
        .expect("listed");
    assert!(undone.starts_with("Page 1 has 1 link,"), "{undone}");
}

#[test]
fn a_link_that_is_too_small_or_over_a_block_on_another_page_or_to_nowhere_is_refused() {
    let (mut desk, handle) = with_paragraphs("linking-refused", &["Anvils."]);
    desk.blocks(&handle, 0).expect("read");
    let small = desk
        .links(
            &handle,
            &Action::Add {
                page: 0,
                place: Place::Area([10.0, 10.0, 11.0, 11.0]),
                goes: Goes::Page(0),
            },
        )
        .expect_err("refused");
    assert!(small.contains("too small for a link"), "{small}");
    let nowhere = desk
        .links(
            &handle,
            &Action::Add {
                page: 0,
                place: Place::Area([10.0, 10.0, 90.0, 40.0]),
                goes: Goes::Page(7),
            },
        )
        .expect_err("refused");
    assert!(nowhere.contains("there is no page 8"), "{nowhere}");
    let far = desk
        .links(&handle, &Action::List { page: 4 })
        .expect_err("refused");
    assert!(far.contains("there is no page 5"), "{far}");
    let unread = desk
        .links(
            &handle,
            &Action::Add {
                page: 0,
                place: Place::Block("p1-b9".to_owned()),
                goes: Goes::Page(0),
            },
        )
        .expect_err("refused");
    assert!(unread.contains("has not been read yet"), "{unread}");
}

#[test]
fn a_link_is_removed_by_the_name_a_list_gave_and_only_while_that_name_still_means_it() {
    let (mut desk, handle) = with_paragraphs("linking-remove", &["Anvils from Bangkok."]);
    let unlisted = desk
        .links(
            &handle,
            &Action::Remove {
                link: "p1-l1".to_owned(),
            },
        )
        .expect_err("refused");
    assert!(unlisted.contains("has not been listed yet"), "{unlisted}");
    made_one(&mut desk, &handle);
    desk.links(&handle, &Action::List { page: 0 })
        .expect("listed");
    desk.links(
        &handle,
        &Action::Add {
            page: 0,
            place: Place::Area([300.0, 500.0, 400.0, 530.0]),
            goes: Goes::Page(0),
        },
    )
    .expect("a second");
    let stale = desk
        .links(
            &handle,
            &Action::Remove {
                link: "p1-l1".to_owned(),
            },
        )
        .expect_err("a change since the list");
    assert!(
        stale.contains("has not been listed yet"),
        "adding forgot the list: {stale}"
    );
    desk.links(&handle, &Action::List { page: 0 })
        .expect("listed again");
    let said = desk
        .links(
            &handle,
            &Action::Remove {
                link: "p1-l1".to_owned(),
            },
        )
        .expect("removed");
    assert!(said.starts_with("Removed p1-l1"), "{said}");
    let left = desk
        .links(&handle, &Action::List { page: 0 })
        .expect("listed");
    assert!(
        left.starts_with("Page 1 has 1 link,") && !left.contains("example.org"),
        "{left}"
    );
    assert!(desk.walk(&handle, true).expect("undone"));
    assert!(
        desk.links(&handle, &Action::List { page: 0 })
            .expect("listed")
            .starts_with("Page 1 has 2 links,"),
        "undo brings it back"
    );
}

#[test]
fn a_link_is_the_one_it_was_when_listed_only_if_its_object_is_the_same() {
    let found = vec![(Reference::new(9, 0), [72.0, 700.0, 172.0, 720.0], None)];
    assert_eq!(
        the_link(&found, 0, Some(&anchor_of(Reference::new(9, 0))), "p1-l1"),
        Ok(Reference::new(9, 0))
    );
    let changed = the_link(&found, 0, Some(&anchor_of(Reference::new(10, 0))), "p1-l1")
        .expect_err("another object");
    assert!(changed.contains("not the link it was"), "{changed}");
    assert!(
        the_link(&found, 3, Some("9 0"), "p1-l4").is_err(),
        "past the end"
    );
    assert!(the_link(&found, 0, None, "p1-l1").is_err(), "never listed");
    let command = remove_command(2, Reference::new(9, 0));
    assert!(matches!(
        command,
        pdf_edit::Command::RemoveLink { page_index: 2, .. }
    ));
    let (mut desk, handle) = with_paragraphs("linking-listing", &["x"]);
    let geometry = desk.page_geometries(&handle).expect("measured")[0];
    let listed = listing(0, &found, &device_of(&geometry).expect("a device"));
    assert_eq!(listed.anchors, ["9 0"]);
    assert!(
        listed
            .text
            .contains("p1-l1 [72, 122, 172, 142] goes nowhere this can read"),
        "a box from the bottom of the page is listed from the top: {}",
        listed.text
    );
}
