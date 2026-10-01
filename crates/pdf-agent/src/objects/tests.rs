use pdf_edit::{Command, PenStep, PenStroke};

use crate::desk::Desk;
use crate::desk::tests::with_paragraphs;
use crate::pictures::tests::red_square;
use crate::pictures::{Asked, Source};

use super::{Action, Name, object_name, parse_name};

#[test]
fn a_name_says_whether_it_is_a_picture_or_drawing_or_a_block_of_text() {
    assert_eq!(parse_name("p3-o2"), Ok(Name::Object { page: 2, index: 1 }));
    assert_eq!(
        parse_name(" p12-b7 "),
        Ok(Name::Block { page: 11, index: 6 })
    );
    assert_eq!(object_name(2, 1), "p3-o2");
    for bad in [
        "", "p0-o1", "p1-o0", "p1-x1", "o1", "p1-o", "p-o1", "3-o1", "p1o1",
    ] {
        let why = parse_name(bad).expect_err(bad);
        assert!(why.contains("is not the name of an object"), "{bad}: {why}");
    }
}

fn picture_at(desk: &mut Desk, handle: &str, (left, top, width): (f64, f64, f64)) {
    desk.place_picture(
        handle,
        &Asked {
            page: 0,
            left,
            top,
            width: Some(width),
            height: None,
            source: Source::Attachment(None),
        },
        red_square(40, 20),
    )
    .expect("placed");
}

fn listed(desk: &mut Desk, handle: &str) -> String {
    desk.objects(handle, &Action::List { page: 0 })
        .expect("listed")
}

#[test]
fn a_picture_is_listed_with_its_name_and_box_and_found_again_by_that_name() {
    let (mut desk, handle) = with_paragraphs("listing", &["Some words."]);
    picture_at(&mut desk, &handle, (100.0, 300.0, 200.0));
    let said = listed(&mut desk, &handle);
    assert!(
        said.contains("1 picture and drawing, and 1 text block"),
        "{said}"
    );
    assert!(
        said.contains("p1-o1 picture [100, 300, 300, 400]"),
        "{said}"
    );
    assert!(said.contains("p1-b1 text, 12 pt"), "{said}");
    assert!(said.contains("\u{201c}Some words.\u{201d}"), "{said}");
}

#[test]
fn a_picture_is_moved_by_its_top_left_corner_and_resized_keeping_its_shape() {
    let (mut desk, handle) = with_paragraphs("moves", &["Some words."]);
    picture_at(&mut desk, &handle, (100.0, 300.0, 200.0));
    listed(&mut desk, &handle);
    let said = desk
        .objects(
            &handle,
            &Action::Move {
                object: "p1-o1".to_owned(),
                left: Some(50.0),
                top: Some(500.0),
            },
        )
        .expect("moved");
    assert!(said.contains("Moved it to left 50, top 500"), "{said}");
    assert!(
        desk.objects(
            &handle,
            &Action::Move {
                object: "p1-o1".to_owned(),
                left: None,
                top: Some(1.0),
            }
        )
        .is_err(),
        "the page changed under the name, so it must be listed again"
    );
    let again = listed(&mut desk, &handle);
    assert!(
        again.contains("p1-o1 picture [50, 500, 250, 600]"),
        "{again}"
    );
    let said = desk
        .objects(
            &handle,
            &Action::Resize {
                object: "p1-o1".to_owned(),
                width: Some(100.0),
                height: None,
            },
        )
        .expect("resized");
    assert!(said.contains("Resized it to 100 x 50 pt"), "{said}");
    let after = listed(&mut desk, &handle);
    assert!(
        after.contains("p1-o1 picture [50, 500, 150, 550]"),
        "{after}"
    );
    assert!(desk.walk(&handle, true).expect("undo the resize"));
    assert!(listed(&mut desk, &handle).contains("p1-o1 picture [50, 500, 250, 600]"));
}

#[test]
fn deleting_a_picture_takes_only_it_away_and_one_undo_brings_it_back() {
    let (mut desk, handle) = with_paragraphs("deletes", &["Some words."]);
    picture_at(&mut desk, &handle, (100.0, 300.0, 200.0));
    picture_at(&mut desk, &handle, (100.0, 500.0, 100.0));
    let both = listed(&mut desk, &handle);
    assert!(both.contains("2 pictures and drawings"), "{both}");
    let said = desk
        .objects(
            &handle,
            &Action::Delete {
                object: "p1-o1".to_owned(),
            },
        )
        .expect("deleted");
    assert!(said.contains("Deleted the picture"), "{said}");
    let one = listed(&mut desk, &handle);
    assert!(one.contains("1 picture and drawing"), "{one}");
    assert!(
        one.contains("[100, 500, 200, 550]"),
        "the other is still there: {one}"
    );
    assert!(desk.walk(&handle, true).expect("undo"));
    assert!(listed(&mut desk, &handle).contains("2 pictures and drawings"));
}

