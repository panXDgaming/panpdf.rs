use super::{KEPT_WHOLE, plan_in, plan_said, shortened};
use crate::connect::{ToolCall, ToolResult, Turn};
use crate::json::Json;
use crate::tools::request::{PlanStep, StepState};

fn asked(id: &str, name: &str, arguments: &str) -> Turn {
    Turn::Model {
        text: String::new(),
        calls: vec![ToolCall::asked(
            id,
            name,
            Json::parse(arguments).expect("JSON"),
        )],
        raw: None,
    }
}

fn answered(id: &str, text: &str) -> Turn {
    Turn::Results {
        results: vec![ToolResult::said(id, text)],
    }
}

fn a_long_run(reads: usize) -> Vec<Turn> {
    let mut turns = vec![Turn::person("tidy up the whole contract")];
    for at in 0..reads {
        let id = format!("call_{at}");
        turns.push(asked(&id, "read_text", "{}"));
        turns.push(answered(
            &id,
            &format!("page {at}: {}", "word ".repeat(2_000)),
        ));
    }
    turns
}

fn text_of(turn: &Turn) -> &str {
    match turn {
        Turn::Results { results } => &results[0].text,
        other => other.text(),
    }
}

#[test]
fn the_latest_results_stay_whole_and_older_readings_are_shortened() {
    let turns = a_long_run(7);
    let kept = shortened(&turns);
    assert_eq!(kept.len(), turns.len());
    let results: Vec<&Turn> = kept
        .iter()
        .filter(|turn| matches!(turn, Turn::Results { .. }))
        .collect();
    assert_eq!(results.len(), 7);
    let old = 7 - KEPT_WHOLE;
    for (at, turn) in results.iter().enumerate() {
        let text = text_of(turn);
        if at < old {
            assert!(
                text.len() < 300,
                "result {at} was not shortened: {} bytes",
                text.len()
            );
            assert!(
                text.contains("read_text") && text.contains("again"),
                "{text}"
            );
        } else {
            assert!(
                text.len() > 9_000,
                "result {at} was shortened: {} bytes",
                text.len()
            );
        }
    }
    assert_eq!(kept[0], turns[0], "the question is untouched");
    let before: usize = turns.iter().map(Turn::size).sum();
    let after: usize = kept.iter().map(Turn::size).sum();
    assert!(after < before * 3 / 5, "{before} became {after}");
}

#[test]
fn only_what_can_be_read_again_is_shortened() {
    let mut turns = vec![Turn::person("go")];
    let long = "x".repeat(5_000);
    for (at, name) in [
        "replace_text",
        "render_page",
        "ask_person",
        "find_text",
        "document_info",
        "list_fonts",
    ]
    .iter()
    .enumerate()
    {
        let id = format!("call_{at}");
        turns.push(asked(&id, name, "{}"));
        turns.push(answered(&id, &long));
    }
    turns.push(asked("call_e", "read_text", "{}"));
    turns.push(Turn::Results {
        results: vec![ToolResult::failed("call_e", &long)],
    });
    for at in 0..KEPT_WHOLE {
        let id = format!("later_{at}");
        turns.push(asked(&id, "read_text", "{}"));
        turns.push(answered(&id, &long));
    }
    let kept = shortened(&turns);
    let shortened_ones: Vec<usize> = kept
        .iter()
        .enumerate()
        .filter(|(_, turn)| matches!(turn, Turn::Results { .. }) && text_of(turn).len() < 300)
        .map(|(at, _)| at)
        .collect();
    assert_eq!(
        shortened_ones,
        vec![8, 10, 12],
        "find_text, document_info and list_fonts only: an edit's answer, a picture's, a \
         person's answer and a failure stay as they were"
    );
    assert_eq!(
        shortened(&kept),
        kept,
        "shortening twice changes nothing more"
    );
}

#[test]
fn a_short_conversation_is_sent_as_it_is() {
    let turns = a_long_run(KEPT_WHOLE);
    assert_eq!(shortened(&turns), turns);
    assert_eq!(shortened(&[]), Vec::<Turn>::new());
}

fn step(text: &str, state: StepState) -> PlanStep {
    PlanStep {
        text: text.to_owned(),
        state,
    }
}

#[test]
fn the_plan_a_chat_ended_on_is_found_again_from_its_calls() {
    let first =
        r#"{"steps":[{"text":"Read","status":"in_progress"},{"text":"Write","status":"pending"}]}"#;
    let second =
        r#"{"steps":[{"text":"Read","status":"done"},{"text":"Write","status":"in_progress"}]}"#;
    let turns = vec![
        Turn::person("write a report"),
        asked("a", "update_plan", first),
        answered("a", "Plan"),
        asked("b", "read_text", "{}"),
        answered("b", "text"),
        asked("c", "update_plan", second),
        answered("c", "Plan"),
        Turn::model("Done."),
    ];
    assert_eq!(
        plan_in(&turns),
        vec![
            step("Read", StepState::Done),
            step("Write", StepState::InProgress)
        ]
    );
    assert_eq!(plan_in(&turns[..3]).len(), 2, "as it stood after the first");
    assert!(plan_in(&[Turn::person("hi")]).is_empty());
    assert!(
        plan_in(&[asked("a", "update_plan", r#"{"steps":[]}"#)]).is_empty(),
        "a plan that cannot be read is no plan"
    );
}

#[test]
fn a_plan_is_said_with_how_far_it_has_come() {
    let said = plan_said(&[
        step("Read the contract", StepState::Done),
        step("Mark the dates", StepState::InProgress),
        step("Summarise", StepState::Pending),
    ]);
    assert_eq!(
        said,
        "Plan: 1 of 3 steps done.\n[done] Read the contract\n[in progress] Mark the dates\n[pending] Summarise"
    );
}
