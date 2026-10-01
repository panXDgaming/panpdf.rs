use super::{
    ALWAYS_ASK, ASK_EVEN_IN_FULL_ACCESS, Decision, Mode, Why, decide, may_allow_all, refusal_text,
};

#[cfg(not(target_arch = "wasm32"))]
use super::describe_change;

type Row = (Mode, &'static str, Option<(bool, bool)>, bool, Decision);

const READS: Option<(bool, bool)> = Some((true, false));
const CHANGES: Option<(bool, bool)> = Some((false, false));
const TAKES_OUT: Option<(bool, bool)> = Some((false, true));
const WRITES_OVER: Option<(bool, bool)> = Some((false, true));

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "the permission table, one row per case, read as a table"
)]
fn the_table_is_the_rule() {
    let rows: Vec<Row> = vec![
        (
            Mode::ChatOnly,
            "read_text",
            READS,
            false,
            Decision::Refuse(Why::ToolsAreOff),
        ),
        (
            Mode::ChatOnly,
            "replace_text",
            CHANGES,
            true,
            Decision::Refuse(Why::ToolsAreOff),
        ),
        (
            Mode::ChatOnly,
            "insert_pages",
            CHANGES,
            false,
            Decision::Refuse(Why::ToolsAreOff),
        ),
        (
            Mode::AskBeforeChanges,
            "read_text",
            READS,
            false,
            Decision::Run,
        ),
        (
            Mode::AskBeforeChanges,
            "replace_text",
            CHANGES,
            false,
            Decision::Ask {
                may_allow_for_chat: true,
            },
        ),
        (
            Mode::AskBeforeChanges,
            "delete_pages",
            TAKES_OUT,
            false,
            Decision::Ask {
                may_allow_for_chat: false,
            },
        ),
        (
            Mode::AskBeforeChanges,
            "delete_pages",
            TAKES_OUT,
            true,
            Decision::Ask {
                may_allow_for_chat: false,
            },
        ),
        (
            Mode::AskBeforeChanges,
            "write_pages",
            CHANGES,
            false,
            Decision::Ask {
                may_allow_for_chat: true,
            },
        ),
        (
            Mode::AskBeforeChanges,
            "write_pages",
            WRITES_OVER,
            true,
            Decision::Ask {
                may_allow_for_chat: false,
            },
        ),
        (
            Mode::AskBeforeChanges,
            "replace_text",
            CHANGES,
            true,
            Decision::Run,
        ),
        (
            Mode::AskBeforeChanges,
            "insert_pages",
            CHANGES,
            false,
            Decision::Ask {
                may_allow_for_chat: false,
            },
        ),
        (
            Mode::AskBeforeChanges,
            "insert_pages",
            CHANGES,
            true,
            Decision::Ask {
                may_allow_for_chat: false,
            },
        ),
        (Mode::DoIt, "read_text", READS, false, Decision::Run),
        (Mode::DoIt, "replace_text", CHANGES, false, Decision::Run),
        (Mode::DoIt, "delete_pages", TAKES_OUT, false, Decision::Run),
        (
            Mode::DoIt,
            "insert_pages",
            CHANGES,
            false,
            Decision::Ask {
                may_allow_for_chat: false,
            },
        ),
        (Mode::Free, "read_text", READS, false, Decision::Run),
        (Mode::Free, "replace_text", CHANGES, false, Decision::Run),
        (Mode::Free, "insert_pages", CHANGES, false, Decision::Run),
        (Mode::Free, "delete_pages", TAKES_OUT, false, Decision::Run),
        (
            Mode::Free,
            "format_the_disk",
            None,
            false,
            Decision::Refuse(Why::UnknownTool),
        ),
        (
            Mode::DoIt,
            "format_the_disk",
            None,
            true,
            Decision::Refuse(Why::UnknownTool),
        ),
        (
            Mode::ChatOnly,
            "format_the_disk",
            None,
            false,
            Decision::Refuse(Why::UnknownTool),
        ),
    ];
    for (mode, name, facts, allowed, wanted) in rows {
        assert_eq!(
            decide(mode, name, facts, allowed),
            wanted,
            "{mode:?} {name} allowed_for_chat={allowed}"
        );
    }
}

