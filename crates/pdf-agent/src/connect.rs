use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::json::Json;

const MAX_RESPONSE: usize = 8 * 1024 * 1024;

const MOST_SPOKEN_BYTES: usize = 2 * 1024 * 1024;

const MOST_ATTACHED_BYTES: usize = crate::attach::MOST_TOTAL_BYTES.div_ceil(3) * 4;

const INLINE_BODY_BYTES: usize = 1024 * 1024;

const LIST_SECONDS: u64 = 30;

const MOST_LIST_PAGES: usize = 10;

const WHOLE_SECONDS: u64 = 600;

const STREAM_CAP: Duration = Duration::from_secs(3600);

const IDLE_REMOTE: Duration = Duration::from_mins(10);

const IDLE_LOCAL: Duration = Duration::from_mins(15);

const AFTER_THE_END: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Provider {
    OpenAi,
    Anthropic,
    Gemini,
    Ollama,
    LmStudio,
    Custom,
}

#[derive(Clone, Eq, PartialEq)]
pub struct Connection {
    pub provider: Provider,
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    pub effort: Effort,
}

impl std::fmt::Debug for Connection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Connection")
            .field("provider", &self.provider)
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("effort", &self.effort)
            .field("api_key", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Model {
    pub id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Picture {
    pub media_type: String,
    pub base64: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AttachmentKind {
    Text,
    Image { media_type: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Attachment {
    pub name: String,
    pub kind: AttachmentKind,
    pub bytes: Vec<u8>,
}

impl Attachment {
    #[must_use]
    pub fn text(name: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind: AttachmentKind::Text,
            bytes: text.into().into_bytes(),
        }
    }

    #[must_use]
    pub fn image(
        name: impl Into<String>,
        media_type: impl Into<String>,
        bytes: impl Into<Vec<u8>>,
    ) -> Self {
        Self {
            name: name.into(),
            kind: AttachmentKind::Image {
                media_type: media_type.into(),
            },
            bytes: bytes.into(),
        }
    }

    #[must_use]
    pub fn as_text(&self) -> std::borrow::Cow<'_, str> {
        match self.kind {
            AttachmentKind::Text => String::from_utf8_lossy(&self.bytes),
            AttachmentKind::Image { .. } => std::borrow::Cow::Borrowed(""),
        }
    }

    fn size(&self) -> usize {
        match self.kind {
            AttachmentKind::Text => self.bytes.len(),
            AttachmentKind::Image { .. } => self.bytes.len().div_ceil(3) * 4,
        }
    }
}

fn attached_words(name: &str, text: &str) -> String {
    format!("Attached file \"{name}\":\n{text}")
}

fn data_url(media_type: &str, bytes: &[u8]) -> String {
    format!("data:{media_type};base64,{}", crate::json::base64(bytes))
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolOffer {
    pub name: String,
    pub description: String,
    pub schema: Json,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Json,
    pub problem: Option<String>,
}

impl ToolCall {
    #[must_use]
    pub fn asked(id: impl Into<String>, name: impl Into<String>, arguments: Json) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            arguments,
            problem: None,
        }
    }

    #[must_use]
    pub fn unreadable(
        id: impl Into<String>,
        name: impl Into<String>,
        problem: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            arguments: Json::Null,
            problem: Some(problem.into()),
        }
    }

    fn arguments_object(&self) -> Json {
        match &self.arguments {
            object @ Json::Object(_) => object.clone(),
            _ => Json::object([]),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolResult {
    pub call_id: String,
    pub text: String,
    pub is_error: bool,
    pub picture: Option<Picture>,
}

impl ToolResult {
    #[must_use]
    pub fn said(call_id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            call_id: call_id.into(),
            text: text.into(),
            is_error: false,
            picture: None,
        }
    }

    #[must_use]
    pub fn failed(call_id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            call_id: call_id.into(),
            text: text.into(),
            is_error: true,
            picture: None,
        }
    }

    fn worded(&self) -> String {
        if self.is_error {
            format!("Error: {}", self.text)
        } else {
            self.text.clone()
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Raw {
    pub provider: Provider,
    pub items: Vec<Json>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

impl Usage {
    #[must_use]
    pub const fn prompt_size(self) -> u64 {
        self.input + self.cache_read + self.cache_write
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reply {
    pub text: String,
    pub calls: Vec<ToolCall>,
    pub raw: Option<Raw>,
    pub cut_short: bool,
    pub usage: Option<Usage>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Said {
    Person,
    Model,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Turn {
    Person {
        text: String,
        attachments: Vec<Attachment>,
    },
    Model {
        text: String,
        calls: Vec<ToolCall>,
        raw: Option<Raw>,
    },
    Results {
        results: Vec<ToolResult>,
    },
}

impl Turn {
    #[must_use]
    pub fn person(text: impl Into<String>) -> Self {
        Self::Person {
            text: text.into(),
            attachments: Vec::new(),
        }
    }

    #[must_use]
    pub fn person_with(text: impl Into<String>, attachments: Vec<Attachment>) -> Self {
        Self::Person {
            text: text.into(),
            attachments,
        }
    }

    #[must_use]
    pub fn model(text: impl Into<String>) -> Self {
        Self::Model {
            text: text.into(),
            calls: Vec::new(),
            raw: None,
        }
    }

    #[must_use]
    pub fn answered(reply: &Reply) -> Self {
        Self::Model {
            text: reply.text.clone(),
            calls: reply.calls.clone(),
            raw: reply.raw.clone(),
        }
    }

    #[must_use]
    pub const fn said(&self) -> Said {
        match self {
            Self::Person { .. } | Self::Results { .. } => Said::Person,
            Self::Model { .. } => Said::Model,
        }
    }

    #[must_use]
    pub fn text(&self) -> &str {
        match self {
            Self::Person { text, .. } | Self::Model { text, .. } => text,
            Self::Results { .. } => "",
        }
    }

    #[must_use]
    pub fn attachments(&self) -> &[Attachment] {
        match self {
            Self::Person { attachments, .. } => attachments,
            _ => &[],
        }
    }

    #[must_use]
    pub fn calls(&self) -> &[ToolCall] {
        match self {
            Self::Model { calls, .. } => calls,
            _ => &[],
        }
    }

    const fn role(&self) -> &'static str {
        match self.said() {
            Said::Person => "user",
            Said::Model => "assistant",
        }
    }

    #[must_use]
    pub fn size(&self) -> usize {
        self.words_size() + self.attached_size()
    }

    fn attached_size(&self) -> usize {
        self.attachments().iter().map(Attachment::size).sum()
    }

    fn words_size(&self) -> usize {
        match self {
            Self::Person { text, .. } => text.len(),
            Self::Model { text, calls, .. } => {
                text.len() + calls.iter().map(|call| call.name.len() + 32).sum::<usize>()
            }
            Self::Results { results } => results
                .iter()
                .map(|result| {
                    result.text.len()
                        + result
                            .picture
                            .as_ref()
                            .map_or(0, |picture| picture.base64.len())
                })
                .sum(),
        }
    }
}

#[must_use]
pub fn without_the_old_pictures(turns: &[Turn]) -> Vec<Turn> {
    let newest = turns
        .iter()
        .enumerate()
        .filter_map(|(at, turn)| match turn {
            Turn::Results { results } => results
                .iter()
                .rposition(|result| result.picture.is_some())
                .map(|which| (at, which)),
            _ => None,
        })
        .next_back();
    let mut out = turns.to_vec();
    for (at, turn) in out.iter_mut().enumerate() {
        let Turn::Results { results } = turn else {
            continue;
        };
        for (which, result) in results.iter_mut().enumerate() {
            if newest != Some((at, which)) {
                result.picture = None;
            }
        }
    }
    out
}

fn has_result_pictures(turns: &[Turn]) -> bool {
    turns.iter().any(|turn| match turn {
        Turn::Results { results } => results.iter().any(|result| result.picture.is_some()),
        _ => false,
    })
}

fn is_about_pictures(said: &str) -> bool {
    said.to_ascii_lowercase().contains("image")
}

fn without_the_result_pictures(turns: &[Turn]) -> Vec<Turn> {
    turns
        .iter()
        .map(|turn| match turn {
            Turn::Results { results } => Turn::Results {
                results: results
                    .iter()
                    .map(|result| {
                        let mut result = result.clone();
                        if result.picture.take().is_some() {
                            result.text.push_str(PICTURE_LOST);
                        }
                        result
                    })
                    .collect(),
            },
            other => other.clone(),
        })
        .collect()
}

const PICTURE_LOST: &str = "\n(The picture could not be shown to this model.)";

const UNANSWERED: &str = "This action was not carried out, and the conversation went on without it. Do not retry it; \
ask the person what they would like instead.";

pub fn settle_dangling_calls(turns: &mut Vec<Turn>) {
    let mut at = 0;
    while at < turns.len() {
        let missing: Vec<ToolResult> = match &turns[at] {
            Turn::Model { calls, .. } => {
                let answered: Vec<&str> = match turns.get(at + 1) {
                    Some(Turn::Results { results }) => {
                        results.iter().map(|it| it.call_id.as_str()).collect()
                    }
                    _ => Vec::new(),
                };
                calls
                    .iter()
                    .filter(|call| !answered.contains(&call.id.as_str()))
                    .map(|call| ToolResult::failed(call.id.clone(), UNANSWERED))
                    .collect()
            }
            _ => Vec::new(),
        };
        if !missing.is_empty() {
            match turns.get_mut(at + 1) {
                Some(Turn::Results { results }) => results.extend(missing),
                _ => turns.insert(at + 1, Turn::Results { results: missing }),
            }
        }
        at += 1;
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Effort {
    #[default]
    Off,
    None,
    Low,
    Medium,
    High,
}

impl Effort {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::None => "none",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "off" => Some(Self::Off),
            "none" => Some(Self::None),
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            _ => None,
        }
    }

    const fn budget_tokens(self) -> u32 {
        match self {
            Self::Off | Self::None => 0,
            Self::Low => 1024,
            Self::Medium => 4096,
            Self::High => 16000,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThinkingStyle {
    Budget,
    Adaptive,
}

fn claude_version(model: &str) -> Option<(u32, u32)> {
    let at = model.rfind("claude-")?;
    let mut parts = model[at + "claude-".len()..].split('-');
    let number = |part: Option<&str>| {
        part.filter(|digits| digits.len() <= 2 && digits.bytes().all(|byte| byte.is_ascii_digit()))
            .and_then(|digits| digits.parse::<u32>().ok())
    };
    let first = parts.next()?;
    if let Ok(major) = first.parse::<u32>() {
        return Some((major, number(parts.next()).unwrap_or(0)));
    }
    let major = number(parts.next())?;
    Some((major, number(parts.next()).unwrap_or(0)))
}

#[must_use]
pub fn anthropic_thinking_style(model: &str) -> ThinkingStyle {
    match claude_version(model) {
        Some((major, minor)) if major < 4 || (major == 4 && minor < 6) => ThinkingStyle::Budget,
        _ => ThinkingStyle::Adaptive,
    }
}

fn most_output_tokens(model: &str) -> u32 {
    match claude_version(model) {
        Some((3, 7)) => 64_000,
        Some((3, 5)) => 8_192,
        Some((3, _)) => 4_096,
        Some((4, 0 | 1)) if model.contains("opus") => 32_000,
        _ => 64_000,
    }
}

const OUTPUT_TOKENS: u32 = 16_000;

const ADAPTIVE_OUTPUT_TOKENS: u32 = 32_000;

#[must_use]
pub fn reasoning_members(
    provider: Provider,
    model: &str,
    effort: Effort,
) -> (Vec<(&'static str, Json)>, u32) {
    let room = match provider.wire() {
        Wire::Messages => most_output_tokens(model),
        Wire::Responses | Wire::Chat => u32::MAX,
    };
    let plain = OUTPUT_TOKENS.min(room);
    if effort == Effort::Off {
        return (Vec::new(), plain);
    }
    if effort == Effort::None {
        return match provider.wire() {
            Wire::Chat => (vec![("reasoning_effort", Json::text("none"))], plain),
            Wire::Messages => (
                vec![("thinking", Json::object([("type", Json::text("disabled"))]))],
                plain,
            ),
            Wire::Responses => (
                vec![(
                    "reasoning",
                    Json::object([("effort", Json::text("minimal"))]),
                )],
                plain,
            ),
        };
    }
    let word = Json::text(effort.as_str());
    match provider.wire() {
        Wire::Responses => (vec![("reasoning", Json::object([("effort", word)]))], plain),
        Wire::Messages => match anthropic_thinking_style(model) {
            ThinkingStyle::Budget => {
                let budget = effort.budget_tokens();
                (
                    vec![(
                        "thinking",
                        Json::object([
                            ("type", Json::text("enabled")),
                            ("budget_tokens", Json::Number(f64::from(budget))),
                        ]),
                    )],
                    (budget + OUTPUT_TOKENS).min(room),
                )
            }
            ThinkingStyle::Adaptive => (
                vec![
                    ("thinking", Json::object([("type", Json::text("adaptive"))])),
                    ("output_config", Json::object([("effort", word)])),
                ],
                ADAPTIVE_OUTPUT_TOKENS.min(room),
            ),
        },
        Wire::Chat => (vec![("reasoning_effort", word)], plain),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConnectError {
    Invalid(String),
    Cancelled,
    Curl(String),
    ResponseTooLarge,
    TooLarge(String),
    Http(String),
    Protocol(String),
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(s) => write!(f, "invalid AI connection: {s}"),
            Self::Cancelled => f.write_str("AI request cancelled"),
            Self::Curl(s) => write!(f, "curl could not make the AI request: {s}"),
            Self::ResponseTooLarge => f.write_str("the AI response is too large"),
            Self::TooLarge(s) => write!(f, "the conversation is too large to send: {s}"),
            Self::Http(s) => write!(f, "the AI service refused the request: {s}"),
            Self::Protocol(s) => write!(f, "the AI service returned an unexpected answer: {s}"),
        }
    }
}

impl std::error::Error for ConnectError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Wire {
    Responses,
    Messages,
    Chat,
}

impl Provider {
    #[must_use]
    pub const fn wire(self) -> Wire {
        match self {
            Self::OpenAi => Wire::Responses,
            Self::Anthropic => Wire::Messages,
            _ => Wire::Chat,
        }
    }

    #[must_use]
    pub const fn default_base_url(self) -> &'static str {
        match self {
            Self::OpenAi => "https://api.openai.com/v1",
            Self::Anthropic => "https://api.anthropic.com/v1",
            Self::Gemini => "https://generativelanguage.googleapis.com/v1beta/openai",
            Self::Ollama => "http://127.0.0.1:11434/v1",
            Self::LmStudio => "http://127.0.0.1:1234/v1",
            Self::Custom => "",
        }
    }
}

struct Ask<'a> {
    turns: &'a [Turn],
    context: Option<&'a str>,
    tools: &'a [ToolOffer],
    system: Option<&'a str>,
}

#[derive(Clone, Copy)]
struct Pace<'a> {
    idle: Duration,
    cap: Duration,
    waits: &'a [Duration],
    streams: bool,
    settle: Duration,
}

const BACKOFF: [Duration; 5] = [
    Duration::from_secs(2),
    Duration::from_secs(5),
    Duration::from_secs(10),
    Duration::from_secs(20),
    Duration::from_secs(40),
];

const LISTING_BACKOFF: [Duration; 2] = [Duration::from_secs(1), Duration::from_secs(3)];

impl Pace<'static> {
    const fn streaming(local: bool) -> Self {
        Self {
            idle: if local { IDLE_LOCAL } else { IDLE_REMOTE },
            cap: STREAM_CAP,
            waits: &BACKOFF,
            streams: true,
            settle: AFTER_THE_END,
        }
    }

    const fn whole() -> Self {
        Self {
            idle: Duration::from_secs(WHOLE_SECONDS),
            cap: Duration::from_secs(WHOLE_SECONDS),
            waits: &BACKOFF,
            streams: false,
            settle: AFTER_THE_END,
        }
    }

    const fn listing() -> Self {
        Self {
            idle: Duration::from_secs(LIST_SECONDS),
            cap: Duration::from_secs(LIST_SECONDS),
            waits: &LISTING_BACKOFF,
            streams: false,
            settle: AFTER_THE_END,
        }
    }
}

impl Connection {
    #[must_use]
    pub fn preset(provider: Provider, model: impl Into<String>) -> Self {
        Self {
            provider,
            base_url: provider.default_base_url().to_owned(),
            model: model.into(),
            api_key: String::new(),
            effort: Effort::Off,
        }
    }

    pub fn models(&self, cancel: &AtomicBool) -> Result<Vec<Model>, ConnectError> {
        let mut found = Vec::new();
        let mut after: Option<String> = None;
        for _ in 0..MOST_LIST_PAGES {
            let suffix = self.models_path(after.as_deref());
            let body = self.get(&suffix, Pace::listing(), cancel)?;
            let root = Json::parse(&body).map_err(|e| ConnectError::Protocol(e.to_string()))?;
            let items = root.get("data").and_then(Json::as_list).ok_or_else(|| {
                ConnectError::Protocol("models answer has no data list".to_owned())
            })?;
            found.extend(
                items
                    .iter()
                    .filter_map(|item| item.get("id").and_then(Json::as_str))
                    .map(|id| Model { id: id.to_owned() }),
            );
            let more = root.get("has_more").and_then(Json::as_bool) == Some(true);
            let next = root
                .get("last_id")
                .and_then(Json::as_str)
                .filter(|id| is_a_page_marker(id))
                .map(str::to_owned);
            if self.provider != Provider::Anthropic || !more || next.is_none() || next == after {
                break;
            }
            after = next;
        }
        Ok(found)
    }

    fn models_path(&self, after: Option<&str>) -> String {
        if self.provider != Provider::Anthropic {
            return "/models".to_owned();
        }
        match after {
            Some(id) => format!("/models?limit=1000&after_id={id}"),
            None => "/models?limit=1000".to_owned(),
        }
    }

    pub fn test_connection(&self, cancel: &AtomicBool) -> Result<(), ConnectError> {
        self.models(cancel).map(|_| ())
    }

    pub fn chat(
        &self,
        prompt: &str,
        context: Option<&str>,
        cancel: &AtomicBool,
    ) -> Result<Reply, ConnectError> {
        self.converse(&[Turn::person(prompt)], context, cancel)
    }

    pub fn converse(
        &self,
        turns: &[Turn],
        context: Option<&str>,
        cancel: &AtomicBool,
    ) -> Result<Reply, ConnectError> {
        self.converse_with(turns, context, &[], None, cancel)
    }

    pub fn converse_with(
        &self,
        turns: &[Turn],
        context: Option<&str>,
        tools: &[ToolOffer],
        system: Option<&str>,
        cancel: &AtomicBool,
    ) -> Result<Reply, ConnectError> {
        let ask = Ask {
            turns,
            context,
            tools,
            system,
        };
        self.converse_within(Pace::whole(), &ask, cancel, None)
    }

    pub fn converse_streaming(
        &self,
        turns: &[Turn],
        context: Option<&str>,
        tools: &[ToolOffer],
        system: Option<&str>,
        cancel: &AtomicBool,
        on_partial: &mut dyn FnMut(Progress<'_>),
    ) -> Result<Reply, ConnectError> {
        let ask = Ask {
            turns,
            context,
            tools,
            system,
        };
        let local = is_loopback(&self.base_url);
        self.converse_within(Pace::streaming(local), &ask, cancel, Some(on_partial))
    }

    fn converse_within(
        &self,
        pace: Pace<'_>,
        ask: &Ask<'_>,
        cancel: &AtomicBool,
        mut on_partial: Option<&mut dyn FnMut(Progress<'_>)>,
    ) -> Result<Reply, ConnectError> {
        if cancel.load(Ordering::Relaxed) {
            return Err(ConnectError::Cancelled);
        }
        let spoken = self.spoken(ask.turns, ask.context)?;
        let streams = on_partial.is_some();
        let mut job = self.job_for(&spoken, ask, pace, streams)?;
        let mut may_lose_pictures =
            self.provider.wire() != Wire::Messages && has_result_pictures(&spoken);
        again_after(cancel, pace.waits, || {
            let told = self.attempt(&job, cancel, &mut on_partial);
            match told {
                Err(Failure {
                    error: ConnectError::Http(ref said),
                    ..
                }) if may_lose_pictures && is_about_pictures(said) => {
                    may_lose_pictures = false;
                    job =
                        self.job_for(&without_the_result_pictures(&spoken), ask, pace, streams)?;
                    self.attempt(&job, cancel, &mut on_partial)
                }
                other => other,
            }
        })
        .map_err(|error| redact_error(error, &self.api_key))
    }

    fn job_for(
        &self,
        spoken: &[Turn],
        ask: &Ask<'_>,
        pace: Pace<'_>,
        streams: bool,
    ) -> Result<Job, ConnectError> {
        let mut payload = self.payload(spoken, ask.tools, ask.system);
        if streams {
            self.ask_for_a_stream(&mut payload);
        }
        let endpoint = match self.provider.wire() {
            Wire::Responses => "/responses",
            Wire::Messages => "/messages",
            Wire::Chat => "/chat/completions",
        };
        self.job("POST", endpoint, Some(&payload.write()), pace)
    }

    fn ask_for_a_stream(&self, payload: &mut Json) {
        put(payload, "stream", Json::Bool(true));
        if matches!(self.provider, Provider::Ollama | Provider::LmStudio) {
            put(
                payload,
                "stream_options",
                Json::object([("include_usage", Json::Bool(true))]),
            );
        }
    }

    fn attempt(
        &self,
        job: &Job,
        cancel: &AtomicBool,
        on_partial: &mut Option<&mut dyn FnMut(Progress<'_>)>,
    ) -> Result<Reply, Failure> {
        let mut gathering = Gathering::new(self.provider.wire());
        let streaming = on_partial.is_some();
        let body = run_curl(job, cancel, &mut |line| {
            if streaming
                && gathering.line(line)
                && let Some(tell) = on_partial.as_mut()
            {
                tell(Progress {
                    said: gathering.said(),
                    thinking: gathering.thinking(),
                });
            }
            gathering.finished()
        })?;
        let root = match gathering.outcome()? {
            Some(root) => root,
            None => Json::parse(&body).map_err(|e| ConnectError::Protocol(e.to_string()))?,
        };
        answer(self.provider, &root).map_err(Failure::of_the_answer)
    }

    fn spoken(&self, turns: &[Turn], context: Option<&str>) -> Result<Vec<Turn>, ConnectError> {
        if self.model.is_empty() {
            return Err(ConnectError::Invalid("the model is empty".to_owned()));
        }
        let last = turns
            .last()
            .ok_or_else(|| ConnectError::Invalid("there is nothing to ask".to_owned()))?;
        let asked = match last {
            Turn::Person { text, attachments } => {
                !text.trim().is_empty() || !attachments.is_empty()
            }
            Turn::Results { results } => !results.is_empty(),
            Turn::Model { .. } => false,
        };
        if !asked {
            return Err(ConnectError::Invalid("the prompt is empty".to_owned()));
        }
        let mut spoken = turns.to_vec();
        for turn in &mut spoken {
            if let Turn::Person { attachments, .. } = turn {
                for attachment in attachments
                    .iter_mut()
                    .filter(|attachment| attachment.bytes.is_empty())
                {
                    *attachment = Attachment::text(std::mem::take(&mut attachment.name), NOT_KEPT);
                }
            }
        }
        if let Some(context) = context.filter(|text| !text.is_empty())
            && let Some(Turn::Person { text, .. }) = spoken
                .iter_mut()
                .rev()
                .find(|turn| matches!(turn, Turn::Person { .. }))
        {
            *text = format!("Document context:\n{context}\n\nUser request:\n{text}");
        }
        let words: usize = spoken.iter().map(Turn::words_size).sum();
        if words > MOST_SPOKEN_BYTES {
            return Err(ConnectError::TooLarge(format!(
                "{words} bytes of words, and {MOST_SPOKEN_BYTES} is the most"
            )));
        }
        let attached: usize = spoken.iter().map(Turn::attached_size).sum();
        if attached > MOST_ATTACHED_BYTES {
            return Err(ConnectError::TooLarge(format!(
                "{attached} bytes of attached files, and {MOST_ATTACHED_BYTES} is the most"
            )));
        }
        Ok(spoken)
    }

    fn payload(&self, spoken: &[Turn], tools: &[ToolOffer], system: Option<&str>) -> Json {
        let (thinking, max_tokens) = reasoning_members(self.provider, &self.model, self.effort);
        let mut body = match self.provider.wire() {
            Wire::Messages => self.messages_body(spoken, tools, system, max_tokens),
            Wire::Responses => self.responses_body(spoken, tools, system),
            Wire::Chat => self.chat_body(spoken, tools, system),
        };
        for (key, value) in thinking {
            put(&mut body, key, value);
        }
        body
    }

    fn messages_body(
        &self,
        spoken: &[Turn],
        tools: &[ToolOffer],
        system: Option<&str>,
        max_tokens: u32,
    ) -> Json {
        let messages = spoken
            .iter()
            .filter_map(|turn| {
                let (role, content) = match turn {
                    Turn::Person { text, attachments } => {
                        ("user", messages_person_content(text, attachments))
                    }
                    Turn::Model { text, calls, raw } => (
                        "assistant",
                        self.assistant_content(text, calls, raw.as_ref()),
                    ),
                    Turn::Results { results } => (
                        "user",
                        (!results.is_empty())
                            .then(|| Json::List(results.iter().map(tool_result_block).collect())),
                    ),
                };
                content
                    .map(|content| Json::object([("role", Json::text(role)), ("content", content)]))
            })
            .collect();
        let mut body = Json::object([
            ("model", Json::text(self.model.clone())),
            ("max_tokens", Json::Number(f64::from(max_tokens))),
            ("messages", Json::List(messages)),
        ]);
        if let Some(system) = system {
            put(&mut body, "system", Json::text(system));
        }
        let offered: Vec<Json> = if tools.is_empty() {
            tools_named_in(spoken)
        } else {
            tools
                .iter()
                .map(|tool| {
                    Json::object([
                        ("name", Json::text(tool.name.clone())),
                        ("description", Json::text(tool.description.clone())),
                        ("input_schema", tool.schema.clone()),
                    ])
                })
                .collect()
        };
        if !offered.is_empty() {
            put(&mut body, "tools", Json::List(offered));
            if tools.is_empty() {
                put(
                    &mut body,
                    "tool_choice",
                    Json::object([("type", Json::text("none"))]),
                );
            }
        }
        body
    }

    fn assistant_content(&self, text: &str, calls: &[ToolCall], raw: Option<&Raw>) -> Option<Json> {
        if let Some(raw) = raw.filter(|raw| raw.provider == self.provider) {
            let items: Vec<Json> = raw
                .items
                .iter()
                .filter(|item| !is_a_blank_text_block(item))
                .cloned()
                .collect();
            return (!items.is_empty()).then_some(Json::List(items));
        }
        if calls.is_empty() {
            return (!text.trim().is_empty()).then(|| Json::text(text.to_owned()));
        }
        let mut blocks = Vec::new();
        if !text.trim().is_empty() {
            blocks.push(Json::object([
                ("type", Json::text("text")),
                ("text", Json::text(text.to_owned())),
            ]));
        }
        for call in calls {
            blocks.push(Json::object([
                ("type", Json::text("tool_use")),
                ("id", Json::text(call.id.clone())),
                ("name", Json::text(call.name.clone())),
                ("input", call.arguments_object()),
            ]));
        }
        Some(Json::List(blocks))
    }

    fn responses_body(&self, spoken: &[Turn], tools: &[ToolOffer], system: Option<&str>) -> Json {
        let mut input = Vec::new();
        for turn in spoken {
            match turn {
                Turn::Person { text, attachments } => {
                    if let Some(content) = responses_person_content(text, attachments) {
                        input.push(Json::object([
                            ("role", Json::text(turn.role())),
                            ("content", content),
                        ]));
                    }
                }
                Turn::Model { text, calls, raw } => {
                    if let Some(raw) = raw.as_ref().filter(|raw| raw.provider == self.provider) {
                        input.extend(replayable_items(&raw.items));
                        continue;
                    }
                    if !text.is_empty() {
                        input.push(Json::object([
                            ("role", Json::text("assistant")),
                            (
                                "content",
                                Json::List(vec![Json::object([
                                    ("type", Json::text("output_text")),
                                    ("text", Json::text(text.clone())),
                                ])]),
                            ),
                        ]));
                    }
                    for call in calls {
                        input.push(Json::object([
                            ("type", Json::text("function_call")),
                            ("call_id", Json::text(call.id.clone())),
                            ("name", Json::text(call.name.clone())),
                            ("arguments", Json::text(call.arguments_object().write())),
                        ]));
                    }
                }
                Turn::Results { results } => {
                    for result in results {
                        input.push(Json::object([
                            ("type", Json::text("function_call_output")),
                            ("call_id", Json::text(result.call_id.clone())),
                            ("output", Json::text(result.worded())),
                        ]));
                    }
                    input.extend(pictures_in(
                        results,
                        |picture| {
                            Json::object([
                                ("type", Json::text("input_image")),
                                ("image_url", Json::text(picture_url(picture))),
                            ])
                        },
                        |words| {
                            Json::object([
                                ("type", Json::text("input_text")),
                                ("text", Json::text(words)),
                            ])
                        },
                    ));
                }
            }
        }
        let mut body = Json::object([
            ("model", Json::text(self.model.clone())),
            ("store", Json::Bool(false)),
            ("input", Json::List(input)),
            (
                "include",
                Json::List(vec![Json::text("reasoning.encrypted_content")]),
            ),
        ]);
        if let Some(system) = system {
            put(&mut body, "instructions", Json::text(system));
        }
        if !tools.is_empty() {
            put(
                &mut body,
                "tools",
                Json::List(
                    tools
                        .iter()
                        .map(|tool| {
                            Json::object([
                                ("type", Json::text("function")),
                                ("name", Json::text(tool.name.clone())),
                                ("description", Json::text(tool.description.clone())),
                                ("parameters", tool.schema.clone()),
                                ("strict", Json::Bool(false)),
                            ])
                        })
                        .collect(),
                ),
            );
            put(&mut body, "tool_choice", Json::text("auto"));
        }
        body
    }

    fn chat_body(&self, spoken: &[Turn], tools: &[ToolOffer], system: Option<&str>) -> Json {
        let mut messages = Vec::new();
        if let Some(system) = system {
            messages.push(Json::object([
                ("role", Json::text("system")),
                ("content", Json::text(system)),
            ]));
        }
        for turn in spoken {
            match turn {
                Turn::Person { text, attachments } => {
                    if let Some(content) = chat_person_content(text, attachments) {
                        messages.push(Json::object([
                            ("role", Json::text(turn.role())),
                            ("content", content),
                        ]));
                    }
                }
                Turn::Model { text, calls, raw } => {
                    if let Some(raw) = raw.as_ref().filter(|raw| raw.provider == self.provider) {
                        messages.extend(raw.items.iter().cloned());
                    } else if calls.is_empty() {
                        if !text.is_empty() {
                            messages.push(Json::object([
                                ("role", Json::text("assistant")),
                                ("content", Json::text(text.clone())),
                            ]));
                        }
                    } else {
                        messages.push(Json::object([
                            ("role", Json::text("assistant")),
                            ("content", Json::text(text.clone())),
                            (
                                "tool_calls",
                                Json::List(calls.iter().map(chat_call_spelled).collect()),
                            ),
                        ]));
                    }
                }
                Turn::Results { results } => {
                    for result in results {
                        messages.push(Json::object([
                            ("role", Json::text("tool")),
                            ("tool_call_id", Json::text(result.call_id.clone())),
                            ("content", Json::text(result.worded())),
                        ]));
                    }
                    messages.extend(pictures_in(
                        results,
                        |picture| {
                            Json::object([
                                ("type", Json::text("image_url")),
                                (
                                    "image_url",
                                    Json::object([("url", Json::text(picture_url(picture)))]),
                                ),
                            ])
                        },
                        |words| {
                            Json::object([
                                ("type", Json::text("text")),
                                ("text", Json::text(words)),
                            ])
                        },
                    ));
                }
            }
        }
        let mut body = Json::object([
            ("model", Json::text(self.model.clone())),
            ("messages", Json::List(messages)),
        ]);
        if !tools.is_empty() {
            put(
                &mut body,
                "tools",
                Json::List(
                    tools
                        .iter()
                        .map(|tool| {
                            Json::object([
                                ("type", Json::text("function")),
                                (
                                    "function",
                                    Json::object([
                                        ("name", Json::text(tool.name.clone())),
                                        ("description", Json::text(tool.description.clone())),
                                        ("parameters", tool.schema.clone()),
                                    ]),
                                ),
                            ])
                        })
                        .collect(),
                ),
            );
        }
        body
    }
}

const NOT_KEPT: &str =
    "(this file was not kept with the saved chat, so its contents are not available)";

fn is_a_page_marker(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

fn is_a_blank_text_block(item: &Json) -> bool {
    item.get("type").and_then(Json::as_str) == Some("text")
        && item
            .get("text")
            .and_then(Json::as_str)
            .is_none_or(|text| text.trim().is_empty())
}

fn tools_named_in(spoken: &[Turn]) -> Vec<Json> {
    let names: BTreeSet<&str> = spoken
        .iter()
        .flat_map(|turn| turn.calls().iter().map(|call| call.name.as_str()))
        .filter(|name| !name.is_empty())
        .collect();
    names
        .into_iter()
        .map(|name| {
            Json::object([
                ("name", Json::text(name)),
                (
                    "description",
                    Json::text("Not available in this conversation."),
                ),
                (
                    "input_schema",
                    Json::object([("type", Json::text("object"))]),
                ),
            ])
        })
        .collect()
}

fn replayable_items(items: &[Json]) -> Vec<Json> {
    let unreadable = |item: &Json| {
        item.get("type").and_then(Json::as_str) == Some("reasoning")
            && item
                .get("encrypted_content")
                .and_then(Json::as_str)
                .is_none_or(str::is_empty)
    };
    if !items.iter().any(unreadable) {
        return items.to_vec();
    }
    items
        .iter()
        .filter(|item| !unreadable(item))
        .map(|item| {
            let mut bare = item.clone();
            if let Json::Object(members) = &mut bare {
                members.remove("id");
            }
            bare
        })
        .collect()
}

fn chat_call_spelled(call: &ToolCall) -> Json {
    Json::object([
        ("id", Json::text(call.id.clone())),
        ("type", Json::text("function")),
        (
            "function",
            Json::object([
                ("name", Json::text(call.name.clone())),
                ("arguments", Json::text(call.arguments_object().write())),
            ]),
        ),
    ])
}

fn picture_url(picture: &Picture) -> String {
    format!("data:{};base64,{}", picture.media_type, picture.base64)
}

fn pictures_in(
    results: &[ToolResult],
    image: impl Fn(&Picture) -> Json,
    words: impl Fn(String) -> Json,
) -> Option<Json> {
    let mut parts = Vec::new();
    for result in results {
        if let Some(picture) = &result.picture {
            parts.push(words(format!(
                "The picture that tool call {} returned:",
                result.call_id
            )));
            parts.push(image(picture));
        }
    }
    (!parts.is_empty())
        .then(|| Json::object([("role", Json::text("user")), ("content", Json::List(parts))]))
}

impl Connection {
    fn get(
        &self,
        suffix: &str,
        pace: Pace<'_>,
        cancel: &AtomicBool,
    ) -> Result<String, ConnectError> {
        let job = self.job("GET", suffix, None, pace)?;
        again_after(cancel, pace.waits, || {
            run_curl(&job, cancel, &mut |_| false)
        })
        .map_err(|error| redact_error(error, &self.api_key))
    }

    fn job(
        &self,
        method: &str,
        suffix: &str,
        payload: Option<&str>,
        pace: Pace<'_>,
    ) -> Result<Job, ConnectError> {
        let url = endpoint(&self.base_url, suffix, self.provider)?;
        let local = is_loopback(&url);
        let mut headers = vec!["Content-Type: application/json".to_owned()];
        if !self.api_key.is_empty() {
            if self.api_key.contains(['\r', '\n']) {
                return Err(ConnectError::Invalid(
                    "API key contains a line break".to_owned(),
                ));
            }
            if self.provider == Provider::Anthropic {
                headers.push(format!("x-api-key: {}", self.api_key));
            } else {
                headers.push(format!("Authorization: Bearer {}", self.api_key));
            }
        } else if matches!(
            self.provider,
            Provider::OpenAi | Provider::Anthropic | Provider::Gemini
        ) {
            return Err(ConnectError::Invalid(
                "this provider requires an API key".to_owned(),
            ));
        }
        if self.provider == Provider::Anthropic {
            headers.push("anthropic-version: 2023-06-01".to_owned());
        }
        let headers_at = Scratch::create(b"").ok();
        let (data, body_at) = match payload {
            None => (None, None),
            Some(payload) if payload.len() <= INLINE_BODY_BYTES => {
                (Some(format!("data = \"{}\"\n", curl_escape(payload))), None)
            }
            Some(payload) => {
                let kept = Scratch::create(payload.as_bytes()).map_err(|error| {
                    ConnectError::Curl(format!(
                        "could not keep a large request in a temporary file: {error}"
                    ))
                })?;
                let line = format!("data-binary = \"@{}\"\n", curl_escape(&kept.text()?));
                (Some(line), Some(kept))
            }
        };
        let mut config = String::new();
        let _ = write!(
            config,
            "url = \"{}\"\nrequest = \"{method}\"\nsilent\nshow-error\nfail-with-body\ngloboff\nproto = \"https,http\"\nconnect-timeout = 10\nmax-time = {}\n",
            curl_escape(&url),
            pace.cap.as_secs().max(1),
        );
        if local {
            config.push_str("noproxy = \"*\"\n");
        }
        if pace.streams {
            config.push_str("no-buffer\n");
        }
        if let Some(kept) = &headers_at
            && let Ok(text) = kept.text()
        {
            let _ = writeln!(config, "dump-header = \"{}\"", curl_escape(&text));
        }
        for header in headers {
            let _ = writeln!(config, "header = \"{}\"", curl_escape(&header));
        }
        if let Some(data) = data {
            config.push_str("header = \"Expect:\"\n");
            config.push_str(&data);
        }
        Ok(Job {
            config,
            headers_at,
            body_kept: body_at,
            idle: pace.idle,
            cap: pace.cap,
            settle: pace.settle,
            local,
        })
    }
}

struct Job {
    config: String,
    headers_at: Option<Scratch>,
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "held so that the file it names outlives the request"
        )
    )]
    body_kept: Option<Scratch>,
    idle: Duration,
    cap: Duration,
    settle: Duration,
    local: bool,
}

impl Job {
    fn headers(&self) -> Headers {
        self.headers_at
            .as_ref()
            .and_then(|kept| std::fs::read(&kept.path).ok())
            .map(|bytes| headers_seen(&String::from_utf8_lossy(&bytes)))
            .unwrap_or_default()
    }
}

struct Scratch {
    path: PathBuf,
}

static SCRATCHES: AtomicUsize = AtomicUsize::new(0);

impl Scratch {
    fn create(contents: &[u8]) -> std::io::Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.subsec_nanos());
        let name = format!(
            "panpdf-ai-{}-{nanos}-{}",
            std::process::id(),
            SCRATCHES.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(name);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path)?;
        let kept = Self { path };
        file.write_all(contents)?;
        Ok(kept)
    }

    fn text(&self) -> Result<String, ConnectError> {
        Path::to_str(&self.path)
            .map(str::to_owned)
            .ok_or_else(|| ConnectError::Curl("the temporary file has no plain name".to_owned()))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[derive(Default)]
struct Headers {
    status: Option<u16>,
    retry_after: Option<Duration>,
}

fn headers_seen(text: &str) -> Headers {
    let mut status = None;
    let (mut seconds, mut millis) = (None, None);
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("HTTP/") {
            status = rest
                .split_whitespace()
                .nth(1)
                .and_then(|code| code.parse::<u16>().ok());
            (seconds, millis) = (None, None);
        } else if let Some((name, value)) = line.split_once(':') {
            let value = value.trim();
            match name.trim().to_ascii_lowercase().as_str() {
                "retry-after" => seconds = value.parse::<f64>().ok(),
                "retry-after-ms" => millis = value.parse::<f64>().ok(),
                _ => {}
            }
        }
    }
    let retry_after = millis
        .map(|millis| millis / 1000.0)
        .or(seconds)
        .filter(|seconds| seconds.is_finite() && (0.0..1e7).contains(seconds))
        .map(Duration::from_secs_f64);
    Headers {
        status,
        retry_after,
    }
}

#[derive(Debug)]
struct Failure {
    error: ConnectError,
    transient: bool,
    wait: Option<Duration>,
    tries: usize,
}

const RETRIES: usize = 5;

const A_FEW: usize = 2;

impl From<ConnectError> for Failure {
    fn from(error: ConnectError) -> Self {
        Self {
            error,
            transient: false,
            wait: None,
            tries: 0,
        }
    }
}

impl Failure {
    const fn transient(error: ConnectError) -> Self {
        Self {
            error,
            transient: true,
            wait: None,
            tries: RETRIES,
        }
    }

    fn of_the_answer(error: ConnectError) -> Self {
        let ConnectError::Http(said) = &error else {
            return error.into();
        };
        let wait = wait_asked_for(said);
        let transient = wait.is_some() || overloaded_for_now(said);
        Self {
            error,
            transient,
            wait,
            tries: RETRIES,
        }
    }

    fn pause(&self, tried: usize, waits: &[Duration]) -> Option<Duration> {
        if !self.transient || tried >= self.tries {
            return None;
        }
        let backoff = waits.get(tried).copied()?;
        match self.wait {
            Some(wait) if wait > MOST_WAIT => None,
            Some(wait) => Some(wait),
            None => Some(backoff),
        }
    }
}

fn refused(headers: &Headers, said: String) -> Failure {
    let said = if said.trim().is_empty() {
        headers.status.map_or_else(
            || "the request was refused".to_owned(),
            |code| format!("HTTP {code}"),
        )
    } else {
        said
    };
    let wait = headers.retry_after.or_else(|| wait_asked_for(&said));
    let lower = said.to_ascii_lowercase();
    let spent_for_good =
        lower.contains("quota") || lower.contains("billing") || lower.contains("credit");
    let transient = match headers.status {
        Some(429) => wait.is_some() || !spent_for_good,
        Some(408 | 409 | 500 | 502 | 503 | 504 | 529) => true,
        Some(code) => (500..600).contains(&code) && !matches!(code, 501 | 505),
        None => wait.is_some() || overloaded_for_now(&said),
    };
    Failure {
        error: ConnectError::Http(said),
        transient,
        wait,
        tries: RETRIES,
    }
}

fn curl_trouble(
    code: Option<i32>,
    complaint: &str,
    wrote: Option<&std::io::Error>,
    (local, ran_out_of_time): (bool, bool),
) -> Failure {
    let said = if !complaint.is_empty() {
        complaint.to_owned()
    } else if let Some(error) = wrote {
        error.to_string()
    } else {
        match code {
            Some(code) => format!("curl stopped with code {code}"),
            None => "curl was stopped".to_owned(),
        }
    };
    let tries = match code {
        _ if ran_out_of_time => 0,
        Some(6 | 7) if !local => A_FEW,
        Some(35) => A_FEW,
        Some(16 | 18 | 28 | 52 | 55 | 56 | 92) => RETRIES,
        _ => 0,
    };
    Failure {
        error: ConnectError::Curl(said),
        transient: tries > 0,
        wait: None,
        tries,
    }
}

const MOST_WAIT: Duration = Duration::from_mins(1);

fn wait_asked_for(body: &str) -> Option<Duration> {
    let asking = body.contains("rate limit")
        || body.contains("Rate limit")
        || body.contains("RESOURCE_EXHAUSTED")
        || body.contains("rate_limit")
        || body.contains("quota")
        || body.contains("Quota");
    if !asking {
        return None;
    }
    let mut rest = body;
    while let Some(at) = rest.find(" in ") {
        rest = &rest[at + 4..];
        if let Some(seconds) = seconds_from(rest)
            && seconds > 0.0
        {
            return Some(Duration::from_secs_f64(seconds + 1.0));
        }
    }
    None
}

fn seconds_from(text: &str) -> Option<f64> {
    let mut total = 0.0;
    let mut counted = false;
    let mut rest = text;
    loop {
        let digits: String = rest
            .chars()
            .take_while(|letter| letter.is_ascii_digit() || *letter == '.')
            .collect();
        let Ok(number) = digits.parse::<f64>() else {
            break;
        };
        let after = rest[digits.len()..].trim_start();
        let unit: String = after
            .chars()
            .take_while(char::is_ascii_alphabetic)
            .collect();
        let each = match unit.as_str() {
            "ms" | "msec" | "milliseconds" => 0.001,
            "s" | "sec" | "secs" | "second" | "seconds" => 1.0,
            "m" | "min" | "mins" | "minute" | "minutes" => 60.0,
            "h" | "hr" | "hour" | "hours" => 3600.0,
            _ => break,
        };
        total += number * each;
        counted = true;
        rest = &after[unit.len()..];
    }
    (counted && total < 1e9).then_some(total)
}

fn wait_watching(how_long: Duration, cancel: &AtomicBool) -> bool {
    let until = Instant::now() + how_long;
    while Instant::now() < until {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        thread::sleep(Duration::from_millis(100).min(until - Instant::now()));
    }
    !cancel.load(Ordering::Relaxed)
}

fn again_after<T>(
    cancel: &AtomicBool,
    waits: &[Duration],
    mut make: impl FnMut() -> Result<T, Failure>,
) -> Result<T, ConnectError> {
    let mut tried = 0;
    loop {
        let failure = match make() {
            Ok(done) => return Ok(done),
            Err(failure) => failure,
        };
        let Some(pause) = failure.pause(tried, waits) else {
            return Err(failure.error);
        };
        if !wait_watching(pause, cancel) {
            return Err(ConnectError::Cancelled);
        }
        tried += 1;
    }
}

fn overloaded_for_now(said: &str) -> bool {
    let said = said.to_ascii_lowercase();
    if said.contains("quota") {
        return false;
    }
    [
        "high demand",
        "overloaded",
        "unavailable",
        "try again later",
    ]
    .iter()
    .any(|sign| said.contains(sign))
}

const TICK: Duration = Duration::from_millis(20);

struct Running {
    child: Child,
    threads: Vec<JoinHandle<()>>,
    lines: Receiver<Vec<u8>>,
    complaints: Receiver<Vec<u8>>,
    beat: Arc<AtomicU64>,
    started: Instant,
    wrote: std::io::Result<()>,
}

impl Running {
    fn start(config: &str) -> Result<Self, ConnectError> {
        let mut command = Command::new("curl");
        command
            .arg("-q")
            .arg("--config")
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        no_console(&mut command);
        let child = command
            .spawn()
            .map_err(|e| ConnectError::Curl(e.to_string()))?;
        let (lines_in, lines) = std::sync::mpsc::channel();
        let (complaints_in, complaints) = std::sync::mpsc::channel();
        let started = Instant::now();
        let mut running = Self {
            child,
            threads: Vec::new(),
            lines,
            complaints,
            beat: Arc::new(AtomicU64::new(0)),
            started,
            wrote: Ok(()),
        };
        let (Some(mut stdin), Some(stdout), Some(stderr)) = (
            running.child.stdin.take(),
            running.child.stdout.take(),
            running.child.stderr.take(),
        ) else {
            return Err(ConnectError::Curl(
                "curl's pipes were unavailable".to_owned(),
            ));
        };
        let beat = Arc::clone(&running.beat);
        running.threads.push(thread::spawn(move || {
            read_by_line(stdout, &lines_in, &beat, started);
        }));
        running.threads.push(thread::spawn(move || {
            let (bytes, _) = read_limited(stderr);
            let _ = complaints_in.send(bytes);
        }));
        running.wrote = stdin.write_all(config.as_bytes());
        Ok(running)
    }

    fn quiet(&self) -> Duration {
        self.started
            .elapsed()
            .saturating_sub(Duration::from_millis(self.beat.load(Ordering::Relaxed)))
    }

    fn exit(&mut self, cancel: &AtomicBool) -> Result<std::process::ExitStatus, ConnectError> {
        let waiting = Instant::now();
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => return Ok(status),
                Ok(None) => {}
                Err(error) => return Err(ConnectError::Curl(error.to_string())),
            }
            if cancel.load(Ordering::Relaxed) {
                return Err(ConnectError::Cancelled);
            }
            if waiting.elapsed() > Duration::from_secs(10) {
                return Err(ConnectError::Curl("curl did not stop".to_owned()));
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn complaint(&self) -> String {
        self.complaints
            .recv_timeout(Duration::from_secs(2))
            .map(|bytes| String::from_utf8_lossy(&bytes).trim().to_owned())
            .unwrap_or_default()
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

#[cfg(target_os = "windows")]
fn no_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x0800_0000);
}

#[cfg(not(target_os = "windows"))]
#[expect(
    clippy::missing_const_for_fn,
    reason = "the Windows arm is not const, and the two must have one signature"
)]
fn no_console(_command: &mut Command) {}

fn run_curl(
    job: &Job,
    cancel: &AtomicBool,
    on_line: &mut dyn FnMut(&str) -> bool,
) -> Result<String, Failure> {
    if cancel.load(Ordering::Relaxed) {
        return Err(ConnectError::Cancelled.into());
    }
    let mut running = Running::start(&job.config)?;
    let mut body = String::new();
    let mut trouble: Option<ConnectError> = None;
    let mut finished = false;
    loop {
        match running.lines.recv_timeout(TICK) {
            Ok(line) => finished |= take_line(line, &mut body, &mut trouble, on_line),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if let Some(why) = trouble.take() {
            return Err(why.into());
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(ConnectError::Cancelled.into());
        }
        let quiet = running.quiet();
        if finished && quiet > job.settle {
            return Ok(body);
        }
        if running.started.elapsed() > job.cap + Duration::from_secs(2) {
            return Err(ConnectError::Curl(format!(
                "the request took longer than {} seconds",
                job.cap.as_secs_f64()
            ))
            .into());
        }
        if quiet > job.idle {
            return Err(Failure {
                tries: 1,
                ..Failure::transient(ConnectError::Curl(format!(
                    "the AI service sent nothing for {} seconds",
                    job.idle.as_secs_f64()
                )))
            });
        }
    }
    let status = running.exit(cancel)?;
    let complaint = running.complaint();
    if status.success() {
        return Ok(body);
    }
    if status.code() == Some(22) {
        return Err(refused(&job.headers(), error_text(&body)));
    }
    if finished {
        return Ok(body);
    }
    let ran_out_of_time = running.started.elapsed() + Duration::from_secs(1) >= job.cap;
    Err(curl_trouble(
        status.code(),
        &complaint,
        running.wrote.as_ref().err(),
        (job.local, ran_out_of_time),
    ))
}

fn take_line(
    line: Vec<u8>,
    body: &mut String,
    trouble: &mut Option<ConnectError>,
    on_line: &mut dyn FnMut(&str) -> bool,
) -> bool {
    if trouble.is_some() {
        return false;
    }
    let Ok(text) = String::from_utf8(line) else {
        *trouble = Some(ConnectError::Protocol("answer was not UTF-8".to_owned()));
        return false;
    };
    if body.len() + text.len() > MAX_RESPONSE {
        *trouble = Some(ConnectError::ResponseTooLarge);
        return false;
    }
    body.push_str(&text);
    on_line(text.trim_end_matches(['\n', '\r']))
}

fn read_by_line(
    mut reader: impl Read,
    lines: &Sender<Vec<u8>>,
    beat: &AtomicU64,
    started: Instant,
) {
    let mut held: Vec<u8> = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(count) => {
                beat.store(
                    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                    Ordering::Relaxed,
                );
                held.extend_from_slice(&chunk[..count]);
                while let Some(at) = held.iter().position(|byte| *byte == b'\n') {
                    let line: Vec<u8> = held.drain(..=at).collect();
                    if lines.send(line).is_err() {
                        return;
                    }
                }
                if held.len() > MAX_RESPONSE {
                    let _ = lines.send(std::mem::take(&mut held));
                    return;
                }
            }
        }
    }
    if !held.is_empty() {
        let _ = lines.send(held);
    }
}

fn read_limited(mut reader: impl Read) -> (Vec<u8>, bool) {
    let mut result = Vec::new();
    let mut chunk = [0_u8; 8192];
    let mut too_large = false;
    loop {
        match reader.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(count) => {
                if result.len() < MAX_RESPONSE {
                    let keep = count.min(MAX_RESPONSE - result.len());
                    result.extend_from_slice(&chunk[..keep]);
                    too_large |= keep != count;
                } else {
                    too_large = true;
                }
            }
        }
    }
    (result, too_large)
}

fn is_loopback(url: &str) -> bool {
    url.strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .and_then(|rest| rest.split('/').next())
        .is_some_and(loopback_authority)
}

fn endpoint(base: &str, suffix: &str, provider: Provider) -> Result<String, ConnectError> {
    if base.is_empty() || base.contains(['\r', '\n', '\t', ' ', '\\', '?', '#']) {
        return Err(ConnectError::Invalid(
            "endpoint is empty or contains whitespace".to_owned(),
        ));
    }
    let local_http = base
        .strip_prefix("http://")
        .and_then(|rest| rest.split('/').next())
        .is_some_and(loopback_authority);
    let secure = base.starts_with("https://");
    let plain_and_local = local_http && base.starts_with("http://");
    if !(secure || plain_and_local) {
        return Err(ConnectError::Invalid(
            "HTTPS is required; HTTP is allowed only for loopback providers".to_owned(),
        ));
    }
    let base = base.trim_end_matches('/');
    let _ = provider;
    Ok(format!("{base}{suffix}"))
}

fn loopback_authority(authority: &str) -> bool {
    if authority.contains('@') {
        return false;
    }
    if let Some(rest) = authority.strip_prefix("[::1]") {
        return rest.is_empty() || valid_port(rest);
    }
    let (host, port) = authority
        .split_once(':')
        .map_or((authority, None), |(host, port)| (host, Some(port)));
    matches!(host, "127.0.0.1" | "localhost")
        && port.is_none_or(|digits| {
            !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn valid_port(port: &str) -> bool {
    port.strip_prefix(':').is_some_and(|digits| {
        !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
    })
}

fn curl_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\r', "\\r")
        .replace('\n', "\\n")
}

fn error_text(body: &str) -> String {
    Json::parse(body)
        .ok()
        .map(|json| match json {
            Json::List(mut list) if list.len() == 1 => list.remove(0),
            other => other,
        })
        .and_then(|json| {
            json.get("error")
                .and_then(|error| error.get("message"))
                .and_then(Json::as_str)
                .or_else(|| json.get("message").and_then(Json::as_str))
                .map(str::to_owned)
        })
        .unwrap_or_else(|| body.chars().take(512).collect())
}

fn redact_error(error: ConnectError, secret: &str) -> ConnectError {
    if secret.is_empty() {
        return error;
    }
    let replace = |text: String| text.replace(secret, "<redacted>");
    match error {
        ConnectError::Curl(text) => ConnectError::Curl(replace(text)),
        ConnectError::Http(text) => ConnectError::Http(replace(text)),
        ConnectError::Protocol(text) => ConnectError::Protocol(replace(text)),
        other => other,
    }
}

fn put(body: &mut Json, key: &str, value: Json) {
    if let Json::Object(members) = body {
        members.insert(key.to_owned(), value);
    }
}

fn messages_person_content(text: &str, attachments: &[Attachment]) -> Option<Json> {
    let has_words = !text.trim().is_empty();
    if attachments.is_empty() {
        return has_words.then(|| Json::text(text.to_owned()));
    }
    let mut blocks = Vec::new();
    if has_words {
        blocks.push(Json::object([
            ("type", Json::text("text")),
            ("text", Json::text(text.to_owned())),
        ]));
    }
    for attachment in attachments {
        blocks.push(match &attachment.kind {
            AttachmentKind::Text => Json::object([
                ("type", Json::text("text")),
                (
                    "text",
                    Json::text(attached_words(&attachment.name, &attachment.as_text())),
                ),
            ]),
            AttachmentKind::Image { media_type } => Json::object([
                ("type", Json::text("image")),
                (
                    "source",
                    Json::object([
                        ("type", Json::text("base64")),
                        ("media_type", Json::text(media_type.clone())),
                        ("data", Json::text(crate::json::base64(&attachment.bytes))),
                    ]),
                ),
            ]),
        });
    }
    Some(Json::List(blocks))
}

fn responses_person_content(text: &str, attachments: &[Attachment]) -> Option<Json> {
    let mut parts = Vec::new();
    if !text.trim().is_empty() {
        parts.push(Json::object([
            ("type", Json::text("input_text")),
            ("text", Json::text(text.to_owned())),
        ]));
    }
    for attachment in attachments {
        parts.push(match &attachment.kind {
            AttachmentKind::Text => Json::object([
                ("type", Json::text("input_text")),
                (
                    "text",
                    Json::text(attached_words(&attachment.name, &attachment.as_text())),
                ),
            ]),
            AttachmentKind::Image { media_type } => Json::object([
                ("type", Json::text("input_image")),
                (
                    "image_url",
                    Json::text(data_url(media_type, &attachment.bytes)),
                ),
            ]),
        });
    }
    (!parts.is_empty()).then_some(Json::List(parts))
}

fn chat_person_content(text: &str, attachments: &[Attachment]) -> Option<Json> {
    let has_words = !text.trim().is_empty();
    if attachments.is_empty() {
        return has_words.then(|| Json::text(text.to_owned()));
    }
    let mut parts = Vec::new();
    if has_words {
        parts.push(Json::object([
            ("type", Json::text("text")),
            ("text", Json::text(text.to_owned())),
        ]));
    }
    for attachment in attachments {
        parts.push(match &attachment.kind {
            AttachmentKind::Text => Json::object([
                ("type", Json::text("text")),
                (
                    "text",
                    Json::text(attached_words(&attachment.name, &attachment.as_text())),
                ),
            ]),
            AttachmentKind::Image { media_type } => Json::object([
                ("type", Json::text("image_url")),
                (
                    "image_url",
                    Json::object([("url", Json::text(data_url(media_type, &attachment.bytes)))]),
                ),
            ]),
        });
    }
    Some(Json::List(parts))
}

fn tool_result_block(result: &ToolResult) -> Json {
    let content = match &result.picture {
        None => Json::text(result.text.clone()),
        Some(picture) => Json::List(vec![
            Json::object([
                ("type", Json::text("text")),
                ("text", Json::text(result.text.clone())),
            ]),
            Json::object([
                ("type", Json::text("image")),
                (
                    "source",
                    Json::object([
                        ("type", Json::text("base64")),
                        ("media_type", Json::text(picture.media_type.clone())),
                        ("data", Json::text(picture.base64.clone())),
                    ]),
                ),
            ]),
        ]),
    };
    Json::object([
        ("type", Json::text("tool_result")),
        ("tool_use_id", Json::text(result.call_id.clone())),
        ("content", content),
        ("is_error", Json::Bool(result.is_error)),
    ])
}

fn answer(provider: Provider, root: &Json) -> Result<Reply, ConnectError> {
    if let Some(said) = error_in(root) {
        return Err(ConnectError::Http(said));
    }
    let reply = read_reply(provider, root)?;
    if reply.text.is_empty() && reply.calls.is_empty() {
        return Err(ConnectError::Protocol(
            if reply.cut_short {
                "the answer was cut off before it said anything"
            } else {
                "answer contains no assistant text"
            }
            .to_owned(),
        ));
    }
    Ok(reply)
}

fn error_words(error: &Json) -> String {
    let said = match error {
        Json::Text(text) => text.clone(),
        Json::Object(_) => ["message", "type", "code"]
            .iter()
            .find_map(|key| {
                error
                    .get(key)
                    .and_then(Json::as_str)
                    .filter(|text| !text.is_empty())
            })
            .unwrap_or_default()
            .to_owned(),
        _ => String::new(),
    };
    if said.is_empty() {
        "the AI service reported an error".to_owned()
    } else {
        said
    }
}

fn error_in(root: &Json) -> Option<String> {
    root.get("error")
        .filter(|error| !matches!(error, Json::Null))
        .map(error_words)
}

fn read_reply(provider: Provider, root: &Json) -> Result<Reply, ConnectError> {
    match provider.wire() {
        Wire::Responses => read_responses(provider, root),
        Wire::Messages => Ok(read_messages(provider, root)),
        Wire::Chat => Ok(read_chat(provider, root)),
    }
}

fn usage_of(wire: Wire, root: &Json) -> Option<Usage> {
    let usage = root
        .get("usage")
        .filter(|usage| matches!(usage, Json::Object(_)))?;
    let count = |json: Option<&Json>| {
        json.and_then(Json::as_count)
            .map_or(0, |count| u64::try_from(count).unwrap_or(u64::MAX))
    };
    let counted = match wire {
        Wire::Messages => Usage {
            input: count(usage.get("input_tokens")),
            output: count(usage.get("output_tokens")),
            cache_read: count(usage.get("cache_read_input_tokens")),
            cache_write: count(usage.get("cache_creation_input_tokens")),
        },
        Wire::Responses => {
            let cached = count(
                usage
                    .get("input_tokens_details")
                    .and_then(|details| details.get("cached_tokens")),
            );
            Usage {
                input: count(usage.get("input_tokens")).saturating_sub(cached),
                output: count(usage.get("output_tokens")),
                cache_read: cached,
                cache_write: 0,
            }
        }
        Wire::Chat => {
            let cached = count(
                usage
                    .get("prompt_tokens_details")
                    .and_then(|details| details.get("cached_tokens")),
            );
            Usage {
                input: count(usage.get("prompt_tokens")).saturating_sub(cached),
                output: count(usage.get("completion_tokens")),
                cache_read: cached,
                cache_write: 0,
            }
        }
    };
    (counted != Usage::default()).then_some(counted)
}

fn text_member(json: &Json, key: &str) -> String {
    json.get(key)
        .and_then(Json::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn mended(json: &Json, key: &str, value: Json) -> Json {
    let mut mended = json.clone();
    put(&mut mended, key, value);
    mended
}

fn cut_off_words() -> String {
    "This call was cut off by the output limit while it was being written, so it was not run. \
Split the work into smaller calls."
        .to_owned()
}

fn unreadable_words(why: &str) -> String {
    format!(
        "The arguments of this call could not be read ({why}), so it was not run. \
Send the call again with complete arguments."
    )
}

fn tool_call(id: String, name: String, arguments: Option<&Json>, cut_short: bool) -> ToolCall {
    match arguments_of(arguments) {
        Ok(arguments) => ToolCall {
            id,
            name,
            arguments,
            problem: None,
        },
        Err(why) => ToolCall::unreadable(
            id,
            name,
            if cut_short {
                cut_off_words()
            } else {
                unreadable_words(&why)
            },
        ),
    }
}

fn read_responses(provider: Provider, root: &Json) -> Result<Reply, ConnectError> {
    let status = root.get("status").and_then(Json::as_str);
    let cut_short = status == Some("incomplete");
    if status.is_some_and(|status| status != "completed" && status != "incomplete") {
        return Err(ConnectError::Protocol(
            "response did not complete".to_owned(),
        ));
    }
    let items = root.get("output").and_then(Json::as_list).unwrap_or(&[]);
    let mut text = String::new();
    let mut calls = Vec::new();
    let mut kept = Vec::with_capacity(items.len());
    for item in items {
        match item.get("type").and_then(Json::as_str) {
            Some("message") => {
                if let Some(parts) = item.get("content").and_then(Json::as_list) {
                    for part in parts {
                        if part.get("type").and_then(Json::as_str) == Some("output_text")
                            && let Some(value) = part.get("text").and_then(Json::as_str)
                        {
                            text.push_str(value);
                        }
                    }
                }
                kept.push(item.clone());
            }
            Some("function_call") => {
                let id = item
                    .get("call_id")
                    .or_else(|| item.get("id"))
                    .and_then(Json::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let name = text_member(item, "name");
                let unfinished = item
                    .get("status")
                    .and_then(Json::as_str)
                    .is_some_and(|status| status != "completed");
                let call = if unfinished {
                    ToolCall::unreadable(id, name, cut_off_words())
                } else {
                    tool_call(id, name, item.get("arguments"), cut_short)
                };
                kept.push(if call.problem.is_some() {
                    mended(item, "arguments", Json::text("{}"))
                } else {
                    item.clone()
                });
                calls.push(call);
            }
            _ => kept.push(item.clone()),
        }
    }
    Ok(Reply {
        text,
        calls,
        raw: raw_of(provider, &kept),
        cut_short,
        usage: usage_of(Wire::Responses, root),
    })
}

fn read_messages(provider: Provider, root: &Json) -> Reply {
    let parts = root.get("content").and_then(Json::as_list).unwrap_or(&[]);
    let stop = root.get("stop_reason").and_then(Json::as_str);
    let cut_short = matches!(stop, Some("max_tokens" | "model_context_window_exceeded"));
    let last_was_cut = cut_short
        && parts
            .last()
            .is_some_and(|part| part.get("type").and_then(Json::as_str) == Some("tool_use"));
    let mut text = String::new();
    let mut calls = Vec::new();
    let mut kept = Vec::with_capacity(parts.len());
    for (at, part) in parts.iter().enumerate() {
        match part.get("type").and_then(Json::as_str) {
            Some("text") => {
                if let Some(value) = part.get("text").and_then(Json::as_str) {
                    text.push_str(value);
                }
                kept.push(part.clone());
            }
            Some("tool_use") => {
                let id = text_member(part, "id");
                let name = text_member(part, "name");
                let call = if last_was_cut && at + 1 == parts.len() {
                    ToolCall::unreadable(id, name, cut_off_words())
                } else {
                    tool_call(id, name, part.get("input"), cut_short)
                };
                kept.push(if call.problem.is_some() {
                    mended(part, "input", Json::object([]))
                } else {
                    part.clone()
                });
                calls.push(call);
            }
            _ => kept.push(part.clone()),
        }
    }
    Reply {
        text,
        calls,
        raw: raw_of(provider, &kept),
        cut_short,
        usage: usage_of(Wire::Messages, root),
    }
}

static MADE_UP_CALLS: AtomicUsize = AtomicUsize::new(1);

fn mended_message(message: &Json, calls: &[ToolCall]) -> Json {
    let mut copy = message.clone();
    if let Some(items) = message.get("tool_calls").and_then(Json::as_list) {
        let items = items
            .iter()
            .zip(calls)
            .map(|(item, call)| {
                let mut item = item.clone();
                put(&mut item, "id", Json::text(call.id.clone()));
                let function = item
                    .get("function")
                    .filter(|_| call.problem.is_some())
                    .map(|function| mended(function, "arguments", Json::text("{}")));
                if let Some(function) = function {
                    put(&mut item, "function", function);
                }
                item
            })
            .collect();
        put(&mut copy, "tool_calls", Json::List(items));
    }
    copy
}

fn read_chat(provider: Provider, root: &Json) -> Reply {
    let choice = root
        .get("choices")
        .and_then(Json::as_list)
        .and_then(<[Json]>::first);
    let message = choice.and_then(|item| item.get("message"));
    let content = message.and_then(|message| message.get("content"));
    let mut text = String::new();
    if let Some(value) = content.and_then(Json::as_str) {
        text.push_str(value);
    } else if let Some(parts) = content.and_then(Json::as_list) {
        for part in parts {
            if part
                .get("type")
                .and_then(Json::as_str)
                .is_some_and(|kind| kind == "text" || kind == "output_text")
                && let Some(value) = part.get("text").and_then(Json::as_str)
            {
                text.push_str(value);
            }
        }
    }
    let finish = choice
        .and_then(|item| item.get("finish_reason"))
        .and_then(Json::as_str);
    let cut_short = matches!(finish, Some("length" | "content_filter"));
    let mut calls = Vec::new();
    if let Some(items) = message
        .and_then(|message| message.get("tool_calls"))
        .and_then(Json::as_list)
    {
        for item in items {
            let function = item.get("function");
            let id = match text_member(item, "id") {
                said if said.is_empty() => {
                    format!(
                        "call_local_{}",
                        MADE_UP_CALLS.fetch_add(1, Ordering::Relaxed)
                    )
                }
                said => said,
            };
            let name = function.map(|function| text_member(function, "name"));
            calls.push(tool_call(
                id,
                name.unwrap_or_default(),
                function.and_then(|function| function.get("arguments")),
                cut_short,
            ));
        }
    }
    Reply {
        text,
        raw: message.map(|message| Raw {
            provider,
            items: vec![mended_message(message, &calls)],
        }),
        calls,
        cut_short,
        usage: usage_of(Wire::Chat, root),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Progress<'a> {
    pub said: &'a str,
    pub thinking: &'a str,
}

enum Step {
    Going,
    Finished,
    Failed(Failure),
}

struct Gathering {
    state: Gathered,
    heard: bool,
    finished: bool,
    failed: Option<Failure>,
}

enum Gathered {
    Responses(FromResponses),
    Messages(FromMessages),
    Chat(FromChat),
}

#[derive(Default)]
struct FromResponses {
    said: String,
    thinking: String,
    items: Vec<Json>,
    ended: Option<Json>,
}

#[derive(Default)]
struct FromMessages {
    said: String,
    thinking: String,
    blocks: Vec<(Json, String)>,
    stop_reason: Option<String>,
    usage: BTreeMap<String, Json>,
}

#[derive(Default)]
struct FromChat {
    said: String,
    reasoning: String,
    calls: ArrivingCalls,
    kept: BTreeMap<String, Json>,
    finish_reason: Option<String>,
    usage: Option<Json>,
}

#[derive(Default)]
struct ArrivingCall {
    id: String,
    name: String,
    arguments: String,
    kept: BTreeMap<String, Json>,
}

impl ArrivingCall {
    fn spelled(&self) -> Json {
        let mut spelled = Json::object([
            ("id", Json::text(self.id.clone())),
            ("type", Json::text("function")),
            (
                "function",
                Json::object([
                    ("name", Json::text(self.name.clone())),
                    ("arguments", Json::text(self.arguments.clone())),
                ]),
            ),
        ]);
        for (key, value) in &self.kept {
            put(&mut spelled, key, value.clone());
        }
        spelled
    }
}

#[derive(Default)]
struct ArrivingCalls {
    calls: Vec<ArrivingCall>,
    slots: BTreeMap<usize, usize>,
    last: Option<usize>,
}

fn non_empty(value: Option<&Json>) -> Option<&str> {
    value.and_then(Json::as_str).filter(|text| !text.is_empty())
}

impl ArrivingCalls {
    fn take(&mut self, chunk: &Json) {
        let function = chunk.get("function");
        let id = non_empty(chunk.get("id"));
        let name = non_empty(function.and_then(|function| function.get("name")));
        let arguments = function
            .and_then(|function| function.get("arguments"))
            .and_then(Json::as_str)
            .unwrap_or_default();
        let index = chunk
            .get("index")
            .and_then(Json::as_count)
            .filter(|at| *at < 1024);
        let slot = match index {
            Some(index) => self.slots.get(&index).copied(),
            None => self.last,
        };
        let at = if let Some(at) = slot.filter(|at| self.continues(*at, id, name, arguments)) {
            at
        } else {
            self.calls.push(ArrivingCall::default());
            self.calls.len() - 1
        };
        if let Some(index) = index {
            self.slots.insert(index, at);
        }
        self.last = Some(at);
        let arriving = &mut self.calls[at];
        if let Some(id) = id {
            id.clone_into(&mut arriving.id);
        }
        if let Some(name) = name {
            name.clone_into(&mut arriving.name);
        }
        arriving.arguments.push_str(arguments);
        if let Json::Object(members) = chunk {
            for (key, value) in members {
                if !matches!(key.as_str(), "id" | "index" | "type" | "function") {
                    arriving.kept.insert(key.clone(), value.clone());
                }
            }
        }
    }

    fn continues(&self, at: usize, id: Option<&str>, name: Option<&str>, arguments: &str) -> bool {
        let call = &self.calls[at];
        if let Some(id) = id {
            return call.id.is_empty() || call.id == id;
        }
        let another_name = name.is_some_and(|name| !call.name.is_empty() && call.name != name);
        let starts_over = arguments.trim_start().starts_with('{')
            && matches!(Json::parse(&call.arguments), Ok(Json::Object(_)));
        !another_name && !starts_over
    }
}

fn stream_failure(kind: &str, said: &str) -> Failure {
    let words = if !said.is_empty() {
        said
    } else if !kind.is_empty() {
        kind
    } else {
        "the AI service reported an error"
    };
    let wait = wait_asked_for(words);
    let transient = wait.is_some()
        || overloaded_for_now(words)
        || matches!(
            kind,
            "overloaded_error"
                | "api_error"
                | "rate_limit_error"
                | "timeout_error"
                | "server_error"
                | "rate_limit_exceeded"
                | "server_is_overloaded"
                | "service_unavailable"
        );
    Failure {
        error: ConnectError::Http(words.to_owned()),
        transient,
        wait,
        tries: RETRIES,
    }
}

impl Gathering {
    fn new(wire: Wire) -> Self {
        Self {
            state: match wire {
                Wire::Responses => Gathered::Responses(FromResponses::default()),
                Wire::Messages => Gathered::Messages(FromMessages::default()),
                Wire::Chat => Gathered::Chat(FromChat::default()),
            },
            heard: false,
            finished: false,
            failed: None,
        }
    }

    fn said(&self) -> &str {
        match &self.state {
            Gathered::Responses(from) => &from.said,
            Gathered::Messages(from) => &from.said,
            Gathered::Chat(from) => &from.said,
        }
    }

    fn thinking(&self) -> &str {
        match &self.state {
            Gathered::Responses(from) => &from.thinking,
            Gathered::Messages(from) => &from.thinking,
            Gathered::Chat(from) => &from.reasoning,
        }
    }

    const fn finished(&self) -> bool {
        self.finished
    }

    fn line(&mut self, line: &str) -> bool {
        let Some(data) = line.trim_end().strip_prefix("data:") else {
            return false;
        };
        let data = data.trim();
        if data == "[DONE]" {
            self.finished |= matches!(self.state, Gathered::Chat(_));
            return false;
        }
        let Ok(event) = Json::parse(data) else {
            return false;
        };
        self.heard = true;
        let before = (self.said().len(), self.thinking().len());
        let step = match &mut self.state {
            Gathered::Responses(from) => from.take(&event),
            Gathered::Messages(from) => from.take(&event),
            Gathered::Chat(from) => from.take(&event),
        };
        match step {
            Step::Going => {}
            Step::Finished => self.finished = true,
            Step::Failed(failure) => {
                self.failed.get_or_insert(failure);
                self.finished = true;
            }
        }
        (self.said().len(), self.thinking().len()) != before
    }

    fn outcome(self) -> Result<Option<Json>, Failure> {
        if let Some(failure) = self.failed {
            return Err(failure);
        }
        if !self.heard {
            return Ok(None);
        }
        if !self.finished {
            return Err(Failure::transient(ConnectError::Protocol(
                "the AI service stopped before the answer was finished".to_owned(),
            )));
        }
        Ok(Some(match &self.state {
            Gathered::Responses(from) => from.whole(),
            Gathered::Messages(from) => from.whole(),
            Gathered::Chat(from) => from.whole(),
        }))
    }
}

impl FromResponses {
    fn take(&mut self, event: &Json) -> Step {
        let kind = event.get("type").and_then(Json::as_str).unwrap_or_default();
        let piece = || event.get("delta").and_then(Json::as_str);
        match kind {
            "response.output_text.delta" => {
                if let Some(piece) = piece() {
                    self.said.push_str(piece);
                }
            }
            "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                if let Some(piece) = piece() {
                    self.thinking.push_str(piece);
                }
            }
            "response.output_item.done" => {
                if let Some(item) = event.get("item") {
                    self.items.push(item.clone());
                }
            }
            "response.completed" => {
                self.ended = Some(
                    event
                        .get("response")
                        .cloned()
                        .unwrap_or_else(|| Json::object([("status", Json::text("completed"))])),
                );
                return Step::Finished;
            }
            "response.incomplete" => {
                let response = event
                    .get("response")
                    .cloned()
                    .unwrap_or_else(|| Json::object([]));
                self.ended = Some(mended(&response, "status", Json::text("incomplete")));
                return Step::Finished;
            }
            "response.failed" => {
                let error = event
                    .get("response")
                    .and_then(|response| response.get("error"));
                let said = error.map(error_words).unwrap_or_default();
                let code = error
                    .and_then(|error| error.get("code"))
                    .and_then(Json::as_str)
                    .unwrap_or_default();
                return Step::Failed(stream_failure(code, &said));
            }
            "error" => {
                let said = event
                    .get("message")
                    .and_then(Json::as_str)
                    .unwrap_or_default();
                let code = event.get("code").and_then(Json::as_str).unwrap_or_default();
                return Step::Failed(stream_failure(code, said));
            }
            _ => {}
        }
        Step::Going
    }

    fn whole(&self) -> Json {
        let mut root = self
            .ended
            .clone()
            .unwrap_or_else(|| Json::object([("status", Json::text("completed"))]));
        let has_output = root
            .get("output")
            .and_then(Json::as_list)
            .is_some_and(|output| !output.is_empty());
        if !has_output {
            let mut output = self.items.clone();
            if output.is_empty() && !self.said.is_empty() {
                output.push(Json::object([
                    ("type", Json::text("message")),
                    (
                        "content",
                        Json::List(vec![Json::object([
                            ("type", Json::text("output_text")),
                            ("text", Json::text(self.said.clone())),
                        ])]),
                    ),
                ]));
            }
            put(&mut root, "output", Json::List(output));
        }
        root
    }
}

impl FromMessages {
    fn add_usage(&mut self, usage: Option<&Json>) {
        if let Some(Json::Object(members)) = usage {
            for (key, value) in members {
                if !matches!(value, Json::Null) {
                    self.usage.insert(key.clone(), value.clone());
                }
            }
        }
    }

    fn take(&mut self, event: &Json) -> Step {
        let kind = event.get("type").and_then(Json::as_str).unwrap_or_default();
        let at = index_of(event);
        match kind {
            "message_start" => {
                self.add_usage(
                    event
                        .get("message")
                        .and_then(|message| message.get("usage")),
                );
            }
            "content_block_start" => {
                let block = event
                    .get("content_block")
                    .cloned()
                    .unwrap_or_else(|| Json::object([]));
                while self.blocks.len() <= at {
                    self.blocks.push((Json::object([]), String::new()));
                }
                self.blocks[at] = (block, String::new());
            }
            "content_block_delta" => {
                let Some((block, arguments)) = self.blocks.get_mut(at) else {
                    return Step::Going;
                };
                let delta = event.get("delta");
                let piece = |name: &str| {
                    delta
                        .and_then(|delta| delta.get(name))
                        .and_then(Json::as_str)
                        .unwrap_or_default()
                        .to_owned()
                };
                match delta
                    .and_then(|delta| delta.get("type"))
                    .and_then(Json::as_str)
                {
                    Some("text_delta") => {
                        let text = piece("text");
                        self.said.push_str(&text);
                        append(block, "text", &text);
                    }
                    Some("input_json_delta") => arguments.push_str(&piece("partial_json")),
                    Some("thinking_delta") => {
                        let thought = piece("thinking");
                        self.thinking.push_str(&thought);
                        append(block, "thinking", &thought);
                    }
                    Some("signature_delta") => {
                        put(block, "signature", Json::text(piece("signature")));
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                if let Some((block, arguments)) = self.blocks.get_mut(at)
                    && !arguments.is_empty()
                    && let Ok(input) = Json::parse(arguments)
                {
                    put(block, "input", input);
                    arguments.clear();
                }
            }
            "message_delta" => {
                self.add_usage(event.get("usage"));
                if let Some(why) = event
                    .get("delta")
                    .and_then(|delta| delta.get("stop_reason"))
                    .and_then(Json::as_str)
                {
                    self.stop_reason = Some(why.to_owned());
                    return Step::Finished;
                }
            }
            "message_stop" => return Step::Finished,
            "error" => {
                let error = event.get("error");
                let said = error.map(error_words).unwrap_or_default();
                let code = error
                    .and_then(|error| error.get("type"))
                    .and_then(Json::as_str)
                    .unwrap_or_default();
                return Step::Failed(stream_failure(code, &said));
            }
            _ => {}
        }
        Step::Going
    }

    fn whole(&self) -> Json {
        let blocks = self
            .blocks
            .iter()
            .filter(|(block, _)| block.get("type").is_some())
            .map(|(block, arguments)| {
                if arguments.is_empty() {
                    block.clone()
                } else {
                    mended(block, "input", Json::text(arguments.clone()))
                }
            })
            .collect();
        let mut body = Json::object([("content", Json::List(blocks))]);
        if let Some(why) = &self.stop_reason {
            put(&mut body, "stop_reason", Json::text(why.clone()));
        }
        if !self.usage.is_empty() {
            put(&mut body, "usage", Json::Object(self.usage.clone()));
        }
        body
    }
}

impl FromChat {
    fn take(&mut self, event: &Json) -> Step {
        if let Some(error) = event
            .get("error")
            .filter(|error| !matches!(error, Json::Null))
        {
            let kind = error.get("type").and_then(Json::as_str).unwrap_or_default();
            let mut failure = stream_failure(kind, &error_words(error));
            if let Some(code) = error.get("code").and_then(Json::as_count) {
                failure.transient |= code == 429 || (500..600).contains(&code);
            }
            return Step::Failed(failure);
        }
        if let Some(usage @ Json::Object(_)) = event.get("usage") {
            self.usage = Some(usage.clone());
        }
        let Some(choice) = event
            .get("choices")
            .and_then(Json::as_list)
            .and_then(<[Json]>::first)
        else {
            return Step::Going;
        };
        let mut step = Step::Going;
        if let Some(why) = choice.get("finish_reason").and_then(Json::as_str) {
            self.finish_reason = Some(why.to_owned());
            step = Step::Finished;
        }
        let Some(delta) = choice.get("delta") else {
            return step;
        };
        if let Some(piece) = delta.get("content").and_then(Json::as_str) {
            self.said.push_str(piece);
        }
        if let Some(piece) = delta.get("reasoning").and_then(Json::as_str) {
            self.reasoning.push_str(piece);
        }
        if let Json::Object(members) = delta {
            for (key, value) in members {
                if !matches!(
                    key.as_str(),
                    "content" | "reasoning" | "role" | "tool_calls"
                ) {
                    self.kept.insert(key.clone(), value.clone());
                }
            }
        }
        if let Some(asked) = delta.get("tool_calls").and_then(Json::as_list) {
            for call in asked {
                self.calls.take(call);
            }
        }
        step
    }

    fn whole(&self) -> Json {
        let mut message = Json::object([
            ("role", Json::text("assistant")),
            ("content", Json::text(self.said.clone())),
        ]);
        for (key, value) in &self.kept {
            put(&mut message, key, value.clone());
        }
        if !self.reasoning.is_empty() {
            put(
                &mut message,
                "reasoning",
                Json::text(self.reasoning.clone()),
            );
        }
        if !self.calls.calls.is_empty() {
            put(
                &mut message,
                "tool_calls",
                Json::List(self.calls.calls.iter().map(ArrivingCall::spelled).collect()),
            );
        }
        let mut choice = Json::object([("message", message)]);
        if let Some(why) = &self.finish_reason {
            put(&mut choice, "finish_reason", Json::text(why.clone()));
        }
        let mut root = Json::object([("choices", Json::List(vec![choice]))]);
        if let Some(usage) = &self.usage {
            put(&mut root, "usage", usage.clone());
        }
        root
    }
}

fn index_of(event: &Json) -> usize {
    event
        .get("index")
        .and_then(Json::as_count)
        .filter(|at| *at < 1024)
        .unwrap_or(0)
}

fn append(block: &mut Json, key: &str, piece: &str) {
    let mut grown = block
        .get(key)
        .and_then(Json::as_str)
        .unwrap_or_default()
        .to_owned();
    grown.push_str(piece);
    put(block, key, Json::text(grown));
}

fn raw_of(provider: Provider, items: &[Json]) -> Option<Raw> {
    (!items.is_empty()).then(|| Raw {
        provider,
        items: items.to_vec(),
    })
}

fn arguments_of(value: Option<&Json>) -> Result<Json, String> {
    match value {
        None | Some(Json::Null) => Ok(Json::object([])),
        Some(Json::Text(written)) if written.trim().is_empty() => Ok(Json::object([])),
        Some(Json::Text(written)) => match Json::parse(written) {
            Ok(object @ Json::Object(_)) => Ok(object),
            Ok(_) => Err("they are not a JSON object".to_owned()),
            Err(error) => Err(format!("they are not valid JSON: {error}")),
        },
        Some(object @ Json::Object(_)) => Ok(object.clone()),
        Some(_) => Err("they are not a JSON object".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::net::{TcpListener, TcpStream};

    const QUICK: [Duration; 3] = [Duration::from_millis(20); 3];

    fn quick(streams: bool) -> Pace<'static> {
        Pace {
            idle: Duration::from_secs(30),
            cap: Duration::from_secs(120),
            waits: &QUICK,
            streams,
            settle: Duration::from_secs(2),
        }
    }

    fn asking(turns: &[Turn]) -> Ask<'_> {
        Ask {
            turns,
            context: None,
            tools: &[],
            system: None,
        }
    }

    fn talking_to(provider: Provider, base_url: String) -> Connection {
        Connection {
            provider,
            base_url,
            model: "a-model".into(),
            api_key: "a-key".into(),
            effort: Effort::Off,
        }
    }

    fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack
            .windows(needle.len())
            .position(|window| window == needle)
    }

    fn read_request(stream: &mut TcpStream) -> String {
        let mut data = Vec::new();
        let mut chunk = vec![0_u8; 1 << 16];
        loop {
            let count = stream.read(&mut chunk).unwrap_or(0);
            if count == 0 {
                break;
            }
            data.extend_from_slice(&chunk[..count]);
            if let Some(end) = find(&data, b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&data[..end]).to_ascii_lowercase();
                let length = head
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:"))
                    .and_then(|value| value.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if data.len() >= end + 4 + length {
                    break;
                }
            }
        }
        String::from_utf8_lossy(&data).into_owned()
    }

    fn accept_within(listener: &TcpListener, how_long: Duration) -> Option<TcpStream> {
        listener.set_nonblocking(true).unwrap();
        let until = Instant::now() + how_long;
        while Instant::now() < until {
            if let Ok((stream, _)) = listener.accept() {
                stream.set_nonblocking(false).unwrap();
                return Some(stream);
            }
            thread::sleep(Duration::from_millis(5));
        }
        None
    }

    fn http(status: &str, headers: &str, body: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    fn event_stream(events: &str) -> Vec<u8> {
        format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{events}")
            .into_bytes()
    }

    fn serve(replies: Vec<Vec<u8>>) -> (String, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!(
            "http://127.0.0.1:{}/v1",
            listener.local_addr().unwrap().port()
        );
        let server = thread::spawn(move || {
            let mut seen = Vec::new();
            for reply in replies {
                let Some(mut stream) = accept_within(&listener, Duration::from_secs(30)) else {
                    break;
                };
                seen.push(read_request(&mut stream));
                let _ = stream.write_all(&reply);
                let _ = stream.flush();
            }
            seen
        });
        (base, server)
    }

    fn told_by(
        connection: &Connection,
        pace: Pace<'_>,
        turns: &[Turn],
    ) -> (Result<Reply, ConnectError>, Vec<String>) {
        let mut partials = Vec::new();
        let mut on_partial = |far: Progress<'_>| partials.push(far.said.to_owned());
        let told = connection.converse_within(
            pace,
            &asking(turns),
            &AtomicBool::new(false),
            if pace.streams {
                Some(&mut on_partial)
            } else {
                None
            },
        );
        (told, partials)
    }

    #[test]
    fn only_the_newest_picture_is_sent_again() {
        let drawn = |id: &str| ToolResult {
            call_id: id.to_owned(),
            text: "a page".to_owned(),
            is_error: false,
            picture: Some(Picture {
                media_type: "image/png".to_owned(),
                base64: id.to_owned(),
            }),
        };
        let turns = vec![
            Turn::person("look at page 1"),
            Turn::Results {
                results: vec![drawn("one"), ToolResult::said("words", "some words")],
            },
            Turn::model("I see it."),
            Turn::Results {
                results: vec![drawn("two"), drawn("three")],
            },
            Turn::model("and that one too"),
        ];
        let sent = without_the_old_pictures(&turns);
        assert_eq!(sent.len(), turns.len());
        let pictures: Vec<String> = sent
            .iter()
            .filter_map(|turn| match turn {
                Turn::Results { results } => Some(results),
                _ => None,
            })
            .flatten()
            .filter_map(|result| result.picture.as_ref().map(|it| it.base64.clone()))
            .collect();
        assert_eq!(pictures, vec!["three".to_owned()]);
        assert_eq!(sent[1], {
            let Turn::Results { results } = &turns[1] else {
                panic!("a turn of results");
            };
            Turn::Results {
                results: vec![
                    ToolResult {
                        picture: None,
                        ..results[0].clone()
                    },
                    results[1].clone(),
                ],
            }
        });
        assert_eq!(sent[0], turns[0]);
        assert_eq!(sent[2], turns[2]);
        assert_eq!(sent[4], turns[4]);
    }

    #[test]
    fn a_conversation_without_pictures_is_unchanged() {
        let turns = vec![
            Turn::person("hello"),
            Turn::Results {
                results: vec![ToolResult::said("one", "done")],
            },
        ];
        assert_eq!(without_the_old_pictures(&turns), turns);
        assert_eq!(without_the_old_pictures(&[]), Vec::<Turn>::new());
    }

    #[test]
    fn presets_and_transport_rules_are_explicit() {
        assert_eq!(
            Provider::Ollama.default_base_url(),
            "http://127.0.0.1:11434/v1"
        );
        assert!(endpoint("http://example.test/v1", "/models", Provider::Custom).is_err());
        assert!(endpoint("http://127.0.0.1:1/v1", "/models", Provider::Ollama).is_ok());
        assert!(endpoint("http://127.0.0.1.evil/v1", "/models", Provider::Ollama).is_err());
        assert!(endpoint("https://example.test/v1", "/models", Provider::Custom).is_ok());
    }

    #[test]
    fn curl_config_escapes_prompt_like_values() {
        let escaped = curl_escape("a\\b\"c");
        assert_eq!(escaped, "a\\\\b\\\"c");
        assert!(!escaped.contains('\n'));
    }

    #[test]
    fn text_normalization_covers_openai_and_anthropic_shapes() {
        let value = Json::parse(
            r#"{"output":[{"type":"message","content":[{"type":"output_text","text":"a"}]}]}"#,
        )
        .unwrap();
        assert_eq!(answer(Provider::OpenAi, &value).unwrap().text, "a");
        let unrelated = Json::parse(r#"{"output":[{"type":"reasoning","content":[{"type":"summary_text","text":"ignore"}]}]}"#).unwrap();
        assert!(answer(Provider::OpenAi, &unrelated).is_err());
    }

    #[test]
    fn a_conversation_is_spelled_for_each_wire_family() {
        let turns = [
            Turn::person("first"),
            Turn::model("answered"),
            Turn::person("second"),
        ];
        let of = |provider| Connection {
            provider,
            base_url: "http://127.0.0.1:1/v1".into(),
            model: "m".into(),
            api_key: "k".into(),
            effort: Effort::Off,
        };
        let anthropic = of(Provider::Anthropic)
            .payload(
                &of(Provider::Anthropic).spoken(&turns, None).unwrap(),
                &[],
                None,
            )
            .write();
        assert_eq!(anthropic.matches("\"role\"").count(), 3, "{anthropic}");
        assert!(anthropic.contains("\"assistant\""), "{anthropic}");
        assert!(
            anthropic.find("first").unwrap() < anthropic.find("second").unwrap(),
            "{anthropic}"
        );
        let openai = of(Provider::OpenAi)
            .payload(
                &of(Provider::OpenAi).spoken(&turns, None).unwrap(),
                &[],
                None,
            )
            .write();
        assert_eq!(openai.matches("input_text").count(), 2, "{openai}");
        assert_eq!(openai.matches("output_text").count(), 1, "{openai}");
        let local = of(Provider::Ollama)
            .payload(
                &of(Provider::Ollama).spoken(&turns, None).unwrap(),
                &[],
                None,
            )
            .write();
        assert_eq!(local.matches("\"content\"").count(), 3, "{local}");
    }

    #[test]
    fn the_page_is_given_with_the_latest_question_and_not_the_first() {
        let connection = of(Provider::Ollama);
        let turns = [
            Turn::person("first"),
            Turn::model("answered"),
            Turn::person("second"),
            Turn::Model {
                text: String::new(),
                calls: vec![ToolCall::asked("c1", "read_text", Json::object([]))],
                raw: None,
            },
            Turn::Results {
                results: vec![ToolResult::said("c1", "page one")],
            },
        ];
        let spoken = connection.spoken(&turns, Some("the page")).unwrap();
        assert_eq!(spoken[0], turns[0], "an earlier question is left as it was");
        assert_eq!(
            spoken[2].text(),
            "Document context:\nthe page\n\nUser request:\nsecond"
        );
        assert_eq!(spoken[3..], turns[3..]);
        let written = connection.payload(&spoken, &[], None).write();
        assert_eq!(written.matches("Document context").count(), 1, "{written}");
        assert!(
            written.find("first").unwrap() < written.find("the page").unwrap(),
            "{written}"
        );
        assert_eq!(connection.spoken(&turns, None).unwrap(), turns.to_vec());
        assert_eq!(connection.spoken(&turns, Some("")).unwrap(), turns.to_vec());
    }

    #[test]
    fn a_conversation_must_end_with_something_the_person_said() {
        let connection = Connection {
            provider: Provider::Ollama,
            base_url: "http://127.0.0.1:1/v1".into(),
            model: "m".into(),
            api_key: String::new(),
            effort: Effort::Off,
        };
        assert!(connection.spoken(&[], None).is_err());
        assert!(connection.spoken(&[Turn::model("hello")], None).is_err());
        assert!(connection.spoken(&[Turn::person("")], None).is_err());
    }

    #[test]
    fn a_request_curl_could_not_make_is_not_a_refusal() {
        use std::net::TcpListener;
        let closed = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = closed.local_addr().unwrap().port();
        drop(closed);
        let connection = Connection {
            provider: Provider::Ollama,
            base_url: format!("http://127.0.0.1:{port}/v1"),
            model: "m".into(),
            api_key: String::new(),
            effort: Effort::Off,
        };
        let error = connection.models(&AtomicBool::new(false)).unwrap_err();
        match error {
            ConnectError::Curl(said) => assert!(!said.is_empty(), "curl says why"),
            other => panic!("a request curl could not make: {other:?}"),
        }
    }

    #[test]
    fn keys_with_lines_are_rejected() {
        let connection = Connection {
            provider: Provider::Ollama,
            base_url: "http://127.0.0.1:1/v1".into(),
            model: "m".into(),
            api_key: "bad\nkey".into(),
            effort: Effort::Off,
        };
        let error = connection.models(&AtomicBool::new(false)).unwrap_err();
        assert!(matches!(error, ConnectError::Invalid(_)));
    }

    #[test]
    fn local_fake_server_proves_models_request_and_json_answer() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 4096];
            let count = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..count]);
            assert!(request.starts_with("GET /v1/models"));
            let body = r#"{"data":[{"id":"local-model"}]}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });
        let connection = Connection {
            provider: Provider::Ollama,
            base_url: format!("http://127.0.0.1:{}/v1", address.port()),
            model: "local-model".into(),
            api_key: String::new(),
            effort: Effort::Off,
        };
        let models = connection.models(&AtomicBool::new(false)).unwrap();
        server.join().unwrap();
        assert_eq!(
            models,
            vec![Model {
                id: "local-model".into()
            }]
        );
    }

    #[test]
    fn a_level_of_thinking_is_written_and_read_as_one_word() {
        for level in [Effort::Off, Effort::Low, Effort::Medium, Effort::High] {
            assert_eq!(Effort::parse(level.as_str()), Some(level));
        }
        assert_eq!(Effort::Medium.as_str(), "medium");
        assert_eq!(Effort::parse("Medium"), None);
        assert_eq!(Effort::parse("maximum"), None);
        assert_eq!(Effort::default(), Effort::Off);
    }

    #[test]
    fn an_anthropic_model_is_asked_to_think_in_the_way_its_own_name_takes() {
        for budget in [
            "claude-3-5-sonnet-20241022",
            "claude-3-opus-20240229",
            "claude-3-7-sonnet-20250219",
            "claude-sonnet-4-5-20250929",
            "claude-opus-4-1-20250805",
            "claude-haiku-4-5-20251001",
            "claude-opus-4-5",
            "claude-opus-4-0",
            "claude-sonnet-4-20250514",
            "claude-opus-4-20250514",
            "anthropic/claude-sonnet-4-5",
        ] {
            assert_eq!(
                anthropic_thinking_style(budget),
                ThinkingStyle::Budget,
                "{budget}"
            );
        }
        for adaptive in [
            "claude-opus-5",
            "claude-sonnet-5-20260101",
            "claude-sonnet-5-5",
            "claude-opus-4-6",
            "claude-sonnet-4-6",
            "claude-opus-4-7",
            "a-model-with-another-name",
        ] {
            assert_eq!(
                anthropic_thinking_style(adaptive),
                ThinkingStyle::Adaptive,
                "{adaptive}"
            );
        }
    }

    #[test]
    fn thinking_is_asked_for_in_each_familys_own_words() {
        let members = |provider, model, effort| {
            let (members, max_tokens) = reasoning_members(provider, model, effort);
            (
                Json::Object(
                    members
                        .into_iter()
                        .map(|(key, value)| (key.to_owned(), value))
                        .collect(),
                )
                .write(),
                max_tokens,
            )
        };
        for provider in [Provider::OpenAi, Provider::Anthropic, Provider::Ollama] {
            let model = if provider == Provider::Anthropic {
                "claude-sonnet-4-5-20250929"
            } else {
                "m"
            };
            assert_eq!(
                members(provider, model, Effort::Off),
                ("{}".to_owned(), OUTPUT_TOKENS),
                "{provider:?} asked for nothing"
            );
        }
        assert_eq!(
            members(Provider::OpenAi, "gpt-5", Effort::Low).0,
            r#"{"reasoning":{"effort":"low"}}"#
        );
        assert_eq!(
            members(Provider::OpenAi, "gpt-5", Effort::High).0,
            r#"{"reasoning":{"effort":"high"}}"#
        );
        assert_eq!(
            members(Provider::Anthropic, "claude-sonnet-4-5", Effort::Low),
            (
                r#"{"thinking":{"budget_tokens":1024,"type":"enabled"}}"#.to_owned(),
                1024 + OUTPUT_TOKENS
            )
        );
        assert_eq!(
            members(Provider::Anthropic, "claude-3-7-sonnet", Effort::Medium),
            (
                r#"{"thinking":{"budget_tokens":4096,"type":"enabled"}}"#.to_owned(),
                4096 + OUTPUT_TOKENS
            )
        );
        assert_eq!(
            members(Provider::Anthropic, "claude-opus-4-1", Effort::High),
            (
                r#"{"thinking":{"budget_tokens":16000,"type":"enabled"}}"#.to_owned(),
                32_000
            )
        );
        assert_eq!(
            members(Provider::Anthropic, "claude-opus-5", Effort::Medium),
            (
                r#"{"output_config":{"effort":"medium"},"thinking":{"type":"adaptive"}}"#
                    .to_owned(),
                ADAPTIVE_OUTPUT_TOKENS
            )
        );
        for provider in [Provider::Ollama, Provider::LmStudio, Provider::Gemini] {
            assert_eq!(
                members(provider, "qwen3.5:latest", Effort::Low).0,
                r#"{"reasoning_effort":"low"}"#,
                "{provider:?}"
            );
        }
    }

    #[test]
    fn what_was_asked_for_is_in_the_body() {
        let connection = Connection {
            provider: Provider::Anthropic,
            base_url: "https://api.anthropic.com/v1".into(),
            model: "claude-sonnet-4-5-20250929".into(),
            api_key: "k".into(),
            effort: Effort::High,
        };
        let spoken = connection.spoken(&[Turn::person("hello")], None).unwrap();
        let written = connection.payload(&spoken, &[], None).write();
        assert!(written.contains(r#""budget_tokens":16000"#), "{written}");
        assert!(written.contains(r#""max_tokens":32000"#), "{written}");
    }

    fn of(provider: Provider) -> Connection {
        Connection {
            provider,
            base_url: "http://127.0.0.1:1/v1".into(),
            model: "m".into(),
            api_key: "k".into(),
            effort: Effort::Off,
        }
    }

    fn one_tool() -> Vec<ToolOffer> {
        vec![ToolOffer {
            name: "read_text".into(),
            description: "Reads a page.".into(),
            schema: Json::parse(r#"{"type":"object","properties":{"page":{"type":"integer"}}}"#)
                .unwrap(),
        }]
    }

    #[test]
    fn with_nothing_offered_the_body_is_what_it_always_was() {
        let turns = [Turn::person("first")];
        let body = |provider| {
            of(provider)
                .payload(&of(provider).spoken(&turns, None).unwrap(), &[], None)
                .write()
        };
        assert_eq!(
            body(Provider::Anthropic),
            r#"{"max_tokens":16000,"messages":[{"content":"first","role":"user"}],"model":"m"}"#
        );
        assert_eq!(
            body(Provider::OpenAi),
            r#"{"include":["reasoning.encrypted_content"],"input":[{"content":[{"text":"first","type":"input_text"}],"role":"user"}],"model":"m","store":false}"#
        );
        assert_eq!(
            body(Provider::Ollama),
            r#"{"messages":[{"content":"first","role":"user"}],"model":"m"}"#
        );
        for provider in [Provider::Anthropic, Provider::OpenAi, Provider::Ollama] {
            let offered = of(provider)
                .payload(
                    &of(provider).spoken(&turns, None).unwrap(),
                    &one_tool(),
                    None,
                )
                .write();
            assert!(offered.contains(r#""tools""#), "{offered}");
        }
        let attached = [Turn::person_with(
            "first",
            vec![Attachment::image("shot.png", "image/png", vec![1, 2, 3])],
        )];
        for provider in [Provider::Anthropic, Provider::OpenAi, Provider::Ollama] {
            let written = of(provider)
                .payload(&of(provider).spoken(&attached, None).unwrap(), &[], None)
                .write();
            assert!(written.contains("AQID"), "{written}");
        }
    }

    #[test]
    fn an_attachment_is_spelled_in_each_familys_own_words() {
        let turns = [Turn::person_with(
            "look at this",
            vec![
                Attachment::text("report.pdf", "page one says hello"),
                Attachment::image("shot.png", "image/png", vec![1, 2, 3]),
            ],
        )];
        let body = |provider| {
            of(provider)
                .payload(&of(provider).spoken(&turns, None).unwrap(), &[], None)
                .write()
        };

        let anthropic = body(Provider::Anthropic);
        assert!(
            anthropic.contains(
                r#"{"source":{"data":"AQID","media_type":"image/png","type":"base64"},"type":"image"}"#
            ),
            "{anthropic}"
        );
        assert!(
            anthropic.contains(r#"Attached file \"report.pdf\":\npage one says hello"#),
            "{anthropic}"
        );

        let openai = body(Provider::OpenAi);
        assert!(
            openai.contains(r#"{"image_url":"data:image/png;base64,AQID","type":"input_image"}"#),
            "{openai}"
        );
        assert_eq!(openai.matches("input_text").count(), 2, "{openai}");

        let local = body(Provider::Ollama);
        assert!(
            local.contains(
                r#"{"image_url":{"url":"data:image/png;base64,AQID"},"type":"image_url"}"#
            ),
            "{local}"
        );
        assert_eq!(local.matches(r#""type":"text""#).count(), 2, "{local}");

        for written in [anthropic, openai, local] {
            assert_eq!(written.matches("AQID").count(), 1, "{written}");
        }
    }

    #[test]
    fn what_is_attached_has_an_allowance_of_its_own() {
        let connection = of(Provider::Ollama);
        let photograph = |bytes: usize| {
            Turn::person_with(
                "",
                vec![Attachment::image("shot.png", "image/png", vec![7; bytes])],
            )
        };
        assert!(connection.spoken(&[photograph(3)], None).is_ok());
        assert!(
            connection
                .spoken(&[Turn::person("x".repeat(MOST_SPOKEN_BYTES + 1))], None)
                .is_err()
        );
        assert!(
            connection
                .spoken(&[photograph(MOST_SPOKEN_BYTES + 1)], None)
                .is_ok()
        );
        assert!(
            connection
                .spoken(&[photograph(crate::attach::MOST_TOTAL_BYTES + 1024)], None)
                .is_err()
        );
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "one spelled-out known answer per wire family"
    )]
    fn a_tool_is_offered_and_answered_in_each_familys_own_words() {
        let turns = [
            Turn::person("read them"),
            Turn::Model {
                text: "Looking.".into(),
                calls: vec![
                    ToolCall {
                        id: "c1".into(),
                        name: "read_text".into(),
                        arguments: Json::parse(r#"{"page":1}"#).unwrap(),
                        problem: None,
                    },
                    ToolCall {
                        id: "c2".into(),
                        name: "read_text".into(),
                        arguments: Json::parse(r#"{"page":2}"#).unwrap(),
                        problem: None,
                    },
                ],
                raw: None,
            },
            Turn::Results {
                results: vec![
                    ToolResult::said("c1", "page one"),
                    ToolResult::failed("c2", "no such page"),
                ],
            },
        ];
        let body = |provider| {
            of(provider)
                .payload(
                    &of(provider).spoken(&turns, None).unwrap(),
                    &one_tool(),
                    Some("you are helping"),
                )
                .write()
        };

        let anthropic = body(Provider::Anthropic);
        assert!(
            anthropic.contains(r#""input_schema":{"properties":{"page":{"type":"integer"}}"#),
            "{anthropic}"
        );
        assert!(
            anthropic.contains(r#""system":"you are helping""#),
            "{anthropic}"
        );
        assert!(
            anthropic
                .contains(r#"{"id":"c1","input":{"page":1},"name":"read_text","type":"tool_use"}"#),
            "{anthropic}"
        );
        assert_eq!(
            anthropic.matches(r#""tool_result""#).count(),
            2,
            "{anthropic}"
        );
        assert_eq!(
            anthropic.matches(r#""role":"user""#).count(),
            2,
            "{anthropic}"
        );
        assert!(
            anthropic.contains(r#"{"content":"no such page","is_error":true,"tool_use_id":"c2","type":"tool_result"}"#),
            "{anthropic}"
        );

        let openai = body(Provider::OpenAi);
        assert!(
            openai.contains(r#""instructions":"you are helping""#),
            "{openai}"
        );
        assert!(openai.contains(r#""tool_choice":"auto""#), "{openai}");
        assert!(
            openai.contains(r#""include":["reasoning.encrypted_content"]"#),
            "{openai}"
        );
        assert!(
            openai.contains(r#"{"description":"Reads a page.","name":"read_text","parameters":{"properties":{"page":{"type":"integer"}},"type":"object"},"strict":false,"type":"function"}"#),
            "{openai}"
        );
        assert!(
            openai.contains(r#"{"arguments":"{\"page\":1}","call_id":"c1","name":"read_text","type":"function_call"}"#),
            "{openai}"
        );
        assert_eq!(
            openai.matches(r#""type":"function_call_output""#).count(),
            2,
            "{openai}"
        );

        let local = body(Provider::Ollama);
        assert!(
            local.contains(r#"{"content":"you are helping","role":"system"}"#),
            "{local}"
        );
        assert!(
            local.contains(
                r#""function":{"description":"Reads a page.","name":"read_text","parameters""#
            ),
            "{local}"
        );
        assert!(
            local.contains(r#"{"function":{"arguments":"{\"page\":1}","name":"read_text"},"id":"c1","type":"function"}"#),
            "{local}"
        );
        assert_eq!(local.matches(r#""role":"tool""#).count(), 2, "{local}");
        assert!(
            local.contains(r#"{"content":"page one","role":"tool","tool_call_id":"c1"}"#),
            "{local}"
        );
    }

    #[test]
    fn a_picture_a_tool_returned_is_shown_to_the_model_on_every_wire() {
        let turns = [
            Turn::person("show me"),
            Turn::Model {
                text: String::new(),
                calls: vec![
                    ToolCall::asked("c1", "render_page", Json::object([])),
                    ToolCall::asked("c2", "read_text", Json::object([])),
                ],
                raw: None,
            },
            Turn::Results {
                results: vec![
                    ToolResult {
                        call_id: "c1".into(),
                        text: "page 1".into(),
                        is_error: false,
                        picture: Some(Picture {
                            media_type: "image/png".into(),
                            base64: "AAAA".into(),
                        }),
                    },
                    ToolResult::failed("c2", "no such page"),
                ],
            },
        ];
        let body = |provider| {
            Json::parse(
                &of(provider)
                    .payload(
                        &of(provider).spoken(&turns, None).unwrap(),
                        &one_tool(),
                        None,
                    )
                    .write(),
            )
            .unwrap()
        };
        let member = |json: &Json, key: &str| json.get(key).map(Json::write).unwrap_or_default();

        let anthropic = body(Provider::Anthropic).write();
        assert!(
            anthropic.contains(
                r#"{"source":{"data":"AAAA","media_type":"image/png","type":"base64"},"type":"image"}"#
            ),
            "{anthropic}"
        );
        assert!(
            anthropic.contains(r#"{"content":"no such page","is_error":true,"tool_use_id":"c2","type":"tool_result"}"#),
            "the wire that has a flag for errors keeps the words as they are: {anthropic}"
        );

        let openai = body(Provider::OpenAi);
        let input = openai.get("input").and_then(Json::as_list).unwrap();
        let (picture, outputs) = input.split_last().unwrap();
        assert_eq!(
            outputs[outputs.len() - 2..]
                .iter()
                .map(|item| (member(item, "call_id"), member(item, "output")))
                .collect::<Vec<_>>(),
            vec![
                (r#""c1""#.to_owned(), r#""page 1""#.to_owned()),
                (r#""c2""#.to_owned(), r#""Error: no such page""#.to_owned()),
            ]
        );
        assert_eq!(member(picture, "role"), r#""user""#);
        let parts = picture.get("content").and_then(Json::as_list).unwrap();
        assert_eq!(
            parts.iter().map(Json::write).collect::<Vec<_>>(),
            vec![
                r#"{"text":"The picture that tool call c1 returned:","type":"input_text"}"#
                    .to_owned(),
                r#"{"image_url":"data:image/png;base64,AAAA","type":"input_image"}"#.to_owned(),
            ]
        );

        let local = body(Provider::Ollama);
        let messages = local.get("messages").and_then(Json::as_list).unwrap();
        let (picture, tools) = messages.split_last().unwrap();
        assert_eq!(
            tools[tools.len() - 2..]
                .iter()
                .map(|message| (member(message, "tool_call_id"), member(message, "content")))
                .collect::<Vec<_>>(),
            vec![
                (r#""c1""#.to_owned(), r#""page 1""#.to_owned()),
                (r#""c2""#.to_owned(), r#""Error: no such page""#.to_owned()),
            ]
        );
        assert_eq!(member(picture, "role"), r#""user""#);
        let parts = picture.get("content").and_then(Json::as_list).unwrap();
        assert_eq!(
            parts.iter().map(Json::write).collect::<Vec<_>>(),
            vec![
                r#"{"text":"The picture that tool call c1 returned:","type":"text"}"#.to_owned(),
                r#"{"image_url":{"url":"data:image/png;base64,AAAA"},"type":"image_url"}"#
                    .to_owned(),
            ]
        );
    }

    #[test]
    fn calls_are_read_from_each_familys_answer() {
        let block = |reply: &Reply| {
            reply.calls[0]
                .arguments
                .get("block")
                .and_then(Json::as_str)
                .unwrap_or("")
                .to_owned()
        };

        let openai = read_reply(
            Provider::OpenAi,
            &Json::parse(
                r#"{"status":"completed","output":[
{"type":"reasoning","id":"rs_1","encrypted_content":"xx"},
{"type":"function_call","call_id":"call_1","name":"replace_text","arguments":"{\"block\":\"p2-b3\",\"text\":\"Hello\"}"}]}"#,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(openai.calls.len(), 1);
        assert_eq!(openai.calls[0].id, "call_1");
        assert_eq!(openai.calls[0].name, "replace_text");
        assert_eq!(block(&openai), "p2-b3");
        assert!(openai.text.is_empty());
        assert!(!openai.cut_short);

        let anthropic = read_reply(
            Provider::Anthropic,
            &Json::parse(
                r#"{"stop_reason":"tool_use","content":[
{"type":"thinking","thinking":"hm","signature":"s"},
{"type":"text","text":"Changing it."},
{"type":"tool_use","id":"toolu_1","name":"replace_text","input":{"block":"p2-b3","text":"Hello"}}]}"#,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(anthropic.calls[0].id, "toolu_1");
        assert_eq!(anthropic.text, "Changing it.");
        assert_eq!(block(&anthropic), "p2-b3");

        let local = read_reply(
            Provider::Ollama,
            &Json::parse(
                r#"{"choices":[{"finish_reason":"tool_calls","message":{"role":"assistant","content":"","reasoning":"thinking","tool_calls":[{"id":"call_1","type":"function","function":{"name":"replace_text","arguments":"{\"block\":\"p2-b3\"}"}}]}}]}"#,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(local.calls[0].id, "call_1");
        assert_eq!(block(&local), "p2-b3");
    }

    #[test]
    fn arguments_are_read_as_an_object_or_as_a_string() {
        let object = read_reply(
            Provider::Ollama,
            &Json::parse(
                r#"{"choices":[{"message":{"tool_calls":[{"id":"call_1","function":{"name":"replace_text","arguments":{"block":"p2-b3"}}}]}}]}"#,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            object.calls[0]
                .arguments
                .get("block")
                .and_then(Json::as_str),
            Some("p2-b3")
        );
        assert_eq!(object.calls[0].problem, None);
    }

    #[test]
    fn a_call_with_arguments_that_are_not_json_does_not_cost_the_rest_of_the_reply() {
        let asked = |wire: &str| {
            Json::parse(&match wire {
                "chat" => r#"{"choices":[{"finish_reason":"tool_calls","message":{"role":"assistant","content":"I will try.","tool_calls":[
{"id":"call_1","type":"function","function":{"name":"replace_text","arguments":"{not json"}},
{"id":"call_2","type":"function","function":{"name":"read_text","arguments":"{\"first\":1}"}}]}}]}"#,
                _ => r#"{"status":"completed","output":[
{"type":"message","content":[{"type":"output_text","text":"I will try."}]},
{"type":"function_call","call_id":"call_1","name":"replace_text","arguments":"{not json"},
{"type":"function_call","call_id":"call_2","name":"read_text","arguments":"{\"first\":1}"}]}"#,
            }
            .replace('\n', ""))
            .unwrap()
        };
        for (provider, wire) in [(Provider::Ollama, "chat"), (Provider::OpenAi, "responses")] {
            let reply = read_reply(provider, &asked(wire)).unwrap();
            assert_eq!(reply.text, "I will try.", "{wire}");
            assert_eq!(reply.calls.len(), 2, "{wire}");
            let broken = &reply.calls[0];
            assert_eq!(broken.id, "call_1");
            assert_eq!(broken.name, "replace_text");
            assert_eq!(broken.arguments, Json::Null);
            assert!(
                broken
                    .problem
                    .as_deref()
                    .is_some_and(|why| why.contains("not valid JSON") && why.contains("not run")),
                "{wire}: {broken:?}"
            );
            let sound = &reply.calls[1];
            assert_eq!(sound.problem, None, "{wire}");
            assert_eq!(sound.arguments.write(), r#"{"first":1}"#, "{wire}");
            let raw = reply
                .raw
                .unwrap()
                .items
                .iter()
                .map(Json::write)
                .collect::<String>();
            assert!(
                !raw.contains("not json"),
                "{wire}: what is sent back is valid: {raw}"
            );
            assert!(raw.contains(r#"{\"first\":1}"#), "{wire}: {raw}");
        }
        let arrays = read_reply(
            Provider::Ollama,
            &Json::parse(
                r#"{"choices":[{"message":{"tool_calls":[{"id":"c","function":{"name":"n","arguments":"[1]"}}]}}]}"#,
            )
            .unwrap(),
        )
        .unwrap();
        assert!(
            arrays.calls[0]
                .problem
                .as_deref()
                .is_some_and(|why| why.contains("JSON object")),
            "{arrays:?}"
        );
    }

    #[test]
    fn an_answer_that_ran_out_of_room_is_marked() {
        let openai = read_reply(
            Provider::OpenAi,
            &Json::parse(
                r#"{"status":"incomplete","incomplete_details":{"reason":"max_output_tokens"},"output":[{"type":"message","content":[{"type":"output_text","text":"half"}]}]}"#,
            )
            .unwrap(),
        )
        .unwrap();
        assert!(openai.cut_short && openai.text == "half");
        let anthropic = read_reply(
            Provider::Anthropic,
            &Json::parse(
                r#"{"stop_reason":"max_tokens","content":[{"type":"text","text":"half"}]}"#,
            )
            .unwrap(),
        )
        .unwrap();
        assert!(anthropic.cut_short);
        let local = read_reply(
            Provider::Ollama,
            &Json::parse(
                r#"{"choices":[{"finish_reason":"length","message":{"content":"half"}}]}"#,
            )
            .unwrap(),
        )
        .unwrap();
        assert!(local.cut_short);
        let finished = read_reply(
            Provider::Ollama,
            &Json::parse(r#"{"choices":[{"finish_reason":"stop","message":{"content":"all"}}]}"#)
                .unwrap(),
        )
        .unwrap();
        assert!(!finished.cut_short);
    }

    #[test]
    fn a_models_own_items_are_sent_back_as_they_came() {
        let raw = Raw {
            provider: Provider::Anthropic,
            items: vec![
                Json::parse(r#"{"type":"thinking","thinking":"hm","signature":"sig-1"}"#).unwrap(),
                Json::parse(r#"{"type":"text","text":"Changing it."}"#).unwrap(),
                Json::parse(
                    r#"{"type":"tool_use","id":"toolu_1","name":"replace_text","input":{"block":"p2-b3"}}"#,
                )
                .unwrap(),
            ],
        };
        let turns = [
            Turn::person("change it"),
            Turn::Model {
                text: "Changing it.".into(),
                calls: vec![ToolCall {
                    id: "toolu_1".into(),
                    name: "replace_text".into(),
                    arguments: Json::parse(r#"{"block":"p2-b3"}"#).unwrap(),
                    problem: None,
                }],
                raw: Some(raw),
            },
            Turn::Results {
                results: vec![ToolResult::said("toolu_1", "done")],
            },
        ];
        let anthropic = of(Provider::Anthropic)
            .payload(
                &of(Provider::Anthropic).spoken(&turns, None).unwrap(),
                &one_tool(),
                None,
            )
            .write();
        assert!(anthropic.contains(r#""signature":"sig-1""#), "{anthropic}");
        let openai = of(Provider::OpenAi)
            .payload(
                &of(Provider::OpenAi).spoken(&turns, None).unwrap(),
                &one_tool(),
                None,
            )
            .write();
        assert!(!openai.contains("sig-1"), "{openai}");
        assert!(openai.contains(r#""type":"function_call""#), "{openai}");
    }

    #[test]
    fn a_conversation_ending_in_results_is_still_a_question() {
        let connection = of(Provider::Ollama);
        let answered = [
            Turn::person("read it"),
            Turn::Model {
                text: String::new(),
                calls: vec![ToolCall {
                    id: "c1".into(),
                    name: "read_text".into(),
                    arguments: Json::object([]),
                    problem: None,
                }],
                raw: None,
            },
            Turn::Results {
                results: vec![ToolResult::said("c1", "page one")],
            },
        ];
        assert!(connection.spoken(&answered, None).is_ok());
        assert!(connection.spoken(&answered[..2], None).is_err());
        assert!(
            connection
                .spoken(
                    &[Turn::person("x"), Turn::Results { results: vec![] }],
                    None
                )
                .is_err()
        );
    }

    #[test]
    fn calls_nobody_answered_are_settled() {
        let call = |id: &str| ToolCall {
            id: id.into(),
            name: "read_text".into(),
            arguments: Json::object([]),
            problem: None,
        };
        let mut half = vec![
            Turn::person("read them"),
            Turn::Model {
                text: String::new(),
                calls: vec![call("c1"), call("c2")],
                raw: None,
            },
            Turn::Results {
                results: vec![ToolResult::said("c1", "page one")],
            },
        ];
        settle_dangling_calls(&mut half);
        assert_eq!(half.len(), 3, "{half:?}");
        let Turn::Results { results } = &half[2] else {
            panic!("the results stay one round: {half:?}");
        };
        assert_eq!(results.len(), 2);
        assert_eq!(results[1].call_id, "c2");
        assert!(results[1].is_error);
        assert!(results[1].text.contains("Do not retry"));

        let mut none = vec![
            Turn::person("read them"),
            Turn::Model {
                text: String::new(),
                calls: vec![call("c1")],
                raw: None,
            },
        ];
        settle_dangling_calls(&mut none);
        assert_eq!(none.len(), 3);
        assert!(matches!(none[2], Turn::Results { .. }));

        let mut whole = vec![
            Turn::person("read them"),
            Turn::Model {
                text: String::new(),
                calls: vec![call("c1")],
                raw: None,
            },
            Turn::Results {
                results: vec![ToolResult::said("c1", "page one")],
            },
        ];
        let before = whole.clone();
        settle_dangling_calls(&mut whole);
        assert_eq!(whole, before);
    }

    #[test]
    fn a_turn_of_results_is_the_persons_side_and_has_no_words() {
        let results = Turn::Results {
            results: vec![ToolResult::said("c1", "page one")],
        };
        assert_eq!(results.said(), Said::Person);
        assert_eq!(results.text(), "");
        assert_eq!(Turn::person("hi").said(), Said::Person);
        assert_eq!(Turn::model("hello").said(), Said::Model);
        assert_eq!(Turn::model("hello").text(), "hello");
        assert!(Turn::model("hello").calls().is_empty());
    }

    #[test]
    fn a_local_server_asking_for_a_tool_is_read_end_to_end() {
        use std::net::TcpListener;
        for arguments in [r#""{\"block\":\"p2-b3\"}""#, r#"{"block":"p2-b3"}"#] {
            let body = format!(
                r#"{{"choices":[{{"finish_reason":"tool_calls","message":{{"role":"assistant","content":"","reasoning":"I should read it.","tool_calls":[{{"id":"call_1","index":0,"type":"function","function":{{"name":"replace_text","arguments":{arguments}}}}}]}}}}]}}"#
            );
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let server = thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0_u8; 8192];
                let _ = stream.read(&mut request).unwrap();
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            });
            let connection = Connection {
                provider: Provider::Ollama,
                base_url: format!("http://127.0.0.1:{}/v1", address.port()),
                model: "local-model".into(),
                api_key: String::new(),
                effort: Effort::Low,
            };
            let reply = connection
                .converse_with(
                    &[Turn::person("change it")],
                    None,
                    &one_tool(),
                    Some("you are helping"),
                    &AtomicBool::new(false),
                )
                .unwrap();
            server.join().unwrap();
            assert_eq!(reply.calls.len(), 1, "{reply:?}");
            assert_eq!(reply.calls[0].id, "call_1");
            assert_eq!(
                reply.calls[0].arguments.get("block").and_then(Json::as_str),
                Some("p2-b3"),
                "{arguments}"
            );
            assert!(reply.text.is_empty());
            assert!(reply.raw.is_some(), "the model's own message is kept");
        }
    }

    #[test]
    #[ignore = "needs Ollama running on this machine"]
    fn a_real_local_model_asks_for_a_tool() {
        let connection = Connection {
            provider: Provider::Ollama,
            base_url: "http://127.0.0.1:11434/v1".into(),
            model: "qwen3.5:latest".into(),
            api_key: String::new(),
            effort: Effort::Low,
        };
        let tools = crate::tools::offered_to_a_window();
        let reply = connection
            .converse_with(
                &[Turn::person("Read page 1 of the document.")],
                None,
                &tools,
                Some(&crate::tools::window_instructions(
                    &crate::tools::DocumentBrief {
                        file_name: "report.pdf".into(),
                        title: "Report".into(),
                        pages: 3,
                    },
                )),
                &AtomicBool::new(false),
            )
            .unwrap();
        assert_eq!(
            reply.calls.first().map(|call| call.name.as_str()),
            Some("read_text"),
            "{reply:?}"
        );
    }

    fn streamed(provider: Provider, body: &str) -> (Reply, Vec<String>) {
        let mut gathering = Gathering::new(provider.wire());
        let mut partials = Vec::new();
        for line in body.lines() {
            if gathering.line(line)
                && !gathering.said().is_empty()
                && partials.last().map(String::as_str) != Some(gathering.said())
            {
                partials.push(gathering.said().to_owned());
            }
        }
        let whole = gathering
            .outcome()
            .expect("the stream ended as it should")
            .expect("the events rebuild an answer");
        (answer(provider, &whole).expect("an answer"), partials)
    }

    #[test]
    fn asking_for_no_thinking_is_not_the_same_as_saying_nothing() {
        let (chat, _) = reasoning_members(Provider::Ollama, "qwen3.5:latest", Effort::None);
        assert_eq!(
            chat,
            vec![("reasoning_effort", Json::text("none"))],
            "{chat:?}"
        );
        let (claude, _) = reasoning_members(Provider::Anthropic, "claude-opus-5", Effort::None);
        assert_eq!(
            claude,
            vec![("thinking", Json::object([("type", Json::text("disabled"))]))],
            "{claude:?}"
        );
        let (openai, _) = reasoning_members(Provider::OpenAi, "gpt-5", Effort::None);
        assert_eq!(
            openai,
            vec![(
                "reasoning",
                Json::object([("effort", Json::text("minimal"))])
            )],
            "{openai:?}"
        );
        for provider in [Provider::Ollama, Provider::Anthropic, Provider::OpenAi] {
            let (nothing, _) = reasoning_members(provider, "a-model", Effort::Off);
            assert!(nothing.is_empty(), "{provider:?} {nothing:?}");
        }
        assert_eq!(Effort::parse(Effort::None.as_str()), Some(Effort::None));
    }

    #[test]
    fn a_model_that_is_still_thinking_says_so_and_says_nothing_else() {
        let each = [
            (
                Provider::Anthropic,
                vec![
                    r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}"#,
                    r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"first I"}}"#,
                    r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":" read it"}}"#,
                ],
            ),
            (
                Provider::OpenAi,
                vec![
                    r#"data: {"type":"response.reasoning_summary_text.delta","delta":"first I"}"#,
                    r#"data: {"type":"response.reasoning_summary_text.delta","delta":" read it"}"#,
                ],
            ),
            (
                Provider::Ollama,
                vec![
                    r#"data: {"choices":[{"delta":{"content":"","reasoning":"first I"}}]}"#,
                    r#"data: {"choices":[{"delta":{"content":"","reasoning":" read it"}}]}"#,
                ],
            ),
        ];
        for (provider, body) in each {
            let mut gathering = Gathering::new(provider.wire());
            let mut grew = 0;
            for line in &body {
                if gathering.line(line) {
                    grew += 1;
                }
            }
            assert_eq!(gathering.thinking(), "first I read it", "{provider:?}");
            assert!(gathering.said().is_empty(), "{provider:?}");
            assert_eq!(grew, 2, "{provider:?}: every piece of thinking is progress");
        }
    }

    #[test]
    fn a_streamed_answer_is_the_same_answer_as_a_whole_one() {
        let stream = "\
event: message_start
data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"role\":\"assistant\",\"content\":[]}}

event: content_block_start
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\",\"signature\":\"\"}}

data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"I should read it.\"}}

data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"sig-abc\"}}

data: {\"type\":\"content_block_stop\",\"index\":0}

data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}

data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"Reading \"}}

data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"page one.\"}}

data: {\"type\":\"content_block_stop\",\"index\":1}

data: {\"type\":\"content_block_start\",\"index\":2,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"read_text\",\"input\":{}}}

data: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"page\\\":\"}}

data: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"1}\"}}

data: {\"type\":\"content_block_stop\",\"index\":2}

data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"}}

data: {\"type\":\"message_stop\"}
";
        let whole = r#"{"content":[{"type":"thinking","thinking":"I should read it.","signature":"sig-abc"},{"type":"text","text":"Reading page one."},{"type":"tool_use","id":"toolu_1","name":"read_text","input":{"page":1}}],"stop_reason":"tool_use"}"#;
        let (reply, partials) = streamed(Provider::Anthropic, stream);
        let at_once = answer(Provider::Anthropic, &Json::parse(whole).unwrap()).unwrap();
        assert_eq!(reply, at_once, "{reply:?}");
        assert_eq!(partials, vec!["Reading ", "Reading page one."]);
        assert_eq!(reply.calls[0].arguments.write(), r#"{"page":1}"#);

        let items = r#"{"type":"reasoning","id":"rs_1","encrypted_content":"abc"},{"type":"message","content":[{"type":"output_text","text":"Reading page one."}]},{"type":"function_call","call_id":"call_1","name":"read_text","arguments":"{\"page\":1}"}"#;
        let stream = "\
data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"reasoning\",\"id\":\"rs_1\",\"encrypted_content\":\"abc\"}}
data: {\"type\":\"response.output_text.delta\",\"delta\":\"Reading \"}
data: {\"type\":\"response.output_text.delta\",\"delta\":\"page one.\"}
data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"Reading page one.\"}]}}
data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"read_text\",\"arguments\":\"{\\\"page\\\":1}\"}}
";
        let whole = format!(r#"{{"status":"completed","output":[{items}]}}"#);
        let at_once = answer(Provider::OpenAi, &Json::parse(&whole).unwrap()).unwrap();
        let bare_end =
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n";
        let (reply, partials) = streamed(Provider::OpenAi, &format!("{stream}{bare_end}"));
        assert_eq!(reply, at_once, "{reply:?}");
        assert_eq!(partials, vec!["Reading ", "Reading page one."]);
        let finished = format!(
            "{stream}data: {{\"type\":\"response.completed\",\"response\":{whole}}}\ndata: [DONE]\n"
        );
        let (reply, _) = streamed(Provider::OpenAi, &finished);
        assert_eq!(reply, at_once, "{reply:?}");

        let stream = "\
data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"\"}}]}
data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning\":\"I should read it.\"}}]}
data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Reading \"}}]}
data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"page one.\"}}]}
data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"read_text\",\"arguments\":\"{\\\"page\\\":\"}}]}}]}
data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"1}\"}}]}}]}
data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}
data: [DONE]
";
        let whole = r#"{"choices":[{"finish_reason":"tool_calls","message":{"role":"assistant","content":"Reading page one.","reasoning":"I should read it.","tool_calls":[{"id":"call_1","type":"function","function":{"name":"read_text","arguments":"{\"page\":1}"}}]}}]}"#;
        let (reply, partials) = streamed(Provider::Ollama, stream);
        let at_once = answer(Provider::Ollama, &Json::parse(whole).unwrap()).unwrap();
        assert_eq!(reply, at_once, "{reply:?}");
        assert_eq!(partials, vec!["Reading ", "Reading page one."]);
        assert_eq!(reply.calls[0].name, "read_text");
    }

    #[test]
    fn a_tool_call_that_arrived_signed_is_sent_back_signed() {
        let stream = "\
data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"extra_content\":{\"google\":{\"thought_signature\":\"sig-xyz\"}},\"function\":{\"name\":\"read_text\",\"arguments\":\"{\\\"first\\\":1,\\\"last\\\":1}\"}}]}}]}
data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}
data: [DONE]
";
        let (reply, _) = streamed(Provider::Gemini, stream);
        let raw = reply.raw.expect("the model's own message");
        let signed = raw.items[0]
            .get("tool_calls")
            .and_then(Json::as_list)
            .and_then(<[Json]>::first)
            .and_then(|call| call.get("extra_content"))
            .map(Json::write);
        assert_eq!(
            signed.as_deref(),
            Some(r#"{"google":{"thought_signature":"sig-xyz"}}"#),
            "the signature Google sent is in the message sent back"
        );
        assert_eq!(reply.calls[0].name, "read_text");
        assert_eq!(reply.calls[0].id, "call_1");

        let stream = "\
data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"ok\"}}]}
data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"extra_content\":{\"google\":{\"thought_signature\":\"sig-plain\"}}},\"finish_reason\":\"stop\"}]}
data: [DONE]
";
        let (reply, _) = streamed(Provider::Gemini, stream);
        let raw = reply.raw.expect("the model's own message");
        assert_eq!(
            raw.items[0]
                .get("extra_content")
                .map(Json::write)
                .as_deref(),
            Some(r#"{"google":{"thought_signature":"sig-plain"}}"#),
            "an answer with no tool call keeps its signature too"
        );
        assert_eq!(reply.text, "ok");
    }

    #[test]
    fn a_provider_that_says_how_long_to_wait_is_read() {
        let google = r#"{"error":{"code":429,"message":"You exceeded your current quota. \n* Quota exceeded for metric: generate_content_free_tier_requests, limit: 20, model: gemini-3.6-flash\nPlease retry in 18.4395459245s.","status":"RESOURCE_EXHAUSTED"}}"#;
        let groq = "Rate limit reached for model `qwen/qwen3.8-27b` in organization `org_1` \
                    service tier `on_demand` on input tokens per minute (ITPM): Limit 7000, \
                    Used 6026, Requested 4769. Please try again in 32.5285714286s.";
        assert_eq!(
            wait_asked_for(google),
            Some(Duration::from_secs_f64(19.439_545_924_5))
        );
        assert_eq!(
            wait_asked_for(groq),
            Some(Duration::from_secs_f64(33.528_571_428_6))
        );
        assert_eq!(
            wait_asked_for(r#"{"error":{"message":"invalid api key"}}"#),
            None
        );
        assert_eq!(
            wait_asked_for("The model refused to answer in 3 sentences"),
            None,
            "a refusal that happens to say \u{201c}in 3 s\u{201d} is still a refusal"
        );
        let hour = "Rate limit reached. Please try again in 3600s.";
        assert!(wait_asked_for(hour).is_some_and(|wait| wait > MOST_WAIT));
    }

    #[test]
    fn a_wait_said_in_milliseconds_or_minutes_is_read() {
        let near = |said: &str, seconds: f64| {
            let wait = wait_asked_for(said).map(|wait| wait.as_secs_f64());
            assert!(
                wait.is_some_and(|wait| (wait - (seconds + 1.0)).abs() < 1e-6),
                "{said}: {wait:?}"
            );
        };
        near("Rate limit reached. Please try again in 820ms.", 0.82);
        near("Rate limit reached. Please try again in 1m30.5s.", 90.5);
        near("Rate limit reached. Please try again in 2 minutes.", 120.0);
        near("Rate limit reached. Please try again in 5 seconds.", 5.0);
        assert_eq!(
            wait_asked_for("Rate limit reached in 3 sentences, in 4 parts."),
            None
        );
    }

    #[test]
    fn what_a_server_says_in_its_headers_is_read() {
        let refused =
            "HTTP/1.1 100 Continue\r\n\r\nHTTP/2 429 \r\nRetry-After: 7\r\nx-other: 1\r\n\r\n";
        let seen = headers_seen(refused);
        assert_eq!(seen.status, Some(429));
        assert_eq!(seen.retry_after, Some(Duration::from_secs(7)));
        let precise = headers_seen(
            "HTTP/1.1 429 Too Many Requests\r\nretry-after: 7\r\nRetry-After-Ms: 250\r\n\r\n",
        );
        assert_eq!(precise.retry_after, Some(Duration::from_millis(250)));
        let by_date = headers_seen(
            "HTTP/1.1 503 Service Unavailable\r\nRetry-After: Wed, 21 Oct 2026 07:28:00 GMT\r\n\r\n",
        );
        assert_eq!((by_date.status, by_date.retry_after), (Some(503), None));
        let new_block = headers_seen(
            "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 9\r\n\r\nHTTP/1.1 200 OK\r\n\r\n",
        );
        assert_eq!((new_block.status, new_block.retry_after), (Some(200), None));
        assert_eq!(headers_seen("").status, None);
        assert_eq!(headers_seen("Retry-After: -3\r\n").retry_after, None);
        assert_eq!(headers_seen("Retry-After: 1e99\r\n").retry_after, None);
    }

    #[test]
    fn an_overloaded_provider_is_asked_again() {
        let cancel = AtomicBool::new(false);
        let busy = "This model is currently experiencing high demand. Please try again later.";
        let mut tries = 0;
        let answer = again_after(&cancel, &QUICK, || {
            tries += 1;
            if tries < 3 {
                Err(Failure::of_the_answer(ConnectError::Http(busy.to_owned())))
            } else {
                Ok("the answer".to_owned())
            }
        });
        assert_eq!(answer.unwrap(), "the answer");
        assert_eq!(tries, 3);
        let mut tries = 0;
        let answer = again_after(&cancel, &QUICK, || {
            tries += 1;
            Err::<String, _>(Failure::of_the_answer(ConnectError::Http(
                "You exceeded your current quota. Please try again later.".to_owned(),
            )))
        });
        assert!(answer.is_err());
        assert_eq!(tries, 1);
        let mut tries = 0;
        let answer = again_after(&cancel, &QUICK, || {
            tries += 1;
            Err::<String, _>(Failure::of_the_answer(ConnectError::Http(busy.to_owned())))
        });
        assert!(matches!(answer, Err(ConnectError::Http(_))));
        assert_eq!(
            tries,
            QUICK.len() + 1,
            "asked again as often as allowed, then told"
        );
        assert!(!overloaded_for_now("invalid api key"));
        assert_eq!(
            error_text(r#"[{"error": {"code": 503, "message": "busy", "status": "UNAVAILABLE"}}]"#),
            "busy"
        );
    }

    #[test]
    fn a_request_told_to_wait_is_made_again() {
        let cancel = AtomicBool::new(false);
        let told_to_wait = |wait: Duration| Failure {
            error: ConnectError::Http("Rate limit reached.".to_owned()),
            transient: true,
            wait: Some(wait),
            tries: RETRIES,
        };
        let mut tries = 0;
        let started = Instant::now();
        let answer = again_after(&cancel, &QUICK, || {
            tries += 1;
            if tries < 3 {
                Err(told_to_wait(Duration::from_millis(60)))
            } else {
                Ok("the answer")
            }
        });
        assert_eq!(answer.unwrap(), "the answer");
        assert_eq!(tries, 3);
        assert!(
            started.elapsed() >= Duration::from_millis(120),
            "the wait the server asked for is the wait made"
        );

        let mut tries = 0;
        let answer = again_after(&cancel, &QUICK, || {
            tries += 1;
            Err::<(), _>(Failure::from(ConnectError::Http(
                "invalid api key".to_owned(),
            )))
        });
        assert!(matches!(answer, Err(ConnectError::Http(_))));
        assert_eq!(tries, 1, "a refusal is not tried again");

        let mut tries = 0;
        let answer = again_after(&cancel, &QUICK, || {
            tries += 1;
            Err::<(), _>(told_to_wait(Duration::from_secs(3600)))
        });
        assert!(matches!(answer, Err(ConnectError::Http(_))));
        assert_eq!(tries, 1, "an hour is not waited for");

        let mut tries = 0;
        let answer = again_after(&cancel, &QUICK, || {
            tries += 1;
            Err::<(), _>(Failure {
                tries: 1,
                ..told_to_wait(Duration::from_millis(1))
            })
        });
        assert!(answer.is_err());
        assert_eq!(tries, 2, "a failure can be worth only one more try");

        let stopped = AtomicBool::new(true);
        let mut tries = 0;
        let answer = again_after(&stopped, &QUICK, || {
            tries += 1;
            Err::<(), _>(told_to_wait(Duration::from_secs(30)))
        });
        assert_eq!(answer, Err(ConnectError::Cancelled));
        assert_eq!(tries, 1);
    }

    #[test]
    fn a_local_server_that_streams_is_seen_as_it_writes() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 8192];
            let count = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..count]);
            assert!(request.contains(r#""stream":true"#), "{request}");
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
            stream.flush().unwrap();
            for piece in ["Reading ", "page ", "one."] {
                write!(
                    stream,
                    "data: {{\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"{piece}\"}}}}]}}\n\n"
                )
                .unwrap();
                stream.flush().unwrap();
                thread::sleep(Duration::from_millis(60));
            }
            write!(
                stream,
                "data: {{\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":\"stop\"}}]}}\n\ndata: [DONE]\n\n"
            )
            .unwrap();
            stream.flush().unwrap();
        });
        let connection = Connection {
            provider: Provider::Ollama,
            base_url: format!("http://127.0.0.1:{}/v1", address.port()),
            model: "local-model".into(),
            api_key: String::new(),
            effort: Effort::Off,
        };
        let mut partials: Vec<String> = Vec::new();
        let reply = connection
            .converse_streaming(
                &[Turn::person("read it")],
                None,
                &[],
                None,
                &AtomicBool::new(false),
                &mut |far| partials.push(far.said.to_owned()),
            )
            .unwrap();
        server.join().unwrap();
        assert_eq!(reply.text, "Reading page one.");
        assert!(!reply.cut_short);
        assert_eq!(
            partials,
            vec!["Reading ", "Reading page ", "Reading page one."],
            "the answer is seen growing"
        );
    }

    #[test]
    fn a_server_that_does_not_stream_is_read_as_one_body() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 8192];
            let _ = stream.read(&mut request).unwrap();
            let body = r#"{"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":"all at once"}}]}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });
        let connection = Connection {
            provider: Provider::Ollama,
            base_url: format!("http://127.0.0.1:{}/v1", address.port()),
            model: "local-model".into(),
            api_key: String::new(),
            effort: Effort::Off,
        };
        let mut partials = 0_usize;
        let reply = connection
            .converse_streaming(
                &[Turn::person("hello")],
                None,
                &[],
                None,
                &AtomicBool::new(false),
                &mut |_| partials += 1,
            )
            .unwrap();
        server.join().unwrap();
        assert_eq!(reply.text, "all at once");
        assert_eq!(partials, 0);
    }

    #[test]
    #[ignore = "needs Ollama running on this machine"]
    fn a_real_local_model_streams_its_answer() {
        let connection = Connection {
            provider: Provider::Ollama,
            base_url: "http://127.0.0.1:11434/v1".into(),
            model: "qwen3.5:latest".into(),
            api_key: String::new(),
            effort: Effort::Low,
        };
        let mut partials: Vec<String> = Vec::new();
        let reply = connection
            .converse_streaming(
                &[Turn::person("Read page 1 of the document.")],
                None,
                &crate::tools::offered_to_a_window(),
                Some(&crate::tools::window_instructions(
                    &crate::tools::DocumentBrief {
                        file_name: "report.pdf".into(),
                        title: "Report".into(),
                        pages: 3,
                    },
                )),
                &AtomicBool::new(false),
                &mut |far| partials.push(far.said.to_owned()),
            )
            .unwrap();
        println!("partials seen: {}", partials.len());
        println!("reply: {reply:?}");
        assert!(!reply.calls.is_empty() || !reply.text.is_empty());
    }

    #[test]
    fn a_long_document_has_room_to_be_written_in_one_reply() {
        let body = |provider, model: &str, effort| {
            let mut connection = of(provider);
            connection.model = model.to_owned();
            connection.effort = effort;
            let spoken = connection
                .spoken(&[Turn::person("write it")], None)
                .unwrap();
            Json::parse(&connection.payload(&spoken, &one_tool(), None).write()).unwrap()
        };
        let room = |provider, model: &str, effort| {
            body(provider, model, effort)
                .get("max_tokens")
                .and_then(Json::as_count)
        };
        for model in [
            "claude-sonnet-4-5-20250929",
            "claude-sonnet-5-5",
            "claude-opus-4-1",
            "a-name-of-its-own",
        ] {
            assert_eq!(
                room(Provider::Anthropic, model, Effort::Off),
                Some(16_000),
                "{model}"
            );
        }
        assert_eq!(
            room(
                Provider::Anthropic,
                "claude-3-7-sonnet-20250219",
                Effort::Off
            ),
            Some(16_000)
        );
        assert_eq!(
            room(
                Provider::Anthropic,
                "claude-3-5-sonnet-20241022",
                Effort::Off
            ),
            Some(8_192),
            "a model that cannot write more than that is not asked to"
        );
        assert_eq!(
            room(Provider::Anthropic, "claude-3-opus-20240229", Effort::Off),
            Some(4_096)
        );
        assert_eq!(
            room(
                Provider::Anthropic,
                "claude-opus-4-1-20250805",
                Effort::High
            ),
            Some(32_000),
            "what Claude Opus 4.1 can write at most"
        );
        assert_eq!(
            room(Provider::Anthropic, "claude-sonnet-5-5", Effort::High),
            Some(32_000)
        );
        for model in [
            "claude-sonnet-4-5-20250929",
            "claude-opus-4-1-20250805",
            "claude-3-7-sonnet-20250219",
            "claude-haiku-4-5-20251001",
        ] {
            for effort in [Effort::Low, Effort::Medium, Effort::High] {
                let asked = body(Provider::Anthropic, model, effort);
                let budget = asked
                    .get("thinking")
                    .and_then(|thinking| thinking.get("budget_tokens"))
                    .and_then(Json::as_count)
                    .unwrap();
                let room = asked.get("max_tokens").and_then(Json::as_count).unwrap();
                assert!(
                    room >= budget + 16_000 || room >= 32_000,
                    "{model} {effort:?}: room for the answer beyond {budget}: {room}"
                );
            }
        }
        for provider in [
            Provider::OpenAi,
            Provider::Ollama,
            Provider::LmStudio,
            Provider::Gemini,
            Provider::Custom,
        ] {
            let written = body(provider, "a-model", Effort::Medium).write();
            for limit in ["max_tokens", "max_output_tokens", "max_completion_tokens"] {
                assert!(
                    !written.contains(limit),
                    "{provider:?} keeps its own default, which no model of it cuts short: {written}"
                );
            }
        }
    }

    #[test]
    fn openai_is_always_asked_to_hand_its_reasoning_back_encrypted() {
        let turns = [Turn::person("hello")];
        for tools in [Vec::new(), one_tool()] {
            let written = of(Provider::OpenAi)
                .payload(
                    &of(Provider::OpenAi).spoken(&turns, None).unwrap(),
                    &tools,
                    None,
                )
                .write();
            assert!(
                written.contains(r#""include":["reasoning.encrypted_content"]"#),
                "{written}"
            );
            assert!(written.contains(r#""store":false"#), "{written}");
        }
    }

    #[test]
    fn reasoning_that_came_without_its_encrypted_part_is_not_sent_back() {
        let raw = |reasoning: &str| {
            Raw {
            provider: Provider::OpenAi,
            items: vec![
                Json::parse(reasoning).unwrap(),
                Json::parse(r#"{"type":"message","id":"msg_1","content":[{"type":"output_text","text":"Hi"}]}"#).unwrap(),
                Json::parse(r#"{"type":"function_call","id":"fc_1","call_id":"c1","name":"read_text","arguments":"{}"}"#).unwrap(),
            ],
        }
        };
        let sent = |raw: Raw| {
            let turns = [
                Turn::person("read it"),
                Turn::Model {
                    text: "Hi".into(),
                    calls: vec![ToolCall::asked("c1", "read_text", Json::object([]))],
                    raw: Some(raw),
                },
                Turn::Results {
                    results: vec![ToolResult::said("c1", "page one")],
                },
            ];
            of(Provider::OpenAi)
                .payload(
                    &of(Provider::OpenAi).spoken(&turns, None).unwrap(),
                    &one_tool(),
                    None,
                )
                .write()
        };
        let bare = sent(raw(r#"{"type":"reasoning","id":"rs_1","summary":[]}"#));
        for gone in ["rs_1", "msg_1", "fc_1"] {
            assert!(
                !bare.contains(gone),
                "{gone} would be looked up and not found: {bare}"
            );
        }
        assert!(bare.contains(r#""call_id":"c1""#), "{bare}");
        assert!(bare.contains(r#""text":"Hi""#), "{bare}");
        let whole = sent(raw(
            r#"{"type":"reasoning","id":"rs_2","summary":[],"encrypted_content":"abc"}"#,
        ));
        for kept in ["rs_2", "msg_1", "fc_1", "abc"] {
            assert!(whole.contains(kept), "{kept}: {whole}");
        }
    }

    #[test]
    fn claude_is_still_told_of_the_tools_its_history_used_when_none_are_offered() {
        let turns = [
            Turn::person("read it"),
            Turn::Model {
                text: String::new(),
                calls: vec![
                    ToolCall::asked("c1", "read_text", Json::object([])),
                    ToolCall::asked("c2", "read_text", Json::object([])),
                    ToolCall::asked("c3", "find_text", Json::object([])),
                ],
                raw: None,
            },
            Turn::Results {
                results: vec![
                    ToolResult::said("c1", "page one"),
                    ToolResult::said("c2", "page two"),
                    ToolResult::said("c3", "found"),
                ],
            },
            Turn::person("now just talk"),
        ];
        let connection = of(Provider::Anthropic);
        let body = |tools: &[ToolOffer]| {
            Json::parse(
                &connection
                    .payload(&connection.spoken(&turns, None).unwrap(), tools, None)
                    .write(),
            )
            .unwrap()
        };
        let none = body(&[]);
        let named: Vec<String> = none
            .get("tools")
            .and_then(Json::as_list)
            .expect("the tools its history uses are defined")
            .iter()
            .filter_map(|tool| tool.get("name").and_then(Json::as_str))
            .map(str::to_owned)
            .collect();
        assert_eq!(named, vec!["find_text", "read_text"]);
        assert_eq!(
            none.get("tool_choice").map(Json::write).as_deref(),
            Some(r#"{"type":"none"}"#),
            "and it is told not to use them"
        );
        assert!(none.write().contains(r#""input_schema":{"type":"object"}"#));

        let offered = body(&one_tool());
        assert_eq!(
            offered
                .get("tools")
                .and_then(Json::as_list)
                .map(<[Json]>::len),
            Some(1)
        );
        assert!(offered.get("tool_choice").is_none(), "{}", offered.write());

        let plain = connection
            .payload(
                &connection
                    .spoken(
                        &[
                            Turn::person("hi"),
                            Turn::model("hello"),
                            Turn::person("again"),
                        ],
                        None,
                    )
                    .unwrap(),
                &[],
                None,
            )
            .write();
        assert!(!plain.contains("tool"), "{plain}");
    }

    #[test]
    fn no_empty_text_is_sent_to_claude_and_none_is_sent_to_the_others() {
        let turns = [
            Turn::person_with(
                "",
                vec![Attachment::image("shot.png", "image/png", vec![1, 2, 3])],
            ),
            Turn::model(""),
            Turn::model("  \n"),
            Turn::Model {
                text: "Hello".into(),
                calls: Vec::new(),
                raw: Some(Raw {
                    provider: Provider::Anthropic,
                    items: vec![
                        Json::parse(r#"{"type":"thinking","thinking":"hm","signature":"s"}"#)
                            .unwrap(),
                        Json::parse(r#"{"type":"text","text":""}"#).unwrap(),
                        Json::parse(r#"{"type":"text","text":"\n\n"}"#).unwrap(),
                        Json::parse(r#"{"type":"text","text":"Hello"}"#).unwrap(),
                    ],
                }),
            },
            Turn::person("next"),
        ];
        let body = |provider| {
            of(provider)
                .payload(&of(provider).spoken(&turns, None).unwrap(), &[], None)
                .write()
        };
        let claude = Json::parse(&body(Provider::Anthropic)).unwrap();
        let messages = claude.get("messages").and_then(Json::as_list).unwrap();
        assert_eq!(messages.len(), 3, "{}", claude.write());
        assert_eq!(
            messages[0].write(),
            r#"{"content":[{"source":{"data":"AQID","media_type":"image/png","type":"base64"},"type":"image"}],"role":"user"}"#
        );
        assert_eq!(
            messages[1].write(),
            r#"{"content":[{"signature":"s","thinking":"hm","type":"thinking"},{"text":"Hello","type":"text"}],"role":"assistant"}"#
        );
        for provider in [Provider::OpenAi, Provider::Ollama] {
            let written = body(provider);
            assert!(!written.contains(r#""text":"""#), "{provider:?}: {written}");
            assert!(
                !written.contains(r#""content":"""#),
                "{provider:?}: {written}"
            );
        }
    }

    #[test]
    fn an_attachment_that_was_not_kept_is_said_so_and_no_empty_picture_is_sent() {
        let turns = [Turn::person_with(
            "what was in it?",
            vec![
                Attachment::image("cover.png", "image/png", Vec::new()),
                Attachment::text("notes.txt", ""),
                Attachment::image("kept.png", "image/png", vec![1, 2, 3]),
            ],
        )];
        for provider in [Provider::Anthropic, Provider::OpenAi, Provider::Ollama] {
            let written = of(provider)
                .payload(&of(provider).spoken(&turns, None).unwrap(), &[], None)
                .write();
            assert_eq!(
                written.matches("was not kept").count(),
                2,
                "{provider:?}: {written}"
            );
            assert!(written.contains("cover.png"), "{written}");
            assert!(!written.contains(r#""data":"""#), "{written}");
            assert!(!written.contains("base64,\""), "{written}");
            assert_eq!(written.matches("AQID").count(), 1, "{written}");
        }
        assert!(
            of(Provider::Ollama)
                .spoken(
                    &[Turn::person_with(
                        "",
                        vec![Attachment::image("a.png", "image/png", Vec::new())]
                    )],
                    None
                )
                .is_ok(),
            "a picture that was not kept is still something that was said"
        );
    }

    fn finished_stream(provider: Provider, body: &str) -> Result<Reply, Failure> {
        let mut gathering = Gathering::new(provider.wire());
        for line in body.lines() {
            gathering.line(line);
        }
        let root = gathering
            .outcome()?
            .ok_or_else(|| Failure::from(ConnectError::Protocol("no events".to_owned())))?;
        answer(provider, &root).map_err(Failure::of_the_answer)
    }

    fn claude_says(events: &[&str]) -> String {
        events.iter().fold(String::new(), |mut said, event| {
            let _ = writeln!(said, "data: {event}");
            said
        })
    }

    const CLAUDE_TEXT: &str =
        r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#;
    const CLAUDE_HALF: &str = r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Half of "}}"#;
    const CLAUDE_STOP: &str = r#"{"type":"content_block_stop","index":0}"#;

    #[test]
    fn a_stream_must_reach_its_own_end_to_be_an_answer() {
        let without_its_end = [
            (
                Provider::Anthropic,
                claude_says(&[CLAUDE_TEXT, CLAUDE_HALF, CLAUDE_STOP]),
            ),
            (
                Provider::OpenAi,
                claude_says(&[
                    r#"{"type":"response.output_text.delta","delta":"Half of "}"#,
                    r#"{"type":"response.output_item.done","item":{"type":"message","content":[{"type":"output_text","text":"Half of "}]}}"#,
                ]),
            ),
            (
                Provider::Ollama,
                claude_says(&[r#"{"choices":[{"index":0,"delta":{"content":"Half of "}}]}"#]),
            ),
        ];
        for (provider, stream) in without_its_end {
            let failure = finished_stream(provider, &stream).unwrap_err();
            assert!(
                matches!(&failure.error, ConnectError::Protocol(said) if said.contains("stopped before")),
                "{provider:?}: {failure:?}"
            );
            assert!(
                failure.transient,
                "{provider:?}: a cut connection is worth asking again"
            );
        }
        let ended = [
            (
                Provider::Anthropic,
                claude_says(&[
                    CLAUDE_TEXT,
                    CLAUDE_HALF,
                    CLAUDE_STOP,
                    r#"{"type":"message_stop"}"#,
                ]),
            ),
            (
                Provider::Anthropic,
                claude_says(&[
                    CLAUDE_TEXT,
                    CLAUDE_HALF,
                    CLAUDE_STOP,
                    r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"}}"#,
                ]),
            ),
            (
                Provider::OpenAi,
                claude_says(&[
                    r#"{"type":"response.output_text.delta","delta":"Half of "}"#,
                    r#"{"type":"response.completed","response":{"status":"completed"}}"#,
                ]),
            ),
            (
                Provider::Ollama,
                claude_says(&[r#"{"choices":[{"index":0,"delta":{"content":"Half of "}}]}"#])
                    + "data: [DONE]\n",
            ),
            (
                Provider::Ollama,
                claude_says(&[
                    r#"{"choices":[{"index":0,"delta":{"content":"Half of "}}]}"#,
                    r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
                ]),
            ),
        ];
        for (provider, stream) in ended {
            let reply = finished_stream(provider, &stream).unwrap();
            assert_eq!(reply.text, "Half of ", "{provider:?}");
            assert!(!reply.cut_short, "{provider:?}");
        }
    }

    #[test]
    fn a_provider_error_inside_a_stream_is_told_in_the_providers_words() {
        let told = |provider, events: &[&str]| {
            finished_stream(provider, &claude_says(events)).unwrap_err()
        };
        let overloaded = told(
            Provider::Anthropic,
            &[
                CLAUDE_TEXT,
                CLAUDE_HALF,
                r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
            ],
        );
        assert_eq!(
            overloaded.error,
            ConnectError::Http("Overloaded".to_owned())
        );
        assert!(overloaded.transient, "an overloaded service is asked again");

        let too_long = told(
            Provider::Anthropic,
            &[
                r#"{"type":"error","error":{"type":"invalid_request_error","message":"prompt is too long: 250000 tokens > 200000 maximum"}}"#,
            ],
        );
        assert_eq!(
            too_long.error,
            ConnectError::Http("prompt is too long: 250000 tokens > 200000 maximum".to_owned())
        );
        assert!(!too_long.transient, "a request that is wrong stays wrong");

        let failed = told(
            Provider::OpenAi,
            &[
                r#"{"type":"response.output_text.delta","delta":"Half of "}"#,
                r#"{"type":"response.failed","response":{"status":"failed","error":{"code":"server_error","message":"The server had an error while processing your request"}}}"#,
            ],
        );
        assert_eq!(
            failed.error,
            ConnectError::Http("The server had an error while processing your request".to_owned())
        );
        assert!(failed.transient);

        let limited = told(
            Provider::OpenAi,
            &[
                r#"{"type":"error","code":"rate_limit_exceeded","message":"Rate limit reached for gpt. Please try again in 1.5s.","param":null}"#,
            ],
        );
        assert!(limited.transient);
        assert_eq!(limited.wait, Some(Duration::from_secs_f64(2.5)));

        let invalid = told(
            Provider::OpenAi,
            &[
                r#"{"type":"response.failed","response":{"error":{"code":"invalid_prompt","message":"Your prompt was flagged"}}}"#,
            ],
        );
        assert!(!invalid.transient, "{invalid:?}");

        let upstream = told(
            Provider::Ollama,
            &[
                r#"{"choices":[{"index":0,"delta":{"content":"Half of "}}]}"#,
                r#"{"error":{"message":"upstream rate limited","type":"upstream","code":429}}"#,
            ],
        );
        assert_eq!(
            upstream.error,
            ConnectError::Http("upstream rate limited".to_owned())
        );
        assert!(upstream.transient);
        let wrong = told(
            Provider::Ollama,
            &[r#"{"error":{"message":"model not found","code":404}}"#],
        );
        assert!(!wrong.transient);
        let bare = told(Provider::Ollama, &[r#"{"error":"the model crashed"}"#]);
        assert_eq!(
            bare.error,
            ConnectError::Http("the model crashed".to_owned())
        );
    }

    #[test]
    fn an_answer_that_ran_out_of_room_in_a_stream_is_marked_and_a_cut_off_call_is_not_run() {
        let claude = finished_stream(
            Provider::Anthropic,
            &claude_says(&[
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"I will write it."}}"#,
                r#"{"type":"content_block_stop","index":0}"#,
                r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"t1","name":"read_text","input":{}}}"#,
                r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"first\":1}"}}"#,
                r#"{"type":"content_block_stop","index":1}"#,
                r#"{"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"t2","name":"write_pages","input":{}}}"#,
                r##"{"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"markdown\":\"# Title\\n\\nThe sta"}}"##,
                r#"{"type":"message_delta","delta":{"stop_reason":"max_tokens"}}"#,
                r#"{"type":"message_stop"}"#,
            ]),
        )
        .unwrap();
        assert!(claude.cut_short);
        assert_eq!(claude.text, "I will write it.");
        assert_eq!(claude.calls.len(), 2);
        assert_eq!(
            claude.calls[0].problem, None,
            "a call that was finished is run"
        );
        assert_eq!(claude.calls[0].arguments.write(), r#"{"first":1}"#);
        let cut = &claude.calls[1];
        assert_eq!((cut.id.as_str(), cut.name.as_str()), ("t2", "write_pages"));
        assert_eq!(cut.arguments, Json::Null);
        assert!(
            cut.problem
                .as_deref()
                .is_some_and(|why| why.contains("cut off by the output limit")),
            "{cut:?}"
        );
        let sent_back = claude.raw.unwrap().items;
        assert_eq!(
            sent_back[2].get("input").map(Json::write).as_deref(),
            Some("{}")
        );
        assert_eq!(
            sent_back[1].get("input").map(Json::write).as_deref(),
            Some(r#"{"first":1}"#)
        );

        let openai = finished_stream(
            Provider::OpenAi,
            &claude_says(&[
                r##"{"type":"response.output_item.done","item":{"type":"function_call","call_id":"c1","name":"write_pages","status":"incomplete","arguments":"{\"markdown\":\"# Ti"}}"##,
                r##"{"type":"response.incomplete","response":{"status":"incomplete","incomplete_details":{"reason":"max_output_tokens"},"output":[{"type":"function_call","call_id":"c1","name":"write_pages","status":"incomplete","arguments":"{\"markdown\":\"# Ti"}]}}"##,
            ]),
        )
        .unwrap();
        assert!(openai.cut_short);
        assert!(
            openai.calls[0]
                .problem
                .as_deref()
                .is_some_and(|why| why.contains("cut off"))
        );
        assert_eq!(
            openai.raw.unwrap().items[0]
                .get("arguments")
                .and_then(Json::as_str),
            Some("{}")
        );
        let words = finished_stream(
            Provider::OpenAi,
            &claude_says(&[
                r#"{"type":"response.output_text.delta","delta":"Half"}"#,
                r#"{"type":"response.incomplete","response":{"incomplete_details":{"reason":"max_output_tokens"},"output":[{"type":"message","content":[{"type":"output_text","text":"Half"}]}]}}"#,
            ]),
        )
        .unwrap();
        assert!(words.cut_short && words.text == "Half");

        let local = finished_stream(
            Provider::Ollama,
            &claude_says(&[
                r##"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"write_pages","arguments":"{\"markdown\":\"# Ti"}}]}}]}"##,
                r#"{"choices":[{"index":0,"delta":{},"finish_reason":"length"}]}"#,
            ]),
        )
        .unwrap();
        assert!(local.cut_short);
        assert!(
            local.calls[0]
                .problem
                .as_deref()
                .is_some_and(|why| why.contains("cut off"))
        );
    }

    #[test]
    fn a_streamed_call_whose_json_is_wrong_is_a_call_with_a_problem_and_the_words_survive() {
        let reply = finished_stream(
            Provider::Anthropic,
            &claude_says(&[
                CLAUDE_TEXT,
                CLAUDE_HALF,
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"it."}}"#,
                CLAUDE_STOP,
                r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"t1","name":"read_text","input":{}}}"#,
                r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"first\": nope}"}}"#,
                r#"{"type":"content_block_stop","index":1}"#,
                r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"}}"#,
                r#"{"type":"message_stop"}"#,
            ]),
        )
        .unwrap();
        assert_eq!(reply.text, "Half of it.");
        assert!(!reply.cut_short);
        assert_eq!(reply.calls.len(), 1);
        assert_eq!(
            reply.calls[0].arguments,
            Json::Null,
            "it is not run with no arguments"
        );
        assert!(
            reply.calls[0]
                .problem
                .as_deref()
                .is_some_and(|why| why.contains("not valid JSON")),
            "{:?}",
            reply.calls[0]
        );
        assert_eq!(
            reply.raw.unwrap().items[1]
                .get("input")
                .map(Json::write)
                .as_deref(),
            Some("{}"),
            "what is sent back to the provider is valid"
        );
    }

    #[test]
    fn a_call_with_no_arguments_at_all_is_still_a_call() {
        let reply = finished_stream(
            Provider::Anthropic,
            &claude_says(&[
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"t1","name":"document_info","input":{}}}"#,
                r#"{"type":"content_block_stop","index":0}"#,
                r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"}}"#,
                r#"{"type":"message_stop"}"#,
            ]),
        )
        .unwrap();
        assert_eq!(
            reply.calls,
            vec![ToolCall::asked("t1", "document_info", Json::object([]))]
        );
    }

    #[test]
    fn a_whole_claude_answer_cut_at_a_tool_call_does_not_run_it() {
        let reply = read_reply(
            Provider::Anthropic,
            &Json::parse(
                r##"{"stop_reason":"max_tokens","content":[
{"type":"tool_use","id":"t1","name":"read_text","input":{"first":1}},
{"type":"tool_use","id":"t2","name":"write_pages","input":{"markdown":"# T"}}]}"##,
            )
            .unwrap(),
        )
        .unwrap();
        assert!(reply.cut_short);
        assert_eq!(reply.calls[0].problem, None);
        assert!(reply.calls[1].problem.is_some(), "{:?}", reply.calls[1]);
        let finished = read_reply(
            Provider::Anthropic,
            &Json::parse(
                r##"{"stop_reason":"tool_use","content":[
{"type":"tool_use","id":"t2","name":"write_pages","input":{"markdown":"# T"}}]}"##,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(finished.calls[0].problem, None);
    }

    #[test]
    fn an_error_in_the_body_of_an_answer_that_looked_fine_is_an_error() {
        let said = |provider, body: &str| answer(provider, &Json::parse(body).unwrap());
        assert_eq!(
            said(
                Provider::Ollama,
                r#"{"error":{"message":"upstream rate limited","code":429}}"#
            ),
            Err(ConnectError::Http("upstream rate limited".to_owned()))
        );
        assert_eq!(
            said(Provider::Ollama, r#"{"error":"plain words"}"#),
            Err(ConnectError::Http("plain words".to_owned()))
        );
        assert_eq!(
            said(
                Provider::OpenAi,
                r#"{"status":"failed","error":{"code":"server_error","message":"It broke"},"output":[]}"#
            ),
            Err(ConnectError::Http("It broke".to_owned()))
        );
        assert_eq!(
            said(
                Provider::OpenAi,
                r#"{"status":"failed","error":null,"output":[]}"#
            ),
            Err(ConnectError::Protocol(
                "response did not complete".to_owned()
            ))
        );
        assert!(
            said(
                Provider::OpenAi,
                r#"{"status":"completed","error":null,"output":[{"type":"message","content":[{"type":"output_text","text":"ok"}]}]}"#
            )
            .is_ok(),
            "a null error is no error"
        );
        assert_eq!(
            said(
                Provider::Anthropic,
                r#"{"stop_reason":"max_tokens","content":[]}"#
            ),
            Err(ConnectError::Protocol(
                "the answer was cut off before it said anything".to_owned()
            ))
        );
        let asked_again = Failure::of_the_answer(ConnectError::Http(
            "Rate limit reached. Please try again in 2s.".to_owned(),
        ));
        assert!(asked_again.transient);
        assert!(
            !Failure::of_the_answer(ConnectError::Http("invalid api key".to_owned())).transient
        );
    }

    #[test]
    fn chat_calls_that_arrive_in_pieces_are_kept_apart_and_together() {
        let calls_of = |chunks: &[&str]| {
            let mut gathering = Gathering::new(Wire::Chat);
            for chunk in chunks {
                gathering.line(&format!(
                    r#"data: {{"choices":[{{"index":0,"delta":{{"tool_calls":[{chunk}]}}}}]}}"#
                ));
            }
            gathering
                .line(r#"data: {"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#);
            let reply = answer(Provider::Ollama, &gathering.outcome().unwrap().unwrap()).unwrap();
            reply
                .calls
                .into_iter()
                .map(|call| {
                    (
                        call.id,
                        call.name,
                        call.arguments.write(),
                        call.problem.is_some(),
                    )
                })
                .collect::<Vec<_>>()
        };
        let call = |id: &str, name: &str, arguments: &str| {
            (id.to_owned(), name.to_owned(), arguments.to_owned(), false)
        };

        assert_eq!(
            calls_of(&[
                r#"{"index":0,"id":"a","function":{"name":"read_text","arguments":"{\"first\":1}"}}"#,
                r#"{"index":0,"id":"b","function":{"name":"read_text","arguments":"{\"first\":2}"}}"#,
            ]),
            vec![
                call("a", "read_text", r#"{"first":1}"#),
                call("b", "read_text", r#"{"first":2}"#)
            ],
            "two calls that both say they are the first are two calls"
        );
        assert_eq!(
            calls_of(&[
                r#"{"index":0,"id":"call_1","type":"function","function":{"name":"read_text","arguments":""}}"#,
                r#"{"index":0,"id":"","function":{"name":"","arguments":"{\"first\":"}}"#,
                r#"{"index":0,"function":{"arguments":"1}"}}"#,
            ]),
            vec![call("call_1", "read_text", r#"{"first":1}"#)],
            "an empty id or name later does not blank the ones that came first"
        );
        assert_eq!(
            calls_of(&[
                r#"{"id":"a","function":{"name":"read_text","arguments":"{\"first\":"}}"#,
                r#"{"function":{"arguments":"1}"}}"#,
                r#"{"id":"b","function":{"name":"find_text","arguments":"{\"text\":\"x\"}"}}"#,
            ]),
            vec![
                call("a", "read_text", r#"{"first":1}"#),
                call("b", "find_text", r#"{"text":"x"}"#)
            ],
            "with no index it is the id that starts a call"
        );
        assert_eq!(
            calls_of(&[
                r#"{"index":0,"function":{"name":"read_text","arguments":"{\"first\":1}"}}"#,
                r#"{"index":0,"function":{"name":"find_text","arguments":"{\"text\":\"x\"}"}}"#,
                r#"{"index":0,"function":{"name":"find_text","arguments":"{\"text\":\"y\"}"}}"#,
            ])
            .iter()
            .map(|(_, name, arguments, _)| (name.clone(), arguments.clone()))
            .collect::<Vec<_>>(),
            vec![
                ("read_text".to_owned(), r#"{"first":1}"#.to_owned()),
                ("find_text".to_owned(), r#"{"text":"x"}"#.to_owned()),
                ("find_text".to_owned(), r#"{"text":"y"}"#.to_owned()),
            ],
            "with no ids at all a whole call is not continued by another"
        );
        let unnamed = calls_of(&[
            r#"{"index":0,"function":{"name":"read_text","arguments":"{\"first\":1}"}}"#,
            r#"{"index":1,"function":{"name":"read_text","arguments":"{\"first\":2}"}}"#,
        ]);
        assert_eq!(unnamed.len(), 2);
        assert!(
            unnamed[0].0.starts_with("call_") && unnamed[1].0.starts_with("call_"),
            "{unnamed:?}"
        );
        assert_ne!(
            unnamed[0].0, unnamed[1].0,
            "the results are matched by these"
        );
    }

    #[test]
    fn the_tokens_each_wire_says_it_used_are_read() {
        let claude = finished_stream(
            Provider::Anthropic,
            &claude_says(&[
                r#"{"type":"message_start","message":{"id":"m","content":[],"usage":{"input_tokens":25,"cache_creation_input_tokens":100,"cache_read_input_tokens":400,"output_tokens":1}}}"#,
                CLAUDE_TEXT,
                CLAUDE_HALF,
                CLAUDE_STOP,
                r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":15}}"#,
                r#"{"type":"message_stop"}"#,
            ]),
        )
        .unwrap();
        let used = claude.usage.unwrap();
        assert_eq!(
            used,
            Usage {
                input: 25,
                output: 15,
                cache_read: 400,
                cache_write: 100
            }
        );
        assert_eq!(used.prompt_size(), 525);

        let whole = answer(
            Provider::Anthropic,
            &Json::parse(
                r#"{"content":[{"type":"text","text":"hi"}],"usage":{"input_tokens":3,"output_tokens":4}}"#,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            whole.usage,
            Some(Usage {
                input: 3,
                output: 4,
                cache_read: 0,
                cache_write: 0
            })
        );

        let openai = finished_stream(
            Provider::OpenAi,
            &claude_says(&[
                r#"{"type":"response.output_text.delta","delta":"Hi"}"#,
                r#"{"type":"response.completed","response":{"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"Hi"}]}],"usage":{"input_tokens":1000,"output_tokens":50,"input_tokens_details":{"cached_tokens":800},"output_tokens_details":{"reasoning_tokens":20}}}}"#,
            ]),
        )
        .unwrap();
        assert_eq!(
            openai.usage,
            Some(Usage {
                input: 200,
                output: 50,
                cache_read: 800,
                cache_write: 0
            }),
            "what was read from the cache is not counted as new input as well"
        );

        let local = finished_stream(
            Provider::Ollama,
            &(claude_says(&[
                r#"{"choices":[{"index":0,"delta":{"content":"Hi"}}]}"#,
                r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
                r#"{"choices":[],"usage":{"prompt_tokens":120,"completion_tokens":30,"prompt_tokens_details":{"cached_tokens":100}}}"#,
            ]) + "data: [DONE]\n"),
        )
        .unwrap();
        assert_eq!(
            local.usage,
            Some(Usage {
                input: 20,
                output: 30,
                cache_read: 100,
                cache_write: 0
            })
        );

        let silent = answer(
            Provider::Ollama,
            &Json::parse(r#"{"choices":[{"message":{"content":"Hi"}}],"usage":{}}"#).unwrap(),
        )
        .unwrap();
        assert_eq!(silent.usage, None);
        let null = answer(
            Provider::Ollama,
            &Json::parse(r#"{"choices":[{"message":{"content":"Hi"}}],"usage":null}"#).unwrap(),
        )
        .unwrap();
        assert_eq!(null.usage, None);
    }

    const FINE: &str =
        r#"{"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":"fine"}}]}"#;

    fn chat_of(words: &str) -> String {
        format!(
            "data: {{\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"{words}\"}}}}]}}\n\ndata: {{\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":\"stop\"}}]}}\n\ndata: [DONE]\n\n"
        )
    }

    #[test]
    fn a_server_that_fails_is_asked_again_and_one_that_refuses_is_not() {
        let (base, server) = serve(vec![
            http(
                "500 Internal Server Error",
                "",
                r#"{"error":{"message":"boom"}}"#,
            ),
            http("502 Bad Gateway", "", "<html>bad gateway</html>"),
            http(
                "529 Overloaded",
                "",
                r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
            ),
            http("200 OK", "", FINE),
        ]);
        let connection = talking_to(Provider::Ollama, base);
        let (told, _) = told_by(&connection, quick(false), &[Turn::person("hi")]);
        assert_eq!(told.unwrap().text, "fine");
        assert_eq!(server.join().unwrap().len(), 4);

        let (base, server) = serve(vec![http(
            "401 Unauthorized",
            "",
            r#"{"error":{"message":"Incorrect API key provided"}}"#,
        )]);
        let connection = talking_to(Provider::Ollama, base);
        let (told, _) = told_by(&connection, quick(false), &[Turn::person("hi")]);
        assert_eq!(
            told.unwrap_err(),
            ConnectError::Http("Incorrect API key provided".to_owned())
        );
        assert_eq!(server.join().unwrap().len(), 1, "a refusal is told at once");

        let (base, server) = serve(vec![http("400 Bad Request", "", ""); 1]);
        let connection = talking_to(Provider::Ollama, base);
        let (told, _) = told_by(&connection, quick(false), &[Turn::person("hi")]);
        assert_eq!(told.unwrap_err(), ConnectError::Http("HTTP 400".to_owned()));
        server.join().unwrap();
    }

    #[test]
    fn a_server_that_keeps_failing_is_asked_a_few_times_and_then_the_error_is_told() {
        let (base, server) = serve(vec![
            http(
                "503 Service Unavailable",
                "",
                r#"{"error":{"message":"still down"}}"#
            );
            QUICK.len() + 1
        ]);
        let connection = talking_to(Provider::Ollama, base);
        let (told, _) = told_by(&connection, quick(false), &[Turn::person("hi")]);
        assert_eq!(
            told.unwrap_err(),
            ConnectError::Http("still down".to_owned())
        );
        assert_eq!(server.join().unwrap().len(), QUICK.len() + 1);
    }

    #[test]
    fn the_wait_a_server_asks_for_in_its_headers_is_the_wait_made() {
        let (base, server) = serve(vec![
            http(
                "429 Too Many Requests",
                "retry-after-ms: 400\r\n",
                r#"{"error":{"message":"Rate limit reached"}}"#,
            ),
            http("200 OK", "", FINE),
        ]);
        let connection = talking_to(Provider::Ollama, base);
        let started = Instant::now();
        let (told, _) = told_by(&connection, quick(false), &[Turn::person("hi")]);
        assert_eq!(told.unwrap().text, "fine");
        assert!(
            started.elapsed() >= Duration::from_millis(400),
            "{:?}: longer than the twenty milliseconds of the usual wait",
            started.elapsed()
        );
        assert_eq!(server.join().unwrap().len(), 2);

        let (base, server) = serve(vec![http(
            "429 Too Many Requests",
            "Retry-After: 3600\r\n",
            r#"{"error":{"message":"Rate limit reached"}}"#,
        )]);
        let connection = talking_to(Provider::Ollama, base);
        let started = Instant::now();
        let (told, _) = told_by(&connection, quick(false), &[Turn::person("hi")]);
        assert_eq!(
            told.unwrap_err(),
            ConnectError::Http("Rate limit reached".to_owned())
        );
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "an hour is not waited for"
        );
        assert_eq!(server.join().unwrap().len(), 1);

        let (base, server) = serve(vec![http(
            "429 Too Many Requests",
            "",
            r#"{"error":{"message":"You exceeded your current quota, please check your plan and billing details."}}"#,
        )]);
        let connection = talking_to(Provider::Ollama, base);
        let (told, _) = told_by(&connection, quick(false), &[Turn::person("hi")]);
        assert!(told.is_err());
        assert_eq!(
            server.join().unwrap().len(),
            1,
            "a spent quota is not asked again"
        );
    }

    #[test]
    fn a_connection_that_drops_is_asked_again() {
        let (base, server) = serve(vec![Vec::new(), http("200 OK", "", FINE)]);
        let connection = talking_to(Provider::Ollama, base);
        let (told, _) = told_by(&connection, quick(false), &[Turn::person("hi")]);
        assert_eq!(told.unwrap().text, "fine");
        assert_eq!(server.join().unwrap().len(), 2);
    }

    #[test]
    fn a_wait_before_asking_again_can_be_stopped() {
        let (base, server) = serve(vec![http(
            "429 Too Many Requests",
            "retry-after: 30\r\n",
            r#"{"error":{"message":"Rate limit reached"}}"#,
        )]);
        let connection = talking_to(Provider::Ollama, base);
        let stop = Arc::new(AtomicBool::new(false));
        let stopper = Arc::clone(&stop);
        let stopping = thread::spawn(move || {
            thread::sleep(Duration::from_millis(500));
            stopper.store(true, Ordering::Relaxed);
        });
        let started = Instant::now();
        let turns = [Turn::person("hi")];
        let told = connection.converse_within(quick(false), &asking(&turns), &stop, None);
        assert_eq!(told.unwrap_err(), ConnectError::Cancelled);
        assert!(started.elapsed() < Duration::from_secs(10));
        stopping.join().unwrap();
        server.join().unwrap();
    }

    #[test]
    fn a_stream_that_fails_halfway_is_asked_again_from_the_start() {
        let broken = event_stream(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Half of \"}}]}\n\ndata: {\"error\":{\"message\":\"The server is overloaded, try again later\",\"type\":\"server_error\"}}\n\n",
        );
        let (base, server) = serve(vec![broken, event_stream(&chat_of("All of it."))]);
        let connection = talking_to(Provider::Ollama, base);
        let (told, partials) = told_by(&connection, quick(true), &[Turn::person("hi")]);
        let reply = told.unwrap();
        assert_eq!(
            reply.text, "All of it.",
            "the halves of two tries are not joined"
        );
        assert_eq!(partials, vec!["Half of ", "All of it."]);
        assert_eq!(server.join().unwrap().len(), 2);

        let cut = event_stream(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Half of \"}}]}\n\n",
        );
        let (base, server) = serve(vec![cut; QUICK.len() + 1]);
        let connection = talking_to(Provider::Ollama, base);
        let (told, _) = told_by(&connection, quick(true), &[Turn::person("hi")]);
        assert!(
            matches!(told, Err(ConnectError::Protocol(ref said)) if said.contains("stopped before")),
            "{told:?}"
        );
        assert_eq!(server.join().unwrap().len(), QUICK.len() + 1);

        let (base, server) = serve(vec![event_stream(
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Half\"}}\n\nevent: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"invalid_request_error\",\"message\":\"prompt is too long\"}}\n\n",
        )]);
        let connection = talking_to(Provider::Anthropic, base);
        let (told, _) = told_by(&connection, quick(true), &[Turn::person("hi")]);
        assert_eq!(
            told.unwrap_err(),
            ConnectError::Http("prompt is too long".to_owned())
        );
        assert_eq!(
            server.join().unwrap().len(),
            1,
            "a request that is wrong is not sent again"
        );
    }

    #[test]
    fn an_error_in_the_body_of_a_successful_reply_is_asked_again_when_it_is_temporary() {
        let (base, server) = serve(vec![
            http(
                "200 OK",
                "",
                r#"{"error":{"message":"Provider returned error: rate limited, try again in 1s"}}"#,
            ),
            http("200 OK", "", FINE),
        ]);
        let connection = talking_to(Provider::Ollama, base);
        let started = Instant::now();
        let (told, _) = told_by(&connection, quick(true), &[Turn::person("hi")]);
        assert_eq!(
            told.unwrap().text,
            "fine",
            "a server that does not stream is read as it is"
        );
        assert!(started.elapsed() >= Duration::from_secs(1));
        assert_eq!(server.join().unwrap().len(), 2);
    }

    fn slow_server(
        pieces: usize,
        gap: Duration,
        then: impl FnOnce(&mut TcpStream) + Send + 'static,
    ) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!(
            "http://127.0.0.1:{}/v1",
            listener.local_addr().unwrap().port()
        );
        let server = thread::spawn(move || {
            let Some(mut stream) = accept_within(&listener, Duration::from_secs(30)) else {
                return;
            };
            read_request(&mut stream);
            let head =
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";
            if stream.write_all(head.as_bytes()).is_err() {
                return;
            }
            for piece in 0..pieces {
                let event = format!(
                    "data: {{\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"{piece},\"}}}}]}}\n\n"
                );
                if stream.write_all(event.as_bytes()).is_err() || stream.flush().is_err() {
                    return;
                }
                thread::sleep(gap);
            }
            then(&mut stream);
        });
        (base, server)
    }

    #[test]
    fn a_stream_that_keeps_writing_is_not_cut_off_however_long_it_takes() {
        let (base, server) = slow_server(12, Duration::from_millis(150), |stream| {
            let end = "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
            let _ = stream.write_all(end.as_bytes());
        });
        let connection = talking_to(Provider::Ollama, base);
        let pace = Pace {
            idle: Duration::from_millis(1000),
            cap: Duration::from_secs(60),
            waits: &[],
            streams: true,
            settle: Duration::from_secs(2),
        };
        let started = Instant::now();
        let (told, partials) = told_by(&connection, pace, &[Turn::person("hi")]);
        let reply = told.unwrap();
        assert_eq!(reply.text, "0,1,2,3,4,5,6,7,8,9,10,11,");
        assert!(
            started.elapsed() > Duration::from_millis(1200),
            "longer than the time it may be quiet: {:?}",
            started.elapsed()
        );
        assert_eq!(partials.len(), 12);
        server.join().unwrap();
    }

    #[test]
    fn a_stream_that_goes_quiet_is_ended_when_the_quiet_has_lasted_long_enough() {
        let (base, server) = slow_server(1, Duration::from_millis(10), |stream| {
            stream
                .set_read_timeout(Some(Duration::from_secs(20)))
                .unwrap();
            let mut waiting = [0_u8; 1];
            let _ = stream.read(&mut waiting);
        });
        let connection = talking_to(Provider::Ollama, base);
        let pace = Pace {
            idle: Duration::from_millis(1500),
            cap: Duration::from_secs(60),
            waits: &[],
            streams: true,
            settle: Duration::from_secs(2),
        };
        let started = Instant::now();
        let (told, partials) = told_by(&connection, pace, &[Turn::person("hi")]);
        assert!(
            matches!(&told, Err(ConnectError::Curl(said)) if said.contains("sent nothing")),
            "{told:?}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(partials, vec!["0,"], "what came was shown while it came");
        server.join().unwrap();
    }

    #[test]
    fn a_stream_that_never_ends_is_ended_by_the_overall_limit() {
        let (base, server) = slow_server(10_000, Duration::from_millis(100), |_| {});
        let connection = talking_to(Provider::Ollama, base);
        let pace = Pace {
            idle: Duration::from_secs(30),
            cap: Duration::from_secs(1),
            waits: &QUICK,
            streams: true,
            settle: Duration::from_secs(2),
        };
        let started = Instant::now();
        let (told, _) = told_by(&connection, pace, &[Turn::person("hi")]);
        assert!(matches!(told, Err(ConnectError::Curl(_))), "{told:?}");
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the limit is not asked again: {:?}",
            started.elapsed()
        );
        server.join().unwrap();
    }

    #[test]
    fn a_stream_that_says_it_is_done_but_does_not_close_is_taken_as_done() {
        let (base, server) = slow_server(1, Duration::from_millis(10), |stream| {
            let end = "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
            let _ = stream.write_all(end.as_bytes());
            stream
                .set_read_timeout(Some(Duration::from_secs(30)))
                .unwrap();
            let mut waiting = [0_u8; 1];
            let _ = stream.read(&mut waiting);
        });
        let connection = talking_to(Provider::Ollama, base);
        let pace = Pace {
            settle: Duration::from_millis(400),
            ..quick(true)
        };
        let started = Instant::now();
        let (told, _) = told_by(&connection, pace, &[Turn::person("hi")]);
        assert_eq!(told.unwrap().text, "0,");
        assert!(
            started.elapsed() < Duration::from_secs(8),
            "{:?}",
            started.elapsed()
        );
        assert!(
            AFTER_THE_END >= Duration::from_secs(5),
            "a usage count may follow the end"
        );
        server.join().unwrap();
    }

    #[test]
    fn a_remote_host_may_use_the_proxy_of_the_environment_and_a_local_one_may_not() {
        let config = |base: &str| {
            talking_to(Provider::Custom, base.to_owned())
                .job("POST", "/chat/completions", Some("{}"), quick(true))
                .unwrap()
                .config
        };
        for local in [
            "http://127.0.0.1:11434/v1",
            "http://localhost:1234/v1",
            "http://[::1]:8080/v1",
            "https://localhost:8443/v1",
        ] {
            assert!(config(local).contains("noproxy = \"*\""), "{local}");
        }
        for remote in [
            "https://api.openai.com/v1",
            "https://api.anthropic.com/v1",
            "https://generativelanguage.googleapis.com/v1beta/openai",
            "https://localhost.example.test/v1",
        ] {
            assert!(!config(remote).contains("noproxy"), "{remote}");
        }
        assert!(!is_loopback("http://127.0.0.1.evil.test/v1"));
        assert!(!is_loopback("https://user@localhost/v1"));
        assert!(!is_loopback("ftp://localhost/v1"));
    }

    #[test]
    fn the_config_asks_for_what_the_retries_and_the_streams_need() {
        let job = talking_to(Provider::Ollama, "http://127.0.0.1:11434/v1".to_owned())
            .job("POST", "/chat/completions", Some("{}"), quick(true))
            .unwrap();
        for wanted in [
            "fail-with-body\n",
            "globoff\n",
            "no-buffer\n",
            "dump-header = \"",
            "header = \"Expect:\"\n",
            "max-time = 120\n",
            "data = \"{}\"\n",
            "header = \"Authorization: Bearer a-key\"\n",
        ] {
            assert!(job.config.contains(wanted), "{wanted}: {}", job.config);
        }
        let list = talking_to(
            Provider::Anthropic,
            "https://api.anthropic.com/v1".to_owned(),
        )
        .job("GET", "/models", None, Pace::listing())
        .unwrap();
        assert!(!list.config.contains("no-buffer"), "{}", list.config);
        assert!(!list.config.contains("data"), "{}", list.config);
        assert!(
            list.config.contains("header = \"x-api-key: a-key\"\n"),
            "{}",
            list.config
        );
        assert!(
            list.config
                .contains("header = \"anthropic-version: 2023-06-01\"\n"),
            "{}",
            list.config
        );
        assert!(list.config.contains("max-time = 30\n"), "{}", list.config);
    }

    #[test]
    fn every_page_of_claude_models_is_listed() {
        let (base, server) = serve(vec![
            http(
                "200 OK",
                "",
                r#"{"data":[{"id":"claude-a"},{"id":"claude-b"}],"has_more":true,"last_id":"claude-b"}"#,
            ),
            http(
                "200 OK",
                "",
                r#"{"data":[{"id":"claude-c"}],"has_more":false,"last_id":"claude-c"}"#,
            ),
        ]);
        let models = talking_to(Provider::Anthropic, base)
            .models(&AtomicBool::new(false))
            .unwrap();
        assert_eq!(
            models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            vec!["claude-a", "claude-b", "claude-c"]
        );
        let requests = server.join().unwrap();
        assert!(
            requests[0].starts_with("GET /v1/models?limit=1000 "),
            "{}",
            requests[0]
        );
        assert!(
            requests[1].starts_with("GET /v1/models?limit=1000&after_id=claude-b "),
            "{}",
            requests[1]
        );

        let (base, server) = serve(vec![http(
            "200 OK",
            "",
            r#"{"data":[{"id":"m1"}],"has_more":true,"last_id":"m1"}"#,
        )]);
        let models = talking_to(Provider::Ollama, base)
            .models(&AtomicBool::new(false))
            .unwrap();
        assert_eq!(
            models,
            vec![Model {
                id: "m1".to_owned()
            }]
        );
        let requests = server.join().unwrap();
        assert!(
            requests[0].starts_with("GET /v1/models "),
            "{}",
            requests[0]
        );

        let (base, server) = serve(vec![
            http(
                "200 OK",
                "",
                r#"{"data":[{"id":"claude-a"}],"has_more":true,"last_id":"claude-a"}"#
            );
            2
        ]);
        let models = talking_to(Provider::Anthropic, base)
            .models(&AtomicBool::new(false))
            .unwrap();
        assert_eq!(
            models.len(),
            2,
            "a server that keeps pointing at the same page is not followed for ever"
        );
        server.join().unwrap();
    }

    fn a_body_of(megabytes: usize) -> String {
        format!(
            r#"{{"x":"{}"}}"#,
            r#"a\"b\\c\n"#.repeat(megabytes * 1024 * 1024 / 9)
        )
    }

    #[test]
    fn a_request_too_big_for_a_curl_config_line_is_sent_whole_and_leaves_nothing_behind() {
        let payload = a_body_of(12);
        assert!(payload.len() > 12 * 1024 * 1024);
        let (base, server) = serve(vec![http("200 OK", "", "{}")]);
        let connection = talking_to(Provider::Ollama, base);
        let job = connection
            .job("POST", "/chat/completions", Some(&payload), quick(false))
            .unwrap();
        assert!(job.config.len() < 4096, "the body is not in the config");
        assert!(!job.config.contains("data = "), "{}", job.config);
        let kept = job
            .body_kept
            .as_ref()
            .expect("the body is kept in a file")
            .path
            .clone();
        assert!(kept.exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&kept).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "nobody else can read it");
        }
        let told = run_curl(&job, &AtomicBool::new(false), &mut |_| false).unwrap();
        assert_eq!(told, "{}");
        let request = server.join().unwrap().remove(0);
        let received = request.split_once("\r\n\r\n").unwrap().1;
        assert_eq!(received.len(), payload.len());
        assert!(received == payload, "every byte arrived");
        let headers = job.headers_at.as_ref().unwrap().path.clone();
        assert!(headers.exists());
        drop(job);
        assert!(!kept.exists(), "the body is not left on the disk");
        assert!(!headers.exists());
    }

    #[test]
    fn a_small_request_stays_in_the_config_and_touches_no_file() {
        let connection = talking_to(Provider::Ollama, "http://127.0.0.1:11434/v1".to_owned());
        let payload = a_body_of(1);
        let job = connection
            .job(
                "POST",
                "/chat/completions",
                Some(&payload[..1000]),
                quick(false),
            )
            .unwrap();
        assert!(job.config.contains("data = \""));
        assert!(job.body_kept.is_none());
        let at_the_edge = "x".repeat(INLINE_BODY_BYTES);
        assert!(
            connection
                .job("POST", "/x", Some(&at_the_edge), quick(false))
                .unwrap()
                .body_kept
                .is_none()
        );
        assert!(
            connection
                .job("POST", "/x", Some(&format!("{at_the_edge}x")), quick(false))
                .unwrap()
                .body_kept
                .is_some()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_curl_that_is_still_running_is_stopped_and_reaped_when_the_request_is_given_up() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!(
            "http://127.0.0.1:{}/v1",
            listener.local_addr().unwrap().port()
        );
        let (heard, has_heard) = std::sync::mpsc::channel();
        let holding = thread::spawn(move || {
            let Some(mut stream) = accept_within(&listener, Duration::from_secs(20)) else {
                return;
            };
            read_request(&mut stream);
            let _ = heard.send(());
            stream
                .set_read_timeout(Some(Duration::from_secs(20)))
                .unwrap();
            let mut waiting = [0_u8; 1];
            let _ = stream.read(&mut waiting);
        });
        let job = talking_to(Provider::Ollama, base)
            .job("POST", "/chat/completions", Some("{}"), quick(false))
            .unwrap();
        let running = Running::start(&job.config).unwrap();
        has_heard
            .recv_timeout(Duration::from_secs(15))
            .expect("curl made its request");
        let there = format!("/proc/{}", running.child.id());
        assert!(Path::new(&there).exists(), "curl is running");
        drop(running);
        assert!(!Path::new(&there).exists(), "curl is stopped and reaped");
        holding.join().unwrap();
    }

    #[test]
    fn a_curl_that_refuses_its_config_is_told_in_its_own_words_and_not_asked_again() {
        let job = Job {
            config: "bogus-option\n".to_owned(),
            headers_at: None,
            body_kept: None,
            idle: Duration::from_secs(5),
            cap: Duration::from_secs(10),
            settle: Duration::from_secs(1),
            local: true,
        };
        let failure = run_curl(&job, &AtomicBool::new(false), &mut |_| false).unwrap_err();
        assert!(
            matches!(&failure.error, ConnectError::Curl(said) if said.contains("bogus")),
            "{failure:?}"
        );
        assert!(!failure.transient);
    }

    #[test]
    fn a_request_that_is_stopped_stops_curl_at_once() {
        let (base, server) = slow_server(1, Duration::from_millis(10), |stream| {
            stream
                .set_read_timeout(Some(Duration::from_secs(20)))
                .unwrap();
            let mut waiting = [0_u8; 1];
            let _ = stream.read(&mut waiting);
        });
        let connection = talking_to(Provider::Ollama, base);
        let stop = Arc::new(AtomicBool::new(false));
        let stopper = Arc::clone(&stop);
        let stopping = thread::spawn(move || {
            thread::sleep(Duration::from_millis(400));
            stopper.store(true, Ordering::Relaxed);
        });
        let started = Instant::now();
        let turns = [Turn::person("hi")];
        let told = connection.converse_within(
            quick(true),
            &asking(&turns),
            &stop,
            Some(&mut |_: Progress<'_>| {}),
        );
        assert_eq!(told.unwrap_err(), ConnectError::Cancelled);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
        stopping.join().unwrap();
        server.join().unwrap();
    }

    #[test]
    fn a_claude_stream_is_read_end_to_end_with_its_room_its_headers_and_its_tokens() {
        let events = "event: message_start
data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"content\":[],\"usage\":{\"input_tokens\":25,\"cache_read_input_tokens\":400,\"output_tokens\":1}}}

event: content_block_start
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}

data: {\"type\":\"ping\"}

data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Reading it.\"}}

data: {\"type\":\"content_block_stop\",\"index\":0}

data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"read_text\",\"input\":{}}}

data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"page\\\":1}\"}}

data: {\"type\":\"content_block_stop\",\"index\":1}

data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":15}}

data: {\"type\":\"message_stop\"}

";
        let (base, server) = serve(vec![event_stream(events)]);
        let connection = talking_to(Provider::Anthropic, base);
        let turns = [Turn::person("read page one")];
        let ask = Ask {
            turns: &turns,
            context: None,
            tools: &one_tool(),
            system: Some("you are helping"),
        };
        let mut said = Vec::new();
        let reply = connection
            .converse_within(
                quick(true),
                &ask,
                &AtomicBool::new(false),
                Some(&mut |far: Progress<'_>| said.push(far.said.to_owned())),
            )
            .unwrap();
        assert_eq!(reply.text, "Reading it.");
        assert_eq!(
            reply.calls,
            vec![ToolCall::asked(
                "toolu_1",
                "read_text",
                Json::parse(r#"{"page":1}"#).unwrap()
            )]
        );
        assert!(!reply.cut_short);
        assert_eq!(
            reply.usage,
            Some(Usage {
                input: 25,
                output: 15,
                cache_read: 400,
                cache_write: 0
            })
        );
        assert_eq!(said, vec!["Reading it."]);
        let request = server.join().unwrap().remove(0);
        assert!(request.starts_with("POST /v1/messages "), "{request}");
        let lower = request.to_ascii_lowercase();
        assert!(lower.contains("\r\nx-api-key: a-key\r\n"), "{request}");
        assert!(
            lower.contains("\r\nanthropic-version: 2023-06-01\r\n"),
            "{request}"
        );
        assert!(!lower.contains("expect:"), "{request}");
        let sent = Json::parse(request.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(
            sent.get("max_tokens").and_then(Json::as_count),
            Some(16_000)
        );
        assert_eq!(sent.get("stream").and_then(Json::as_bool), Some(true));
        assert!(sent.get("stream_options").is_none());
    }

    #[test]
    fn a_local_server_is_asked_for_the_tokens_it_used_and_the_others_are_not() {
        let asked = |provider| {
            let mut payload = Json::object([]);
            talking_to(provider, String::new()).ask_for_a_stream(&mut payload);
            payload.write()
        };
        for provider in [Provider::Ollama, Provider::LmStudio] {
            assert_eq!(
                asked(provider),
                r#"{"stream":true,"stream_options":{"include_usage":true}}"#
            );
        }
        for provider in [
            Provider::OpenAi,
            Provider::Anthropic,
            Provider::Gemini,
            Provider::Custom,
        ] {
            assert_eq!(asked(provider), r#"{"stream":true}"#, "{provider:?}");
        }
    }

    fn a_page_was_rendered() -> [Turn; 3] {
        [
            Turn::person("what does page 1 look like?"),
            Turn::Model {
                text: String::new(),
                calls: vec![ToolCall::asked("c1", "render_page", Json::object([]))],
                raw: None,
            },
            Turn::Results {
                results: vec![ToolResult {
                    call_id: "c1".into(),
                    text: "Page 1 as shown.".into(),
                    is_error: false,
                    picture: Some(Picture {
                        media_type: "image/png".into(),
                        base64: "AAAA".into(),
                    }),
                }],
            },
        ]
    }

    #[test]
    fn a_model_that_cannot_see_pictures_is_sent_the_result_again_without_them() {
        let refusal = |said: &str| {
            http(
                "400 Bad Request",
                "",
                &format!(r#"{{"error":{{"message":"{said}"}}}}"#),
            )
        };
        for (provider, streams) in [
            (Provider::Ollama, false),
            (Provider::Ollama, true),
            (Provider::OpenAi, false),
        ] {
            let second = if streams {
                event_stream(&chat_of("It is a letter."))
            } else if provider == Provider::OpenAi {
                http(
                    "200 OK",
                    "",
                    r#"{"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"It is a letter."}]}]}"#,
                )
            } else {
                http(
                    "200 OK",
                    "",
                    r#"{"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":"It is a letter."}}]}"#,
                )
            };
            let (base, server) = serve(vec![
                refusal("image input is not supported by this model"),
                second,
            ]);
            let connection = talking_to(provider, base);
            let (told, _) = told_by(&connection, quick(streams), &a_page_was_rendered());
            assert_eq!(
                told.unwrap().text,
                "It is a letter.",
                "{provider:?} {streams}"
            );
            let requests = server.join().unwrap();
            assert_eq!(requests.len(), 2, "{provider:?}");
            assert!(
                requests[0].contains("AAAA"),
                "{provider:?}: the picture is tried first"
            );
            assert!(
                !requests[1].contains("AAAA"),
                "{provider:?}: {}",
                requests[1]
            );
            assert!(
                requests[1].contains("The picture could not be shown to this model."),
                "{provider:?}: the model is told"
            );
        }

        let (base, server) = serve(vec![refusal("that model does not exist")]);
        let (told, _) = told_by(
            &talking_to(Provider::Ollama, base),
            quick(false),
            &a_page_was_rendered(),
        );
        assert!(told.is_err());
        assert_eq!(
            server.join().unwrap().len(),
            1,
            "an error about something else is not tried again"
        );

        let (base, server) = serve(vec![refusal("image input is not supported")]);
        let (told, _) = told_by(
            &talking_to(Provider::Anthropic, base),
            quick(false),
            &a_page_was_rendered(),
        );
        assert!(told.is_err());
        assert_eq!(
            server.join().unwrap().len(),
            1,
            "Claude takes pictures, so its refusal is its own"
        );

        let (base, server) = serve(vec![refusal("image input is not supported")]);
        let (told, _) = told_by(
            &talking_to(Provider::Ollama, base),
            quick(false),
            &[Turn::person("hello")],
        );
        assert!(told.is_err());
        assert_eq!(
            server.join().unwrap().len(),
            1,
            "with no picture there is none to leave out"
        );
    }

    #[test]
    fn a_stream_that_went_quiet_is_asked_again_only_once() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!(
            "http://127.0.0.1:{}/v1",
            listener.local_addr().unwrap().port()
        );
        let counted = Arc::new(AtomicUsize::new(0));
        let over = Arc::new(AtomicBool::new(false));
        let (counting, ending) = (Arc::clone(&counted), Arc::clone(&over));
        let server = thread::spawn(move || {
            let mut held = Vec::new();
            while !ending.load(Ordering::Relaxed) {
                if let Some(mut stream) = accept_within(&listener, Duration::from_millis(100)) {
                    read_request(&mut stream);
                    counting.fetch_add(1, Ordering::Relaxed);
                    let opening = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";
                    let _ = stream.write_all(opening.as_bytes());
                    held.push(stream);
                }
            }
        });
        let pace = Pace {
            idle: Duration::from_millis(800),
            cap: Duration::from_secs(60),
            waits: &QUICK,
            streams: true,
            settle: Duration::from_secs(2),
        };
        let (told, _) = told_by(
            &talking_to(Provider::Ollama, base),
            pace,
            &[Turn::person("hi")],
        );
        over.store(true, Ordering::Relaxed);
        server.join().unwrap();
        assert!(
            matches!(&told, Err(ConnectError::Curl(said)) if said.contains("sent nothing")),
            "{told:?}"
        );
        assert_eq!(
            counted.load(Ordering::Relaxed),
            2,
            "a server that says nothing for that long is not worth five tries"
        );
    }
}
