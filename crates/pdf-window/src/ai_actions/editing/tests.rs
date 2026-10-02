use std::sync::Arc;

use pdf_agent::connect::{Attachment, ToolCall, ToolResult, Turn};
use pdf_agent::json::Json;
use pdf_app::ai_permission::{Decision, Mode};

use crate::ai_actions::Performed;
use crate::ai_actions::tests::{a_blank_window, land_what_was_sent, tool, undo_steps_left};
use crate::window_state::Window;

pub(crate) fn load_page(window: &mut Window, page: usize) {
    let source = window.editor.source().cloned().expect("a document");
    let view = pdf_session::interpret_page_fully(&source, page, b"", None, window.editor.fonts())
        .expect("the page reads");
    window.editor.adopt_page(page, Arc::new(view));
}

pub(crate) fn run_on_pages(window: &mut Window, call: &ToolCall) -> ToolResult {
    window.ai.tools.take(std::slice::from_ref(call));
    window.ai.tools.revision_before = window.editor.revision();
    for _ in 0..50 {
        match window.perform(call) {
            Performed::Done(result) => {
                window.ai.tools.answer(result);
                break;
            }
            Performed::Sent => {
                land_what_was_sent(window);
                break;
            }
            Performed::NeedPages(pages) => {
                for page in pages {
                    load_page(window, page);
                }
            }
            Performed::Busy | Performed::Waiting => panic!("{} was left waiting", call.name),
        }
    }
    window
        .ai
        .tools
        .results
        .pop()
        .expect("the call was answered")
}

pub(crate) fn a_window_written(markdown: &str) -> Window {
    let mut window = a_blank_window();
    assert!(
        window.editor.fonts().is_some(),
        "this test needs the packaged fonts"
    );
    let write = tool(
        "w0",
        "write_pages",
        &format!(
            r#"{{"document":"doc-1","markdown":{},"font":"DejaVu Sans","replace":true}}"#,
            Json::text(markdown).write()
        ),
    );
    let result = run_on_pages(&mut window, &write);
    assert!(!result.is_error, "{}", result.text);
    window
}

pub(crate) fn said(window: &mut Window, name: &str, arguments: &str) -> String {
    let result = run_on_pages(window, &tool("t", name, arguments));
    assert!(!result.is_error, "{name} {arguments}: {}", result.text);
    result.text
}

pub(crate) fn refused(window: &mut Window, name: &str, arguments: &str) -> String {
    let result = run_on_pages(window, &tool("t", name, arguments));
    assert!(
        result.is_error,
        "{name} {arguments} went ahead: {}",
        result.text
    );
    result.text
}

pub(crate) fn text_of_page(window: &mut Window, page: usize) -> String {
    said(
        window,
        "read_text",
        &format!(
            r#"{{"document":"doc-1","first_page":{0},"last_page":{0}}}"#,
            page + 1
        ),
    )
}

#[test]
fn every_place_in_the_document_is_replaced_by_one_call_that_is_one_undo_step() {
    let mut window =
        a_window_written("Acme sells anvils. Acme ships fast.\n\nAsk Acme about rockets.");
    said(
        &mut window,
        "add_blank_page",
        r#"{"document":"doc-1","after_page":1}"#,
    );
    said(
        &mut window,
        "add_text",
        r#"{"document":"doc-1","page":2,"left":72,"top":72,"width":300,"text":"Acme again on page two","font":"DejaVu Sans"}"#,
    );
    let text = said(
        &mut window,
        "find_and_replace",
        r#"{"document":"doc-1","find":"Acme","replace_with":"Beta"}"#,
    );
    assert!(
        text.contains("Replaced 4 matches of \u{201c}Acme\u{201d} with \u{201c}Beta\u{201d} on 2 pages (page 1: 3, page 2: 1)"),
        "{text}"
    );
    let first = text_of_page(&mut window, 0);
    assert!(
        !first.contains("Acme") && first.contains("Beta sells anvils. Beta ships fast."),
        "{first}"
    );
    assert!(text_of_page(&mut window, 1).contains("Beta again on page two"));
    assert_eq!(
        window.ai.tools.run.steps(),
        4,
        "the write, the page, the text, the replace"
    );
    assert_eq!(
        undo_steps_left(&mut window),
        4,
        "the replace was one step, not four"
    );
}