#[test]
fn a_decision_that_ignores_the_mode_fails_the_chat_only_row() {
    fn ignores_the_mode(
        _mode: Mode,
        name: &str,
        facts: Option<(bool, bool)>,
        allowed_for_chat: bool,
    ) -> Decision {
        decide(Mode::AskBeforeChanges, name, facts, allowed_for_chat)
    }
    assert_eq!(
        decide(Mode::ChatOnly, "read_text", READS, false),
        Decision::Refuse(Why::ToolsAreOff)
    );
    assert_eq!(
        ignores_the_mode(Mode::ChatOnly, "read_text", READS, false),
        Decision::Run,
        "the control must disagree, or it is not measuring anything"
    );
}

#[test]
fn a_mode_is_written_and_read_back_or_refused() {
    for mode in [
        Mode::ChatOnly,
        Mode::AskBeforeChanges,
        Mode::DoIt,
        Mode::Free,
    ] {
        assert_eq!(Mode::parse(mode.as_str()), Some(mode));
    }
    assert_eq!(Mode::parse("yolo"), None);
    assert_eq!(Mode::parse(""), None);
    assert_eq!(
        Mode::parse(crate::ai_choice::DEFAULT_MODE),
        Some(Mode::AskBeforeChanges)
    );
    assert_eq!(Mode::default(), Mode::AskBeforeChanges);
}

#[test]
fn a_destructive_call_is_never_offered_allow_for_this_chat_in_any_way_it_is_asked() {
    for mode in [Mode::AskBeforeChanges] {
        for allowed in [false, true] {
            let decision = decide(mode, "delete_pages", TAKES_OUT, allowed);
            assert_eq!(
                decision,
                Decision::Ask {
                    may_allow_for_chat: false
                },
                "{mode:?} allowed_for_chat={allowed}"
            );
        }
    }
    assert_eq!(
        decide(Mode::AskBeforeChanges, "replace_text", CHANGES, false),
        Decision::Ask {
            may_allow_for_chat: true
        },
        "known answer: an ordinary change can be allowed for the chat"
    );
}

#[test]
fn a_batch_can_be_allowed_at_once_unless_each_call_must_be_looked_at() {
    assert!(may_allow_all("delete_pages"));
    assert!(may_allow_all("replace_text"));
    assert!(!may_allow_all("insert_pages"));
    assert!(!may_allow_all("protect_document"));
    assert!(!may_allow_all("save_copy"));
    assert!(
        may_allow_all("convert"),
        "negative control: a conversion may be allowed for the chat"
    );
}

#[test]
fn protecting_with_a_password_and_saving_a_copy_ask_in_every_mode_even_full_access() {
    for name in ["protect_document", "save_copy"] {
        for mode in [Mode::AskBeforeChanges, Mode::DoIt, Mode::Free] {
            for allowed_for_chat in [false, true] {
                assert_eq!(
                    decide(mode, name, CHANGES, allowed_for_chat),
                    Decision::Ask {
                        may_allow_for_chat: false
                    },
                    "{name} in {mode:?}"
                );
            }
        }
        assert_eq!(
            decide(Mode::ChatOnly, name, CHANGES, false),
            Decision::Refuse(Why::ToolsAreOff),
            "a chat that only talks writes no file"
        );
        assert!(ALWAYS_ASK.contains(&name) && ASK_EVEN_IN_FULL_ACCESS.contains(&name));
    }
    assert_eq!(
        decide(Mode::Free, "insert_pages", CHANGES, false),
        Decision::Run,
        "negative control: full access still takes pages from other files without asking"
    );
    assert_eq!(
        decide(Mode::DoIt, "convert", CHANGES, false),
        Decision::Run,
        "negative control: a plain conversion runs when the person said do it"
    );
    assert_eq!(
        decide(Mode::Free, "convert", TAKES_OUT, false),
        Decision::Run,
        "full access is full access for the rest"
    );
    assert_eq!(
        decide(Mode::AskBeforeChanges, "convert", TAKES_OUT, true),
        Decision::Ask {
            may_allow_for_chat: false
        },
        "a redaction is destructive: never allowed for the whole chat"
    );
}

