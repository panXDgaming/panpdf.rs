use crate::desk::tests::{view_of_page, with_paragraphs};

use super::{Align, Look};

fn faces(desk: &mut crate::desk::Desk, handle: &str) -> Vec<Vec<pdf_edit::ClusterFace>> {
    let view = view_of_page(desk, handle, 0);
    let (_, parts) = crate::desk::read(&view, 0, 0).expect("a block");
    pdf_edit::read_block_faces(
        &view.program,
        &view.graph,
        &parts.rows,
        parts.frame,
        (0, 0),
        None,
    )
    .expect("the faces read")
}

#[test]
fn a_look_that_asks_for_nothing_says_so_and_one_that_asks_is_worded() {
    assert!(Look::default().asks_for_nothing());
    let look = Look {
        bold: Some(true),
        italic: Some(false),
        size: Some(14.0),
        fill: Some([1.0, 0.0, 0.0]),
        line_spacing: Some(1.5),
        align: Some(Align::Centre),
        ..Look::default()
    };
    assert!(!look.asks_for_nothing());
    assert_eq!(
        look.words(),
        "bold, not italic, 14 pt, colour #ff0000, line spacing 1.5 times the text size, centre text"
    );
}

#[test]
fn a_spacing_is_asked_in_times_the_text_size_and_given_to_the_engine_in_points() {
    let look = Look {
        line_spacing: Some(1.5),
        ..Look::default()
    };
    let style = look.text_style(12.0);
    assert_eq!(style.line_spacing, Some(18.0));
    assert_eq!(Look::default().text_style(12.0).line_spacing, None);
}

#[test]
fn alignment_words_are_understood_in_either_spelling() {
    assert_eq!(Align::of("center"), Some(Align::Centre));
    assert_eq!(Align::of("Centre"), Some(Align::Centre));
    assert_eq!(Align::of("justified"), Some(Align::Justify));
    assert_eq!(Align::of("end"), Some(Align::Right));
    assert_eq!(Align::of("sideways"), None);
}

#[test]
fn a_whole_block_is_made_bold_and_larger_in_one_step_and_reads_back() {
    let (mut desk, handle) = with_paragraphs("bold", &["A plain sentence to dress up."]);
    desk.blocks(&handle, 0).expect("read");
    let look = Look {
        bold: Some(true),
        size: Some(20.0),
        ..Look::default()
    };
    let now = desk.style(&handle, "p1-b1", None, &look).expect("styled");
    assert_eq!(now.text, "A plain sentence to dress up.");
    assert!((now.size - 20.0).abs() < 0.6, "{}", now.size);
    let faces = faces(&mut desk, &handle);
    assert!(
        faces[0]
            .iter()
            .filter(|face| !face.blank)
            .all(|face| face.bold),
        "{faces:?}"
    );
    assert!(desk.walk(&handle, true).expect("one undo"));
    let back = desk.blocks(&handle, 0).expect("read");
    assert!((back[0].size - 12.0).abs() < 0.6, "{}", back[0].size);
}

#[test]
fn only_the_words_found_are_styled_and_the_rest_keeps_its_look() {
    let (mut desk, handle) = with_paragraphs("span", &["Hello brave new world"]);
    desk.blocks(&handle, 0).expect("read");
    let look = Look {
        italic: Some(true),
        underline: Some(true),
        ..Look::default()
    };
    desk.style(&handle, "p1-b1", Some("brave"), &look)
        .expect("styled");
    let faces = faces(&mut desk, &handle);
    let row = &faces[0];
    assert!(
        row[6..11].iter().all(|face| face.italic && face.underline),
        "{row:?}"
    );
    assert!(
        row[0..5].iter().all(|face| !face.italic && !face.underline),
        "{row:?}"
    );
    assert!(row[12..].iter().all(|face| !face.italic), "{row:?}");
}

fn left_of_last_line(desk: &mut crate::desk::Desk, handle: &str) -> f64 {
    let view = view_of_page(desk, handle, 0);
    let overlay = pdf_cli::page_overlay_view(&view, 1.0).expect("an overlay");
    let last = *overlay.blocks[0].lines.last().expect("a line");
    overlay
        .clusters
        .iter()
        .filter(|cluster| cluster.line == last)
        .filter_map(|cluster| cluster.box_pixels)
        .map(|boxed| boxed[0])
        .fold(f64::INFINITY, f64::min)
}

#[test]
fn alignment_moves_the_short_last_line_across_the_frame() {
    let words = "Alignment needs a paragraph that runs over more than one line, so that the last line is shorter than the others.";
    let (mut desk, handle) = with_paragraphs("align", &[words]);
    let before = left_of_last_line(&mut desk, &handle);
    desk.blocks(&handle, 0).expect("read");
    let look = Look {
        align: Some(Align::Right),
        ..Look::default()
    };
    desk.style(&handle, "p1-b1", None, &look).expect("styled");
    let right = left_of_last_line(&mut desk, &handle);
    assert!(right > before + 50.0, "{before} then {right}");
    desk.blocks(&handle, 0).expect("read");
    let look = Look {
        align: Some(Align::Centre),
        ..Look::default()
    };
    desk.style(&handle, "p1-b1", None, &look).expect("styled");
    let centre = left_of_last_line(&mut desk, &handle);
    assert!(
        centre > before + 10.0 && centre < right - 10.0,
        "{before} then {centre} then {right}"
    );
}

#[test]
fn a_style_that_cannot_be_applied_says_why_and_leaves_the_block_alone() {
    let (mut desk, handle) = with_paragraphs("refused", &["Hello world"]);
    desk.blocks(&handle, 0).expect("read");
    let absent = desk
        .style(
            &handle,
            "p1-b1",
            Some("moon"),
            &Look {
                bold: Some(true),
                ..Look::default()
            },
        )
        .expect_err("not in the block");
    assert!(absent.contains("is not in the block"), "{absent}");
    let too_tight = desk
        .style(
            &handle,
            "p1-b1",
            None,
            &Look {
                line_spacing: Some(0.2),
                ..Look::default()
            },
        )
        .expect_err("tighter than the engine allows");
    assert!(too_tight.contains("0.8"), "{too_tight}");
    let unread = desk
        .style(
            &handle,
            "p1-b9",
            None,
            &Look {
                bold: Some(true),
                ..Look::default()
            },
        )
        .expect_err("never read");
    assert!(unread.contains("has not been read"), "{unread}");
    assert_eq!(
        desk.blocks(&handle, 0).expect("read")[0].text,
        "Hello world"
    );
}
