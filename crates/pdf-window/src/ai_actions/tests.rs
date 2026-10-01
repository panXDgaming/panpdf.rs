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
        of_this_tool: 1,
        written_to: None,
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
fn a_fresh_chat_drops_everything_that_was_queued() {
    let mut tools = waiting_to_ask("replace_text");
    tools.take(&[call("call_2", "delete_pages")]);
    assert!(tools.busy());
    tools.drop_the_queue();
    assert!(!tools.busy());
    assert!(tools.ask.is_none());
}

#[test]
fn stopping_answers_the_calls_not_run_and_keeps_what_was_finished() {
    let mut tools = Tools::default();
    tools.take(&[
        call("call_1", "read_text"),
        call("call_2", "replace_text"),
        call("call_3", "delete_pages"),
    ]);
    tools.answer(ToolResult::said("call_1", "read"));
    let results = tools
        .stop()
        .expect("nothing is on its way, so it ends at once");
    assert!(!tools.busy());
    let said: Vec<(&str, bool, &str)> = results
        .iter()
        .map(|result| {
            (
                result.call_id.as_str(),
                result.is_error,
                result.text.as_str(),
            )
        })
        .collect();
    assert_eq!(
        said,
        [
            ("call_1", false, "read"),
            ("call_2", true, "not run: the person stopped the assistant"),
            ("call_3", true, "not run: the person stopped the assistant"),
        ]
    );
}

#[test]
fn stopping_while_a_card_or_a_question_waits_answers_that_call_too() {
    let mut tools = waiting_to_ask("delete_pages");
    tools.take(&[call("call_2", "replace_text")]);
    let results = tools.stop().expect("it ends at once");
    assert_eq!(results.len(), 2);
    assert!(tools.ask.is_none());

    let mut asking = asked_a_question();
    let results = asking.stop().expect("it ends at once");
    assert_eq!(results.len(), 1);
    assert!(asking.question.is_none());
}

#[test]
fn stopping_while_an_edit_is_on_its_way_waits_for_it_and_keeps_its_answer() {
    let mut tools = Tools::default();
    tools.take(&[
        call("call_1", "delete_pages"),
        call("call_2", "replace_text"),
    ]);
    tools.sent = Some(super::Sent {
        call: call("call_1", "delete_pages"),
        request: Request::DeletePages(vec![0]),
        was: None,
        success: None,
        changed: Vec::new(),
    });
    assert!(tools.stop().is_none(), "the edit has to land first");
    assert!(tools.busy());
    assert_eq!(tools.queue.len(), 1, "only the edit in the air stays");

    tools.sent = None;
    tools.answer(ToolResult::said("call_1", "Took out 1 page."));
    assert_eq!(tools.closing_results().len(), 2);
    let results = tools.closing_results();
    assert!(results.is_empty(), "and it is not said twice");
    assert!(!tools.busy());
}