#[test]
fn a_refusal_says_not_to_retry() {
    assert!(refusal_text().contains("Do not retry"));
    assert_eq!(
        ALWAYS_ASK,
        ["insert_pages", "protect_document", "save_copy"]
    );
}

#[cfg(not(target_arch = "wasm32"))]
mod cards {
    use pdf_agent::json::Json;
    use pdf_agent::tools::request::{NewText, PlanStep, Request, StepState};

    use crate::ai_permission::describe_call;
    use crate::wording::Lang;

    #[test]
    #[expect(clippy::too_many_lines, reason = "one card per tool, read as a table")]
    fn every_tool_says_what_it_would_do() {
        let rows: Vec<(Request, &str)> = vec![
            (
                Request::DocumentInfo,
                "Read what the document says about itself",
            ),
            (
                Request::ReadText {
                    first: None,
                    last: None,
                },
                "Read the document's text",
            ),
            (
                Request::ReadText {
                    first: Some(1),
                    last: Some(3),
                },
                "Read the text of pages 2 to 4",
            ),
            (
                Request::FindText {
                    text: "Bangkok".to_owned(),
                    match_case: false,
                    first: None,
                    last: None,
                },
                "Find \u{201c}Bangkok\u{201d} in the document",
            ),
            (
                Request::RenderPage { page: 2, dpi: 96.0 },
                "Look at page 3 as a picture",
            ),
            (
                Request::ListFonts { name: None },
                "List the fonts new text can be set in",
            ),
            (
                Request::ReplaceText {
                    block: "p2-b3".to_owned(),
                    find: None,
                    text: "Hello".to_owned(),
                },
                "Replace the text of block p2-b3 with: Hello",
            ),
            (
                Request::ReplaceText {
                    block: "p2-b3".to_owned(),
                    find: Some("Hi".to_owned()),
                    text: "Hello".to_owned(),
                },
                "Replace \u{201c}Hi\u{201d} in block p2-b3 with: Hello",
            ),
            (
                Request::ReplaceText {
                    block: "p1-b1".to_owned(),
                    find: None,
                    text: String::new(),
                },
                "Delete all the text of block p1-b1",
            ),
            (
                Request::ReplaceText {
                    block: "p1-b1".to_owned(),
                    find: None,
                    text: "   \n ".to_owned(),
                },
                "Delete all the text of block p1-b1",
            ),
            (
                Request::ReplaceText {
                    block: "p1-b1".to_owned(),
                    find: Some("Hi".to_owned()),
                    text: String::new(),
                },
                "Delete \u{201c}Hi\u{201d} from block p1-b1",
            ),
            (
                Request::AddText {
                    page: 1,
                    area: [10.0, 10.0, 110.0, 30.0],
                    text: "Hello".to_owned(),
                    style: NewText {
                        family: "Noto Sans".to_owned(),
                        size: 12.0,
                        bold: false,
                        italic: false,
                        fill: None,
                    },
                },
                "Write new text on page 2: Hello",
            ),
            (
                Request::AddText {
                    page: 1,
                    area: [10.0, 10.0, 110.0, 30.0],
                    text: String::new(),
                    style: NewText {
                        family: "Noto Sans".to_owned(),
                        size: 12.0,
                        bold: false,
                        italic: false,
                        fill: None,
                    },
                },
                "Write nothing at all on page 2",
            ),
            (
                Request::SetProperties(pdf_edit::info::InfoEdit {
                    title: Some("A report".to_owned()),
                    author: Some("Kham".to_owned()),
                    ..pdf_edit::info::InfoEdit::default()
                }),
                "Set the document's properties (title, author)",
            ),
            (
                Request::FillField {
                    name: "Surname".to_owned(),
                    value: Json::text("Vongsa"),
                },
                "Fill the form field \u{201c}Surname\u{201d} with: Vongsa",
            ),
            (
                Request::AddBlankPage {
                    after: 2,
                    size: None,
                },
                "Add a blank page after page 2",
            ),
            (
                Request::AddBlankPage {
                    after: 0,
                    size: None,
                },
                "Add a blank page at the front",
            ),
            (Request::DeletePages(vec![2, 4]), "Delete pages 3 and 5"),
            (Request::DeletePages(vec![2]), "Delete page 3"),
            (
                Request::MovePages {
                    pages: vec![2],
                    to: 0,
                },
                "Move page 3 so that the first becomes page 1",
            ),
            (
                Request::RotatePages {
                    pages: vec![0, 1],
                    quarter_turns: -1,
                },
                "Turn pages 1 and 2 by -90 degrees",
            ),
            (
                Request::InsertPages {
                    from: std::path::PathBuf::from("/tmp/other.pdf"),
                    pages: Some(vec![0, 1]),
                    after: 0,
                    password: None,
                },
                "Put pages 1 and 2 of the file /tmp/other.pdf in at the front",
            ),
            (
                Request::InsertPages {
                    from: std::path::PathBuf::from("/tmp/other.pdf"),
                    pages: None,
                    after: 3,
                    password: None,
                },
                "Put every page of the file /tmp/other.pdf in after page 3",
            ),
            (Request::Undo, "Take back the last change"),
            (
                Request::Redo,
                "Put back the last change that was taken back",
            ),
            (
                Request::WritePages {
                    from_page: 1,
                    markdown: "# Summary\n\nA few words here".to_owned(),
                    replace: true,
                    size: 11.0,
                    family: "Noto Sans".to_owned(),
                    margin: 56.0,
                    theme: "classic".to_owned(),
                },
                "Write a document over the pages from page 2, 6 words, starting \u{201c}# Summary\u{201d}; what is there is covered, not removed",
            ),
            (
                Request::WritePages {
                    from_page: 0,
                    markdown: "# Summary\n\nA few words here".to_owned(),
                    replace: false,
                    size: 11.0,
                    family: "Noto Sans".to_owned(),
                    margin: 56.0,
                    theme: "classic".to_owned(),
                },
                "Write a document onto the pages from page 1, 6 words, starting \u{201c}# Summary\u{201d}",
            ),
            (
                Request::UpdatePlan {
                    steps: vec![
                        PlanStep {
                            text: "Read".to_owned(),
                            state: StepState::Done,
                        },
                        PlanStep {
                            text: "Write".to_owned(),
                            state: StepState::Pending,
                        },
                    ],
                },
                "Show you its plan, 2 steps",
            ),
        ];
        for (request, english) in rows {
            assert_eq!(describe_call(&request, Lang::English), english);
        }
    }