#[test]
fn a_replacement_that_finds_nothing_changes_nothing_and_says_where_it_looked() {
    let mut window = a_window_written("Nothing to see here.");
    let before = window.editor.revision();
    let text = said(
        &mut window,
        "find_and_replace",
        r#"{"document":"doc-1","find":"Gamma","replace_with":"Delta","first_page":1,"last_page":1}"#,
    );
    assert!(text.starts_with("Nothing was replaced"), "{text}");
    assert_eq!(
        window.editor.revision(),
        before,
        "the document is as it was"
    );
    let capital = said(
        &mut window,
        "find_and_replace",
        r#"{"document":"doc-1","find":"NOTHING","replace_with":"Something","match_case":true}"#,
    );
    assert!(
        capital.starts_with("Nothing was replaced"),
        "match_case is honoured: {capital}"
    );
    let words = said(
        &mut window,
        "find_and_replace",
        r#"{"document":"doc-1","find":"Noth","replace_with":"Some","whole_words":true}"#,
    );
    assert!(
        words.starts_with("Nothing was replaced"),
        "whole_words is honoured: {words}"
    );
    let loose = said(
        &mut window,
        "find_and_replace",
        r#"{"document":"doc-1","find":"nothing","replace_with":"Something"}"#,
    );
    assert!(
        loose.starts_with("Replaced 1 match"),
        "negative control: the same words found: {loose}"
    );
}

