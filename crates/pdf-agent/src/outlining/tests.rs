use pdf_edit::outline::Bookmark;
use pdf_syntax::Reference;

use crate::desk::tests::with_paragraphs;
use crate::desk::{Block, Desk};

use super::{Action, Heading, Place, Step, headings_in, listing, moved, named};

fn bookmark(number: u32, title: &str, depth: usize, parent: Option<u32>) -> Bookmark {
    Bookmark {
        reference: Reference::new(number, 0),
        title: title.to_owned(),
        page: Some(usize::try_from(number).expect("small")),
        depth,
        open: true,
        children: 0,
        previous: None,
        parent: parent.map(|parent| Reference::new(parent, 0)),
    }
}

fn outline() -> Vec<Bookmark> {
    vec![
        bookmark(10, "One", 0, None),
        bookmark(11, "One A", 1, Some(10)),
        bookmark(12, "One B", 1, Some(10)),
        bookmark(13, "Two", 0, None),
    ]
}

#[test]
fn the_list_numbers_the_bookmarks_in_order_and_indents_the_nested_ones() {
    assert_eq!(
        listing(&outline()),
        "4 bookmarks, numbered in the order they are listed:\n\
         1. One (page 11)\n  2. One A (page 12)\n  3. One B (page 13)\n4. Two (page 14)"
    );
    assert_eq!(listing(&[]), "The document has no bookmarks.");
}

#[test]
fn a_number_that_is_not_in_the_list_is_refused_and_the_list_is_told_to_have_changed() {
    let list = outline();
    assert_eq!(named(&list, 3).expect("the third").title, "One B");
    let why = named(&list, 0).expect_err("from one");
    assert!(why.contains("there is no bookmark 0"), "{why}");
    let why = named(&list, 5).expect_err("past the end");
    assert!(why.contains("the document has 4"), "{why}");
}

#[test]
fn a_bookmark_moves_up_down_in_and_out_within_its_own_level() {
    let list = outline();
    let change = |number: usize, step: Step| moved(&list, number, step);
    assert!(matches!(
        change(3, Step::Up).expect("up"),
        pdf_edit::outline::Change::Move {
            before: true,
            inside: false,
            ..
        }
    ));
    assert!(
        change(2, Step::Up).is_err(),
        "the first of its level cannot go up"
    );
    assert!(
        change(4, Step::Down).is_err(),
        "the last of its level cannot go down"
    );
    assert!(matches!(
        change(3, Step::In).expect("in"),
        pdf_edit::outline::Change::Move { inside: true, .. }
    ));
    assert!(
        change(1, Step::In).is_err(),
        "nothing above it to go inside"
    );
    assert!(matches!(
        change(2, Step::Out).expect("out"),
        pdf_edit::outline::Change::Move {
            inside: false,
            before: false,
            ..
        }
    ));
    assert!(change(1, Step::Out).is_err(), "already at the top");
}

fn block(page: usize, index: usize, text: &str, size: f64) -> Block {
    Block {
        page,
        index,
        text: text.to_owned(),
        area: [0.0; 4],
        size,
        fixed: None,
    }
}

#[test]
fn the_headings_are_the_blocks_set_larger_than_the_body_and_nest_by_size() {
    let body = "word ".repeat(80);
    let pages = vec![
        (
            0,
            vec![
                block(0, 0, "Annual report", 24.0),
                block(0, 1, "Summary", 16.0),
                block(0, 2, &body, 11.0),
                block(0, 3, "Running header", 18.0),
            ],
        ),
        (
            1,
            vec![
                block(1, 0, "Running header", 18.0),
                block(1, 1, "Results", 16.0),
                block(1, 2, &body, 11.0),
            ],
        ),
        (
            2,
            vec![
                block(2, 0, "Running header", 18.0),
                block(2, 1, &body, 11.0),
                block(2, 2, "Appendix", 16.0),
            ],
        ),
    ];
    let found = headings_in(&pages);
    let titles: Vec<(&str, usize, usize)> = found
        .iter()
        .map(|heading| (heading.title.as_str(), heading.page, heading.level))
        .collect();
    assert_eq!(
        titles,
        vec![
            ("Annual report", 0, 0),
            ("Summary", 0, 1),
            ("Results", 1, 1),
            ("Appendix", 2, 1)
        ],
        "the line repeated on every page is a running header, not a heading"
    );
    let flat = vec![(
        0,
        vec![block(0, 0, &body, 11.0), block(0, 1, "Same size", 11.0)],
    )];
    assert_eq!(
        headings_in(&flat),
        Vec::<Heading>::new(),
        "negative control: no larger text"
    );
}

