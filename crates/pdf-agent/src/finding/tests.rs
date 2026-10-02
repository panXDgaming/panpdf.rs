use crate::desk::Desk;
use crate::desk::tests::{view_of_page, with_paragraphs};

use super::{Search, matches_in};

fn texts(desk: &mut Desk, handle: &str) -> Vec<String> {
    desk.blocks(handle, 0)
        .expect("the page reads")
        .into_iter()
        .map(|block| block.text)
        .collect()
}

fn reading_of(desk: &mut Desk, handle: &str) -> pdf_edit::BlockReading {
    let view = view_of_page(desk, handle, 0);
    crate::desk::read(&view, 0, 0).expect("a block").1.reading
}

#[test]
fn a_search_finds_every_place_and_says_where_in_the_block_it_starts() {
    let (mut desk, handle) = with_paragraphs("search", &["one two one three one"]);
    let reading = reading_of(&mut desk, &handle);
    let matches = matches_in(&reading, &Search::exactly("one"));
    assert_eq!(matches.found.len(), 3, "{matches:?}");
    assert_eq!(matches.found[0].bytes, 0..3);
    assert_eq!(matches.found[1].bytes, 8..11);
    assert_eq!(matches.found[2].bytes, 18..21);
    assert_eq!(matches.found[1].from, (0, 8));
    assert_eq!(matches.found[1].to, (0, 11));
    let none = matches_in(&reading, &Search::exactly("four"));
    assert!(none.found.is_empty() && none.inside_a_character == 0);
}

#[test]
fn a_search_ignores_capitals_unless_told_not_to_and_can_ask_for_whole_words() {
    let (mut desk, handle) = with_paragraphs("options", &["Cat concatenate CAT cat's"]);
    let reading = reading_of(&mut desk, &handle);
    let search = |wanted: &str, match_case: bool, whole_words: bool| Search {
        wanted: wanted.to_owned(),
        match_case,
        whole_words,
    };
    let count = |search: &Search| matches_in(&reading, search).found.len();
    assert_eq!(
        count(&search("cat", false, false)),
        4,
        "Cat, conCATenate, CAT and cat's"
    );
    assert_eq!(
        count(&search("cat", true, false)),
        2,
        "only the lower case ones"
    );
    assert_eq!(
        count(&search("cat", false, true)),
        3,
        "Cat, CAT and the cat of cat's, not the one inside concatenate"
    );
    assert_eq!(
        count(&search("cat", true, true)),
        1,
        "negative control: both options at once"
    );
}

#[test]
fn every_word_in_the_document_is_replaced_in_one_step_and_counted_by_page() {
    let (mut desk, handle) = with_paragraphs(
        "everywhere",
        &[
            "Acme sells anvils. Acme ships fast.",
            "Ask Acme about rockets.",
            "Nothing here.",
        ],
    );
    let found = desk
        .replace_everywhere(&handle, (&Search::exactly("Acme"), "Beta"), (0, 0))
        .expect("replaced");
    assert_eq!(found.per_page, vec![(0, 3)], "{found:?}");
    assert!(found.left.is_empty(), "{:?}", found.left);
    let after = texts(&mut desk, &handle);
    assert_eq!(
        after,
        vec![
            "Beta sells anvils. Beta ships fast.".to_owned(),
            "Ask Beta about rockets.".to_owned(),
            "Nothing here.".to_owned()
        ]
    );
    assert!(desk.walk(&handle, true).expect("one undo"));
    let before = texts(&mut desk, &handle);
    assert_eq!(
        before[0], "Acme sells anvils. Acme ships fast.",
        "{before:?}"
    );
    assert_eq!(before[1], "Ask Acme about rockets.", "{before:?}");
}

fn works_on_page(desk: &mut Desk, handle: &str, with: &str, reject: &[usize]) -> super::PageWork {
    let view = view_of_page(desk, handle, 0);
    let mut seen = 0;
    super::replacements_on(&view, 0, (&Search::exactly("Acme"), with), &mut |_| {
        seen += 1;
        if reject.contains(&seen) {
            Err("the engine said no".to_owned())
        } else {
            Ok(())
        }
    })
}

#[test]
fn a_block_the_engine_would_refuse_is_left_alone_and_named_while_the_others_are_replaced() {
    let (mut desk, handle) = with_paragraphs("refusal", &["Acme one.", "Acme two.", "Acme three."]);
    let work = works_on_page(&mut desk, &handle, "Beta", &[2]);
    assert_eq!(work.found, 2, "{work:?}");
    assert_eq!(work.commands.len(), 2);
    assert_eq!(work.left.len(), 1, "{:?}", work.left);
    assert!(
        work.left[0].contains("could not be changed: the engine said no"),
        "{:?}",
        work.left
    );
    let named = work.left[0].split(' ').next().expect("a name");
    assert!(named.starts_with("p1-b"), "{named}");
    let none = works_on_page(&mut desk, &handle, "Beta", &[1, 2, 3]);
    assert!(
        none.commands.is_empty() && none.found == 0,
        "negative control: {none:?}"
    );
    assert_eq!(none.left.len(), 3);
}

#[test]
fn text_the_page_cannot_draw_is_refused_for_the_block_and_the_document_is_untouched() {
    let (mut desk, handle) = with_paragraphs("undrawable", &["Acme one.", "Acme two."]);
    let before = texts(&mut desk, &handle);
    let found = desk
        .replace_everywhere(&handle, (&Search::exactly("Acme"), "\u{13000}"), (0, 0))
        .expect("the call itself goes through");
    assert!(found.commands.is_empty(), "{found:?}");
    assert_eq!(found.left.len(), 2, "{:?}", found.left);
    assert!(
        found
            .left
            .iter()
            .all(|why| why.contains("could not be changed")),
        "{:?}",
        found.left
    );
    assert_eq!(texts(&mut desk, &handle), before);
}

#[test]
fn commands_made_for_several_blocks_only_apply_last_block_first() {
    let (mut desk, handle) = with_paragraphs("order", &["Acme one.", "Acme two.", "Acme three."]);
    let work = works_on_page(&mut desk, &handle, "Beta", &[]);
    assert_eq!(work.commands.len(), 3);
    let mut reading_order = work.commands.clone();
    reading_order.reverse();
    let refused = desk
        .commands(&handle, &reading_order)
        .expect_err("the engine finds the second block's place gone once the first is rewritten");
    assert!(
        refused.contains("no text run is written where the selection anchors"),
        "{refused}"
    );
    desk.commands(&handle, &work.commands)
        .expect("the same commands, last block first, go through");
    assert_eq!(
        texts(&mut desk, &handle),
        vec![
            "Beta one.".to_owned(),
            "Beta two.".to_owned(),
            "Beta three.".to_owned()
        ]
    );
}
