use std::sync::Arc;

use eframe::egui;
use pdf_agent::connect::{Model, ToolCall, ToolResult, Turn};
use pdf_agent::history::{Chat, write_turns};
use pdf_agent::json::Json;
use pdf_agent::tools::request::{PlanStep, Request, StepState};
use pdf_app::ai_permission::Mode;
use pdf_app::wording::{Lang, Message};

use super::header::Badge;
use super::steps::arguments_said;
use super::{AiState, Asking, Notice, NoticeAction, Provider, mode_means, mode_said};
use crate::ai_actions::Pending;

fn listed(ids: &[&str]) -> Vec<Model> {
    ids.iter()
        .map(|id| Model {
            id: (*id).to_owned(),
        })
        .collect()
}

fn chat(id: &str, changed: u64) -> Chat {
    Chat {
        id: id.to_owned(),
        title: format!("chat {id}"),
        changed,
        turns: vec![Turn::person("a question"), Turn::model("an answer")],
        ..Chat::default()
    }
}

#[test]
fn a_key_given_for_one_provider_is_not_carried_to_another() {
    let mut state = AiState {
        key: "sk-for-openai".to_owned(),
        model: "gpt-5".to_owned(),
        connected: true,
        ..AiState::default()
    };
    state.switch_provider(Provider::Claude);
    assert!(
        state.key.is_empty(),
        "the new provider is not sent the old key"
    );
    assert!(state.model.is_empty() && state.models.is_empty() && !state.connected);
    assert_eq!(state.base_url, Provider::Claude.base_url());
    assert!(state.key_missing(), "it asks for its own");
    state.key = "sk-for-claude".to_owned();
    state.switch_provider(Provider::OpenAi);
    assert_eq!(
        state.key, "sk-for-openai",
        "each provider gets its own key back"
    );
    state.switch_provider(Provider::Claude);
    assert_eq!(state.key, "sk-for-claude");
}

#[test]
fn a_provider_cannot_be_changed_in_the_middle_of_a_question() {
    let (_send, answers) = std::sync::mpsc::channel();
    let mut state = AiState {
        key: "sk-for-openai".to_owned(),
        asking: Some(Asking {
            stop: Arc::default(),
            answers,
        }),
        ..AiState::default()
    };
    state.switch_provider(Provider::Gemini);
    assert_eq!(state.provider, Provider::OpenAi);
    assert_eq!(state.key, "sk-for-openai");
}

#[test]
fn a_connection_that_works_picks_a_sensible_model_and_folds_the_card() {
    let mut state = AiState {
        provider: Provider::Claude,
        key: "sk".to_owned(),
        settings_open: true,
        edited: true,
        ..AiState::default()
    };
    state.take_the_models(listed(&[
        "claude-opus-4-1-20250805",
        "claude-sonnet-4-5-20250929",
        "claude-sonnet-4-5",
        "claude-haiku-4-5",
    ]));
    assert_eq!(state.model, "claude-sonnet-4-5");
    assert!(state.connected && !state.edited);
    assert!(!state.settings_open, "the card folds to its one line");
    assert!(state.ready());
}

#[test]
fn a_model_the_person_chose_is_kept_while_it_is_still_listed_and_dropped_when_it_is_not() {
    let mut state = AiState {
        provider: Provider::OpenAi,
        key: "sk".to_owned(),
        model: "gpt-4.1".to_owned(),
        ..AiState::default()
    };
    state.take_the_models(listed(&["gpt-4.1", "gpt-5"]));
    assert_eq!(state.model, "gpt-4.1");
    state.take_the_models(listed(&["gpt-5", "gpt-5-mini"]));
    assert_eq!(state.model, "gpt-5");
}

#[test]
fn a_provider_with_no_models_leaves_the_card_open_to_choose_one() {
    let mut state = AiState {
        provider: Provider::Ollama,
        settings_open: true,
        ..AiState::default()
    };
    state.take_the_models(Vec::new());
    assert!(state.model.is_empty());
    assert!(state.settings_open);
}

#[test]
fn changing_what_was_connected_asks_for_a_new_connection() {
    let mut state = AiState {
        provider: Provider::Ollama,
        model: "llama3".to_owned(),
        ..AiState::default()
    };
    assert!(
        state.ready() && !state.edited,
        "a kept choice is ready at once"
    );
    state.invalidate();
    assert!(state.edited && !state.connected);
}