#[test]
fn a_drawing_is_listed_and_moved_like_a_picture() {
    let (mut desk, handle) = with_paragraphs("drawing", &["Some words."]);
    let steps = vec![
        PenStep::Move((100.0, 600.0)),
        PenStep::Line((200.0, 600.0)),
        PenStep::Line((200.0, 650.0)),
        PenStep::Line((100.0, 650.0)),
    ];
    desk.command(
        &handle,
        &Command::DrawPath {
            page_index: 0,
            steps,
            closed: true,
            stroke: Some(PenStroke::pen([0.0, 0.0, 1.0], 2.0)),
            fill: None,
        },
    )
    .expect("a drawing");
    let said = listed(&mut desk, &handle);
    assert!(said.contains("p1-o1 drawing"), "{said}");
    desk.objects(
        &handle,
        &Action::Move {
            object: "p1-o1".to_owned(),
            left: Some(300.0),
            top: None,
        },
    )
    .expect("moved");
    let after = listed(&mut desk, &handle);
    assert!(
        after.contains("p1-o1 drawing [300, 192, 400, 242]"),
        "{after}"
    );
}

#[test]
fn a_block_of_text_is_moved_by_name_and_told_how_to_resize_or_delete() {
    let (mut desk, handle) = with_paragraphs("blocks", &["Move me please."]);
    desk.blocks(&handle, 0).expect("read");
    let said = desk
        .objects(
            &handle,
            &Action::Move {
                object: "p1-b1".to_owned(),
                left: Some(200.0),
                top: Some(400.0),
            },
        )
        .expect("moved");
    assert!(said.contains("Moved p1-b1 to left 200, top 400"), "{said}");
    let now = desk.blocks(&handle, 0).expect("read");
    assert!((now[0].area[0] - 200.0).abs() < 1.5, "{:?}", now[0].area);
    assert!((now[0].area[1] - 400.0).abs() < 1.5, "{:?}", now[0].area);
    let why = desk
        .objects(
            &handle,
            &Action::Resize {
                object: "p1-b1".to_owned(),
                width: Some(50.0),
                height: None,
            },
        )
        .expect_err("text is not resized this way");
    assert!(why.contains("style_text"), "{why}");
    let why = desk
        .objects(
            &handle,
            &Action::Delete {
                object: "p1-b1".to_owned(),
            },
        )
        .expect_err("text is not deleted this way");
    assert!(why.contains("replace_text"), "{why}");
}

#[test]
fn an_object_never_listed_or_on_a_page_that_is_not_there_is_refused() {
    let (mut desk, handle) = with_paragraphs("refused", &["Words."]);
    picture_at(&mut desk, &handle, (100.0, 300.0, 200.0));
    let why = desk
        .objects(
            &handle,
            &Action::Delete {
                object: "p1-o1".to_owned(),
            },
        )
        .expect_err("never listed");
    assert!(why.contains("has not been listed yet"), "{why}");
    let why = desk
        .objects(
            &handle,
            &Action::Delete {
                object: "p4-o1".to_owned(),
            },
        )
        .expect_err("no page 4");
    assert!(why.contains("there is no page 4"), "{why}");
    listed(&mut desk, &handle);
    let why = desk
        .objects(
            &handle,
            &Action::Delete {
                object: "p1-o2".to_owned(),
            },
        )
        .expect_err("only one is listed");
    assert!(why.contains("has not been listed yet"), "{why}");
    let why = desk
        .objects(
            &handle,
            &Action::Resize {
                object: "p1-o1".to_owned(),
                width: None,
                height: None,
            },
        )
        .expect_err("no size asked");
    assert!(why.contains("`width` or `height` is needed"), "{why}");
}

#[test]
fn a_region_of_a_page_is_drawn_larger_than_the_page_would_be() {
    let (mut desk, handle) = with_paragraphs("closer", &["Small print."]);
    let (_, width, height) = desk
        .look_closer(&handle, 0, [60.0, 60.0, 160.0, 110.0], 288.0)
        .expect("drawn");
    assert!((395..=405).contains(&width), "{width}");
    assert!((195..=205).contains(&height), "{height}");
    let (_, whole, _) = desk
        .look_closer(&handle, 0, [0.0, 0.0, 595.0, 842.0], 600.0)
        .expect("drawn");
    assert!(
        whole <= 2400,
        "the longer side is held to 2400 pixels, not {whole}"
    );
    let (_, clipped, _) = desk
        .look_closer(&handle, 0, [500.0, 700.0, 900.0, 1200.0], 72.0)
        .expect("the part on the page is drawn");
    assert!(
        (90..=97).contains(&clipped),
        "only 95 points of it are on the page: {clipped}"
    );
}

#[test]
fn a_region_off_the_page_or_a_page_that_is_not_there_is_refused_in_words() {
    let (mut desk, handle) = with_paragraphs("closer-refused", &["Small print."]);
    let why = desk
        .look_closer(&handle, 0, [700.0, 900.0, 800.0, 950.0], 144.0)
        .expect_err("off the page");
    assert!(why.contains("has nothing of the page in it"), "{why}");
    assert!(why.contains("595 x 842"), "{why}");
    let why = desk
        .look_closer(&handle, 4, [0.0, 0.0, 10.0, 10.0], 144.0)
        .expect_err("no page 5");
    assert!(why.contains("there is no page 5"), "{why}");
}