#[test]
fn a_block_is_styled_in_one_step_and_the_words_are_untouched() {
    let mut window = a_window_written("A plain sentence to dress up.");
    let read = text_of_page(&mut window, 0);
    let name = read
        .split('[')
        .nth(1)
        .and_then(|rest| rest.split(' ').next())
        .expect("a block name")
        .to_owned();
    let before = window.editor.revision();
    let text = said(
        &mut window,
        "style_text",
        &format!(r#"{{"document":"doc-1","block":"{name}","bold":true,"size":20}}"#),
    );
    assert!(
        text.starts_with(&format!("Styled {name}: bold, 20 pt")),
        "{text}"
    );
    assert_ne!(window.editor.revision(), before);
    load_page(&mut window, 0);
    let leaf = window.editor.leaf(0).cloned().expect("a leaf");
    let blocks: Vec<_> = (0..leaf.view.index.blocks.len())
        .filter_map(|index| pdf_agent::desk::read_block(&leaf.view, 0, index))
        .collect();
    let styled = blocks
        .iter()
        .find(|block| block.text == "A plain sentence to dress up.")
        .expect("the same words");
    assert!((styled.size - 20.0).abs() < 0.6, "{}", styled.size);
    assert_eq!(undo_steps_left(&mut window), 2, "the write and the style");
}

#[test]
fn only_the_words_found_are_styled_and_a_style_with_no_change_is_refused() {
    let mut window = a_window_written("Hello brave new world");
    let read = text_of_page(&mut window, 0);
    let name = read
        .split('[')
        .nth(1)
        .and_then(|rest| rest.split(' ').next())
        .expect("a name")
        .to_owned();
    let text = said(
        &mut window,
        "style_text",
        &format!(r#"{{"document":"doc-1","block":"{name}","find":"brave","italic":true}}"#),
    );
    assert!(text.contains("(the words \u{201c}brave\u{201d})"), "{text}");
    let why = refused(
        &mut window,
        "style_text",
        &format!(r#"{{"document":"doc-1","block":"{name}","find":"moon","bold":true}}"#),
    );
    assert!(
        why.contains("is not in the block") || why.contains("no longer on the page"),
        "{why}"
    );
    let none = refused(
        &mut window,
        "style_text",
        &format!(r#"{{"document":"doc-1","block":"{name}"}}"#),
    );
    assert!(none.contains("nothing to change"), "{none}");
}

#[test]
fn words_are_marked_with_a_band_in_one_step_and_the_count_is_by_page() {
    let mut window = a_window_written("Pay the invoice by Friday. The invoice is overdue.");
    let before = window
        .editor
        .source()
        .map(pdf_bytes::ByteStore::len)
        .expect("a source");
    let text = said(
        &mut window,
        "mark_text",
        r#"{"document":"doc-1","text":"invoice","how":"highlight"}"#,
    );
    assert!(
        text.contains(
            "Marked 2 places of \u{201c}invoice\u{201d} (highlighted) on 1 page (page 1: 2)"
        ),
        "{text}"
    );
    let after = window
        .editor
        .source()
        .map(pdf_bytes::ByteStore::len)
        .expect("a source");
    assert!(
        after > before,
        "the bands were written: {before} then {after}"
    );
    assert_eq!(
        undo_steps_left(&mut window),
        2,
        "the write and one for both bands"
    );
}

#[test]
fn page_numbers_are_stamped_on_every_page_in_one_step() {
    let mut window = a_window_written("Body text.");
    said(
        &mut window,
        "add_blank_page",
        r#"{"document":"doc-1","after_page":1}"#,
    );
    let text = said(
        &mut window,
        "add_stamp",
        r#"{"document":"doc-1","kind":"page_numbers","text":"Page {page} of {pages}","font":"DejaVu Sans"}"#,
    );
    assert!(text.contains("Stamped 2 pages at footer_centre"), "{text}");
    assert!(text.contains("\u{201c}Page 1 of 2\u{201d}"), "{text}");
    assert!(text_of_page(&mut window, 1).contains("Page 2 of 2"));
    assert!(text_of_page(&mut window, 0).contains("Page 1 of 2"));
    assert_eq!(
        undo_steps_left(&mut window),
        3,
        "the write, the page, and one stamp for both"
    );
    let why = refused(
        &mut window,
        "add_stamp",
        r#"{"document":"doc-1","kind":"watermark","pages":"9"}"#,
    );
    assert!(why.contains("no page of that number"), "{why}");
}

#[test]
fn bookmarks_are_added_listed_renamed_and_deleted_and_each_reply_lists_them_again() {
    let mut window = a_window_written("Body text.");
    said(
        &mut window,
        "add_blank_page",
        r#"{"document":"doc-1","after_page":1}"#,
    );
    let none = said(
        &mut window,
        "bookmarks",
        r#"{"document":"doc-1","action":"list"}"#,
    );
    assert_eq!(none, "The document has no bookmarks.");
    let added = said(
        &mut window,
        "bookmarks",
        r#"{"document":"doc-1","action":"add","title":"Start","page":1}"#,
    );
    assert!(
        added.contains("Added the bookmark \u{201c}Start\u{201d} on page 1."),
        "{added}"
    );
    assert!(
        added.contains("1. Start (page 1)"),
        "the new list comes with the answer: {added}"
    );
    said(
        &mut window,
        "bookmarks",
        r#"{"document":"doc-1","action":"add","title":"Later","page":2}"#,
    );
    let renamed = said(
        &mut window,
        "bookmarks",
        r#"{"document":"doc-1","action":"rename","bookmark":2,"title":"Appendix"}"#,
    );
    assert!(renamed.contains("2. Appendix (page 2)"), "{renamed}");
    let moved = said(
        &mut window,
        "bookmarks",
        r#"{"document":"doc-1","action":"move","bookmark":2,"direction":"up"}"#,
    );
    assert!(
        moved.contains("1. Appendix (page 2)\n2. Start (page 1)"),
        "{moved}"
    );
    let deleted = said(
        &mut window,
        "bookmarks",
        r#"{"document":"doc-1","action":"delete","bookmark":1}"#,
    );
    assert!(
        deleted.contains("1 bookmark") && deleted.contains("1. Start (page 1)"),
        "{deleted}"
    );
    let why = refused(
        &mut window,
        "bookmarks",
        r#"{"document":"doc-1","action":"delete","bookmark":7}"#,
    );
    assert!(
        why.contains("there is no bookmark 7: the document has 1"),
        "{why}"
    );
    assert_eq!(
        undo_steps_left(&mut window),
        7,
        "the write, the page, and five bookmark changes"
    );
}

#[test]
fn a_table_of_contents_is_made_from_the_headings_in_one_step() {
    let mut window = a_window_written(
        "# Annual report\n\n## Summary\n\nWords of the summary that go on for a while, so that the body text is the commonest size.\n\n## Results\n\nWords of the results that go on for a while, so that the body text is the commonest size.",
    );
    let before = window.ai.tools.run.steps();
    let made = said(
        &mut window,
        "bookmarks",
        r#"{"document":"doc-1","action":"from_headings"}"#,
    );
    assert!(
        made.contains("Made 3 bookmarks from the headings"),
        "{made}"
    );
    assert!(
        made.contains("1. Annual report (page 1)\n  2. Summary (page 1)\n  3. Results (page 1)"),
        "{made}"
    );
    let why = refused(
        &mut window,
        "bookmarks",
        r#"{"document":"doc-1","action":"from_headings"}"#,
    );
    assert!(why.contains("already has 3 bookmarks"), "{why}");
    assert_eq!(
        undo_steps_left(&mut window),
        before + 1,
        "the whole table is one step"
    );
}

fn a_picture_file(name: &str) -> String {
    let path =
        std::env::temp_dir().join(format!("panpdf-window-{}-{name}.png", std::process::id()));
    let rgb: Vec<u8> = std::iter::repeat_n([220_u8, 20, 20], 40 * 20)
        .flatten()
        .collect();
    std::fs::write(
        &path,
        pdf_edit::png::write((40, 20), &rgb, None).expect("a picture"),
    )
    .expect("written");
    path.display().to_string()
}

#[test]
fn a_picture_is_placed_listed_moved_resized_and_deleted_and_each_is_one_step() {
    let mut window = a_blank_window();
    let file = a_picture_file("objects");
    let placed = said(
        &mut window,
        "place_picture",
        &format!(
            r#"{{"document":"doc-1","page":1,"left":100,"top":300,"width":200,"path":"{file}"}}"#
        ),
    );
    assert!(
        placed.contains("Placed the picture on page 1, 200 x 100 pt"),
        "{placed}"
    );
    let list = |window: &mut Window| {
        said(
            window,
            "objects",
            r#"{"document":"doc-1","action":"list","page":1}"#,
        )
    };
    assert!(list(&mut window).contains("p1-o1 picture [100, 300, 300, 400]"));
    let moved = said(
        &mut window,
        "objects",
        r#"{"document":"doc-1","action":"move","object":"p1-o1","left":50,"top":500}"#,
    );
    assert!(moved.contains("Moved it to left 50, top 500"), "{moved}");
    let stale = refused(
        &mut window,
        "objects",
        r#"{"document":"doc-1","action":"resize","object":"p1-o1","width":100}"#,
    );
    assert!(
        stale.contains("has not been listed yet"),
        "the page changed under the name: {stale}"
    );
    assert!(list(&mut window).contains("p1-o1 picture [50, 500, 250, 600]"));
    let resized = said(
        &mut window,
        "objects",
        r#"{"document":"doc-1","action":"resize","object":"p1-o1","width":100}"#,
    );
    assert!(resized.contains("Resized it to 100 x 50 pt"), "{resized}");
    assert!(list(&mut window).contains("p1-o1 picture [50, 500, 150, 550]"));
    let deleted = said(
        &mut window,
        "objects",
        r#"{"document":"doc-1","action":"delete","object":"p1-o1"}"#,
    );
    assert!(deleted.contains("Deleted the picture"), "{deleted}");
    assert!(list(&mut window).contains("holds 0 pictures and drawings"));
    assert_eq!(
        undo_steps_left(&mut window),
        4,
        "place, move, resize, delete"
    );
}

#[test]
fn a_text_block_is_moved_by_its_name_and_told_how_to_do_what_objects_cannot() {
    let mut window = a_window_written("Move me please.");
    let list = said(
        &mut window,
        "objects",
        r#"{"document":"doc-1","action":"list","page":1}"#,
    );
    let name = list
        .lines()
        .find(|line| line.contains("Move me please."))
        .and_then(|line| line.split(' ').next())
        .expect("the block is listed")
        .to_owned();
    let moved = said(
        &mut window,
        "objects",
        &format!(
            r#"{{"document":"doc-1","action":"move","object":"{name}","left":200,"top":400}}"#
        ),
    );
    assert!(moved.contains("to left 200, top 400"), "{moved}");
    let why = refused(
        &mut window,
        "objects",
        &format!(r#"{{"document":"doc-1","action":"delete","object":"{name}"}}"#),
    );
    assert!(
        why.contains("replace_text") || why.contains("read"),
        "{why}"
    );
}

#[test]
fn a_picture_the_person_attached_is_placed_by_its_number_or_the_latest() {
    let mut window = a_blank_window();
    let red = |across: u32, down: u32| {
        let rgb: Vec<u8> = std::iter::repeat_n([220_u8, 20, 20], (across * down) as usize)
            .flatten()
            .collect();
        pdf_edit::png::write((across, down), &rgb, None).expect("a picture")
    };
    window.ai.hold_a_turn(Turn::person_with(
        "here are my pictures",
        vec![
            Attachment::image("first.png", "image/png", red(40, 20)),
            Attachment::text("notes.txt", "not a picture"),
            Attachment::image("second.png", "image/png", red(20, 40)),
        ],
    ));
    let latest = said(
        &mut window,
        "place_picture",
        r#"{"document":"doc-1","page":1,"left":10,"top":10,"width":100}"#,
    );
    assert!(
        latest.contains("100 x 200 pt"),
        "the latest is the tall one: {latest}"
    );
    let first = said(
        &mut window,
        "place_picture",
        r#"{"document":"doc-1","page":1,"left":10,"top":300,"width":100,"attachment":1}"#,
    );
    assert!(
        first.contains("100 x 50 pt"),
        "the first is the wide one: {first}"
    );
    let none = refused(
        &mut window,
        "place_picture",
        r#"{"document":"doc-1","page":1,"left":10,"top":300,"attachment":3}"#,
    );
    assert!(
        none.contains("there is no attached picture 3: 2 pictures are attached"),
        "{none}"
    );
    let mut empty = a_blank_window();
    let why = refused(
        &mut empty,
        "place_picture",
        r#"{"document":"doc-1","page":1,"left":10,"top":10}"#,
    );
    assert!(why.contains("no picture is attached to this chat"), "{why}");
    let off = refused(
        &mut window,
        "place_picture",
        r#"{"document":"doc-1","page":1,"left":500,"top":10,"width":300}"#,
    );
    assert!(off.contains("runs off the page"), "{off}");
    let nowhere = refused(
        &mut window,
        "place_picture",
        r#"{"document":"doc-1","page":5,"left":0,"top":0}"#,
    );
    assert!(nowhere.contains("there is no page 5"), "{nowhere}");
}

#[test]
fn the_window_is_taken_to_a_page_and_a_part_of_a_page_is_drawn_larger() {
    let mut window = a_window_written("Small print on a page.");
    said(
        &mut window,
        "add_blank_page",
        r#"{"document":"doc-1","after_page":1}"#,
    );
    assert!(window.wanted_offset.is_none());
    let shown = said(
        &mut window,
        "go_to_page",
        r#"{"document":"doc-1","page":2}"#,
    );
    assert!(shown.contains("The window now shows page 2"), "{shown}");
    assert!(
        window.wanted_offset.is_some(),
        "the window was told to scroll"
    );
    let revision = window.editor.revision();
    let why = refused(
        &mut window,
        "go_to_page",
        r#"{"document":"doc-1","page":9}"#,
    );
    assert!(
        why.contains("there is no page 9: the document has 2"),
        "{why}"
    );
    assert_eq!(
        window.editor.revision(),
        revision,
        "looking changes nothing"
    );

    let result = run_on_pages(
        &mut window,
        &tool(
            "l",
            "look_closer",
            r#"{"document":"doc-1","page":1,"left":50,"top":50,"right":250,"bottom":150,"dpi":144}"#,
        ),
    );
    assert!(!result.is_error, "{}", result.text);
    assert!(
        result.text.contains("drawn 400 x 200 pixels"),
        "{}",
        result.text
    );
    assert!(result.picture.is_some());
    let off = refused(
        &mut window,
        "look_closer",
        r#"{"document":"doc-1","page":1,"left":900,"top":900,"right":950,"bottom":950}"#,
    );
    assert!(off.contains("has nothing of the page in it"), "{off}");
}

#[test]
fn a_call_that_only_lists_runs_unasked_and_one_that_deletes_asks_without_allow_for_the_chat() {
    let mut window = a_blank_window();
    window.ai.mode = Mode::AskBeforeChanges;
    let decide = |window: &Window, name: &str, arguments: &str| {
        window.decide_about(&tool("d", name, arguments))
    };
    let asks = |chat: bool| Decision::Ask {
        may_allow_for_chat: chat,
    };
    assert_eq!(
        decide(
            &window,
            "bookmarks",
            r#"{"document":"doc-1","action":"list"}"#
        ),
        Decision::Run
    );
    assert_eq!(
        decide(
            &window,
            "bookmarks",
            r#"{"document":"doc-1","action":"delete","bookmark":1}"#
        ),
        asks(false)
    );
    assert_eq!(
        decide(
            &window,
            "bookmarks",
            r#"{"document":"doc-1","action":"rename","bookmark":1,"title":"x"}"#
        ),
        asks(true)
    );
    assert_eq!(
        decide(
            &window,
            "objects",
            r#"{"document":"doc-1","action":"list","page":1}"#
        ),
        Decision::Run
    );
    assert_eq!(
        decide(
            &window,
            "objects",
            r#"{"document":"doc-1","action":"delete","object":"p1-o1"}"#
        ),
        asks(false)
    );
    assert_eq!(
        decide(
            &window,
            "objects",
            r#"{"document":"doc-1","action":"move","object":"p1-o1","left":1}"#
        ),
        asks(true)
    );
    assert_eq!(
        decide(
            &window,
            "find_and_replace",
            r#"{"document":"doc-1","find":"a","replace_with":"b"}"#
        ),
        asks(true)
    );
    assert_eq!(
        decide(&window, "go_to_page", r#"{"document":"doc-1","page":1}"#),
        Decision::Run
    );
    assert_eq!(
        decide(
            &window,
            "look_closer",
            r#"{"document":"doc-1","page":1,"left":0,"top":0,"right":9,"bottom":9}"#
        ),
        Decision::Run
    );
    window.ai.mode = Mode::DoIt;
    assert_eq!(
        decide(
            &window,
            "find_and_replace",
            r#"{"document":"doc-1","find":"a","replace_with":"b"}"#
        ),
        Decision::Run
    );
}

#[test]
fn a_new_tool_called_with_a_call_that_cannot_be_read_is_answered_not_performed() {
    let mut window = a_blank_window();
    let why = refused(
        &mut window,
        "find_and_replace",
        r#"{"document":"doc-1","find":"a"}"#,
    );
    assert!(why.contains("`replace_with` is needed"), "{why}");
    assert!(!window.editor.can_undo(), "nothing was done");
    assert_eq!(window.ai.tools.run.steps(), 0);
}

#[test]
fn a_block_the_assistant_deletes_is_said_to_be_deleted_and_no_neighbour_is_read_back_for_it() {
    let mut window = a_window_written("Gone soon.");
    let read = text_of_page(&mut window, 0);
    let name = read
        .split('[')
        .nth(1)
        .and_then(|rest| rest.split(' ').next())
        .expect("a block name")
        .to_owned();
    let text = said(
        &mut window,
        "replace_text",
        &format!(r#"{{"document":"doc-1","block":"{name}","text":""}}"#),
    );
    assert!(text.starts_with(&format!("Deleted {name}.")), "{text}");
    assert!(!text.contains("now reads"), "{text}");
    assert!(
        text_of_page(&mut window, 0).contains("no text"),
        "the page has nothing left to read"
    );
}

#[test]
fn new_text_on_a_page_shown_turned_is_refused_for_the_assistant_and_nothing_is_written() {
    let mut window = a_blank_window();
    said(
        &mut window,
        "rotate_pages",
        r#"{"document":"doc-1","pages":[1],"degrees":90}"#,
    );
    let steps = window.ai.tools.run.steps();
    let why = refused(
        &mut window,
        "add_text",
        r#"{"document":"doc-1","page":1,"left":72,"top":72,"width":300,"text":"Approved","font":"DejaVu Sans"}"#,
    );
    assert!(why.contains("shown turned"), "{why}");
    assert_eq!(window.ai.tools.run.steps(), steps, "nothing was done");
    let written = refused(
        &mut window,
        "write_pages",
        r#"{"document":"doc-1","markdown":"A paragraph.","font":"DejaVu Sans","replace":true}"#,
    );
    assert!(written.contains("shown turned"), "{written}");
    assert_eq!(
        window.ai.tools.run.steps(),
        steps,
        "and not half a document"
    );
}