#[test]
fn the_list_of_chats_is_shared_and_not_copied_each_time_it_is_asked_for() {
    let mut state = AiState {
        history: Some(Arc::new(vec![chat("000000000001", 10)])),
        ..AiState::default()
    };
    let first = state.the_chats();
    let second = state.the_chats();
    assert!(Arc::ptr_eq(&first, &second));
}

#[test]
fn a_chat_that_was_saved_goes_to_the_top_of_the_list_without_reading_the_folder_again() {
    let mut state = AiState {
        history: Some(Arc::new(vec![
            chat("000000000001", 10),
            chat("000000000002", 5),
        ])),
        ..AiState::default()
    };
    let before = state.the_chats();
    let fresh = chat("000000000003", 20);
    state.shelve(&write_turns(&fresh, &fresh.turns), &[]);
    let after = state.the_chats();
    let ids: Vec<&str> = after.iter().map(|chat| chat.id.as_str()).collect();
    assert_eq!(ids, ["000000000003", "000000000001", "000000000002"]);
    assert_eq!(
        before.len(),
        2,
        "a list someone is holding does not change under them"
    );

    let changed = chat("000000000001", 30);
    state.shelve(
        &write_turns(&changed, &changed.turns),
        &["000000000002".to_owned()],
    );
    let ids: Vec<String> = state
        .the_chats()
        .iter()
        .map(|chat| chat.id.clone())
        .collect();
    assert_eq!(
        ids,
        ["000000000001", "000000000003"],
        "a chat saved again is once in the list"
    );
}

#[test]
fn a_chat_that_is_forgotten_leaves_the_list_at_once() {
    let mut state = AiState {
        history: Some(Arc::new(vec![
            chat("000000000001", 10),
            chat("000000000002", 5),
        ])),
        ..AiState::default()
    };
    state.forget_a_chat("000000000002");
    let ids: Vec<String> = state
        .the_chats()
        .iter()
        .map(|chat| chat.id.clone())
        .collect();
    assert_eq!(ids, ["000000000001"]);
}

fn asked_to_allow() -> Pending {
    Pending {
        call: ToolCall::asked("c1", "delete_pages", Json::Null),
        request: Request::DeletePages(vec![1]),
        may_allow_for_chat: false,
        of_this_tool: 1,
        written_to: None,
    }
}

#[test]
fn the_badge_says_when_the_assistant_works_and_when_it_needs_the_person() {
    let mut state = AiState::default();
    assert_eq!(state.badge(), None);
    let (_send, answers) = std::sync::mpsc::channel();
    state.asking = Some(Asking {
        stop: Arc::default(),
        answers,
    });
    assert_eq!(state.badge(), Some(Badge::Working));
    state.tools.ask = Some(asked_to_allow());
    assert_eq!(
        state.badge(),
        Some(Badge::NeedsYou),
        "needing the person outranks working"
    );
}

#[test]
fn checking_a_connection_is_not_work_the_badge_shows() {
    let (_send, answers) = std::sync::mpsc::channel();
    let state = AiState {
        checking: true,
        asking: Some(Asking {
            stop: Arc::default(),
            answers,
        }),
        ..AiState::default()
    };
    assert_eq!(state.badge(), None);
}

fn a_small_text_file(name: &str, text: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("panpdf-ai-{}-{name}", std::process::id()));
    std::fs::write(&path, text).expect("a file in the temporary folder");
    path
}

