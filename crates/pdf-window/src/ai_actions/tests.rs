use pdf_agent::connect::{ToolCall, ToolResult};
use pdf_agent::desk::Block;
use pdf_agent::json::Json;
use pdf_agent::tools::request::{NewText, Request};
use pdf_app::Applied;
use pdf_app::ai_permission::{Answer, Decision, Mode, decide, refusal_text};
use pdf_app::wording::{Lang, Message, Refusal};

use super::{Named, Pending, Tools, parse_block_name, result_of, the_same_block};

fn call(id: &str, name: &str) -> ToolCall {
    ToolCall {
        id: id.to_owned(),
        name: name.to_owned(),
        arguments: Json::Null,
        problem: None,
    }
}

fn waiting_to_ask(name: &str) -> Tools {
    let mut tools = Tools::default();
    let call = call("call_1", name);
    tools.take(std::slice::from_ref(&call));
    tools.ask = Some(Pending {
        call,
        request: Request::ReplaceText {
            block: "p1-b1".to_owned(),
            find: None,
            text: "Hello".to_owned(),
        },
        may_allow_for_chat: true,
    });
    tools
}

fn would_ask(tools: &Tools, name: &str) -> bool {
    matches!(
        decide(
            Mode::AskBeforeChanges,
            name,
            pdf_agent::tools::facts(name).map(|facts| (facts.read_only, facts.destructive)),
            tools.allowed_for_chat.contains(name),
        ),
        Decision::Ask { .. }
    )
}

#[test]
fn allowing_once_asks_again_next_time() {
    let mut tools = waiting_to_ask("replace_text");
    tools.answer_the_card(Answer::Once);
    assert!(tools.ask.is_none());
    assert!(tools.allowed_for_chat.is_empty());
    assert!(would_ask(&tools, "replace_text"));

    let mut tools = waiting_to_ask("replace_text");
    tools.answer_the_card(Answer::ForThisChat);
    assert!(!would_ask(&tools, "replace_text"));
}

#[test]
fn the_call_just_allowed_is_performed_rather_than_asked_about_again() {
    let mut tools = waiting_to_ask("replace_text");
    let this = call("call_1", "replace_text");
    let next = call("call_2", "replace_text");
    tools.answer_the_card(Answer::Once);
    assert!(tools.allowed_just_now(&this));
    assert!(!tools.allowed_just_now(&next));
    tools.answer(ToolResult::said("call_1", "Done."));
    assert!(!tools.allowed_just_now(&this));
}

#[test]
fn allowing_for_this_chat_is_forgotten_by_a_new_chat() {
    let mut tools = waiting_to_ask("replace_text");
    tools.answer_the_card(Answer::ForThisChat);
    assert!(tools.allowed_for_chat.contains("replace_text"));
    tools.clear();
    assert!(tools.allowed_for_chat.is_empty());
    assert!(would_ask(&tools, "replace_text"));
}

#[test]
fn a_refusal_reaches_the_model_in_words() {
    let mut tools = waiting_to_ask("delete_pages");
    tools.answer_the_card(Answer::Refuse);
    assert!(tools.ask.is_none());
    assert!(tools.queue.is_empty());
    assert_eq!(tools.results.len(), 1);
    assert!(tools.results[0].is_error);
    assert_eq!(tools.results[0].text, refusal_text());
    assert_eq!(tools.results[0].call_id, "call_1");
    assert!(tools.allowed_for_chat.is_empty());
}

fn block(index: usize, text: &str, area: [f64; 4]) -> Block {
    Block {
        page: 0,
        index,
        text: text.to_owned(),
        area,
        size: 12.0,
        fixed: None,
    }
}