fn with_pages(name: &str) -> (Desk, String) {
    let (mut desk, handle) = with_paragraphs(name, &["Body text on the first page."]);
    for _ in 0..2 {
        desk.command(
            &handle,
            &pdf_edit::Command::AddBlankPage {
                beside: 0,
                before: false,
                size: [595.0, 842.0],
            },
        )
        .expect("a page is added");
    }
    (desk, handle)
}

fn add(title: &str, page: usize, after: Option<usize>, inside: Option<usize>) -> Action {
    Action::Add {
        title: Some(title.to_owned()),
        place: Place::Page(page),
        after,
        inside,
    }
}

#[test]
fn bookmarks_are_added_renamed_nested_moved_and_deleted_and_each_is_one_step() {
    let (mut desk, handle) = with_pages("outline");
    let said = desk
        .bookmarks(&handle, &add("Chapter one", 0, None, None))
        .expect("added");
    assert!(said.contains("1. Chapter one (page 1)"), "{said}");
    desk.bookmarks(&handle, &add("Chapter two", 2, None, None))
        .expect("added");
    let said = desk
        .bookmarks(&handle, &add("A part of one", 1, None, Some(1)))
        .expect("added inside");
    assert!(
        said.contains(
            "1. Chapter one (page 1)\n  2. A part of one (page 2)\n3. Chapter two (page 3)"
        ),
        "{said}"
    );
    let said = desk
        .bookmarks(
            &handle,
            &Action::Rename {
                bookmark: 3,
                title: "The end".to_owned(),
            },
        )
        .expect("renamed");
    assert!(
        said.contains("Renamed \u{201c}Chapter two\u{201d}"),
        "{said}"
    );
    assert!(said.contains("3. The end (page 3)"), "{said}");
    let said = desk
        .bookmarks(
            &handle,
            &Action::Retarget {
                bookmark: 3,
                page: 1,
            },
        )
        .expect("retargeted");
    assert!(said.contains("3. The end (page 2)"), "{said}");
    let said = desk
        .bookmarks(
            &handle,
            &Action::Move {
                bookmark: 2,
                step: Step::Out,
            },
        )
        .expect("moved out");
    assert!(
        said.contains("1. Chapter one (page 1)\n2. A part of one (page 2)"),
        "{said}"
    );
    let said = desk
        .bookmarks(
            &handle,
            &Action::Move {
                bookmark: 3,
                step: Step::Up,
            },
        )
        .expect("moved up");
    assert!(
        said.contains("2. The end (page 2)\n3. A part of one (page 2)"),
        "{said}"
    );
    let said = desk
        .bookmarks(&handle, &Action::Delete { bookmark: 2 })
        .expect("deleted");
    assert!(said.contains("2 bookmarks"), "{said}");
    assert!(!said.contains("The end (page 2)\n"), "{said}");
    let listed = desk.bookmarks(&handle, &Action::List).expect("listed");
    assert!(listed.starts_with("2 bookmarks"), "{listed}");
    assert!(desk.walk(&handle, true).expect("one undo"));
    let listed = desk.bookmarks(&handle, &Action::List).expect("listed");
    assert!(
        listed.starts_with("3 bookmarks"),
        "the deletion was one step: {listed}"
    );
}