#[test]
fn a_file_is_read_off_the_window_s_thread_and_sending_waits_for_it() {
    let path = a_small_text_file("notes.txt", "hello from a file");
    let mut state = AiState {
        composer: "what is in it?".to_owned(),
        ..AiState::default()
    };
    state.attach_file(&path);
    assert!(
        state.pending[0].reading(),
        "the window did not stop to read it"
    );
    assert!(
        state.may_send() && !state.can_send(),
        "the question waits for the file"
    );
    let context = egui::Context::default();
    for _ in 0..200 {
        state.poll(&context);
        if !state.reading() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let _ = std::fs::remove_file(&path);
    assert!(state.can_send());
    let ready = state.pending[0].ready().expect("it was read");
    assert_eq!(ready.len(), 1);
    assert_eq!(state.pending[0].kind, pdf_agent::attach::Kind::Text);
}

#[test]
fn a_file_that_is_missing_is_refused_at_once_with_the_reason() {
    let mut state = AiState::default();
    state.attach_file(std::path::Path::new("/no/such/file.pdf"));
    assert!(state.pending[0].refused());
    assert!(!state.pending[0].reading());
}

#[test]
fn a_file_larger_than_a_question_can_carry_is_refused_without_being_read() {
    let path = std::env::temp_dir().join(format!("panpdf-ai-{}-huge.bin", std::process::id()));
    let file = std::fs::File::create(&path).expect("a file in the temporary folder");
    file.set_len(pdf_agent::attach::MOST_TOTAL_BYTES as u64 + 1)
        .expect("a sparse file");
    drop(file);
    let mut state = AiState::default();
    state.attach_file(&path);
    let _ = std::fs::remove_file(&path);
    assert!(
        state.pending[0].refused(),
        "no thread was started to read it"
    );
}

#[test]
fn a_seventh_file_is_refused() {
    let path = a_small_text_file("many.txt", "x");
    let mut state = AiState::default();
    for _ in 0..=pdf_agent::attach::MOST_FILES {
        state.attach_file(&path);
    }
    let _ = std::fs::remove_file(&path);
    assert!(state.pending.last().expect("a seventh").refused());
    assert_eq!(
        state.pending.iter().filter(|item| item.refused()).count(),
        1
    );
}

#[test]
fn the_notice_that_opens_the_settings_does_so_and_goes() {
    let mut state = AiState {
        notice: Some(Notice::with(
            Message::AiKeyNeeded,
            NoticeAction::OpenSettings,
        )),
        ..AiState::default()
    };
    let brief = pdf_agent::tools::DocumentBrief::default();
    state.act_on_the_notice(
        &egui::Context::default(),
        &brief,
        NoticeAction::OpenSettings,
    );
    assert!(state.settings_open);
    assert!(state.notice.is_none());
}

#[test]
fn the_modes_are_named_plainly_and_each_says_what_it_does() {
    let say = |mode| mode_said(mode).say(Lang::English);
    assert_eq!(say(Mode::ChatOnly), "Chat only");
    assert_eq!(say(Mode::AskBeforeChanges), "Ask before changes");
    assert_eq!(say(Mode::DoIt), "Make changes");
    assert_eq!(say(Mode::Free), "Full access");
    let every = [
        Mode::ChatOnly,
        Mode::AskBeforeChanges,
        Mode::DoIt,
        Mode::Free,
    ];
    for (at, mode) in every.iter().enumerate() {
        assert!(!mode_means(*mode).say(Lang::English).trim().is_empty());
        for other in &every[at + 1..] {
            assert_ne!(say(*mode), say(*other));
            assert_ne!(
                mode_means(*mode).say(Lang::English),
                mode_means(*other).say(Lang::English)
            );
        }
    }
}

#[test]
fn what_a_step_asked_for_reads_as_lines() {
    let said = arguments_said(&Json::object([
        ("block", Json::text("p1-b3")),
        ("document", Json::text("doc-1")),
        ("first_page", Json::Number(2.0)),
        ("match_case", Json::Bool(false)),
        ("find", Json::Null),
    ]));
    assert_eq!(
        said,
        "block: p1-b3\ndocument: doc-1\nfind: none\nfirst_page: 2\nmatch_case: false"
    );
    assert_eq!(arguments_said(&Json::Null), "");
    let long = arguments_said(&Json::object([("text", Json::text("x".repeat(900)))]));
    assert!(long.chars().count() < 450, "{}", long.chars().count());
    assert!(long.ends_with('\u{2026}'));
}

fn drawn_in_a_panel(state: &mut AiState, tall: f32) {
    let context = egui::Context::default();
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(360.0, tall),
        )),
        ..egui::RawInput::default()
    };
    for _ in 0..3 {
        let _ = context.run_ui(raw.clone(), |ui| {
            let lang = Lang::English;
            let _ = state.the_heading(ui, lang);
            if state.settings_open {
                state.the_connection(ui, &context, lang);
            } else {
                state.the_summary(ui, lang);
            }
            let _ = state.the_conversation(ui, lang);
            let _ = state.the_composer(ui, &context, lang, Some(3), (2, 15.0));
        });
    }
}

