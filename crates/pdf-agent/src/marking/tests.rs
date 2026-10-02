use crate::desk::Desk;
use crate::desk::tests::{view_of_page, with_paragraphs};
use crate::finding::Search;

use super::{How, Marking, YELLOW};

fn near(one: [f64; 3], other: [f64; 3]) -> bool {
    one.iter()
        .zip(&other)
        .all(|(left, right)| (left - right).abs() < 1e-9)
}

fn one_paragraph(words: &str) -> (Desk, String) {
    with_paragraphs("marking", &[words])
}

#[test]
fn a_highlight_is_a_translucent_band_over_the_found_words_and_one_undo_takes_it_back() {
    let (mut desk, handle) = one_paragraph("Pay the invoice by Friday. The invoice is overdue.");
    let marked = desk
        .mark_text(
            &handle,
            (
                &Search::exactly("invoice"),
                Marking::new(How::Highlight, None),
            ),
            (0, 0),
        )
        .expect("marked");
    assert_eq!(marked.per_page, vec![(0, 2)], "{marked:?}");
    assert_eq!(marked.commands.len(), 2, "one band for each place");
    for command in &marked.commands {
        let pdf_edit::Command::DrawPath { stroke, steps, .. } = command else {
            panic!("a path was expected: {command:?}");
        };
        let stroke = stroke.expect("a stroke");
        assert!(near(stroke.colour, YELLOW), "{:?}", stroke.colour);
        assert_eq!(stroke.blend, pdf_edit::PenBlend::Multiply);
        assert!((stroke.opacity - 0.4).abs() < 1e-9);
        assert!(
            stroke.width > 6.0 && stroke.width < 30.0,
            "{}",
            stroke.width
        );
        assert_eq!(steps.len(), 2);
    }
    let (source, _) = desk.source(&handle).expect("a source");
    assert!(
        desk.walk(&handle, true).expect("undo"),
        "one step to take back"
    );
    let (after, _) = desk.source(&handle).expect("a source");
    assert!(after.len() < source.len(), "the bands are gone");
}

fn pixels_of(desk: &mut Desk, handle: &str) -> (u32, u32, Vec<u8>) {
    let view = view_of_page(desk, handle, 0);
    let (canvas, _) = pdf_cli::render_page_view(&view, 1.0).expect("it draws");
    (canvas.width, canvas.height, canvas.to_rgb8())
}

#[test]
fn the_band_lies_over_the_words_it_marks_and_nowhere_else() {
    let (mut desk, handle) = one_paragraph("Pay the invoice by Friday.");
    let block = desk.blocks(&handle, 0).expect("read")[0].clone();
    let (width, _, before) = pixels_of(&mut desk, &handle);
    desk.mark_text(
        &handle,
        (
            &Search::exactly("invoice"),
            Marking::new(How::Highlight, None),
        ),
        (0, 0),
    )
    .expect("marked");
    let (_, _, after) = pixels_of(&mut desk, &handle);
    let across = usize::try_from(width).expect("a width");
    let mut changed: Vec<(usize, usize)> = Vec::new();
    for (at, (was, now)) in before.chunks(3).zip(after.chunks(3)).enumerate() {
        if was != now {
            changed.push((at % across, at / across));
        }
    }
    assert!(
        !changed.is_empty(),
        "the page looks the same: nothing was drawn"
    );
    let point = |value: usize| f64::from(u32::try_from(value).expect("a small coordinate"));
    let left = point(changed.iter().map(|at| at.0).min().expect("a pixel"));
    let right = point(changed.iter().map(|at| at.0).max().expect("a pixel"));
    let top = point(changed.iter().map(|at| at.1).min().expect("a pixel"));
    let bottom = point(changed.iter().map(|at| at.1).max().expect("a pixel"));
    let [block_left, block_top, block_right, block_bottom] = block.area;
    assert!(
        left >= block_left + 20.0 && right < block_right - 60.0,
        "{left}..{right} is not just the word inside {:?}",
        block.area
    );
    assert!(
        top >= block_top - 4.0 && bottom <= block_bottom + 6.0,
        "{top}..{bottom} is not on the line {:?}",
        block.area
    );
    let yellowish = changed.iter().any(|(across, down)| {
        let at = (down * usize::try_from(width).expect("a width") + across) * 3;
        after[at] > 200 && after[at + 1] > 200 && after[at + 2] < 200
    });
    assert!(yellowish, "the band is yellow");
}

#[test]
fn underlining_and_striking_through_are_thin_opaque_lines_in_black_unless_a_colour_is_given() {
    let (mut desk, handle) = one_paragraph("Strike this out.");
    let stroke_of = |how: How, colour: Option<[f64; 3]>, desk: &mut Desk| {
        let marked = desk
            .mark_text(
                &handle,
                (&Search::exactly("this"), Marking::new(how, colour)),
                (0, 0),
            )
            .expect("marked");
        let pdf_edit::Command::DrawPath { stroke, .. } = &marked.commands[0] else {
            panic!("a path was expected");
        };
        stroke.expect("a stroke")
    };
    let under = stroke_of(How::Underline, None, &mut desk);
    assert!(near(under.colour, [0.0; 3]), "{:?}", under.colour);
    assert_eq!(under.blend, pdf_edit::PenBlend::Normal);
    assert!((under.opacity - 1.0).abs() < 1e-9);
    assert!(under.width < 2.0, "{}", under.width);
    let strike = stroke_of(How::StrikeThrough, Some([1.0, 0.0, 0.0]), &mut desk);
    assert!(near(strike.colour, [1.0, 0.0, 0.0]), "{:?}", strike.colour);
}

#[test]
fn words_that_are_not_there_mark_nothing() {
    let (mut desk, handle) = one_paragraph("Nothing to see.");
    let marked = desk
        .mark_text(
            &handle,
            (
                &Search::exactly("invoice"),
                Marking::new(How::Highlight, None),
            ),
            (0, 0),
        )
        .expect("marked");
    assert!(
        marked.commands.is_empty() && marked.total() == 0,
        "{marked:?}"
    );
}