#[test]
fn an_applied_edit_becomes_the_answer_to_its_call() {
    let request = Request::ReplaceText {
        block: "p1-b1".to_owned(),
        find: None,
        text: "Hello".to_owned(),
    };
    let now = block(0, "Hello", [10.0, 10.0, 100.0, 30.0]);
    let result = result_of(
        "call_1",
        (
            &Applied::Changed {
                page: 0,
                region: None,
            },
            &request,
        ),
        Some(&now),
    );
    assert!(!result.is_error);
    assert_eq!(result.text, "Done. p1-b1 now reads: Hello");

    let result = result_of(
        "call_1",
        (
            &Applied::Changed {
                page: 0,
                region: None,
            },
            &request,
        ),
        None,
    );
    assert!(result.text.contains("read_text that page again"));

    for (request, text) in [
        (Request::DeletePages(vec![1, 2]), "Took out 2 pages."),
        (
            Request::MovePages {
                pages: vec![0],
                to: 1,
            },
            "Moved.",
        ),
        (
            Request::RotatePages {
                pages: vec![0],
                quarter_turns: 1,
            },
            "Turned.",
        ),
        (Request::Undo, "Took back the last change."),
        (
            Request::AddText {
                page: 1,
                area: [0.0, 0.0, 10.0, 10.0],
                text: "x".to_owned(),
                style: NewText {
                    family: "Noto Sans".to_owned(),
                    size: 12.0,
                    bold: false,
                    italic: false,
                    fill: None,
                },
            },
            "Written on page 2 in Noto Sans, 12 pt.",
        ),
    ] {
        let result = result_of(
            "call_1",
            (
                &Applied::Changed {
                    page: 1,
                    region: None,
                },
                &request,
            ),
            None,
        );
        assert_eq!(result.text, text);
        assert!(!result.is_error);
    }
}

#[test]
fn an_edit_that_changed_nothing_is_still_an_answer() {
    let result = result_of("call_1", (&Applied::Unchanged, &Request::Undo), None);
    assert!(!result.is_error);
    assert_eq!(result.text, "There is nothing to undo.");
}

#[test]
fn an_engine_refusal_becomes_an_english_error_result() {
    let reason = Refusal::EditingRestricted;
    let result = result_of(
        "call_1",
        (&Applied::Refused(reason.clone()), &Request::Undo),
        None,
    );
    assert!(result.is_error);
    assert_eq!(result.text, Message::Refused(reason).say(Lang::English));
}

fn named(epoch: u64, text: &str, area: [f64; 4]) -> Named {
    Named {
        epoch,
        arranged: 0,
        text: text.to_owned(),
        area,
    }
}

#[test]
fn a_block_name_is_refused_once_its_page_has_changed_under_it() {
    let was = named(7, "The heading", [10.0, 10.0, 100.0, 30.0]);
    assert_eq!(
        the_same_block("p1-b1", (0, 0), &was, (0, 7), None),
        Ok((0, 0))
    );
    let now = [
        block(0, "Something new", [10.0, 40.0, 100.0, 60.0]),
        block(1, "The heading", [10.0, 10.0, 100.0, 30.0]),
    ];
    assert_eq!(
        the_same_block("p1-b1", (0, 0), &was, (0, 8), Some(&now)),
        Ok((0, 1))
    );
    let gone = [block(0, "Something else", [10.0, 10.0, 100.0, 30.0])];
    let refused = the_same_block("p1-b1", (0, 0), &was, (0, 8), Some(&gone));
    assert!(
        refused
            .as_ref()
            .is_err_and(|why| why.contains("no longer on the page")),
        "{refused:?}"
    );
    let twice = [
        block(0, "The heading", [10.0, 10.0, 100.0, 30.0]),
        block(1, "The heading", [12.0, 12.0, 102.0, 32.0]),
    ];
    let refused = the_same_block("p1-b1", (0, 0), &was, (0, 8), Some(&twice));
    assert!(
        refused
            .as_ref()
            .is_err_and(|why| why.contains("cannot be told apart")),
        "{refused:?}"
    );
    let refused = the_same_block("p1-b1", (0, 0), &was, (0, 8), None);
    assert!(
        refused
            .as_ref()
            .is_err_and(|why| why.contains("has not been read since it changed")),
        "{refused:?}"
    );
}

