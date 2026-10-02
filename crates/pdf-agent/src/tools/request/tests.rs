use super::{HANDLE, PlanStep, Request, StepState, parse};
use crate::finding::Search;
use crate::json::Json;
use crate::marking::{How, Marking};
use crate::objects::Action as ObjectAction;
use crate::outlining::{Action as BookmarkAction, Place, Step};
use crate::pictures::{Asked as PictureAsked, Source};
use crate::stamping::{Asked as StampAsked, Kind};
use crate::styling::{Align, Look};
use crate::tools::offered_to_a_window;
use pdf_convert::{Choice, Setting, Tool, Value, Values};

fn args(text: &str) -> Json {
    Json::parse(text).expect("the arguments in this test are JSON")
}

fn loose(wanted: &str) -> Search {
    Search {
        wanted: wanted.to_owned(),
        match_case: false,
        whole_words: false,
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "one example call per tool, read as a table"
)]
fn examples() -> Vec<(&'static str, &'static str, Request)> {
    vec![
        (
            "document_info",
            r#"{"document":"doc-1"}"#,
            Request::DocumentInfo,
        ),
        (
            "read_text",
            r#"{"document":"doc-1","first_page":2,"last_page":4}"#,
            Request::ReadText {
                first: Some(1),
                last: Some(3),
            },
        ),
        (
            "read_text",
            r#"{"document":"doc-1"}"#,
            Request::ReadText {
                first: None,
                last: None,
            },
        ),
        (
            "find_text",
            r#"{"document":"doc-1","text":"Bangkok","match_case":true}"#,
            Request::FindText {
                text: "Bangkok".to_owned(),
                match_case: true,
                first: None,
                last: None,
            },
        ),
        (
            "render_page",
            r#"{"document":"doc-1","page":3,"dpi":150}"#,
            Request::RenderPage {
                page: 2,
                dpi: 150.0,
            },
        ),
        (
            "list_fonts",
            r#"{"name":"Noto"}"#,
            Request::ListFonts {
                name: Some("Noto".to_owned()),
            },
        ),
        (
            "replace_text",
            r#"{"document":"doc-1","block":"p2-b3","text":"Hello","find":"Hi"}"#,
            Request::ReplaceText {
                block: "p2-b3".to_owned(),
                find: Some("Hi".to_owned()),
                text: "Hello".to_owned(),
            },
        ),
        (
            "fill_field",
            r#"{"document":"doc-1","name":"Surname","value":"Vongsa"}"#,
            Request::FillField {
                name: "Surname".to_owned(),
                value: Json::text("Vongsa"),
            },
        ),
        (
            "add_blank_page",
            r#"{"document":"doc-1","after_page":2,"width":595,"height":842}"#,
            Request::AddBlankPage {
                after: 2,
                size: Some([595.0, 842.0]),
            },
        ),
        (
            "add_blank_page",
            r#"{"document":"doc-1","after_page":0}"#,
            Request::AddBlankPage {
                after: 0,
                size: None,
            },
        ),
        (
            "delete_pages",
            r#"{"document":"doc-1","pages":[3,5]}"#,
            Request::DeletePages(vec![2, 4]),
        ),
        (
            "move_pages",
            r#"{"document":"doc-1","pages":[3],"to":1}"#,
            Request::MovePages {
                pages: vec![2],
                to: 0,
            },
        ),
        (
            "rotate_pages",
            r#"{"document":"doc-1","pages":[1],"degrees":-90}"#,
            Request::RotatePages {
                pages: vec![0],
                quarter_turns: -1,
            },
        ),
        (
            "insert_pages",
            r#"{"document":"doc-1","from":"/tmp/other.pdf","pages":[1,2],"after_page":0,"password":"view"}"#,
            Request::InsertPages {
                from: std::path::PathBuf::from("/tmp/other.pdf"),
                pages: Some(vec![0, 1]),
                after: 0,
                password: Some("view".to_owned()),
            },
        ),
        ("undo", r#"{"document":"doc-1"}"#, Request::Undo),
        ("redo", r#"{"document":"doc-1"}"#, Request::Redo),
        (
            "find_and_replace",
            r#"{"document":"doc-1","find":"Acme","replace_with":"Beta","match_case":true,"first_page":2,"last_page":5}"#,
            Request::FindAndReplace {
                search: Search {
                    wanted: "Acme".to_owned(),
                    match_case: true,
                    whole_words: false,
                },
                with: "Beta".to_owned(),
                first: Some(1),
                last: Some(4),
            },
        ),
        (
            "find_and_replace",
            r#"{"document":"doc-1","find":"cat","replace_with":"","whole_words":true}"#,
            Request::FindAndReplace {
                search: Search {
                    wanted: "cat".to_owned(),
                    match_case: false,
                    whole_words: true,
                },
                with: String::new(),
                first: None,
                last: None,
            },
        ),
        (
            "style_text",
            r##"{"document":"doc-1","block":"p2-b3","find":"Total","bold":true,"italic":false,"size":14,"color":"#ff0000","line_spacing":1.5,"align":"center"}"##,
            Request::StyleText {
                block: "p2-b3".to_owned(),
                find: Some("Total".to_owned()),
                look: Look {
                    bold: Some(true),
                    italic: Some(false),
                    underline: None,
                    size: Some(14.0),
                    fill: Some([1.0, 0.0, 0.0]),
                    family: None,
                    line_spacing: Some(1.5),
                    align: Some(Align::Centre),
                },
            },
        ),
        (
            "mark_text",
            r#"{"document":"doc-1","text":"invoice"}"#,
            Request::MarkText {
                search: loose("invoice"),
                marking: Marking::new(How::Highlight, None),
                first: None,
                last: None,
            },
        ),
        (
            "mark_text",
            r##"{"document":"doc-1","text":"due","how":"strike_through","color":"#336699","last_page":3}"##,
            Request::MarkText {
                search: loose("due"),
                marking: Marking::new(How::StrikeThrough, Some([0.2, 0.4, 0.6])),
                first: None,
                last: Some(2),
            },
        ),
        (
            "add_stamp",
            r#"{"document":"doc-1","kind":"page_numbers","text":"Page {page} of {pages}","position":"footer_right","pages":"2-9","only":"even","start_number":1,"opacity":50}"#,
            Request::AddStamp({
                let mut asked = StampAsked::new(Kind::PageNumbers);
                asked.wording = Some("Page {page} of {pages}".to_owned());
                asked.spot = crate::stamping::spot_of("footer_right");
                asked.pages = "2-9".to_owned();
                asked.only = pdf_edit::stamp::Only::Even;
                asked.start = Some(1);
                asked.opacity = Some(0.5);
                asked
            }),
        ),
        (
            "bookmarks",
            r#"{"document":"doc-1","action":"list"}"#,
            Request::Bookmarks(BookmarkAction::List),
        ),
        (
            "bookmarks",
            r#"{"document":"doc-1","action":"add","title":"Results","page":4,"inside":2}"#,
            Request::Bookmarks(BookmarkAction::Add {
                title: Some("Results".to_owned()),
                place: Place::Page(3),
                after: None,
                inside: Some(2),
            }),
        ),
        (
            "bookmarks",
            r#"{"document":"doc-1","action":"add","block":"p3-b2"}"#,
            Request::Bookmarks(BookmarkAction::Add {
                title: None,
                place: Place::Block("p3-b2".to_owned()),
                after: None,
                inside: None,
            }),
        ),
        (
            "bookmarks",
            r#"{"document":"doc-1","action":"move","bookmark":3,"direction":"in"}"#,
            Request::Bookmarks(BookmarkAction::Move {
                bookmark: 3,
                step: Step::In,
            }),
        ),
        (
            "bookmarks",
            r#"{"document":"doc-1","action":"from_headings","replace":true}"#,
            Request::Bookmarks(BookmarkAction::FromHeadings { replace: true }),
        ),
        (
            "place_picture",
            r#"{"document":"doc-1","page":2,"left":72,"top":100,"width":150,"attachment":2}"#,
            Request::PlacePicture(PictureAsked {
                page: 1,
                left: 72.0,
                top: 100.0,
                width: Some(150.0),
                height: None,
                source: Source::Attachment(Some(2)),
            }),
        ),
        (
            "place_picture",
            r#"{"document":"doc-1","page":1,"left":0,"top":0,"path":"/tmp/logo.png"}"#,
            Request::PlacePicture(PictureAsked {
                page: 0,
                left: 0.0,
                top: 0.0,
                width: None,
                height: None,
                source: Source::File(std::path::PathBuf::from("/tmp/logo.png")),
            }),
        ),
        (
            "objects",
            r#"{"document":"doc-1","action":"list","page":3}"#,
            Request::Objects(ObjectAction::List { page: 2 }),
        ),
        (
            "objects",
            r#"{"document":"doc-1","action":"resize","object":"p3-o2","width":120}"#,
            Request::Objects(ObjectAction::Resize {
                object: "p3-o2".to_owned(),
                width: Some(120.0),
                height: None,
            }),
        ),
        (
            "go_to_page",
            r#"{"document":"doc-1","page":7}"#,
            Request::GoToPage { page: 6 },
        ),
        (
            "look_closer",
            r#"{"document":"doc-1","page":2,"left":10,"top":20,"right":110,"bottom":70,"dpi":300}"#,
            Request::LookCloser {
                page: 1,
                region: [10.0, 20.0, 110.0, 70.0],
                dpi: 300.0,
            },
        ),
        (
            "look_closer",
            r#"{"document":"doc-1","page":1,"left":0,"top":0,"right":50,"bottom":50}"#,
            Request::LookCloser {
                page: 0,
                region: [0.0, 0.0, 50.0, 50.0],
                dpi: 200.0,
            },
        ),
        (
            "convert",
            r#"{"document":"doc-1","tool":"compress-pdf","options":{"level":"low"}}"#,
            Request::Convert(crate::converting::Asked {
                tool: Tool::Compress,
                files: Vec::new(),
                values: Values::new().with(Setting::Level, Value::Choice(Choice::Low)),
                pictures: Vec::new(),
                open_result: false,
            }),
        ),
        (
            "protect_document",
            r#"{"document":"doc-1","password":"pw","open_result":true}"#,
            Request::Convert(crate::converting::Asked {
                tool: Tool::Protect,
                files: Vec::new(),
                values: Values::new().with(Setting::NewPassword, Value::Secret("pw".to_owned())),
                pictures: Vec::new(),
                open_result: true,
            }),
        ),
        (
            "ocr_pages",
            r#"{"document":"doc-1","pages":"2-3","languages":["eng"]}"#,
            Request::OcrPages(crate::recognizing::Asked {
                pages: "2-3".to_owned(),
                languages: vec!["eng".to_owned()],
                skip_text: true,
            }),
        ),
        (
            "extract_pages",
            r#"{"document":"doc-1","pages":"1-3, 5"}"#,
            Request::ExtractPages(crate::taking::Extract {
                pages: "1-3, 5".to_owned(),
            }),
        ),
        (
            "split_document",
            r#"{"document":"doc-1","every":10}"#,
            Request::SplitDocument(crate::taking::Split::Every(10)),
        ),
        (
            "export_page_pictures",
            r#"{"document":"doc-1","pages":"2","dpi":300}"#,
            Request::ExportPictures(crate::taking::Pictures {
                pages: "2".to_owned(),
                dpi: 300.0,
            }),
        ),
        (
            "save_copy",
            r#"{"document":"doc-1","path":"/tmp/copy.pdf"}"#,
            Request::SaveCopy {
                path: Some(std::path::PathBuf::from("/tmp/copy.pdf")),
            },
        ),
        (
            "save_copy",
            r#"{"document":"doc-1"}"#,
            Request::SaveCopy { path: None },
        ),
        (
            "links",
            r#"{"document":"doc-1","action":"list","page":2}"#,
            Request::Links(crate::linking::Action::List { page: 1 }),
        ),
        (
            "draw_shape",
            r#"{"document":"doc-1","page":1,"shape":"line","left":0,"top":0,"right":50,"bottom":50}"#,
            Request::DrawShape(crate::shaping::Asked {
                page: 0,
                shape: crate::shaping::Shape::Line,
                from: (0.0, 0.0),
                to: (50.0, 50.0),
                colour: [0.0; 3],
                width: 2.0,
                fill: None,
            }),
        ),
        (
            "add_field",
            r#"{"document":"doc-1","page":1,"kind":"text","left":10,"top":20,"width":100,"height":24}"#,
            Request::AddField(crate::fielding::Asked {
                page: 0,
                kind: pdf_edit::new_field::NewFieldKind::Text,
                area: [10.0, 20.0, 110.0, 44.0],
                name: None,
                options: Vec::new(),
            }),
        ),
        (
            "set_tab_order",
            r#"{"document":"doc-1","page":1,"order":"structure"}"#,
            Request::SetTabOrder {
                page: 0,
                order: crate::fielding::Order::Structure,
            },
        ),
        (
            "ask_person",
            r#"{"question":" Which theme? ","options":[{"label":"ocean (Recommended)","description":"Blue and calm"},{"label":"forest"}]}"#,
            Request::AskPerson {
                question: "Which theme?".to_owned(),
                options: vec![
                    ("ocean (Recommended)".to_owned(), "Blue and calm".to_owned()),
                    ("forest".to_owned(), String::new()),
                ],
            },
        ),
        (
            "update_plan",
            r#"{"steps":[{"text":" Read the contract ","status":"done"},{"text":"Mark the dates","status":"in_progress"},{"text":"Summarise","status":"pending"}]}"#,
            Request::UpdatePlan {
                steps: vec![
                    PlanStep {
                        text: "Read the contract".to_owned(),
                        state: StepState::Done,
                    },
                    PlanStep {
                        text: "Mark the dates".to_owned(),
                        state: StepState::InProgress,
                    },
                    PlanStep {
                        text: "Summarise".to_owned(),
                        state: StepState::Pending,
                    },
                ],
            },
        ),
    ]
}

#[test]
fn a_question_needs_two_to_four_answers() {
    let with = |count: usize| {
        let options: Vec<String> = (0..count)
            .map(|at| format!(r#"{{"label":"answer {at}"}}"#))
            .collect();
        parse(
            "ask_person",
            &args(&format!(
                r#"{{"question":"Which?","options":[{}]}}"#,
                options.join(",")
            )),
        )
    };
    assert!(with(2).is_ok());
    assert!(with(4).is_ok());
    for count in [1, 5] {
        let refused = with(count).expect_err("refused");
        assert!(refused.contains("two to 4 options"), "{refused}");
    }
    assert_eq!(
        parse(
            "ask_person",
            &args(r#"{"question":"Which?","options":[{"label":"a"},{"description":"b"}]}"#)
        ),
        Err("every option needs a `label`".to_owned())
    );
    assert!(
        parse("ask_person", &args(r#"{"question":"Which?"}"#))
            .expect_err("refused")
            .starts_with("`options` is needed")
    );
}

#[test]
fn every_tool_reads_its_own_example() {
    for (name, arguments, wanted) in examples() {
        assert_eq!(
            parse(name, &args(arguments)),
            Ok(wanted),
            "{name} did not read {arguments}"
        );
    }
}

#[test]
fn what_is_offered_is_what_can_be_read() {
    let offered: Vec<String> = offered_to_a_window()
        .into_iter()
        .map(|tool| tool.name)
        .collect();
    assert_eq!(offered.len(), 39);
    for name in &offered {
        let read = parse(name, &args(r#"{"document":"doc-1"}"#));
        assert_ne!(
            read,
            Err(format!("there is no tool called {name}")),
            "{name} is offered and cannot be read"
        );
    }
    assert_eq!(
        parse("save_document", &args(r#"{"document":"doc-1"}"#)),
        Err("there is no tool called save_document".to_owned()),
        "a tool the window does not offer is not performed because it was asked for"
    );
}

#[test]
fn new_text_is_given_a_frame_to_wrap_in() {
    let read = parse(
        "add_text",
        &args(
            r##"{"document":"doc-1","page":1,"left":72,"top":100,"width":200,
                "text":"one\ntwo","size":20,"font":"Noto Sans","bold":true,"color":"#ff0000"}"##,
        ),
    );
    let Ok(Request::AddText {
        page,
        area,
        text,
        style,
    }) = read
    else {
        panic!("add_text did not read as new text: {read:?}");
    };
    assert_eq!(page, 0);
    for (measured, wanted) in area.into_iter().zip([72.0, 100.0, 272.0, 156.0]) {
        assert!((measured - wanted).abs() < 1e-9, "{area:?}");
    }
    assert_eq!(text, "one\ntwo");
    assert_eq!(style.family, "Noto Sans");
    assert!((style.size - 20.0).abs() < 1e-9);
    assert!(style.bold);
    assert!(!style.italic);
    assert_eq!(style.fill, Some([1.0, 0.0, 0.0]));
}

#[test]
fn properties_are_read_and_an_empty_change_is_refused() {
    let read = parse(
        "set_properties",
        &args(r#"{"document":"doc-1","title":"A report"}"#),
    );
    let Ok(Request::SetProperties(edit)) = read else {
        panic!("set_properties did not read: {read:?}");
    };
    assert_eq!(edit.title.as_deref(), Some("A report"));
    assert_eq!(edit.author, None);
    assert_eq!(
        parse("set_properties", &args(r#"{"document":"doc-1"}"#)),
        Err("nothing to set: pass title, author, subject or keywords".to_owned())
    );
}

#[test]
fn a_missing_argument_is_refused_in_the_desks_words() {
    for (name, arguments, why) in [
        (
            "replace_text",
            r#"{"document":"doc-1","block":"p2-b3"}"#,
            "`text` is needed, as text",
        ),
        (
            "replace_text",
            r#"{"document":"doc-1","text":"Hello"}"#,
            "`block` is needed, as text",
        ),
        (
            "find_text",
            r#"{"document":"doc-1"}"#,
            "`text` is needed, as text",
        ),
        ("find_text", r#"{"text":""}"#, "`text` is empty"),
        (
            "render_page",
            r#"{"document":"doc-1"}"#,
            "`page` is needed, as a page number from 1",
        ),
        (
            "render_page",
            r#"{"document":"doc-1","page":0}"#,
            "`page` is needed, as a page number from 1",
        ),
        (
            "add_text",
            r#"{"document":"doc-1","page":1,"top":10,"width":100,"text":"x"}"#,
            "`left` is needed, in points",
        ),
        (
            "add_text",
            r#"{"document":"doc-1","page":1,"left":10,"top":10,"width":0,"text":"x"}"#,
            "`width` is needed, in points",
        ),
        (
            "fill_field",
            r#"{"document":"doc-1","name":"Surname"}"#,
            "`value` is needed",
        ),
        (
            "add_blank_page",
            r#"{"document":"doc-1"}"#,
            "`after_page` is needed: 0 puts it first",
        ),
        (
            "delete_pages",
            r#"{"document":"doc-1"}"#,
            "`pages` is needed, as a list of page numbers",
        ),
        (
            "delete_pages",
            r#"{"document":"doc-1","pages":[0]}"#,
            "`pages` holds something that is not a page number",
        ),
        (
            "rotate_pages",
            r#"{"document":"doc-1","pages":[1],"degrees":45}"#,
            "`degrees` is 90, 180 or 270, or the same negative",
        ),
        (
            "insert_pages",
            r#"{"document":"doc-1","after_page":0}"#,
            "`from` is needed, as text",
        ),
    ] {
        assert_eq!(
            parse(name, &args(arguments)),
            Err(why.to_owned()),
            "{name} with {arguments}"
        );
    }
}

#[test]
fn a_handle_that_is_not_the_open_document_is_refused() {
    assert_eq!(
        parse("read_text", &args(r#"{"document":"doc-2"}"#)),
        Err(format!(
            "there is no document called doc-2: the document open in this window is \
             {HANDLE}, and it is the only one"
        ))
    );
    assert_eq!(
        parse("read_text", &Json::Null),
        Ok(Request::ReadText {
            first: None,
            last: None
        })
    );
}

#[test]
fn a_request_says_whether_it_only_reads() {
    for (name, arguments, _) in examples() {
        let request = parse(name, &args(arguments)).expect("the examples parse");
        let facts = crate::tools::facts(name).expect("every example is a tool");
        let by_action = matches!(
            request,
            Request::Bookmarks(_) | Request::Objects(_) | Request::Links(_)
        );
        assert!(
            by_action || request.only_reads() == facts.read_only,
            "{name} disagrees with the table"
        );
        assert!(
            !by_action || !facts.read_only,
            "{name} has actions that change the document, so the table never calls it read-only"
        );
    }
    assert!(!Request::Undo.only_reads());
}

fn writing(extra: &str) -> Result<Request, String> {
    parse(
        "write_pages",
        &args(&format!(
            r##"{{"document":"doc-1","markdown":"# Hi","font":"Noto Sans"{extra}}}"##
        )),
    )
}

#[test]
fn a_write_that_names_a_page_or_a_size_wrongly_is_refused_not_quietly_changed() {
    let Ok(Request::WritePages {
        from_page,
        size,
        margin,
        ..
    }) = writing("")
    else {
        panic!("a plain write reads");
    };
    assert_eq!(
        (from_page, size.to_bits(), margin.to_bits()),
        (0, 11.0_f64.to_bits(), 56.0_f64.to_bits())
    );
    for (extra, why) in [
        (
            r#","from_page":0"#,
            "`from_page` is needed, as a page number from 1",
        ),
        (
            r#","from_page":"2""#,
            "`from_page` is needed, as a page number from 1",
        ),
        (
            r#","from_page":-1"#,
            "`from_page` is needed, as a page number from 1",
        ),
        (r#","size":2"#, "`size` is a number from 4 to 96, in points"),
        (
            r#","size":"big""#,
            "`size` is a number from 4 to 96, in points",
        ),
        (
            r#","margin":400"#,
            "`margin` is a number from 0 to 300, in points",
        ),
    ] {
        assert_eq!(writing(extra), Err(why.to_owned()), "{extra}");
    }
    let Ok(Request::WritePages {
        from_page, size, ..
    }) = writing(r#","from_page":3,"size":14"#)
    else {
        panic!("a write that names a page reads");
    };
    assert_eq!((from_page, size.to_bits()), (2, 14.0_f64.to_bits()));
}

#[test]
fn an_argument_the_model_sent_as_null_is_an_argument_it_left_out() {
    assert_eq!(
        parse(
            "read_text",
            &args(r#"{"document":"doc-1","first_page":3,"last_page":null}"#)
        ),
        Ok(Request::ReadText {
            first: Some(2),
            last: None
        })
    );
    assert_eq!(
        parse(
            "find_text",
            &args(r#"{"document":"doc-1","text":"x","first_page":null,"last_page":null}"#)
        ),
        Ok(Request::FindText {
            text: "x".to_owned(),
            match_case: false,
            first: None,
            last: None
        })
    );
    assert_eq!(
        parse(
            "insert_pages",
            &args(r#"{"document":"doc-1","from":"/tmp/a.pdf","after_page":1,"pages":null}"#)
        ),
        Ok(Request::InsertPages {
            from: std::path::PathBuf::from("/tmp/a.pdf"),
            pages: None,
            after: 1,
            password: None
        })
    );
    assert_eq!(
        parse("read_text", &args(r#"{"document":"doc-1","first_page":0}"#)),
        Err("`first_page` is needed, as a page number from 1".to_owned()),
        "a wrong number is still wrong: only null means left out"
    );
}

#[test]
fn a_page_size_is_both_sides_or_neither() {
    assert!(
        parse(
            "add_blank_page",
            &args(r#"{"document":"doc-1","after_page":1,"width":300}"#)
        )
        .expect_err("refused")
        .contains("both `width` and `height`")
    );
    assert_eq!(
        parse(
            "add_blank_page",
            &args(r#"{"document":"doc-1","after_page":1,"width":null,"height":null}"#)
        ),
        Ok(Request::AddBlankPage {
            after: 1,
            size: None
        })
    );
    assert_eq!(
        parse(
            "add_text",
            &args(
                r#"{"document":"doc-1","page":1,"left":1,"top":1,"width":50,"text":"x","size":0,"font":"Noto Sans"}"#
            )
        ),
        Err("`size` is a number above 0, in points".to_owned())
    );
}

#[test]
fn a_plan_has_one_to_twenty_steps_each_with_words_and_a_state() {
    let plan = |steps: &str| parse("update_plan", &args(&format!(r#"{{"steps":{steps}}}"#)));
    let step = |text: &str| format!(r#"{{"text":"{text}","status":"pending"}}"#);
    let many = |count: usize| {
        format!(
            "[{}]",
            (0..count)
                .map(|at| step(&format!("step {at}")))
                .collect::<Vec<_>>()
                .join(",")
        )
    };
    assert!(plan(&many(1)).is_ok());
    assert!(plan(&many(20)).is_ok());
    assert!(
        plan(&many(21))
            .expect_err("too long")
            .contains("at most 20")
    );
    assert!(plan("[]").expect_err("empty").contains("is empty"));
    assert!(
        parse("update_plan", &args("{}"))
            .expect_err("missing")
            .starts_with("`steps` is needed")
    );
    assert!(
        plan(r#"[{"text":"  ","status":"done"}]"#)
            .expect_err("no words")
            .contains("`text`")
    );
    assert!(
        plan(r#"[{"text":"x","status":"doing"}]"#)
            .expect_err("no such state")
            .contains("pending, in_progress or done")
    );
    let Ok(Request::UpdatePlan { steps }) = plan(&format!(
        r#"[{{"text":"{}","status":"done"}}]"#,
        "word ".repeat(100)
    )) else {
        panic!("a long step reads");
    };
    assert_eq!(steps[0].text.chars().count(), 121, "cut short with a mark");
    assert!(steps[0].text.ends_with('\u{2026}'));
}

#[test]
fn only_a_write_over_and_taking_pages_out_are_destructive_calls() {
    let write = |extra: &str| writing(extra).expect("a write reads");
    assert!(!write("").is_destructive());
    assert!(!write(r#","replace":false"#).is_destructive());
    assert!(write(r#","replace":true"#).is_destructive());
    assert!(Request::DeletePages(vec![0]).is_destructive());
    assert!(!Request::Undo.is_destructive());
    assert!(
        !Request::UpdatePlan { steps: Vec::new() }.is_destructive(),
        "a plan changes nothing"
    );
}

#[test]
fn a_call_made_with_page_numbers_says_so() {
    let says = |name: &str, arguments: &str| {
        parse(name, &args(arguments))
            .expect("it reads")
            .counts_pages()
    };
    assert!(says("delete_pages", r#"{"document":"doc-1","pages":[2]}"#));
    assert!(says("render_page", r#"{"document":"doc-1","page":2}"#));
    assert!(says("read_text", r#"{"document":"doc-1","first_page":2}"#));
    assert!(!says("read_text", r#"{"document":"doc-1"}"#));
    assert!(!says(
        "replace_text",
        r#"{"document":"doc-1","block":"p1-b1","text":"x"}"#
    ));
    assert!(!says("undo", r#"{"document":"doc-1"}"#));
}

fn refused(name: &str, arguments: &str) -> String {
    parse(name, &args(arguments)).expect_err(arguments)
}

#[test]
fn a_replacement_is_refused_when_it_names_nothing_or_changes_nothing() {
    let call = |extra: &str| {
        refused(
            "find_and_replace",
            &format!(r#"{{"document":"doc-1"{extra}}}"#),
        )
    };
    assert!(call("").contains("`find` is needed"), "{}", call(""));
    assert!(call(r#","find":"x""#).contains("`replace_with` is needed"));
    assert!(call(r#","find":"","replace_with":"y""#).contains("`find` is empty"));
    assert!(
        call(r#","find":"same","replace_with":"same""#).contains("nothing to change"),
        "{}",
        call(r#","find":"same","replace_with":"same""#)
    );
    assert!(
        call(r#","find":"a","replace_with":"b","first_page":5,"last_page":2"#)
            .contains("`last_page` comes before `first_page`")
    );
    assert!(
        call(r#","find":"a","replace_with":"b","first_page":0"#).contains("`first_page`"),
        "pages are counted from 1"
    );
}

#[test]
fn a_style_is_refused_when_it_asks_for_nothing_or_for_something_that_is_not_one() {
    let call = |extra: &str| {
        refused(
            "style_text",
            &format!(r#"{{"document":"doc-1","block":"p1-b1"{extra}}}"#),
        )
    };
    assert!(call("").contains("nothing to change"), "{}", call(""));
    assert!(call(r#","align":"sideways""#).contains("`align` is left, center, right or justify"));
    assert!(call(r#","color":"red""#).contains("is not a colour"));
    assert!(call(r#","size":0"#).contains("`size` is a number from 1 to 1000"));
    assert!(call(r#","line_spacing":0.5"#).contains("`line_spacing` is a number from 0.8 to 10"));
    assert!(call(r#","bold":"yes""#).contains("`bold` is true or false"));
    assert!(call(r#","find":"","bold":true"#).contains("`find` is empty"));
    let none = refused("style_text", r#"{"document":"doc-1","bold":true}"#);
    assert!(none.contains("`block` is needed"), "{none}");
}

#[test]
fn a_mark_is_refused_for_a_way_that_is_not_one() {
    let why = refused(
        "mark_text",
        r#"{"document":"doc-1","text":"x","how":"circle"}"#,
    );
    assert!(
        why.contains("highlight, underline or strike_through"),
        "{why}"
    );
    assert!(refused("mark_text", r#"{"document":"doc-1","text":""}"#).contains("`text` is empty"));
    assert!(refused("mark_text", r#"{"document":"doc-1"}"#).contains("`text` is needed"));
}

#[test]
fn a_stamp_is_refused_for_a_kind_a_place_or_an_amount_that_is_not_one() {
    let call = |extra: &str| refused("add_stamp", &format!(r#"{{"document":"doc-1"{extra}}}"#));
    assert!(call("").contains("`kind` is needed"), "{}", call(""));
    assert!(call(r#","kind":"banner""#).contains("page_numbers, header_footer or watermark"));
    assert!(call(r#","kind":"watermark","position":"under""#).contains("`position` is"));
    assert!(call(r#","kind":"watermark","only":"third""#).contains("`only` is every, odd or even"));
    assert!(call(r#","kind":"watermark","opacity":0"#).contains("`opacity`"));
    assert!(call(r#","kind":"watermark","text":"  ""#).contains("`text` is empty"));
    assert!(call(r#","kind":"page_numbers","start_number":1.5"#).contains("whole number"));
}

#[test]
fn the_bookmark_actions_each_say_what_they_still_need() {
    let call = |extra: &str| refused("bookmarks", &format!(r#"{{"document":"doc-1"{extra}}}"#));
    assert!(call("").contains("`action` is needed"), "{}", call(""));
    assert!(call(r#","action":"sort""#).contains("`action` is list, add"));
    assert!(call(r#","action":"add""#).contains("`page` or `block` is needed"));
    assert!(call(r#","action":"add","page":2"#).contains("`title` is needed"));
    assert!(call(r#","action":"add","page":2,"block":"p1-b1","title":"x""#).contains("not both"));
    assert!(call(r#","action":"rename","bookmark":1"#).contains("`title` is needed"));
    assert!(call(r#","action":"rename","title":"x""#).contains("`bookmark` is needed"));
    assert!(call(r#","action":"retarget","bookmark":1"#).contains("`page` is needed"));
    assert!(call(r#","action":"move","bookmark":1"#).contains("`direction` is needed"));
    assert!(
        call(r#","action":"move","bookmark":1,"direction":"sideways""#)
            .contains("up, down, in or out")
    );
    assert!(call(r#","action":"delete""#).contains("`bookmark` is needed"));
    assert!(call(r#","action":"delete","bookmark":0"#).contains("whole number from 1"));
}

#[test]
fn a_picture_is_refused_without_a_place_or_with_two_sources_or_no_size() {
    let call = |extra: &str| {
        refused(
            "place_picture",
            &format!(r#"{{"document":"doc-1","page":1{extra}}}"#),
        )
    };
    assert!(call("").contains("`left` is needed"), "{}", call(""));
    assert!(call(r#","left":0"#).contains("`top` is needed"));
    assert!(call(r#","left":0,"top":0,"attachment":1,"path":"/a.png""#).contains("not both"));
    assert!(call(r#","left":0,"top":0,"width":0"#).contains("`width` is a number from 1"));
    assert!(call(r#","left":0,"top":0,"path":" ""#).contains("`path` is empty"));
}

#[test]
fn an_object_call_names_a_page_or_an_object_the_way_list_gave_it() {
    let call = |extra: &str| refused("objects", &format!(r#"{{"document":"doc-1"{extra}}}"#));
    assert!(call("").contains("`action` is needed"), "{}", call(""));
    assert!(call(r#","action":"list""#).contains("`page` is needed"));
    assert!(call(r#","action":"move","left":1"#).contains("`object` is needed"));
    assert!(
        call(r#","action":"move","object":"the logo""#).contains("is not the name of an object")
    );
    assert!(
        call(r#","action":"spin","object":"p1-o1""#)
            .contains("`action` is list, move, resize or delete")
    );
}

#[test]
fn a_page_to_show_and_a_region_to_look_at_are_read_as_asked_or_refused() {
    assert!(
        refused("go_to_page", r#"{"document":"doc-1","page":0}"#).contains("page number from 1")
    );
    let region = |extra: &str| {
        refused(
            "look_closer",
            &format!(r#"{{"document":"doc-1","page":1{extra}}}"#),
        )
    };
    assert!(region("").contains("`left` is needed, in points from the left edge"));
    assert!(region(r#","left":50,"top":0,"right":10,"bottom":10"#).contains("region is empty"));
    assert!(
        region(r#","left":0,"top":0,"right":10,"bottom":10,"dpi":5"#)
            .contains("`dpi` is a number from 20 to 600")
    );
    assert!(region(r#","left":0,"top":0,"right":10,"bottom":10,"dpi":900"#).contains("`dpi`"));
}

#[test]
fn deleting_a_bookmark_or_an_object_is_destructive_and_listing_them_only_reads() {
    let read = |name: &str, arguments: &str| parse(name, &args(arguments)).expect("it reads");
    let delete_bookmark = read(
        "bookmarks",
        r#"{"document":"doc-1","action":"delete","bookmark":1}"#,
    );
    assert!(delete_bookmark.is_destructive() && !delete_bookmark.only_reads());
    let replace_all = read(
        "bookmarks",
        r#"{"document":"doc-1","action":"from_headings","replace":true}"#,
    );
    assert!(
        replace_all.is_destructive(),
        "it takes the bookmarks there are out"
    );
    let keep = read(
        "bookmarks",
        r#"{"document":"doc-1","action":"from_headings"}"#,
    );
    assert!(!keep.is_destructive(), "negative control: adding only");
    let list = read("bookmarks", r#"{"document":"doc-1","action":"list"}"#);
    assert!(list.only_reads() && !list.is_destructive());
    let delete_object = read(
        "objects",
        r#"{"document":"doc-1","action":"delete","object":"p1-o1"}"#,
    );
    assert!(delete_object.is_destructive());
    let move_object = read(
        "objects",
        r#"{"document":"doc-1","action":"move","object":"p1-o1","left":1}"#,
    );
    assert!(!move_object.is_destructive() && !move_object.only_reads());
    assert!(
        read(
            "objects",
            r#"{"document":"doc-1","action":"list","page":1}"#
        )
        .only_reads()
    );
    assert!(read("go_to_page", r#"{"document":"doc-1","page":1}"#).only_reads());
    assert!(
        read(
            "look_closer",
            r#"{"document":"doc-1","page":1,"left":0,"top":0,"right":1,"bottom":1}"#
        )
        .only_reads()
    );
    assert!(
        !read(
            "find_and_replace",
            r#"{"document":"doc-1","find":"a","replace_with":"b"}"#
        )
        .is_destructive()
    );
}

#[test]
fn the_new_calls_say_whether_they_carry_page_numbers() {
    let says = |name: &str, arguments: &str| {
        parse(name, &args(arguments))
            .expect("it reads")
            .counts_pages()
    };
    assert!(says(
        "find_and_replace",
        r#"{"document":"doc-1","find":"a","replace_with":"b","first_page":2}"#
    ));
    assert!(!says(
        "find_and_replace",
        r#"{"document":"doc-1","find":"a","replace_with":"b"}"#
    ));
    assert!(says(
        "mark_text",
        r#"{"document":"doc-1","text":"a","last_page":2}"#
    ));
    assert!(says(
        "add_stamp",
        r#"{"document":"doc-1","kind":"page_numbers","pages":"2-3"}"#
    ));
    assert!(!says(
        "add_stamp",
        r#"{"document":"doc-1","kind":"page_numbers"}"#
    ));
    assert!(says(
        "bookmarks",
        r#"{"document":"doc-1","action":"add","title":"x","page":3}"#
    ));
    assert!(!says(
        "bookmarks",
        r#"{"document":"doc-1","action":"delete","bookmark":3}"#
    ));
    assert!(says(
        "place_picture",
        r#"{"document":"doc-1","page":1,"left":0,"top":0}"#
    ));
    assert!(says(
        "objects",
        r#"{"document":"doc-1","action":"list","page":1}"#
    ));
    assert!(!says(
        "objects",
        r#"{"document":"doc-1","action":"delete","object":"p1-o1"}"#
    ));
    assert!(says("go_to_page", r#"{"document":"doc-1","page":2}"#));
    assert!(says(
        "look_closer",
        r#"{"document":"doc-1","page":1,"left":0,"top":0,"right":1,"bottom":1}"#
    ));
    assert!(!says(
        "style_text",
        r#"{"document":"doc-1","block":"p1-b1","bold":true}"#
    ));
}

#[test]
fn only_redacting_and_removing_a_link_are_destructive_among_the_file_and_form_calls() {
    let read = |name: &str, arguments: &str| parse(name, &args(arguments)).expect("it reads");
    assert!(
        read(
            "convert",
            r#"{"document":"doc-1","tool":"redact-pdf","options":{"search":["x"]}}"#
        )
        .is_destructive()
    );
    for harmless in [
        r#"{"document":"doc-1","tool":"compress-pdf"}"#,
        r#"{"document":"doc-1","tool":"pdf-to-word"}"#,
        r#"{"document":"doc-1","tool":"compare-pdf","files":["/a.pdf"]}"#,
    ] {
        assert!(!read("convert", harmless).is_destructive(), "{harmless}");
    }
    assert!(!read("protect_document", r#"{"document":"doc-1","password":"x"}"#).is_destructive());
    assert!(
        read(
            "links",
            r#"{"document":"doc-1","action":"remove","link":"p1-l1"}"#
        )
        .is_destructive()
    );
    for (name, arguments) in [
        ("ocr_pages", r#"{"document":"doc-1"}"#),
        ("extract_pages", r#"{"document":"doc-1","pages":"1"}"#),
        ("split_document", r#"{"document":"doc-1","every":2}"#),
        ("export_page_pictures", r#"{"document":"doc-1"}"#),
        ("save_copy", r#"{"document":"doc-1"}"#),
        (
            "draw_shape",
            r#"{"document":"doc-1","page":1,"shape":"line","left":0,"top":0,"right":9,"bottom":9}"#,
        ),
        (
            "add_field",
            r#"{"document":"doc-1","page":1,"kind":"text","left":0,"top":0,"width":50,"height":20}"#,
        ),
        (
            "set_tab_order",
            r#"{"document":"doc-1","page":1,"order":"rows"}"#,
        ),
    ] {
        let request = read(name, arguments);
        assert!(!request.is_destructive() && !request.only_reads(), "{name}");
    }
    assert!(
        read("links", r#"{"document":"doc-1","action":"list","page":1}"#).only_reads(),
        "listing links changes nothing"
    );
}

#[test]
fn the_file_and_form_calls_say_whether_they_carry_page_numbers() {
    let says = |name: &str, arguments: &str| {
        parse(name, &args(arguments))
            .expect("it reads")
            .counts_pages()
    };
    assert!(says("extract_pages", r#"{"document":"doc-1","pages":"2"}"#));
    assert!(says("split_document", r#"{"document":"doc-1","every":2}"#));
    assert!(says(
        "export_page_pictures",
        r#"{"document":"doc-1","pages":"2"}"#
    ));
    assert!(!says("export_page_pictures", r#"{"document":"doc-1"}"#));
    assert!(says("ocr_pages", r#"{"document":"doc-1","pages":"2"}"#));
    assert!(!says("ocr_pages", r#"{"document":"doc-1"}"#));
    assert!(says(
        "convert",
        r#"{"document":"doc-1","tool":"pdf-to-text","options":{"pages":"2"}}"#
    ));
    assert!(!says(
        "convert",
        r#"{"document":"doc-1","tool":"pdf-to-text"}"#
    ));
    assert!(says(
        "links",
        r#"{"document":"doc-1","action":"list","page":1}"#
    ));
    assert!(!says(
        "links",
        r#"{"document":"doc-1","action":"remove","link":"p1-l1"}"#
    ));
    assert!(says(
        "draw_shape",
        r#"{"document":"doc-1","page":1,"shape":"line","left":0,"top":0,"right":9,"bottom":9}"#
    ));
    assert!(says(
        "set_tab_order",
        r#"{"document":"doc-1","page":1,"order":"rows"}"#
    ));
    assert!(!says("save_copy", r#"{"document":"doc-1"}"#));
    assert!(!says(
        "protect_document",
        r#"{"document":"doc-1","password":"x"}"#
    ));
}

#[test]
fn a_save_copy_path_is_a_full_path_and_nothing_else() {
    let refuse = |arguments: &str| refused("save_copy", arguments);
    assert!(refuse(r#"{"document":"doc-1","path":"copy.pdf"}"#).contains("is not a full path"));
    assert!(refuse(r#"{"document":"doc-1","path":"../copy.pdf"}"#).contains("is not a full path"));
    assert!(refuse(r#"{"document":"doc-1","path":"  "}"#).contains("is empty"));
    let home = parse(
        "save_copy",
        &args(r#"{"document":"doc-1","path":"~/copy.pdf"}"#),
    );
    if std::env::var_os("HOME").is_some() {
        assert!(
            home.is_ok(),
            "a path from the home folder is full: {home:?}"
        );
    }
}