#[test]
fn a_bookmark_that_is_not_there_or_a_page_that_is_not_there_is_refused_in_words() {
    let (mut desk, handle) = with_pages("refused");
    let why = desk
        .bookmarks(&handle, &Action::Delete { bookmark: 1 })
        .expect_err("none yet");
    assert!(
        why.contains("there is no bookmark 1: the document has 0"),
        "{why}"
    );
    let why = desk
        .bookmarks(&handle, &add("Off the end", 9, None, None))
        .expect_err("no page 10");
    assert!(
        why.contains("there is no page 10: the document has 3"),
        "{why}"
    );
    let why = desk
        .bookmarks(&handle, &add("   ", 0, None, None))
        .expect_err("no title");
    assert!(why.contains("`title` is needed"), "{why}");
    let why = desk
        .bookmarks(&handle, &add("Both", 0, Some(1), Some(1)))
        .expect_err("two places at once");
    assert!(why.contains("not both"), "{why}");
}

#[test]
fn a_block_is_bookmarked_under_its_own_words_on_its_own_page() {
    let (mut desk, handle) = with_pages("block");
    desk.blocks(&handle, 0).expect("read");
    let said = desk
        .bookmarks(
            &handle,
            &Action::Add {
                title: None,
                place: Place::Block("p1-b1".to_owned()),
                after: None,
                inside: None,
            },
        )
        .expect("added");
    assert!(
        said.contains("1. Body text on the first page. (page 1)"),
        "{said}"
    );
    let why = desk
        .bookmarks(
            &handle,
            &Action::Add {
                title: None,
                place: Place::Block("p2-b1".to_owned()),
                after: None,
                inside: None,
            },
        )
        .expect_err("never read");
    assert!(why.contains("has not been read"), "{why}");
}

#[test]
fn a_table_of_contents_is_made_from_the_headings_in_one_step_and_nests_by_size() {
    let (mut desk, handle) = with_pages("contents");
    let at = |desk: &mut Desk, page: usize, top: f64, size: f64, text: &str| {
        desk.place_text(
            &handle,
            page,
            [72.0, top, 500.0, top + 3.0 * size],
            text,
            ("DejaVu Sans", size, false, false, None),
        )
        .expect("placed");
    };
    at(&mut desk, 0, 150.0, 26.0, "Annual report");
    at(&mut desk, 0, 250.0, 18.0, "Summary");
    at(&mut desk, 1, 100.0, 18.0, "Results");
    at(
        &mut desk,
        1,
        200.0,
        12.0,
        &"A long body of words. ".repeat(12),
    );
    at(&mut desk, 2, 100.0, 18.0, "Appendix");
    let said = desk
        .bookmarks(&handle, &Action::FromHeadings { replace: false })
        .expect("made");
    assert!(
        said.contains("Made 4 bookmarks from the headings"),
        "{said}"
    );
    let listed = desk.bookmarks(&handle, &Action::List).expect("listed");
    assert!(
        listed.ends_with(
            "1. Annual report (page 1)\n  2. Summary (page 1)\n  3. Results (page 2)\n  4. Appendix (page 3)"
        ),
        "{listed}"
    );
    let why = desk
        .bookmarks(&handle, &Action::FromHeadings { replace: false })
        .expect_err("already has them");
    assert!(why.contains("already has 4 bookmarks"), "{why}");
    assert!(
        desk.walk(&handle, true)
            .expect("one undo takes back the whole table")
    );
    let listed = desk.bookmarks(&handle, &Action::List).expect("listed");
    assert_eq!(listed, "The document has no bookmarks.");
    desk.bookmarks(&handle, &Action::FromHeadings { replace: false })
        .expect("made again");
    let again = desk
        .bookmarks(&handle, &Action::FromHeadings { replace: true })
        .expect("replaced");
    assert!(
        again.contains("after taking out the 1 that were there"),
        "{again}"
    );
    assert!(again.contains("1. Annual report (page 1)"), "{again}");
}