#[test]
fn a_name_from_before_the_pages_moved_is_refused_outright() {
    let was = named(7, "The heading", [10.0, 10.0, 100.0, 30.0]);
    let now = [block(0, "The heading", [10.0, 10.0, 100.0, 30.0])];
    let refused = the_same_block("p1-b1", (0, 0), &was, (1, 7), Some(&now));
    assert!(
        refused
            .as_ref()
            .is_err_and(|why| why.contains("before the pages were moved about")),
        "{refused:?}"
    );
}

#[test]
fn a_block_name_reads_as_a_page_and_a_block() {
    assert_eq!(parse_block_name("p3-b12"), Ok((2, 11)));
    assert_eq!(parse_block_name(" p1-b1 "), Ok((0, 0)));
    for wrong in ["p0-b1", "p1-b0", "3-12", "p3b12", "pa-b1", ""] {
        assert!(parse_block_name(wrong).is_err(), "{wrong}");
    }
}

#[test]
fn stopping_drops_what_has_not_been_done() {
    let mut tools = waiting_to_ask("replace_text");
    tools.take(&[call("call_2", "delete_pages")]);
    assert!(tools.busy());
    tools.drop_the_queue();
    assert!(!tools.busy());
    assert!(tools.ask.is_none());
    assert_eq!(tools.rounds, 0);
}

#[test]
fn every_result_answers_the_call_that_asked_for_it() {
    let mut tools = Tools::default();
    tools.take(&[call("call_1", "read_text"), call("call_2", "list_fonts")]);
    tools.answer(ToolResult::said("call_1", "read"));
    tools.answer(ToolResult::said("call_2", "fonts"));
    assert!(tools.queue.is_empty());
    let ids: Vec<&str> = tools
        .results
        .iter()
        .map(|result| result.call_id.as_str())
        .collect();
    assert_eq!(ids, ["call_1", "call_2"]);
}

fn asked_a_question() -> Tools {
    let mut tools = Tools::default();
    tools.take(&[call("call_q", "ask_person")]);
    tools.question = Some(super::Question {
        call: "call_q".to_owned(),
        asked: "Which theme?".to_owned(),
        options: vec![
            ("ocean".to_owned(), String::new()),
            ("forest".to_owned(), String::new()),
        ],
        own: String::new(),
    });
    tools
}

#[test]
fn the_persons_answer_is_the_questions_result() {
    let mut tools = asked_a_question();
    assert!(tools.busy(), "waiting on the person");
    tools.answer_the_question(super::QuestionReply::Said("forest".to_owned()));
    assert!(tools.question.is_none());
    assert_eq!(
        tools.results,
        [ToolResult::said("call_q", "The person answered: forest")]
    );
    assert!(!tools.busy());

    let mut tools = asked_a_question();
    tools.answer_the_question(super::QuestionReply::Skipped);
    let [result] = tools.results.as_slice() else {
        panic!("one result");
    };
    assert!(!result.is_error, "skipping is an answer, not a failure");
    assert!(
        result.text.starts_with("The person skipped"),
        "{}",
        result.text
    );
    assert!(!tools.busy());
}

#[test]
fn a_question_needs_no_permission() {
    let facts =
        pdf_agent::tools::facts("ask_person").map(|facts| (facts.read_only, facts.destructive));
    for mode in [Mode::AskBeforeChanges, Mode::DoIt] {
        assert_eq!(decide(mode, "ask_person", facts, false), Decision::Run);
    }
    assert!(matches!(
        decide(Mode::ChatOnly, "ask_person", facts, false),
        Decision::Refuse(_)
    ));
}

