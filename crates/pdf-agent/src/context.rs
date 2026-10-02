use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::connect::Turn;
use crate::tools::request::{PlanStep, Request, StepState};

pub const KEPT_WHOLE: usize = 4;

const WORTH_SHORTENING: usize = 400;

const SHORTENED: [&str; 4] = ["read_text", "find_text", "document_info", "list_fonts"];

fn names_of(turns: &[Turn]) -> BTreeMap<&str, &str> {
    turns
        .iter()
        .flat_map(Turn::calls)
        .map(|call| (call.id.as_str(), call.name.as_str()))
        .collect()
}

#[must_use]
pub fn shortened(turns: &[Turn]) -> Vec<Turn> {
    let names = names_of(turns);
    let mut candidates: Vec<(usize, usize)> = Vec::new();
    for (at, turn) in turns.iter().enumerate() {
        let Turn::Results { results } = turn else {
            continue;
        };
        for (which, result) in results.iter().enumerate() {
            let reads = names
                .get(result.call_id.as_str())
                .is_some_and(|name| SHORTENED.contains(name));
            if reads && !result.is_error && result.text.len() > WORTH_SHORTENING {
                candidates.push((at, which));
            }
        }
    }
    let old = candidates.len().saturating_sub(KEPT_WHOLE);
    let mut out = turns.to_vec();
    for (at, which) in &candidates[..old] {
        let Turn::Results { results } = &mut out[*at] else {
            continue;
        };
        let result = &mut results[*which];
        let name = names
            .get(result.call_id.as_str())
            .copied()
            .unwrap_or_default();
        result.text = format!(
            "({name} answered here with {} characters; they were shortened to save room. \
             Run {name} again if they are needed.)",
            result.text.chars().count()
        );
    }
    out
}

#[must_use]
pub fn plan_in(turns: &[Turn]) -> Vec<PlanStep> {
    for turn in turns.iter().rev() {
        for call in turn.calls().iter().rev() {
            if call.name != "update_plan" || call.problem.is_some() {
                continue;
            }
            if let Ok(Request::UpdatePlan { steps }) =
                crate::tools::request::parse(&call.name, &call.arguments)
            {
                return steps;
            }
        }
    }
    Vec::new()
}

#[must_use]
pub fn plan_said(steps: &[PlanStep]) -> String {
    let done = steps
        .iter()
        .filter(|step| step.state == StepState::Done)
        .count();
    let mut said = format!("Plan: {done} of {} steps done.", steps.len());
    for step in steps {
        let state = match step.state {
            StepState::Pending => "pending",
            StepState::InProgress => "in progress",
            StepState::Done => "done",
        };
        let _ = write!(said, "\n[{state}] {}", step.text);
    }
    said
}

#[cfg(test)]
mod tests;