#[test]
fn allowing_all_of_a_batch_covers_each_call_of_that_tool_and_no_other() {
    let mut tools = waiting_to_ask("replace_text");
    tools.take(&[
        call("call_2", "replace_text"),
        call("call_3", "delete_pages"),
    ]);
    tools.answer_the_card(Answer::AllOfThem);
    assert!(tools.allowed_just_now(&call("call_1", "replace_text")));
    assert!(tools.allowed_just_now(&call("call_2", "replace_text")));
    assert!(!tools.allowed_just_now(&call("call_3", "delete_pages")));
    assert!(
        tools.allowed_for_chat.is_empty(),
        "it covers this batch, not the chat"
    );
    tools.allowed_all = None;
    assert!(!tools.allowed_just_now(&call("call_2", "replace_text")));
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
        success: None,
        changed: Vec::new(),
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

pub(super) fn tool(id: &str, name: &str, arguments: &str) -> ToolCall {
    ToolCall::asked(id, name, Json::parse(arguments).expect("JSON"))
}

pub(super) fn land_what_was_sent(window: &mut crate::window_state::Window) {
    let context = eframe::egui::Context::default();
    for _ in 0..5_000 {
        window.collect(&context);
        if window.running.is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(window.running.is_none(), "the edit never finished");
    window.collect_a_sent_edit();
}

pub(super) fn run(window: &mut crate::window_state::Window, call: &ToolCall) -> ToolResult {
    window.ai.tools.take(std::slice::from_ref(call));
    window.ai.tools.revision_before = window.editor.revision();
    match window.perform(call) {
        super::Performed::Done(result) => {
            window.ai.tools.answer(result);
        }
        super::Performed::Sent => land_what_was_sent(window),
        _ => panic!("{} neither finished nor was sent", call.name),
    }
    window
        .ai
        .tools
        .results
        .pop()
        .expect("the call was answered")
}

pub(super) fn a_blank_window() -> crate::window_state::Window {
    let editor = pdf_app::Editor::blank(crate::chrome::A4).expect("a blank page");
    crate::window_state::Window::new(editor, std::path::PathBuf::new(), Vec::new())
}

pub(super) fn undo_steps_left(window: &mut crate::window_state::Window) -> usize {
    let mut steps = 0;
    while window.editor.can_undo() {
        assert!(matches!(window.editor.undo(), Applied::Changed { .. }));
        steps += 1;
        assert!(steps < 100, "undo never ends");
    }
    steps
}

#[test]
fn each_tool_call_that_changes_the_document_is_one_undo_step() {
    let mut window = a_blank_window();
    let calls = [
        tool(
            "c1",
            "add_blank_page",
            r#"{"document":"doc-1","after_page":1}"#,
        ),
        tool(
            "c2",
            "set_properties",
            r#"{"document":"doc-1","title":"A report"}"#,
        ),
        tool(
            "c3",
            "rotate_pages",
            r#"{"document":"doc-1","pages":[1],"degrees":90}"#,
        ),
        tool(
            "c4",
            "add_blank_page",
            r#"{"document":"doc-1","after_page":0}"#,
        ),
    ];
    for call in &calls {
        let result = run(&mut window, call);
        assert!(!result.is_error, "{}: {}", call.name, result.text);
    }
    assert_eq!(window.ai.tools.run.steps(), 4, "the run counts its steps");
    assert_eq!(window.editor.page_count(), 3);
    assert_eq!(undo_steps_left(&mut window), 4, "one step for each call");
    assert_eq!(window.editor.page_count(), 1);
}

#[test]
fn a_whole_document_written_by_one_call_is_one_undo_step() {
    let mut window = a_blank_window();
    assert!(
        window.editor.fonts().is_some(),
        "this test needs the packaged fonts"
    );
    let markdown = "A paragraph of words that goes on and on and on.\\n\\n".repeat(14);
    let call = tool(
        "w1",
        "write_pages",
        &format!(
            r#"{{"document":"doc-1","markdown":"{markdown}","font":"DejaVu Sans","size":40,"replace":true}}"#
        ),
    );
    let result = run(&mut window, &call);
    assert!(!result.is_error, "{}", result.text);
    assert!(result.text.contains("one step"), "{}", result.text);
    let pages = window.editor.page_count();
    assert!(pages >= 2, "the words ran onto new pages: {pages}");
    assert_eq!(window.ai.tools.run.steps(), 1);
    assert_eq!(
        undo_steps_left(&mut window),
        1,
        "{pages} pages of text and shapes are one undo"
    );
    assert_eq!(
        window.editor.page_count(),
        1,
        "and the pages are gone with it"
    );
}

#[test]
fn the_assistants_own_undo_never_takes_back_a_step_that_was_not_its_own() {
    let mut window = a_blank_window();
    let size = [595.0, 842.0];
    assert!(matches!(
        window.editor.add_page(0, false, size),
        Applied::Changed { .. }
    ));
    let undo = tool("u1", "undo", r#"{"document":"doc-1"}"#);
    let refused = run(&mut window, &undo);
    assert!(refused.is_error, "{refused:?}");
    assert!(
        refused.text.contains("nothing of yours"),
        "{}",
        refused.text
    );
    assert_eq!(
        window.editor.page_count(),
        2,
        "the person's page is still there"
    );

    let add = tool(
        "a1",
        "add_blank_page",
        r#"{"document":"doc-1","after_page":0}"#,
    );
    assert!(!run(&mut window, &add).is_error);
    assert_eq!(window.editor.page_count(), 3);
    let undone = run(&mut window, &tool("u2", "undo", r#"{"document":"doc-1"}"#));
    assert!(!undone.is_error, "{undone:?}");
    assert_eq!(window.editor.page_count(), 2, "its own step went");
    let again = run(&mut window, &tool("u3", "undo", r#"{"document":"doc-1"}"#));
    assert!(
        again.is_error && again.text.contains("nothing of yours"),
        "{again:?}"
    );
    assert_eq!(window.editor.page_count(), 2);

    let redo = run(&mut window, &tool("r1", "redo", r#"{"document":"doc-1"}"#));
    assert!(!redo.is_error, "{redo:?}");
    assert_eq!(
        window.editor.page_count(),
        3,
        "it may put back what it took back"
    );
}

fn the_run_is_taken_back(window: &mut crate::window_state::Window) {
    window.take_the_run_back();
    let context = eframe::egui::Context::default();
    for _ in 0..5_000 {
        window.collect(&context);
        window.carry_on_taking_the_run_back();
        if window.ai.tools.taking_back.is_none() && window.running.is_none() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    panic!("the run was never taken back");
}

#[test]
fn taking_the_whole_run_back_leaves_the_persons_own_steps_alone() {
    let mut window = a_blank_window();
    assert!(matches!(
        window.editor.add_page(0, false, [595.0, 842.0]),
        Applied::Changed { .. }
    ));
    for (id, name, arguments) in [
        (
            "a1",
            "add_blank_page",
            r#"{"document":"doc-1","after_page":0}"#,
        ),
        (
            "a2",
            "set_properties",
            r#"{"document":"doc-1","title":"X"}"#,
        ),
        (
            "a3",
            "add_blank_page",
            r#"{"document":"doc-1","after_page":1}"#,
        ),
    ] {
        assert!(!run(&mut window, &tool(id, name, arguments)).is_error);
    }
    assert_eq!(window.editor.page_count(), 4);
    assert_eq!(window.ai.tools.run.steps(), 3);

    the_run_is_taken_back(&mut window);
    assert_eq!(
        window.editor.page_count(),
        2,
        "only the person's page is left"
    );
    assert_eq!(window.ai.tools.run.steps(), 0);
    assert!(
        window.editor.can_undo(),
        "the person's step is still theirs to undo"
    );
    assert_eq!(undo_steps_left(&mut window), 1);
}

#[test]
fn a_run_is_not_taken_back_once_the_person_has_edited_after_it() {
    let mut window = a_blank_window();
    assert!(
        !run(
            &mut window,
            &tool(
                "a1",
                "add_blank_page",
                r#"{"document":"doc-1","after_page":1}"#
            )
        )
        .is_error
    );
    assert!(matches!(
        window.editor.add_page(0, false, [595.0, 842.0]),
        Applied::Changed { .. }
    ));
    window.take_the_run_back();
    assert!(window.ai.tools.taking_back.is_none(), "nothing was started");
    assert_eq!(window.editor.page_count(), 3, "nobody's page was taken");

    let mut control = a_blank_window();
    assert!(
        !run(
            &mut control,
            &tool(
                "a1",
                "add_blank_page",
                r#"{"document":"doc-1","after_page":1}"#
            )
        )
        .is_error
    );
    control.take_the_run_back();
    assert!(
        control.ai.tools.taking_back.is_some(),
        "known answer: with no edit of the person's in between it starts"
    );
}

#[test]
fn page_numbers_in_a_reply_are_refused_once_an_earlier_call_in_it_moved_the_pages() {
    let mut window = a_blank_window();
    let delete = tool("c2", "delete_pages", r#"{"document":"doc-1","pages":[1]}"#);
    window.ai.tools.take(std::slice::from_ref(&delete));
    let control = window.perform(&delete);
    assert!(
        matches!(control, super::Performed::Sent),
        "known answer: with the pages as they were it goes ahead"
    );
    land_what_was_sent(&mut window);

    let mut window = a_blank_window();
    window.ai.tools.take(&[
        tool(
            "c1",
            "add_blank_page",
            r#"{"document":"doc-1","after_page":1}"#,
        ),
        delete.clone(),
    ]);
    window.ai.tools.pages_renumbered();
    let super::Performed::Done(refused) = window.perform(&delete) else {
        panic!("the call is answered, not sent");
    };
    assert!(refused.is_error);
    assert!(refused.text.contains("put pages in"), "{}", refused.text);
    assert_eq!(window.editor.page_count(), 1);
    let reading = tool("c3", "read_text", r#"{"document":"doc-1"}"#);
    assert!(
        !matches!(window.perform(&reading), super::Performed::Done(ref said) if said.is_error),
        "a call that names no page is not stopped"
    );
}

#[test]
fn a_plan_is_kept_in_the_tool_state_and_answered_in_words() {
    let mut window = a_blank_window();
    let plan = tool(
        "p1",
        "update_plan",
        r#"{"steps":[{"text":"Read","status":"done"},{"text":"Write","status":"in_progress"},{"text":"Check","status":"pending"}]}"#,
    );
    let result = run(&mut window, &plan);
    assert!(!result.is_error, "{}", result.text);
    assert!(
        result.text.starts_with("Plan: 1 of 3 steps done."),
        "{}",
        result.text
    );
    assert_eq!(window.ai.tools.plan.len(), 3);
    assert_eq!(
        window.ai.tools.run.steps(),
        0,
        "a plan changes nothing in the document"
    );
    assert!(!window.editor.can_undo());
}

#[test]
fn a_call_that_covers_a_page_is_asked_about_even_when_the_tool_is_otherwise_allowed() {
    let mut window = a_blank_window();
    window.ai.mode = Mode::AskBeforeChanges;
    window
        .ai
        .tools
        .allowed_for_chat
        .insert("write_pages".to_owned());
    let writes = |replace: bool| {
        tool(
            "w",
            "write_pages",
            &format!(
                r#"{{"document":"doc-1","markdown":"Hello","font":"Noto Sans","replace":{replace}}}"#
            ),
        )
    };
    assert_eq!(
        window.decide_about(&writes(false)),
        Decision::Run,
        "known answer: an ordinary write the person allowed for the chat runs"
    );
    assert_eq!(
        window.decide_about(&writes(true)),
        Decision::Ask {
            may_allow_for_chat: false
        },
        "a write over the page is asked about, and cannot be allowed for the chat"
    );
    assert_eq!(
        window.decide_about(&tool(
            "d",
            "delete_pages",
            r#"{"document":"doc-1","pages":[1]}"#
        )),
        Decision::Ask {
            may_allow_for_chat: false
        }
    );
}

#[test]
fn a_reading_that_waits_on_a_busy_editor_is_not_told_there_is_no_document() {
    let mut window = a_blank_window();
    let job = window
        .editor
        .begin_add_page(0, false, [595.0, 842.0])
        .expect("an edit to hold the session");
    let info = tool("i1", "document_info", r#"{"document":"doc-1"}"#);
    assert!(matches!(window.perform(&info), super::Performed::Busy));
    window.editor.adopt(job.run());
    assert!(
        matches!(window.perform(&info), super::Performed::Done(ref said) if !said.is_error),
        "and when the session is home it answers"
    );
}

#[test]
fn a_question_comes_with_the_page_on_screen_and_whether_there_are_unsaved_changes() {
    let mut window = a_blank_window();
    let before = window.whereabouts();
    assert_eq!((before.page_on_screen, before.pages), (1, 1));
    assert!(!before.unsaved);
    assert_eq!(before.selected, None);
    assert!(matches!(
        window.editor.add_page(0, false, [595.0, 842.0]),
        Applied::Changed { .. }
    ));
    window.focus = 1;
    let after = window.whereabouts();
    assert_eq!((after.page_on_screen, after.pages), (2, 2));
    assert!(
        after.unsaved,
        "the document has changed since it was opened"
    );
}

#[test]
fn a_reading_longer_than_one_reply_says_where_to_go_on_and_does_not_call_the_rest_scans() {
    let page = |at: usize, characters: usize| {
        (
            at,
            vec![Block {
                page: at,
                index: 0,
                text: "word ".repeat(characters / 5),
                area: [0.0, 0.0, 10.0, 10.0],
                size: 10.0,
                fixed: None,
            }],
        )
    };
    let gathered = vec![page(0, 30_000)];
    let (text, more) = super::read_reply((0, 2), &gathered).expect("it reads");
    assert!(
        text.contains("=== Page 1 ==="),
        "{}",
        &text[..80.min(text.len())]
    );
    assert!(
        !text.contains("=== Page 2 ===") && !text.contains("may be a picture or a scan"),
        "pages that were never read are not shown as empty"
    );
    assert_eq!(
        more.as_deref(),
        Some("\n(The reply is full. Continue with first_page: 2.)")
    );

    let (_, none) = super::read_reply((0, 0), &gathered).expect("it reads");
    assert_eq!(
        none, None,
        "a reading that reached its last page says nothing more"
    );
}

#[test]
fn the_assistant_keeps_working_while_the_panel_is_closed_and_the_window_is_on_home() {
    let mut window = a_blank_window();
    window.home = true;
    window.ai.open = false;
    window.ai.mode = Mode::DoIt;
    window.ai.tools.take(&[tool(
        "c1",
        "add_blank_page",
        r#"{"document":"doc-1","after_page":1}"#,
    )]);
    let context = eframe::egui::Context::default();
    for _ in 0..2_000 {
        window.collect(&context);
        window.keep_the_assistant_going(&context);
        if window.editor.page_count() == 2 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(
        window.editor.page_count(),
        2,
        "the call ran with nobody looking at the panel"
    );
    assert!(!window.ai.open, "and it did not open the panel for that");
}

#[test]
fn a_card_that_needs_the_person_opens_a_closed_panel_once_and_is_never_lost() {
    let mut window = a_blank_window();
    window.ai.open = false;
    window.ai.mode = Mode::AskBeforeChanges;
    window.ai.tools.take(&[tool(
        "d1",
        "delete_pages",
        r#"{"document":"doc-1","pages":[1]}"#,
    )]);
    let context = eframe::egui::Context::default();
    window.keep_the_assistant_going(&context);
    assert!(window.ai.tools.ask.is_some(), "the card is waiting");
    assert!(window.ai.open, "and the panel opened to show it");

    window.ai.open = false;
    window.keep_the_assistant_going(&context);
    assert!(
        !window.ai.open,
        "a person who closed the panel is not trapped by it opening again"
    );
    assert!(window.ai.tools.ask.is_some(), "the card is still there");
    assert_eq!(
        window.editor.page_count(),
        1,
        "and nothing was done unasked"
    );

    let mut away = a_blank_window();
    away.home = true;
    away.ai.open = false;
    away.ai.tools.take(&[tool(
        "d1",
        "delete_pages",
        r#"{"document":"doc-1","pages":[1]}"#,
    )]);
    away.keep_the_assistant_going(&context);
    assert!(
        away.ai.tools.ask.is_some(),
        "the card waits for the person to come back"
    );
    assert!(
        !away.ai.open,
        "and the panel is not opened under the home screen"
    );
    away.home = false;
    away.keep_the_assistant_going(&context);
    assert!(
        away.ai.open,
        "back at the document it opens, because the card was never seen"
    );
}

#[test]
fn the_selected_block_is_named_and_the_page_text_comes_a_block_to_a_line() {
    let mut window = a_blank_window();
    let write = tool(
        "w1",
        "write_pages",
        r##"{"document":"doc-1","markdown":"# Quarterly report\n\n## Totals\n\nHello there","font":"DejaVu Sans","replace":true}"##,
    );
    assert!(!run(&mut window, &write).is_error);
    let source = window.editor.source().cloned().expect("a document");
    let view = pdf_session::interpret_page_fully(&source, 0, b"", None, window.editor.fonts())
        .expect("the page reads");
    window.editor.adopt_page(0, std::sync::Arc::new(view));
    assert_eq!(
        window.whereabouts().selected,
        None,
        "known answer: nothing is selected until the person selects"
    );

    window.pointing = crate::window_state::Pointing::Block { page: 0, block: 0 };
    let selected = window.whereabouts().selected.expect("a block is selected");
    assert_eq!(selected.0, "p1-b1");
    assert!(!selected.1.trim().is_empty(), "{selected:?}");
    assert!(
        window.ai.tools.named.contains_key(&(0, 0)),
        "so the model can edit it by that name without reading the page first"
    );

    let page = window.text_of_the_page_on_screen(1_000);
    let lines: Vec<&str> = page.lines().collect();
    assert!(lines.len() >= 2, "a block to a line: {page:?}");
    assert!(
        page.contains("Quarterly report") && page.contains("Hello there"),
        "{page:?}"
    );
    assert!(
        window.text_of_the_page_on_screen(5).chars().count() <= 5,
        "and it is cut to the room there is"
    );
}