    #[test]
    fn the_editing_tools_say_what_they_would_do_before_they_do_it() {
        let rows = [
            (
                "find_and_replace",
                r#"{"find":"cat","replace_with":"dog","first_page":2,"last_page":3,"whole_words":true}"#,
                "Replace every \u{201c}cat\u{201d} with \u{201c}dog\u{201d} on pages 2 to 3, whole words only",
            ),
            (
                "find_and_replace",
                r#"{"find":"cat","replace_with":""}"#,
                "Delete every \u{201c}cat\u{201d} in the whole document",
            ),
            (
                "style_text",
                r#"{"block":"p2-b3","find":"Hi","italic":true}"#,
                "Change how \u{201c}Hi\u{201d} in block p2-b3 looks: italic",
            ),
            (
                "mark_text",
                r#"{"text":"Bangkok"}"#,
                "Highlight every \u{201c}Bangkok\u{201d} in the whole document",
            ),
            (
                "mark_text",
                r#"{"text":"old","how":"strike_through","first_page":4,"last_page":4}"#,
                "Strike through every \u{201c}old\u{201d} on page 4",
            ),
            (
                "add_stamp",
                r#"{"kind":"page_numbers","position":"footer_centre","only":"odd"}"#,
                "Put page numbers at footer centre on every page, the odd ones only",
            ),
            (
                "add_stamp",
                r#"{"kind":"watermark","text":"DRAFT","pages":"1-3"}"#,
                "Put a watermark on pages 1-3: \u{201c}DRAFT\u{201d}",
            ),
            ("bookmarks", r#"{"action":"list"}"#, "List the bookmarks"),
            (
                "bookmarks",
                r#"{"action":"add","title":"Intro","page":3}"#,
                "Add the bookmark \u{201c}Intro\u{201d} for page 3",
            ),
            (
                "bookmarks",
                r#"{"action":"move","bookmark":2,"direction":"in"}"#,
                "Move bookmark 2 in",
            ),
            (
                "bookmarks",
                r#"{"action":"delete","bookmark":4}"#,
                "Delete bookmark 4",
            ),
            (
                "bookmarks",
                r#"{"action":"from_headings","replace":true}"#,
                "Make a table of contents from the headings, taking out the bookmarks there are",
            ),
            (
                "place_picture",
                r#"{"page":2,"left":72,"top":100,"attachment":2}"#,
                "Put attached picture 2 on page 2 at left 72, top 100",
            ),
            (
                "objects",
                r#"{"action":"list","page":1}"#,
                "List the pictures, drawings and text blocks of page 1",
            ),
            (
                "objects",
                r#"{"action":"resize","object":"p1-o2","width":120}"#,
                "Resize p1-o2 to 120 pt wide",
            ),
            (
                "objects",
                r#"{"action":"delete","object":"p1-o2"}"#,
                "Delete p1-o2",
            ),
            ("go_to_page", r#"{"page":5}"#, "Show page 5 in the window"),
            (
                "look_closer",
                r#"{"page":3,"left":10,"top":20,"right":110,"bottom":70}"#,
                "Look closer at the part [10, 20, 110, 70] of page 3",
            ),
        ];
        for (name, arguments, english) in rows {
            let arguments = Json::parse(arguments).expect("JSON");
            let request = pdf_agent::tools::request::parse(name, &arguments)
                .unwrap_or_else(|why| panic!("{name} did not read: {why}"));
            assert_eq!(describe_call(&request, Lang::English), english, "{name}");
        }
    }

    #[test]
    #[expect(clippy::too_many_lines, reason = "one card per tool, read as a table")]
    fn the_tools_for_files_scans_links_shapes_and_forms_say_what_they_would_do() {
        let rows = [
            (
                "convert",
                r#"{"tool":"pdf-to-word"}"#,
                "Turn the open document into a Word file, writing a new file beside it",
            ),
            (
                "convert",
                r#"{"tool":"pdf-to-jpg","files":["/home/a/scan.pdf","/home/a/b.pdf"]}"#,
                "Save the pages or pictures of scan.pdf, b.pdf as picture files, writing a new file beside it",
            ),
            (
                "convert",
                r#"{"tool":"word-to-pdf","files":["/a/1.docx","/a/2.docx","/a/3.docx","/a/4.docx"]}"#,
                "Make a PDF from 1.docx, 2.docx, 3.docx and 1 more, writing a new file beside it",
            ),
            (
                "convert",
                r#"{"tool":"compare-pdf","files":["/a/newer.pdf"]}"#,
                "Compare the open document with newer.pdf, writing a new file beside it",
            ),
            (
                "convert",
                r#"{"tool":"redact-pdf","options":{"search":["Acme","Bangkok"]}}"#,
                "Remove \u{201c}Acme, Bangkok\u{201d} from a copy of the open document for good: the words cannot be read back from the new file, writing a new file beside it",
            ),
            (
                "convert",
                r#"{"tool":"sign-pdf","options":{"text":"Alice Example"}}"#,
                "Sign a copy of the open document with the name \u{201c}Alice Example\u{201d}, writing a new file beside it",
            ),
            (
                "protect_document",
                r#"{"password":"hunter2","deny":["print","copy"]}"#,
                "Protect a copy of the open document with a password, forbidding print, copy, writing a new file beside it",
            ),
            (
                "ocr_pages",
                r#"{"pages":"2-4","languages":["tha","eng"]}"#,
                "Read the words of pages 2-4 in tha and eng with the text recogniser, so they can be searched",
            ),
            (
                "ocr_pages",
                "{}",
                "Read the words of every page with the text recogniser, so they can be searched",
            ),
            (
                "extract_pages",
                r#"{"pages":"1-3, 5"}"#,
                "Save pages 1-3, 5 of the document as a new PDF file beside it",
            ),
            (
                "split_document",
                r#"{"every":1}"#,
                "Split the document into new PDF files of 1 page each, in a new folder beside it",
            ),
            (
                "split_document",
                r#"{"at":"5, 12"}"#,
                "Split the document into new PDF files starting at pages 5, 12, in a new folder beside it",
            ),
            (
                "export_page_pictures",
                r#"{"pages":"2","dpi":300}"#,
                "Save pages 2 as PNG pictures at 300 dpi, as new files beside the document",
            ),
            (
                "save_copy",
                r#"{"path":"/home/a/copy.pdf"}"#,
                "Save a copy of the document, with its changes, as the new file /home/a/copy.pdf",
            ),
            (
                "save_copy",
                "{}",
                "Save a copy of the document, with its changes, beside the original under a name no file has yet",
            ),
            (
                "links",
                r#"{"action":"list","page":2}"#,
                "List the links of page 2",
            ),
            (
                "links",
                r#"{"action":"add","block":"p2-b1","url":"https://example.org"}"#,
                "Add a link over block p2-b1 on page 2 that goes to https://example.org",
            ),
            (
                "links",
                r#"{"action":"add","page":1,"left":10,"top":20,"right":110,"bottom":40,"to_page":5}"#,
                "Add a link over the box [10, 20, 110, 40] on page 1 that goes to page 5",
            ),
            (
                "links",
                r#"{"action":"remove","link":"p3-l2"}"#,
                "Delete link p3-l2",
            ),
            (
                "draw_shape",
                r#"{"page":2,"shape":"arrow","left":0,"top":0,"right":9,"bottom":9}"#,
                "Draw an arrow on page 2",
            ),
            (
                "draw_shape",
                r#"{"page":1,"shape":"rectangle","left":0,"top":0,"right":9,"bottom":9}"#,
                "Draw a rectangle on page 1",
            ),
            (
                "add_field",
                r#"{"page":1,"kind":"checkbox","left":0,"top":0,"width":20,"height":20,"name":"Agree"}"#,
                "Add a checkbox form field named \u{201c}Agree\u{201d} to page 1",
            ),
            (
                "set_tab_order",
                r#"{"page":3,"order":"columns"}"#,
                "Put the form fields of page 3 in tab order by columns",
            ),
        ];
        for (name, arguments, english) in rows {
            let arguments = Json::parse(arguments).expect("JSON");
            let request = pdf_agent::tools::request::parse(name, &arguments)
                .unwrap_or_else(|why| panic!("{name} did not read: {why}"));
            assert_eq!(describe_call(&request, Lang::English), english, "{name}");
        }
    }

    #[test]
    fn a_card_never_shows_a_password() {
        for (name, arguments) in [
            (
                "protect_document",
                r#"{"password":"hunter2","owner_password":"boss"}"#,
            ),
            (
                "convert",
                r#"{"tool":"pdf-to-text","options":{"password":"hunter2"}}"#,
            ),
        ] {
            let arguments = Json::parse(arguments).expect("JSON");
            let request = pdf_agent::tools::request::parse(name, &arguments).expect("reads");
            let said = describe_call(&request, Lang::English);
            let shown = crate::ai_permission::describe_change(&request, None, Lang::English);
            for text in [said, shown.headline, shown.after.unwrap_or_default()] {
                assert!(
                    !text.contains("hunter2") && !text.contains("boss"),
                    "{name}: {text}"
                );
            }
        }
    }

    #[test]
    fn a_long_piece_of_text_is_cut_to_one_line() {
        let said = describe_call(
            &Request::ReplaceText {
                block: "p1-b1".to_owned(),
                find: None,
                text: format!("{}\nand more", "x".repeat(400)),
            },
            Lang::English,
        );
        assert!(!said.contains('\n'));
        assert!(said.ends_with('\u{2026}'));
        assert!(said.chars().count() < 260);
    }
}

#[test]
fn full_access_is_not_kept_for_the_next_launch() {
    assert_eq!(Mode::Free.kept_for_next_time(), Mode::DoIt);
    for mode in [Mode::ChatOnly, Mode::AskBeforeChanges, Mode::DoIt] {
        assert_eq!(mode.kept_for_next_time(), mode);
    }
    assert_eq!(
        Mode::parse("free").map(Mode::kept_for_next_time),
        Some(Mode::DoIt),
        "a file an older version wrote does not bring it back either"
    );
}

#[cfg(not(target_arch = "wasm32"))]
mod change {
    use pdf_agent::tools::request::Request;

    use super::describe_change;
    use crate::wording::Lang;

    fn replace(block: &str, find: Option<&str>, text: &str) -> Request {
        Request::ReplaceText {
            block: block.to_owned(),
            find: find.map(str::to_owned),
            text: text.to_owned(),
        }
    }

    #[test]
    fn a_replaced_block_shows_what_it_said_and_what_it_will_say_and_the_page() {
        let shown = describe_change(
            &replace("p3-b12", None, "Dear Madam"),
            Some("Dear Sir"),
            Lang::English,
        );
        assert_eq!(shown.headline, "Replace text on page 3");
        assert_eq!(shown.before.as_deref(), Some("Dear Sir"));
        assert_eq!(shown.after.as_deref(), Some("Dear Madam"));
    }

    #[test]
    fn a_replaced_word_shows_the_word_and_not_the_whole_block() {
        let shown = describe_change(
            &replace("p2-b1", Some("teh"), "the"),
            Some("Fix teh typo here"),
            Lang::English,
        );
        assert_eq!(shown.before.as_deref(), Some("teh"));
        assert_eq!(shown.after.as_deref(), Some("the"));
    }

    #[test]
    fn a_deleted_block_has_a_before_and_an_empty_after() {
        let shown = describe_change(&replace("p1-b2", None, ""), Some("old"), Lang::English);
        assert_eq!(shown.before.as_deref(), Some("old"));
        assert_eq!(shown.after.as_deref(), Some(""));
    }

    #[test]
    fn a_block_with_no_readable_name_is_described_in_the_plain_way() {
        for name in ["", "p0-b1", "p2", "b3", "pX-b1", "p2-b"] {
            let shown = describe_change(&replace(name, None, "x"), None, Lang::English);
            assert!(shown.before.is_none() && shown.after.is_none(), "{name}");
            assert!(shown.headline.starts_with("Replace"), "{name}");
        }
    }

    #[test]
    fn a_change_that_is_not_a_text_has_only_its_sentence() {
        let shown = describe_change(&Request::DeletePages(vec![1, 2]), None, Lang::English);
        assert_eq!(shown.headline, "Delete pages 2 and 3");
        assert!(shown.before.is_none() && shown.after.is_none());
    }
}