#[test]
fn what_is_being_done_is_said() {
    use super::Doing;
    let mut tools = Tools::default();
    assert!(tools.doing().is_none());
    tools.take(&[ToolCall {
        id: "call_r".to_owned(),
        name: "read_text".to_owned(),
        arguments: Json::parse(r#"{"document":"doc-1","first_page":2,"last_page":2}"#)
            .expect("JSON"),
        problem: None,
    }]);
    let Some(Doing::Tool(name, request)) = tools.doing() else {
        panic!("a tool");
    };
    assert_eq!(name, "read_text");
    assert_eq!(
        pdf_app::ai_status::doing(&request, Lang::English),
        "Reading page 2"
    );
    let mut asking = asked_a_question();
    asking.queue.clear();
    assert!(matches!(asking.doing(), Some(Doing::Asking)));
}

#[test]
fn a_call_that_could_not_be_read_is_answered_as_failed_and_the_rest_go_on() {
    let mut tools = Tools::default();
    let broken = ToolCall::unreadable("call_1", "write_pages", "it was cut off");
    let fine = call("call_2", "read_text");
    tools.take(&[broken.clone(), fine.clone()]);
    assert!(tools.answer_a_call_that_could_not_be_read(&broken));
    assert_eq!(
        tools.results,
        vec![ToolResult::failed("call_1", "it was cut off")]
    );
    assert!(!tools.answer_a_call_that_could_not_be_read(&fine));
    assert_eq!(
        tools.queue.front().map(|call| call.id.as_str()),
        Some("call_2")
    );
    assert_eq!(tools.results.len(), 1);
}

fn a_window_the_assistant_is_waiting_on() -> crate::window_state::Window {
    let editor = pdf_app::Editor::blank(crate::chrome::A4).expect("a blank page");
    let mut window =
        crate::window_state::Window::new(editor, std::path::PathBuf::new(), Vec::new());
    let waiting = call("call_9", "delete_pages");
    window.ai.tools.take(std::slice::from_ref(&waiting));
    window.ai.tools.sent = Some(super::Sent {
        call: waiting,
        request: Request::DeletePages(vec![0]),
        was: None,
    });
    window
}

fn the_only_page_cannot_be_taken_out(
    window: &mut crate::window_state::Window,
) -> pdf_app::EditOutcome {
    window
        .editor
        .begin_remove_pages(&[0])
        .expect("an edit to try")
        .run()
}

#[test]
fn an_edit_that_stopped_unexpectedly_is_answered_to_the_assistant_and_not_waited_for() {
    let mut window = a_window_the_assistant_is_waiting_on();
    let stopped = std::thread::spawn(|| -> pdf_app::EditOutcome {
        panic!("the edit thread stopped on purpose in this test")
    });
    while !stopped.is_finished() {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    window.running = Some(crate::window_state::Running { handle: stopped });
    window.collect(&eframe::egui::Context::default());
    window.collect_a_sent_edit();
    assert!(!window.ai.tools.waits_for_an_edit(), "still waiting");
    assert!(window.ai.tools.queue.is_empty());
    let [result] = window.ai.tools.results.as_slice() else {
        panic!("one answer was owed: {:?}", window.ai.tools.results);
    };
    assert_eq!(result.call_id, "call_9");
    assert!(result.is_error, "{result:?}");
    assert!(result.text.contains("failed unexpectedly"), "{result:?}");
}

#[test]
fn an_edit_refused_for_the_assistant_does_not_take_the_persons_waiting_typing_with_it() {
    let mut control = a_window_the_assistant_is_waiting_on();
    control.ai.tools.sent = None;
    control.input.accept("typed meanwhile");
    let outcome = the_only_page_cannot_be_taken_out(&mut control);
    assert!(matches!(control.took_back(outcome), Applied::Refused(_)));
    assert!(
        control.input.draft().is_some(),
        "known answer: a refusal of the person's own edit parks what was waiting"
    );

    let mut window = a_window_the_assistant_is_waiting_on();
    window.input.accept("typed meanwhile");
    let outcome = the_only_page_cannot_be_taken_out(&mut window);
    assert!(matches!(window.took_back(outcome), Applied::Refused(_)));
    assert!(window.input.draft().is_none(), "the typing was set aside");
    assert_eq!(
        window.input.queued_chars(),
        "typed meanwhile".chars().count()
    );
}