#[test]
fn every_state_of_the_panel_can_be_drawn_at_the_narrowest_and_a_short_window() {
    let mut first_run = AiState::default();
    drawn_in_a_panel(&mut first_run, 700.0);
    let mut local = AiState {
        provider: Provider::Ollama,
        models: listed(&["llama3.2:3b", "qwen2.5:7b"]),
        model: "llama3.2:3b".to_owned(),
        settings_open: true,
        ..AiState::default()
    };
    drawn_in_a_panel(&mut local, 300.0);
    local.settings_open = false;
    drawn_in_a_panel(&mut local, 300.0);
    local.mode = Mode::Free;
    drawn_in_a_panel(&mut local, 300.0);
}

#[test]
fn a_run_with_steps_a_plan_a_card_and_a_notice_can_be_drawn() {
    let replace = ToolCall::asked(
        "c1",
        "replace_text",
        Json::object([
            ("block", Json::text("p1-b2")),
            ("text", Json::text("new words")),
        ]),
    );
    let mut state = AiState {
        provider: Provider::Ollama,
        model: "llama3.2:3b".to_owned(),
        turns: vec![
            Turn::person("fix it"),
            Turn::Model {
                text: String::new(),
                calls: vec![replace],
                raw: None,
            },
            Turn::Results {
                results: vec![
                    ToolResult::said("c1", "Done."),
                    ToolResult::failed("c2", "no such block"),
                ],
            },
            Turn::model("All **done**, see [the docs](https://example.org)."),
        ],
        ..AiState::default()
    };
    state.tools.plan = vec![
        PlanStep {
            text: "read".to_owned(),
            state: StepState::Done,
        },
        PlanStep {
            text: "change".to_owned(),
            state: StepState::InProgress,
        },
    ];
    state.tools.ask = Some(Pending {
        call: ToolCall::asked(
            "c3",
            "replace_text",
            Json::object([("block", Json::text("p1-b2")), ("text", Json::text("x"))]),
        ),
        request: Request::ReplaceText {
            block: "p1-b2".to_owned(),
            find: None,
            text: "x".to_owned(),
        },
        may_allow_for_chat: true,
        of_this_tool: 3,
        written_to: None,
    });
    state.notice = Some(Notice::with(
        Message::AiAnswerCutShort,
        NoticeAction::Continue,
    ));
    drawn_in_a_panel(&mut state, 800.0);
}

#[test]
fn a_chat_is_copied_with_who_said_what_and_without_the_tool_turns() {
    let turns = vec![
        Turn::person("hello"),
        Turn::Model {
            text: String::new(),
            calls: vec![ToolCall::asked("c1", "read_text", Json::Null)],
            raw: None,
        },
        Turn::Results {
            results: vec![ToolResult::said("c1", "text")],
        },
        Turn::model("  hi there \n"),
    ];
    let said = super::going_back::whole_chat(&turns, "llama3", Lang::English);
    assert_eq!(said, "You:\nhello\n\nllama3:\nhi there");
    let nameless = super::going_back::whole_chat(&turns, "", Lang::English);
    assert!(nameless.contains("Response:\nhi there"), "{nameless}");
}

#[test]
fn a_card_for_a_call_that_writes_a_file_is_drawn_with_the_place_it_would_write_to() {
    for (name, arguments, place) in [
        (
            "convert",
            r#"{"tool":"pdf-to-word"}"#,
            pdf_app::ai_permission::WrittenTo::Folder("/home/someone/Documents".to_owned()),
        ),
        (
            "save_copy",
            r#"{"path":"/home/someone/Documents/copy.pdf"}"#,
            pdf_app::ai_permission::WrittenTo::File("/home/someone/Documents/copy.pdf".to_owned()),
        ),
    ] {
        let call = ToolCall::asked(
            "c9",
            name,
            Json::parse(arguments).expect("the arguments in this test are JSON"),
        );
        let request = pdf_agent::tools::request::parse(name, &call.arguments).expect("reads");
        let mut state = AiState {
            provider: Provider::Ollama,
            model: "llama3.2:3b".to_owned(),
            turns: vec![Turn::person("make it a copy")],
            ..AiState::default()
        };
        state.tools.ask = Some(Pending {
            call,
            request,
            may_allow_for_chat: false,
            of_this_tool: 1,
            written_to: Some(place),
        });
        drawn_in_a_panel(&mut state, 700.0);
    }
}
