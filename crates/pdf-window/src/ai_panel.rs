use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{
    Arc,
    mpsc::{self, Receiver, TryRecvError},
};
use std::thread;

use eframe::egui;
use pdf_agent::attach;
use pdf_agent::connect::{
    Attachment, ConnectError, Connection, Effort, Model, Provider as WireProvider, Reply,
    ToolResult, Turn, settle_dangling_calls,
};
use pdf_agent::tools::DocumentBrief;

use pdf_app::ai_layout;
use pdf_app::ai_permission::{Answer as Allowed, Mode};
use pdf_app::wording::{Assistant, Lang, Message};

use crate::ai_actions::{Tools, tools_are_offered};
use crate::icons::Icon;
use crate::window_state::Window;

mod cards;
mod chats;
mod composer;
mod conversation;
mod going_back;
mod header;
mod history;
mod keeping;
mod look;
mod setup;
mod steps;

#[cfg(test)]
mod behaviour;

pub(crate) use header::Badge;

const MAX_CONTEXT: usize = 12_000;

pub(crate) const MOST_MODEL_CALLS: usize = 60;

fn go_on() -> String {
    Message::AiGoOn.say(Lang::English)
}

const STEPS_USED_UP: &str = "not run: this request used up the rounds it is allowed";

fn not_run(calls: &[pdf_agent::connect::ToolCall], why: &str) -> Vec<ToolResult> {
    calls
        .iter()
        .map(|call| ToolResult::failed(call.id.clone(), why))
        .collect()
}

const BUDGET_SPENT: &str = "\n\nYou have used all the rounds this request is allowed. Call no tool now. In a few \
short sentences say what is done, what is left, and what you would do next; the person can let \
you go on.";

const GAP: f32 = 8.0;

const DEFAULT_CHROME: f32 = 110.0;

const EFFORTS: [Effort; 5] = [
    Effort::Off,
    Effort::None,
    Effort::Low,
    Effort::Medium,
    Effort::High,
];

fn plain_enter(ui: &mut egui::Ui) -> bool {
    ui.input_mut(|input| {
        let before = input.events.len();
        input.events.retain(|event| {
            !matches!(
                event,
                egui::Event::Key {
                    key: egui::Key::Enter,
                    pressed: true,
                    modifiers,
                    ..
                } if modifiers.is_none()
            )
        });
        input.events.len() < before
    })
}

fn shorten_model(id: &str) -> String {
    let last = id.rsplit('/').next().unwrap_or(id);
    if last.chars().count() <= 28 {
        return last.to_owned();
    }
    let mut clipped: String = last.chars().take(27).collect();
    clipped.push('\u{2026}');
    clipped
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Provider {
    OpenAi,
    Claude,
    Gemini,
    Ollama,
    LmStudio,
    Custom,
}

impl Provider {
    const ALL: [Self; 6] = [
        Self::OpenAi,
        Self::Claude,
        Self::Gemini,
        Self::Ollama,
        Self::LmStudio,
        Self::Custom,
    ];
    const fn name(self) -> &'static str {
        match self {
            Self::OpenAi => "OpenAI",
            Self::Claude => "Claude",
            Self::Gemini => "Gemini",
            Self::Ollama => "Ollama",
            Self::LmStudio => "LM Studio",
            Self::Custom => "Custom compatible",
        }
    }
    fn by_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|one| one.name() == name)
    }
    const fn wire(self) -> WireProvider {
        match self {
            Self::OpenAi => WireProvider::OpenAi,
            Self::Claude => WireProvider::Anthropic,
            Self::Gemini => WireProvider::Gemini,
            Self::Ollama => WireProvider::Ollama,
            Self::LmStudio => WireProvider::LmStudio,
            Self::Custom => WireProvider::Custom,
        }
    }
    const fn base_url(self) -> &'static str {
        self.wire().default_base_url()
    }
}

fn choice_file() -> Option<std::path::PathBuf> {
    if cfg!(test) {
        return None;
    }
    crate::own_folder::own_file("ai")
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Arriving {
    said: String,
    thinking: String,
}

impl Arriving {
    fn is_empty(&self) -> bool {
        self.said.is_empty() && self.thinking.is_empty()
    }
}

enum Answer {
    Models(u64, Result<Vec<Model>, ConnectError>),
    Partial(u64, Arriving),
    Chat(u64, Result<Reply, ConnectError>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NoticeAction {
    Continue,
    Retry,
    OpenSettings,
}

#[derive(Clone, Debug, PartialEq)]
struct Notice {
    said: Message,
    detail: Option<String>,
    action: Option<NoticeAction>,
}

impl Notice {
    fn plain(said: Message) -> Self {
        Self {
            said,
            detail: None,
            action: None,
        }
    }

    const fn with(said: Message, action: NoticeAction) -> Self {
        Self {
            said,
            detail: None,
            action: Some(action),
        }
    }
}

fn notice_of(error: &ConnectError) -> Notice {
    let (said, detail, action) = match error {
        ConnectError::Invalid(why) => (
            Message::AiConnectionInvalid,
            Some(why),
            Some(NoticeAction::OpenSettings),
        ),
        ConnectError::Cancelled => (Message::AiStopped, None, None),
        ConnectError::Curl(why) => (Message::AiCouldNotReach, Some(why), None),
        ConnectError::ResponseTooLarge => (Message::AiAnswerTooLarge, None, None),
        ConnectError::TooLarge(why) => (Message::AiConversationTooLarge, Some(why), None),
        ConnectError::Http(why) => (
            Message::AiServiceRefused,
            Some(why),
            Some(NoticeAction::OpenSettings),
        ),
        ConnectError::Protocol(why) => (Message::AiAnswerUnexpected, Some(why), None),
    };
    Notice {
        said,
        detail: detail.cloned(),
        action,
    }
}

struct Asking {
    stop: Arc<AtomicBool>,
    answers: Receiver<Answer>,
}

type Prepared = (attach::Kind, Result<Vec<Attachment>, String>);

enum Preparing {
    Reading(Receiver<Prepared>),
    Done(Result<Vec<Attachment>, String>),
}

struct PendingAttachment {
    name: String,
    bytes: usize,
    kind: attach::Kind,
    state: Preparing,
}

impl PendingAttachment {
    fn refused(&self) -> bool {
        matches!(self.state, Preparing::Done(Err(_)))
    }

    fn reading(&self) -> bool {
        matches!(self.state, Preparing::Reading(_))
    }

    fn ready(&self) -> Option<&[Attachment]> {
        match &self.state {
            Preparing::Done(Ok(attachments)) => Some(attachments),
            _ => None,
        }
    }

    fn label(&self, lang: Lang) -> String {
        if self.reading() {
            return Assistant::Attaching.say(lang);
        }
        if self.refused() {
            return Message::AiAttachRefused.say(lang);
        }
        match self.kind {
            attach::Kind::Picture => Message::AiAttachPicture,
            attach::Kind::Pdf => Message::AiAttachPdf,
            attach::Kind::Office | attach::Kind::Text => Message::AiAttachText,
            attach::Kind::Unsupported => Message::AiAttachRefused,
        }
        .say(lang)
    }
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "four independent things a person can turn on: the panel, its \
              settings, whether a provider answered, and whether the page \
              goes with the question"
)]
pub(crate) struct AiState {
    pub(crate) open: bool,
    pub(crate) width: f32,
    provider: Provider,
    base_url: String,
    key: String,
    model: String,
    models: Vec<Model>,
    turns: Vec<Turn>,
    composer: String,
    settings_open: bool,
    notice: Option<Notice>,
    connected: bool,
    edited: bool,
    include_context: bool,
    generation: u64,
    asking: Option<Asking>,
    partial: Option<Arriving>,
    wrapped_rows: usize,
    drawn: Vec<Option<(conversation::TurnShape, f32)>>,
    drawn_width: f32,
    undo_the_run_asked: bool,
    model_calls: usize,
    summarising: bool,
    effort: Effort,
    pub(crate) mode: Mode,
    pub(crate) tools: Tools,
    context: String,
    brief: DocumentBrief,
    notes: Vec<(usize, Message)>,
    remember_key: bool,
    chat_id: String,
    saved_turns: usize,
    save_again_at: u64,
    documents: Vec<String>,
    places: Vec<String>,
    confirming_free: bool,
    history: Option<Arc<Vec<pdf_agent::history::Chat>>>,
    pending: Vec<PendingAttachment>,
    recall: pdf_app::ai_recall::Recall,
    rewound: Option<going_back::Rewound>,
    send_now: bool,
    checking: bool,
    other_keys: BTreeMap<&'static str, String>,
    chats_open: bool,
    deleting: Option<String>,
    copied_chat: Option<String>,
    composer_chrome: f32,
    was_moving: bool,
    focus_composer: bool,
    act_on_the_notice_later: Option<NoticeAction>,
}

impl Default for AiState {
    fn default() -> Self {
        let provider = Provider::OpenAi;
        Self {
            open: false,
            provider,
            base_url: provider.base_url().into(),
            key: String::new(),
            model: String::new(),
            models: Vec::new(),
            turns: Vec::new(),
            composer: String::new(),
            settings_open: true,
            notice: None,
            width: 360.0,
            connected: false,
            edited: false,
            include_context: true,
            generation: 0,
            asking: None,
            partial: None,
            wrapped_rows: ai_layout::LEAST_ROWS,
            drawn: Vec::new(),
            drawn_width: 0.0,
            undo_the_run_asked: false,
            model_calls: 0,
            summarising: false,
            effort: Effort::Off,
            mode: Mode::default(),
            tools: Tools::default(),
            context: String::new(),
            brief: DocumentBrief::default(),
            notes: Vec::new(),
            remember_key: false,
            chat_id: String::new(),
            saved_turns: 0,
            save_again_at: 0,
            documents: Vec::new(),
            places: Vec::new(),
            confirming_free: false,
            history: None,
            pending: Vec::new(),
            recall: pdf_app::ai_recall::Recall::default(),
            rewound: None,
            send_now: false,
            checking: false,
            other_keys: BTreeMap::new(),
            chats_open: false,
            deleting: None,
            copied_chat: None,
            composer_chrome: DEFAULT_CHROME,
            was_moving: false,
            focus_composer: false,
            act_on_the_notice_later: None,
        }
    }
}

impl AiState {
    pub(crate) fn remembered() -> Self {
        let mut state = Self::default();
        let choice = choice_file()
            .and_then(|file| std::fs::read_to_string(file).ok())
            .and_then(|text| pdf_app::ai_choice::read(&text));
        match choice
            .as_ref()
            .and_then(|kept| Provider::by_name(&kept.provider).map(|provider| (provider, kept)))
        {
            Some((provider, choice)) => {
                state.provider = provider;
                state.base_url = if choice.base_url.is_empty() {
                    provider.base_url().into()
                } else {
                    choice.base_url.clone()
                };
                state.model.clone_from(&choice.model);
                state.effort = Effort::parse(&choice.effort).unwrap_or(Effort::Off);
                state.mode = Mode::parse(&choice.mode)
                    .or_else(|| Mode::parse(pdf_app::ai_choice::DEFAULT_MODE))
                    .unwrap_or_default()
                    .kept_for_next_time();
            }
            None => {
                if let Some(provider) = keeping::kept_for() {
                    state.provider = provider;
                    state.base_url = provider.base_url().into();
                }
            }
        }
        state.take_the_kept_key();
        state.settings_open = state.model.is_empty() || state.key_missing();
        state
    }

    fn remember(&self) {
        let choice = pdf_app::ai_choice::Choice {
            provider: self.provider.name().to_owned(),
            base_url: self.base_url.clone(),
            model: self.model.clone(),
            effort: self.effort.as_str().to_owned(),
            mode: self.mode.kept_for_next_time().as_str().to_owned(),
        };
        let (Some(file), Some(line)) = (choice_file(), pdf_app::ai_choice::write(&choice)) else {
            return;
        };
        if let Some(folder) = file.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        let temporary = file.with_extension("new");
        if std::fs::write(&temporary, line).is_ok() {
            let _ = std::fs::rename(&temporary, &file);
        }
    }

    pub(crate) fn busy(&self) -> bool {
        self.asking.is_some()
    }

    fn may_send(&self) -> bool {
        !self.composer.trim().is_empty() || self.pending.iter().any(|item| item.ready().is_some())
    }

    fn reading(&self) -> bool {
        self.pending.iter().any(PendingAttachment::reading)
    }

    fn can_send(&self) -> bool {
        self.may_send() && !self.reading()
    }

    pub(crate) fn attach_file(&mut self, path: &std::path::Path) {
        let name = path
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        let refused = |name: String, why: String| PendingAttachment {
            name,
            bytes: 0,
            kind: attach::Kind::Unsupported,
            state: Preparing::Done(Err(why)),
        };
        let size = match std::fs::metadata(path) {
            Ok(facts) => usize::try_from(facts.len()).unwrap_or(usize::MAX),
            Err(error) => {
                self.pending.push(refused(name, error.to_string()));
                return;
            }
        };
        let so_far: Vec<(String, usize)> = self
            .pending
            .iter()
            .filter(|item| !item.refused())
            .map(|item| (item.name.clone(), item.bytes))
            .collect();
        if let Err(error) = attach::fits(&so_far, (&name, size)) {
            self.pending.push(refused(name, error.to_string()));
            return;
        }
        let (send, receive) = mpsc::channel();
        let (worker_path, worker_name) = (path.to_owned(), name.clone());
        thread::spawn(move || {
            let outcome = match std::fs::read(&worker_path) {
                Ok(bytes) => (
                    attach::kind_of(&bytes),
                    attach::prepare(&worker_name, &bytes).map_err(|error| error.to_string()),
                ),
                Err(error) => (attach::Kind::Unsupported, Err(error.to_string())),
            };
            let _ = send.send(outcome);
        });
        self.pending.push(PendingAttachment {
            name,
            bytes: size,
            kind: attach::Kind::Unsupported,
            state: Preparing::Reading(receive),
        });
    }

    fn take_what_was_read(&mut self, ctx: &egui::Context) {
        let mut waiting = false;
        for item in &mut self.pending {
            let Preparing::Reading(receive) = &item.state else {
                continue;
            };
            match receive.try_recv() {
                Ok((kind, outcome)) => {
                    item.kind = kind;
                    item.state = Preparing::Done(outcome);
                }
                Err(TryRecvError::Empty) => waiting = true,
                Err(TryRecvError::Disconnected) => {
                    item.state = Preparing::Done(Err(Message::AiWorkerStopped.say(Lang::English)));
                }
            }
        }
        if waiting {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    pub(crate) fn remove_pending(&mut self, at: usize) {
        if at < self.pending.len() {
            self.pending.remove(at);
        }
    }

    fn connection(&self) -> Connection {
        Connection {
            provider: self.provider.wire(),
            base_url: self.base_url.trim_end_matches('/').to_owned(),
            model: self.model.clone(),
            api_key: self.key.clone(),
            effort: self.effort,
        }
    }
    fn invalidate(&mut self) {
        self.generation += 1;
        self.connected = false;
        self.edited = true;
        self.models.clear();
        self.notice = None;
    }
    fn disconnect(&mut self) {
        self.cancel();
        self.key.clear();
        self.remember_key = false;
        self.keep_the_key();
        self.invalidate();
    }
    fn cancel(&mut self) {
        if let Some(asking) = self.asking.take() {
            asking.stop.store(true, Ordering::Relaxed);
        }
        self.checking = false;
        if let Some(arrived) = self.partial.take()
            && !arrived.said.trim().is_empty()
        {
            self.turns.push(Turn::model(arrived.said));
        }
        self.generation += 1;
    }
    pub(crate) fn poll(&mut self, ctx: &egui::Context) {
        self.take_what_was_read(ctx);
        let Some(asking) = self.asking.as_ref() else {
            return;
        };
        let mut result = None;
        let mut fresh = false;
        loop {
            match asking.answers.try_recv() {
                Ok(Answer::Partial(generation, said)) => {
                    if generation == self.generation {
                        self.partial = Some(said);
                        fresh = true;
                    }
                }
                Ok(answer) => {
                    result = Some(answer);
                    break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.asking = None;
                    self.checking = false;
                    self.partial = None;
                    self.notice = Some(Notice::plain(Message::AiWorkerStopped));
                    return;
                }
            }
        }
        let Some(answer) = result else {
            if fresh {
                ctx.request_repaint();
            }
            return;
        };
        self.asking = None;
        self.checking = false;
        self.partial = None;
        let generation = match &answer {
            Answer::Models(g, _) | Answer::Partial(g, _) | Answer::Chat(g, _) => *g,
        };
        if generation != self.generation {
            return;
        }
        match answer {
            Answer::Partial(..) => {}
            Answer::Models(_, result) => match result {
                Ok(models) => self.take_the_models(models),
                Err(error) => {
                    self.connected = false;
                    self.notice = Some(notice_of(&error));
                }
            },
            Answer::Chat(_, result) => match result {
                Ok(reply) => self.took_the_reply(ctx, &reply),
                Err(error) => self.the_reply_failed(&error),
            },
        }
        ctx.request_repaint();
    }
    fn start_models(&mut self, ctx: &egui::Context) {
        if self.busy() {
            return;
        }
        let generation = self.generation;
        let connection = self.connection();
        let flag = Arc::new(AtomicBool::new(false));
        let worker_flag = flag.clone();
        let (tx, rx) = mpsc::channel();
        self.notice = None;
        self.asking = Some(Asking {
            stop: flag,
            answers: rx,
        });
        self.checking = true;
        let repaint = ctx.clone();
        thread::spawn(move || {
            let result = connection.models(&worker_flag);
            let _ = tx.send(Answer::Models(generation, result));
            repaint.request_repaint();
        });
    }
    fn took_the_reply(&mut self, ctx: &egui::Context, reply: &Reply) {
        self.settings_open = false;
        self.connected = true;
        self.turns.push(Turn::answered(reply));
        if std::mem::take(&mut self.summarising) {
            let results = not_run(&reply.calls, STEPS_USED_UP);
            self.keep_what_was_done(results);
            self.notice = Some(Notice::with(Message::AiStepsUsedUp, NoticeAction::Continue));
            return;
        }
        if reply.calls.is_empty() {
            if reply.cut_short {
                self.notice = Some(Notice::with(
                    Message::AiAnswerCutShort,
                    NoticeAction::Continue,
                ));
            }
            return;
        }
        if self.model_calls >= MOST_MODEL_CALLS {
            self.turns.push(Turn::Results {
                results: not_run(&reply.calls, STEPS_USED_UP),
            });
            self.summarising = true;
            self.ask_the_model(ctx);
            return;
        }
        self.tools.take(&reply.calls);
    }

    fn the_reply_failed(&mut self, error: &ConnectError) {
        self.summarising = false;
        self.tools.drop_the_queue();
        let mut notice = notice_of(error);
        match self.turns.last() {
            Some(Turn::Person { text, .. }) if *text == go_on() => {
                self.turns.pop();
                notice.action = Some(NoticeAction::Continue);
            }
            Some(Turn::Person { .. }) => {
                if let Some(Turn::Person { text, attachments }) = self.turns.pop() {
                    self.composer = text;
                    self.pending = attachments
                        .into_iter()
                        .map(going_back::pending_again)
                        .collect();
                }
            }
            Some(Turn::Results { .. }) => notice.action = Some(NoticeAction::Retry),
            _ => {}
        }
        self.notice = Some(notice);
    }

    pub(crate) fn stop_the_run(&mut self) {
        self.cancel();
        self.summarising = false;
        if let Some(results) = self.tools.stop() {
            self.keep_what_was_done(results);
        }
    }

    pub(crate) fn keep_what_was_done(&mut self, results: Vec<ToolResult>) {
        if !results.is_empty() {
            self.turns.push(Turn::Results { results });
        }
    }

    fn start_answer(&mut self, ctx: &egui::Context, context: String, brief: &DocumentBrief) {
        if self.checking {
            self.cancel();
        }
        if self.busy() || !self.can_send() {
            return;
        }
        if self.tools.busy() {
            self.stop_the_run();
            if self.tools.busy() {
                self.send_now = true;
                return;
            }
        }
        settle_dangling_calls(&mut self.turns);
        self.rewound = None;
        self.recall.forget();
        let asked = std::mem::take(&mut self.composer);
        let attachments: Vec<Attachment> = std::mem::take(&mut self.pending)
            .into_iter()
            .filter_map(|item| match item.state {
                Preparing::Done(Ok(attachments)) => Some(attachments),
                _ => None,
            })
            .flatten()
            .collect();
        self.turns
            .push(Turn::person_with(asked.trim(), attachments));
        self.model_calls = 0;
        self.summarising = false;
        self.tools.run = pdf_app::ai_run::Run::default();
        self.tools.plan.clear();
        self.context = context;
        self.brief = brief.clone();
        self.ask_the_model(ctx);
    }

    pub(crate) fn start_round(&mut self, ctx: &egui::Context, brief: &DocumentBrief) {
        if self.busy() {
            return;
        }
        self.brief = brief.clone();
        self.ask_the_model(ctx);
    }

    fn continue_the_run(&mut self, ctx: &egui::Context, brief: &DocumentBrief) {
        if self.busy() || self.tools.busy() {
            return;
        }
        self.notice = None;
        self.turns.push(Turn::person(go_on()));
        self.model_calls = 0;
        self.summarising = false;
        self.context.clear();
        self.brief = brief.clone();
        self.ask_the_model(ctx);
    }

    fn retry(&mut self, ctx: &egui::Context, brief: &DocumentBrief) {
        if self.busy()
            || self.tools.busy()
            || !matches!(
                self.turns.last(),
                Some(Turn::Person { .. } | Turn::Results { .. })
            )
        {
            return;
        }
        self.notice = None;
        self.summarising = self.model_calls >= MOST_MODEL_CALLS;
        self.brief = brief.clone();
        self.ask_the_model(ctx);
    }

    fn act_on_the_notice(
        &mut self,
        ctx: &egui::Context,
        brief: &DocumentBrief,
        action: NoticeAction,
    ) {
        match action {
            NoticeAction::Continue => self.continue_the_run(ctx, brief),
            NoticeAction::Retry => self.retry(ctx, brief),
            NoticeAction::OpenSettings => {
                self.notice = None;
                self.settings_open = true;
            }
        }
    }

    pub(crate) fn round_done(&mut self, results: Vec<ToolResult>) {
        self.turns.push(Turn::Results { results });
    }

    pub(crate) fn say_the_run_cannot_be_taken_back(&mut self) {
        self.notice = Some(Notice::plain(Message::AiUndoRunPersonEdited));
    }

    pub(crate) fn say_the_run_was_taken_back(&mut self, steps: usize) {
        self.notes
            .push((self.turns.len(), Message::AiRunTakenBack { steps }));
    }

    pub(crate) fn say_the_document_changed(&mut self, pages: &[usize]) {
        self.notes.push((
            self.turns.len(),
            Message::AiChangedTheDocument {
                pages: pages.iter().map(|page| page + 1).collect(),
            },
        ));
    }

    fn what_is_offered(&self) -> (Vec<pdf_agent::connect::ToolOffer>, Option<String>) {
        if !tools_are_offered(self.mode) {
            return (Vec::new(), None);
        }
        let mut system = pdf_agent::tools::window_instructions(&self.brief)
            + &self.what_changed_hands().unwrap_or_default();
        if self.summarising {
            system.push_str(BUDGET_SPENT);
            return (Vec::new(), Some(system));
        }
        (pdf_agent::tools::offered_to_a_window(), Some(system))
    }

    fn ask_the_model(&mut self, ctx: &egui::Context) {
        self.model_calls += 1;
        let generation = self.generation;
        let connection = self.connection();
        let turns = pdf_agent::context::shortened(&pdf_agent::connect::without_the_old_pictures(
            &self.turns,
        ));
        let context = self.context.clone();
        let (tools, system) = self.what_is_offered();
        let flag = Arc::new(AtomicBool::new(false));
        let worker_flag = flag.clone();
        let (tx, rx) = mpsc::channel();
        self.notice = None;
        self.asking = Some(Asking {
            stop: flag,
            answers: rx,
        });
        let repaint = ctx.clone();
        thread::spawn(move || {
            let told = tx.clone();
            let told_repaint = repaint.clone();
            let result = connection.converse_streaming(
                &turns,
                (!context.is_empty()).then_some(context.as_str()),
                &tools,
                system.as_deref(),
                &worker_flag,
                &mut |far| {
                    let arrived = Arriving {
                        said: far.said.to_owned(),
                        thinking: far.thinking.to_owned(),
                    };
                    if told.send(Answer::Partial(generation, arrived)).is_ok() {
                        told_repaint.request_repaint();
                    }
                },
            );
            let _ = tx.send(Answer::Chat(generation, result));
            repaint.request_repaint();
        });
    }

    fn new_chat(&mut self) {
        self.cancel();
        self.turns.clear();
        self.notes.clear();
        self.tools.clear();
        self.drawn.clear();
        self.model_calls = 0;
        self.summarising = false;
        self.notice = None;
        self.context.clear();
        self.partial = None;
        self.pending.clear();
        self.rewound = None;
        self.recall.forget();
        self.deleting = None;
        self.chat_id.clear();
        self.saved_turns = 0;
        let here = self.documents.pop();
        self.documents.clear();
        self.documents.extend(here);
        let place = self.places.pop();
        self.places.clear();
        self.places.extend(place);
    }

    pub(crate) fn document_arrived(&mut self, name: String, place: String) {
        self.tools.forget_the_names();
        if !place.is_empty() && self.places.last() == Some(&place) {
            return;
        }
        if !place.is_empty() && self.places.contains(&place) {
            self.moved_to(name, place);
            return;
        }
        self.save_the_chat();
        let found = pdf_agent::history::chat_about(&self.the_chats(), &place).cloned();
        if let Some(chat) = found {
            self.take_up(&chat);
            self.notes
                .push((self.turns.len(), Message::AiThisDocumentsChat));
            self.moved_to(name, place);
        } else {
            if !self.turns.is_empty() || self.working() {
                self.new_chat();
            }
            self.documents = vec![name];
            self.places = if place.is_empty() {
                Vec::new()
            } else {
                vec![place]
            };
        }
    }

    fn moved_to(&mut self, name: String, place: String) {
        if !place.is_empty() && !self.places.contains(&place) {
            self.places.push(place);
            self.saved_turns = usize::MAX;
        }
        if self.documents.last() == Some(&name) {
            return;
        }
        if self.turns.is_empty() {
            self.documents.clear();
        } else {
            self.notes
                .push((self.turns.len(), Message::AiNowOnDocument(name.clone())));
            self.saved_turns = usize::MAX;
        }
        self.documents.push(name);
    }

    pub(crate) fn saved_as(&mut self, place: String) {
        if place.is_empty() || self.places.contains(&place) {
            return;
        }
        self.places.push(place);
        self.saved_turns = usize::MAX;
    }

    fn what_changed_hands(&self) -> Option<String> {
        if self.documents.len() < 2 {
            return None;
        }
        let named: Vec<String> = self
            .documents
            .iter()
            .map(|name| pdf_agent::tools::as_data(name, 120))
            .collect();
        Some(format!(
            "\n\nThis conversation has moved between documents: {}. Only the last is open now. \
             Block names, page numbers and text quoted earlier belong to whichever document was \
             open when they were said: read the open document again before you change it.",
            named.join(", then "),
        ))
    }

    pub(crate) fn working(&self) -> bool {
        (self.busy() && !self.checking) || self.tools.busy()
    }

    fn ready(&self) -> bool {
        !self.base_url.is_empty() && !self.model.is_empty() && !self.key_missing()
    }

    fn key_missing(&self) -> bool {
        self.key.trim().is_empty()
            && matches!(
                self.provider,
                Provider::OpenAi | Provider::Claude | Provider::Gemini
            )
    }
}

pub(crate) fn composer_id() -> egui::Id {
    egui::Id::new("ai-composer-text")
}

impl Window {
    pub(crate) fn open_ai_panel(&mut self, now: f64) {
        if self.ai.open {
            return;
        }
        self.ai.open = true;
        self.ai.focus_composer = true;
        self.ai_flow = Some(crate::room::Flow::new(0.0, self.ai.width, now));
    }

    pub(crate) fn ai_panel_rect(&self) -> Option<egui::Rect> {
        (!self.home && self.ai.open)
            .then_some(self.ai_panel_shape)
            .flatten()
    }

    fn the_chat_handle(&mut self, ui: &mut egui::Ui) {
        let title = Message::AiTitle.say(self.lang);
        let badge = self.ai.badge();
        let mut open = false;
        egui::Panel::right("ai edge")
            .resizable(false)
            .exact_size(EDGE_WIDE)
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                let strip = ui.max_rect();
                let handle = ui.interact(
                    strip,
                    egui::Id::new("ai-panel-handle"),
                    egui::Sense::click_and_drag(),
                );
                let live = handle.hovered() || handle.dragged();
                if live {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                }
                if handle.clicked() || handle.drag_delta().x < -2.0 {
                    open = true;
                }
                let visuals = ui.visuals();
                let hairline = visuals.widgets.noninteractive.bg_stroke;
                let painter = ui.painter();
                painter.rect_filled(strip, 0.0, visuals.panel_fill);
                painter.line_segment([strip.left_top(), strip.left_bottom()], hairline);
                let tall = crate::room::handle_tall(strip.height());
                let tab = egui::Rect::from_center_size(
                    strip.center(),
                    egui::vec2(strip.width() - 4.0, tall),
                );
                let (fill, ink) = if live {
                    (
                        visuals.widgets.hovered.bg_fill,
                        visuals.selection.stroke.color,
                    )
                } else {
                    (visuals.widgets.inactive.bg_fill, visuals.weak_text_color())
                };
                painter.rect_filled(tab, 4.0, fill);
                painter.rect_stroke(tab, 4.0, hairline, egui::StrokeKind::Inside);
                let middle = tab.center();
                let arm = egui::Stroke::new(1.6, ink);
                painter.line_segment(
                    [
                        egui::pos2(middle.x + 2.0, middle.y - 4.0),
                        egui::pos2(middle.x - 2.0, middle.y),
                    ],
                    arm,
                );
                painter.line_segment(
                    [
                        egui::pos2(middle.x - 2.0, middle.y),
                        egui::pos2(middle.x + 2.0, middle.y + 4.0),
                    ],
                    arm,
                );
                if let Some(badge) = badge {
                    paint_badge(ui, egui::pos2(tab.center().x, tab.top() + 7.0), badge);
                }
                handle.on_hover_text(title);
            });
        if open {
            let now = ui.input(|input| input.time);
            self.open_ai_panel(now);
        }
    }

    fn chat_width_now(&mut self, ui: &egui::Ui) -> (f32, bool) {
        let now = ui.input(|input| input.time);
        let wanted = if self.ai.open { self.ai.width } else { 0.0 };
        let drifting = self
            .ai_flow
            .filter(|flow| (flow.to() - wanted).abs() <= crate::room::SAME_WIDTH);
        if let Some(flow) = drifting
            && flow.running(now)
        {
            return (flow.at(now), true);
        }
        self.ai_flow = None;
        (wanted, false)
    }

    pub(crate) fn ai_panel(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if self.home {
            self.ai_panel_shape = None;
            return;
        }
        let (width, moving) = self.chat_width_now(ui);
        if moving {
            ctx.request_repaint();
        }
        if width < crate::room::PANEL_GONE {
            self.ai_panel_shape = None;
            self.the_chat_handle(ui);
            return;
        }
        let lang = self.lang;
        let page = (self.editor.page_count() > 0).then_some(self.focus + 1);
        let mut asked = composer::Asked::default();
        let mut close = false;
        let mut answered = None;
        let mut noticed = cards::Noticed::default();
        let mut undo_the_run = false;
        let frame = egui::Frame::side_top_panel(&ui.style().clone())
            .inner_margin(egui::Margin::symmetric(8, 8));
        let was_moving = std::mem::replace(&mut self.ai.was_moving, moving);
        let settling = moving || was_moving;
        let mut panel = egui::Panel::right("ai chat")
            .resizable(!settling)
            .default_size(self.ai.width)
            .size_range(300.0..=640.0)
            .frame(frame);
        if settling {
            panel = panel.exact_size(width);
        }
        let panel = panel.show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(GAP, GAP * 0.75);
            let panel_height = ui.available_height();
            close = self.ai.the_heading(ui, lang);
            look::hairline(ui);
            let card = self.ai.settings_open || !self.ai.ready();
            if card {
                self.ai.the_connection(ui, &ctx, lang);
            } else if self.ai.turns.is_empty() {
                self.ai.the_summary(ui, lang);
            }
            let row_height = ui.text_style_height(&egui::TextStyle::Body);
            let rows = ai_layout::rows_shown(
                self.ai.wrapped_rows,
                ai_layout::most_rows(panel_height, row_height, self.ai.composer_chrome),
            );
            let bottom = egui::Panel::bottom("ai bottom")
                .resizable(false)
                .show_separator_line(false)
                .frame(egui::Frame::NONE)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(GAP, GAP * 0.75);
                    if !card && let Some(notice) = self.ai.notice.clone() {
                        noticed = cards::the_notice(ui, &notice, lang);
                        if let Some(action) = noticed.chosen {
                            self.ai.notice = None;
                            self.ai.act_on_the_notice_later = Some(action);
                        }
                        if noticed.dismissed {
                            self.ai.notice = None;
                        }
                    }
                    if !self.ai.tools.plan.is_empty() {
                        cards::the_plan(ui, &self.ai.tools.plan, lang);
                    }
                    if !self.ai.working() && self.ai.tools.run.steps() > 0 {
                        undo_the_run = cards::the_run_strip(ui, self.ai.tools.run.steps(), lang);
                    }
                    asked = self
                        .ai
                        .the_composer(ui, &ctx, lang, page, (rows, row_height));
                });
            let text_height = ai_layout::composer_height(rows, row_height, 0.0);
            self.ai.composer_chrome =
                (bottom.response.rect.height() - text_height).clamp(60.0, 400.0);
            egui::CentralPanel::no_frame().show(ui, |ui| {
                answered = self.ai.the_conversation(ui, lang);
            });
        });
        self.ai_panel_shape = Some(panel.response.rect);
        if !settling {
            self.ai.width = panel.response.rect.width().clamp(300.0, 640.0);
        }
        if undo_the_run {
            self.ai.undo_the_run_asked = true;
        }
        self.hear_the_panel(&ctx, (asked, answered), close);
    }

    fn hear_the_panel(
        &mut self,
        ctx: &egui::Context,
        (asked, answered): (composer::Asked, Option<Allowed>),
        close: bool,
    ) {
        if let Some(answer) = answered {
            self.ai.tools.answer_the_card(answer);
            ctx.request_repaint();
        }
        self.ai.confirm_full_access(ctx, self.lang);
        if asked.attach {
            self.choosing_for = crate::page_actions::Choosing::ChatAttachment;
            self.asking_to_open = true;
        }
        if asked.asked {
            let brief = self.document_brief();
            let here = self.whereabouts();
            let page_text = self
                .ai
                .include_context
                .then(|| self.text_of_the_page_on_screen(MAX_CONTEXT));
            let context = pdf_agent::tools::question_context(&here, page_text.as_deref());
            self.ai.start_answer(ctx, context, &brief);
            ctx.request_repaint();
        }
        if let Some(action) = self.ai.act_on_the_notice_later.take() {
            let brief = self.document_brief();
            self.ai.act_on_the_notice(ctx, &brief, action);
            ctx.request_repaint();
        }
        if std::mem::take(&mut self.ai.undo_the_run_asked) {
            self.take_the_run_back();
            ctx.request_repaint();
        }
        if close {
            self.ai.open = false;
            let now = ctx.input(|input| input.time);
            self.ai_flow = Some(crate::room::Flow::new(self.ai.width, 0.0, now));
        }
    }
}

const EDGE_WIDE: f32 = 14.0;

pub(crate) fn paint_badge(ui: &egui::Ui, at: egui::Pos2, badge: Badge) {
    let visuals = ui.visuals();
    match badge {
        Badge::NeedsYou => {
            ui.painter().circle_filled(at, 3.5, visuals.warn_fg_color);
        }
        Badge::Working => {
            let time = ui.input(|input| input.time);
            let beat = 0.5 + 0.5 * (time * std::f64::consts::TAU * 0.9).sin();
            #[expect(clippy::cast_possible_truncation, reason = "a share of one")]
            let share = (0.4 + 0.6 * beat) as f32;
            ui.painter().circle_filled(
                at,
                3.5,
                visuals.selection.stroke.color.gamma_multiply(share),
            );
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(60));
        }
    }
}

fn tool_icon(name: &str) -> Icon {
    match name {
        "document_info" | "read_text" | "list_fonts" => Icon::Document,
        "find_text" => Icon::ZoomIn,
        "render_page" => Icon::Picture,
        "replace_text" | "set_properties" | "fill_field" => Icon::Edit,
        "add_text" => Icon::Text,
        "write_pages" => Icon::Pen,
        "add_blank_page" => Icon::NewDocument,
        "insert_pages" => Icon::Open,
        "delete_pages" => Icon::Delete,
        "move_pages" => Icon::Arrange,
        "rotate_pages" => Icon::RotateRight,
        "undo" => Icon::Undo,
        "redo" => Icon::Redo,
        "ask_person" => Icon::RadioButton,
        "update_plan" => Icon::Checkbox,
        _ => Icon::Settings,
    }
}

const fn effort_said(effort: Effort) -> Message {
    match effort {
        Effort::Off => Message::AiEffortOff,
        Effort::None => Message::AiEffortNone,
        Effort::Low => Message::AiEffortLow,
        Effort::Medium => Message::AiEffortMedium,
        Effort::High => Message::AiEffortHigh,
    }
}

const fn effort_means(effort: Effort) -> Message {
    match effort {
        Effort::Off => Message::AiEffortOffMeans,
        Effort::None => Message::AiEffortNoneMeans,
        Effort::Low => Message::AiEffortLowMeans,
        Effort::Medium => Message::AiEffortMediumMeans,
        Effort::High => Message::AiEffortHighMeans,
    }
}

const fn mode_said(mode: Mode) -> Message {
    match mode {
        Mode::ChatOnly => Message::AiModeChatOnly,
        Mode::AskBeforeChanges => Message::AiModeAskBeforeChanges,
        Mode::DoIt => Message::AiModeDoIt,
        Mode::Free => Message::AiModeFree,
    }
}

const fn mode_means(mode: Mode) -> Message {
    match mode {
        Mode::ChatOnly => Message::AiModeChatOnlyWhat,
        Mode::AskBeforeChanges => Message::AiModeAskBeforeChangesWhat,
        Mode::DoIt => Message::AiModeDoItWhat,
        Mode::Free => Message::AiModeFreeWhat,
    }
}

fn server_path() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|program| program.parent().map(|beside| beside.join("panpdf-mcp")))
        .map_or_else(
            || "panpdf-mcp".to_owned(),
            |path| path.display().to_string(),
        )
}

fn claude_code_line(server: &str) -> String {
    format!("claude mcp add panpdf -- {server}")
}

fn client_configuration(server: &str) -> String {
    let quoted: String = server
        .chars()
        .flat_map(|letter| match letter {
            '\\' => vec!['\\', '\\'],
            '"' => vec!['\\', '"'],
            other => vec![other],
        })
        .collect();
    format!("{{\n  \"mcpServers\": {{\n    \"panpdf\": {{ \"command\": \"{quoted}\" }}\n  }}\n}}")
}

impl Window {
    pub(crate) fn open_the_agents_window(&mut self) {
        self.agents_open = true;
    }

    pub(crate) fn agents_window(&mut self, ctx: &egui::Context) {
        if !self.agents_open {
            return;
        }
        let lang = self.lang;
        let say = |message: Message| message.say(lang);
        let server = server_path();
        let mut open = self.agents_open;
        egui::Window::new(say(Message::AgentsTitle))
            .open(&mut open)
            .resizable(true)
            .default_width(520.0)
            .show(ctx, |ui| {
                ui.label(say(Message::AgentsWhat));
                for (heading, snippet) in [
                    (Message::AgentsForClaudeCode, claude_code_line(&server)),
                    (Message::AgentsForClients, client_configuration(&server)),
                ] {
                    ui.separator();
                    ui.strong(say(heading));
                    ui.add(
                        egui::TextEdit::multiline(&mut snippet.as_str())
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY),
                    );
                    if ui.button(say(Message::DraftCopy)).clicked() {
                        ui.ctx().copy_text(snippet.clone());
                    }
                }
                ui.separator();
                ui.small(say(Message::AgentsNote));
            });
        self.agents_open = open;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use eframe::egui;

    use pdf_agent::connect::{ConnectError, Reply, Said, ToolCall, ToolResult};
    use pdf_agent::json::Json;

    use super::{
        AiState, Answer, Arriving, Asking, MOST_MODEL_CALLS, Notice, NoticeAction, Provider, Turn,
        claude_code_line, client_configuration, notice_of,
    };
    use pdf_app::ai_layout;
    use pdf_app::wording::{Lang, Message};

    fn arriving(said: &str) -> Arriving {
        Arriving {
            said: said.to_owned(),
            thinking: String::new(),
        }
    }

    fn said(text: &str) -> Reply {
        Reply {
            text: text.to_owned(),
            ..Reply::default()
        }
    }

    fn asking_state() -> (mpsc::Sender<Answer>, AiState) {
        let (send, answers) = mpsc::channel();
        let state = AiState {
            asking: Some(Asking {
                stop: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                answers,
            }),
            ..AiState::default()
        };
        (send, state)
    }

    fn asks_for(tools: &[&str]) -> Reply {
        Reply {
            calls: tools
                .iter()
                .enumerate()
                .map(|(at, name)| ToolCall::asked(format!("call_{at}"), *name, Json::Null))
                .collect(),
            ..said("on it")
        }
    }

    fn the_reply_arrives(state: &mut AiState, reply: Reply) {
        let (send, answers) = mpsc::channel();
        state.asking = Some(Asking {
            stop: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            answers,
        });
        send.send(Answer::Chat(state.generation, Ok(reply)))
            .expect("the worker answers");
        state.poll(&egui::Context::default());
    }

    #[test]
    fn a_new_panel_holds_no_key_and_no_model() {
        let state = AiState::default();
        assert!(state.key.is_empty());
        assert!(state.model.is_empty());
        assert!(!state.connected);
        assert!(!state.busy());
        assert_eq!(state.base_url, Provider::OpenAi.base_url());
    }

    #[test]
    fn disconnecting_forgets_the_key_and_stops_the_question() {
        let (_send, answers) = mpsc::channel::<Answer>();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut state = AiState {
            key: "a-secret".to_owned(),
            connected: true,
            asking: Some(Asking {
                stop: std::sync::Arc::clone(&stop),
                answers,
            }),
            ..AiState::default()
        };
        state.disconnect();
        assert!(state.key.is_empty());
        assert!(!state.connected);
        assert!(!state.busy());
        assert!(
            stop.load(std::sync::atomic::Ordering::Relaxed),
            "the worker is told to stop, not left asking"
        );
    }

    #[test]
    fn an_answer_from_the_old_connection_is_not_shown() {
        let (send, answers) = mpsc::channel();
        let mut state = AiState {
            asking: Some(Asking {
                stop: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                answers,
            }),
            ..AiState::default()
        };
        let asked_under = state.generation;
        state.invalidate();
        assert!(state.generation > asked_under);
        send.send(Answer::Chat(asked_under, Ok(said("stale"))))
            .expect("the worker answers");
        let context = egui::Context::default();
        state.poll(&context);
        assert!(
            state.turns.is_empty(),
            "an answer to a question about another connection is not an answer to this one"
        );
        assert!(!state.busy());
    }

    #[test]
    fn an_answer_becomes_the_model_s_turn() {
        let (send, answers) = mpsc::channel();
        let mut state = AiState {
            settings_open: true,
            asking: Some(Asking {
                stop: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                answers,
            }),
            ..AiState::default()
        };
        state.turns.push(Turn::person("what is this page about?"));
        send.send(Answer::Chat(state.generation, Ok(said("a map"))))
            .expect("the worker answers");
        state.poll(&egui::Context::default());
        assert_eq!(state.turns.len(), 2);
        assert_eq!(state.turns[1], Turn::model("a map"));
        assert!(!state.settings_open, "an answer settles the connection");
        assert!(state.connected);
    }

    #[test]
    fn the_answer_so_far_is_held_while_it_is_still_arriving() {
        let (send, answers) = mpsc::channel();
        let mut state = AiState {
            asking: Some(Asking {
                stop: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                answers,
            }),
            ..AiState::default()
        };
        state.turns.push(Turn::person("what is this page about?"));
        for said in ["a", "a map"] {
            send.send(Answer::Partial(state.generation, arriving(said)))
                .expect("the worker streams");
        }
        state.poll(&egui::Context::default());
        assert_eq!(
            state.partial.as_ref().map(|far| far.said.as_str()),
            Some("a map")
        );
        assert_eq!(state.turns.len(), 1, "{:?}", state.turns);
        assert!(state.busy(), "the reading is not over");
    }

    #[test]
    fn an_answer_that_ran_out_of_room_offers_to_go_on() {
        for (cut_short, notes) in [(true, 1), (false, 0)] {
            let (send, answers) = mpsc::channel();
            let mut state = AiState {
                asking: Some(Asking {
                    stop: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                    answers,
                }),
                ..AiState::default()
            };
            state.turns.push(Turn::person("go on then"));
            let reply = Reply {
                cut_short,
                ..said("half of an ans")
            };
            send.send(Answer::Chat(state.generation, Ok(reply)))
                .expect("the worker answers");
            state.poll(&egui::Context::default());
            assert_eq!(
                state.notice.is_some(),
                notes == 1,
                "cut_short = {cut_short}"
            );
            if cut_short {
                assert_eq!(
                    state.notice,
                    Some(Notice::with(
                        Message::AiAnswerCutShort,
                        NoticeAction::Continue
                    ))
                );
            }
        }
    }

    #[test]
    fn the_finished_answer_replaces_the_half_written_one() {
        let (send, answers) = mpsc::channel();
        let mut state = AiState {
            asking: Some(Asking {
                stop: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                answers,
            }),
            ..AiState::default()
        };
        state.turns.push(Turn::person("what is this page about?"));
        send.send(Answer::Partial(state.generation, arriving("a ma")))
            .expect("the worker streams");
        send.send(Answer::Chat(state.generation, Ok(said("a map"))))
            .expect("the worker answers");
        state.poll(&egui::Context::default());
        assert_eq!(state.partial, None);
        assert_eq!(state.turns.len(), 2);
        assert_eq!(state.turns[1], Turn::model("a map"));
    }

    #[test]
    fn stopping_keeps_the_part_of_the_answer_that_arrived() {
        let (send, answers) = mpsc::channel();
        let mut state = AiState {
            asking: Some(Asking {
                stop: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                answers,
            }),
            ..AiState::default()
        };
        state.turns.push(Turn::person("tell me about this page"));
        send.send(Answer::Partial(
            state.generation,
            arriving("it is a map of"),
        ))
        .expect("the worker streams");
        state.poll(&egui::Context::default());
        state.cancel();
        assert_eq!(state.turns.len(), 2);
        assert_eq!(state.turns[1], Turn::model("it is a map of"));
        assert_eq!(state.partial, None);

        let (_send, answers) = mpsc::channel();
        let mut nothing_yet = AiState {
            asking: Some(Asking {
                stop: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                answers,
            }),
            ..AiState::default()
        };
        nothing_yet.turns.push(Turn::person("tell me"));
        nothing_yet.cancel();
        assert_eq!(nothing_yet.turns.len(), 1, "nothing arrived, nothing kept");
    }

    #[test]
    fn a_partial_from_the_old_connection_is_not_shown() {
        let (send, answers) = mpsc::channel();
        let mut state = AiState {
            asking: Some(Asking {
                stop: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                answers,
            }),
            ..AiState::default()
        };
        let asked_under = state.generation;
        state.invalidate();
        send.send(Answer::Partial(asked_under, arriving("stale")))
            .expect("the worker streams");
        state.poll(&egui::Context::default());
        assert_eq!(state.partial, None);
    }

    #[test]
    fn a_refused_question_is_given_back_to_be_asked_again() {
        let (send, answers) = mpsc::channel();
        let mut state = AiState {
            asking: Some(Asking {
                stop: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                answers,
            }),
            ..AiState::default()
        };
        state.turns.push(Turn::person("summarise this"));
        send.send(Answer::Chat(
            state.generation,
            Err(ConnectError::Http("401 Unauthorized".to_owned())),
        ))
        .expect("the worker answers");
        state.poll(&egui::Context::default());
        assert_eq!(state.composer, "summarise this");
        assert!(state.turns.is_empty(), "nothing was said after all");
        assert_eq!(
            state.notice,
            Some(Notice {
                said: Message::AiServiceRefused,
                detail: Some("401 Unauthorized".to_owned()),
                action: Some(NoticeAction::OpenSettings),
            })
        );
    }

    #[test]
    fn a_new_chat_forgets_the_conversation() {
        let mut state = AiState::default();
        state.turns.push(Turn::person("one"));
        state.turns.push(Turn::model("two"));
        state.notice = Some(Notice::plain(Message::AiStopped));
        let before = state.generation;
        state.new_chat();
        assert!(state.turns.is_empty());
        assert!(state.notice.is_none());
        assert!(state.generation > before, "an answer on its way is dropped");
    }

    #[test]
    fn a_provider_s_name_reads_back_as_itself() {
        for provider in Provider::ALL {
            assert_eq!(Provider::by_name(provider.name()), Some(provider));
        }
        assert_eq!(Provider::by_name("Something else"), None);
    }

    #[test]
    fn only_the_hosted_providers_ask_for_a_key() {
        let asks = |provider| {
            AiState {
                provider,
                ..AiState::default()
            }
            .key_missing()
        };
        assert!(asks(Provider::OpenAi) && asks(Provider::Claude) && asks(Provider::Gemini));
        assert!(!asks(Provider::Ollama) && !asks(Provider::LmStudio));
    }

    #[test]
    fn a_worker_that_stopped_without_answering_is_said_so() {
        let (send, answers) = mpsc::channel::<Answer>();
        let mut state = AiState {
            asking: Some(Asking {
                stop: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                answers,
            }),
            ..AiState::default()
        };
        drop(send);
        let context = egui::Context::default();
        state.poll(&context);
        assert!(!state.busy());
        assert_eq!(state.notice, Some(Notice::plain(Message::AiWorkerStopped)));
        assert!(!Message::AiWorkerStopped.say(Lang::English).is_empty());
    }

    #[test]
    fn every_way_a_connection_fails_is_said_differently() {
        let every = [
            ConnectError::Invalid("no model".to_owned()),
            ConnectError::Cancelled,
            ConnectError::Curl("could not connect to host".to_owned()),
            ConnectError::ResponseTooLarge,
            ConnectError::Http("401 Unauthorized".to_owned()),
            ConnectError::Protocol("no choices in the answer".to_owned()),
            ConnectError::TooLarge("2200000 bytes of words".to_owned()),
        ];
        let notices: Vec<Notice> = every.iter().map(notice_of).collect();
        for (at, notice) in notices.iter().enumerate() {
            for (also, other) in notices.iter().enumerate() {
                assert!(
                    at == also || notice.said != other.said,
                    "{:?} and {:?} are said the same way",
                    every[at],
                    every[also]
                );
            }
            for lang in Lang::ALL {
                assert!(!notice.said.say(*lang).trim().is_empty());
            }
        }
        assert_eq!(
            notices[4],
            Notice {
                said: Message::AiServiceRefused,
                detail: Some("401 Unauthorized".to_owned()),
                action: Some(NoticeAction::OpenSettings),
            }
        );
        assert_eq!(
            notices[0].action,
            Some(NoticeAction::OpenSettings),
            "a connection that cannot be used is fixed in the settings"
        );
        assert!(
            notices[1..4]
                .iter()
                .chain(&notices[5..])
                .all(|notice| notice.action.is_none()),
            "nothing else is mended by the settings"
        );
        assert_eq!(notices[1], Notice::plain(Message::AiStopped));
        assert_eq!(notices[3], Notice::plain(Message::AiAnswerTooLarge));
        assert_eq!(
            notices[2].detail.as_deref(),
            Some("could not connect to host"),
            "the provider's own words are kept, not translated away"
        );
        assert_eq!(notices[0].detail.as_deref(), Some("no model"));
        assert_eq!(
            notices[5].detail.as_deref(),
            Some("no choices in the answer")
        );
    }

    #[test]
    fn enter_alone_sends_and_enter_with_a_modifier_does_not() {
        fn frame_with(modifiers: egui::Modifiers) -> egui::RawInput {
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers,
                }],
                ..Default::default()
            }
        }
        fn asked_in(raw: egui::RawInput, take: fn(&mut egui::Ui) -> bool) -> bool {
            let mut asked = false;
            let context = egui::Context::default();
            let _ = context.run_ui(raw, |ui| asked = take(ui));
            asked
        }
        let plain = |ui: &mut egui::Ui| super::plain_enter(ui);
        assert!(asked_in(frame_with(egui::Modifiers::NONE), plain));
        assert!(!asked_in(frame_with(egui::Modifiers::SHIFT), plain));
        assert!(!asked_in(frame_with(egui::Modifiers::CTRL), plain));
        assert!(!asked_in(frame_with(egui::Modifiers::ALT), plain));
        assert!(!asked_in(egui::RawInput::default(), plain));
        let logically = |ui: &mut egui::Ui| {
            ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
        };
        assert!(asked_in(frame_with(egui::Modifiers::SHIFT), logically));
    }

    #[test]
    fn an_empty_box_asks_for_the_two_rows_it_starts_with() {
        let state = AiState::default();
        assert_eq!(state.wrapped_rows, ai_layout::LEAST_ROWS);
        assert_eq!(
            ai_layout::rows_shown(state.wrapped_rows, ai_layout::most_rows(600.0, 18.0, 44.0)),
            2
        );
    }

    #[test]
    fn what_the_tools_answered_goes_back_as_one_turn() {
        let mut state = AiState::default();
        state.turns.push(Turn::person("change the heading"));
        state.round_done(vec![
            ToolResult::said("call_1", "Done. p2-b3 now reads: X"),
            ToolResult::failed("call_2", "there is no page 40"),
        ]);
        assert_eq!(state.turns.len(), 2);
        let last = state.turns.last().expect("the results are a turn");
        assert_eq!(last.said(), Said::Person);
        assert_eq!(last.text(), "");
        assert!(last.calls().is_empty());
    }

    #[test]
    fn the_model_may_ask_for_tools_until_the_budget_is_spent_and_not_after() {
        for (made, queued) in [(MOST_MODEL_CALLS - 1, 2), (MOST_MODEL_CALLS, 0)] {
            let (_send, mut state) = asking_state();
            state.turns.push(Turn::person("tidy the whole document"));
            state.model_calls = made;
            the_reply_arrives(&mut state, asks_for(&["read_text", "find_text"]));
            assert_eq!(state.tools.queue.len(), queued, "after {made} calls");
        }
    }

    #[test]
    fn a_reply_asking_for_tools_past_the_budget_is_not_run_and_the_model_is_asked_to_sum_up() {
        let (_send, mut state) = asking_state();
        state.turns.push(Turn::person("tidy the whole document"));
        state.model_calls = MOST_MODEL_CALLS;
        the_reply_arrives(&mut state, asks_for(&["replace_text", "delete_pages"]));
        assert!(state.tools.queue.is_empty(), "nothing in the batch ran");
        let Some(Turn::Results { results }) = state.turns.last() else {
            panic!("the batch is answered as a turn: {:?}", state.turns);
        };
        assert_eq!(results.len(), 2);
        assert!(
            results
                .iter()
                .all(|result| result.is_error && result.text.starts_with("not run")),
            "{results:?}"
        );
        assert!(state.summarising, "and the last call asks for a summary");
        assert!(state.busy(), "that call is on its way");
        assert_eq!(state.model_calls, MOST_MODEL_CALLS + 1);
    }

    #[test]
    fn the_call_that_sums_up_the_work_withholds_every_tool() {
        let mut state = AiState::default();
        let (tools, system) = state.what_is_offered();
        assert_eq!(tools.len(), pdf_agent::tools::offered_to_a_window().len());
        assert!(
            !system
                .as_deref()
                .unwrap_or_default()
                .contains("Call no tool now"),
            "a normal call is not told to stop calling"
        );
        state.summarising = true;
        let (tools, system) = state.what_is_offered();
        assert!(tools.is_empty(), "{} tools were offered", tools.len());
        assert!(
            system
                .as_deref()
                .unwrap_or_default()
                .contains("Call no tool now"),
            "{system:?}"
        );
        state.mode = pdf_app::ai_permission::Mode::ChatOnly;
        assert_eq!(
            state.what_is_offered(),
            (Vec::new(), None),
            "chat only offers nothing"
        );
    }

    #[test]
    fn after_the_summary_the_person_may_continue_and_the_budget_starts_again() {
        let (_send, mut state) = asking_state();
        state.turns.push(Turn::person("tidy the whole document"));
        state.model_calls = MOST_MODEL_CALLS;
        state.summarising = true;
        the_reply_arrives(&mut state, said("Done: pages 1 to 5. Left: the footers."));
        assert!(!state.summarising);
        assert_eq!(
            state.notice,
            Some(Notice::with(Message::AiStepsUsedUp, NoticeAction::Continue))
        );
        assert!(!state.busy(), "the run is over until the person says go on");

        state.act_on_the_notice(
            &egui::Context::default(),
            &pdf_agent::tools::DocumentBrief::default(),
            NoticeAction::Continue,
        );
        assert!(state.notice.is_none());
        assert_eq!(
            state.model_calls, 1,
            "a fresh budget, and this is its first call"
        );
        assert!(state.busy(), "the model is asked again");
        let Some(Turn::Person { text, .. }) = state.turns.last() else {
            panic!("going on is a person's turn: {:?}", state.turns.last());
        };
        assert!(text.contains("go on"), "{text}");
    }

    #[test]
    fn stopping_keeps_the_results_that_were_in_and_says_the_rest_were_not_run() {
        let mut state = AiState::default();
        state.turns.push(Turn::person("fix every heading"));
        state.turns.push(Turn::Model {
            text: String::new(),
            calls: ["a", "b", "c"]
                .iter()
                .map(|id| ToolCall::asked(*id, "replace_text", Json::Null))
                .collect(),
            raw: None,
        });
        state.tools.take(&[
            ToolCall::asked("a", "replace_text", Json::Null),
            ToolCall::asked("b", "replace_text", Json::Null),
            ToolCall::asked("c", "replace_text", Json::Null),
        ]);
        state.tools.queue.pop_front();
        state.tools.results.push(ToolResult::said("a", "Done."));
        state.stop_the_run();
        assert!(!state.tools.busy());
        let Some(Turn::Results { results }) = state.turns.last() else {
            panic!("what was done is kept as a turn: {:?}", state.turns);
        };
        assert_eq!(results[0], ToolResult::said("a", "Done."));
        let rest: Vec<(&str, &str)> = results[1..]
            .iter()
            .map(|result| (result.call_id.as_str(), result.text.as_str()))
            .collect();
        assert_eq!(
            rest,
            [
                ("b", "not run: the person stopped the assistant"),
                ("c", "not run: the person stopped the assistant")
            ]
        );
        assert!(results[1..].iter().all(|result| result.is_error));
        let mut again = state.turns.clone();
        pdf_agent::connect::settle_dangling_calls(&mut again);
        assert_eq!(again, state.turns, "no call is left without its answer");
    }

    #[test]
    fn a_new_question_while_tools_run_keeps_the_finished_results_and_then_asks() {
        let mut state = AiState::default();
        state.turns.push(Turn::person("fix every heading"));
        state.turns.push(Turn::Model {
            text: String::new(),
            calls: ["a", "b"]
                .iter()
                .map(|id| ToolCall::asked(*id, "replace_text", Json::Null))
                .collect(),
            raw: None,
        });
        state
            .tools
            .take(&[ToolCall::asked("b", "replace_text", Json::Null)]);
        state
            .tools
            .results
            .push(ToolResult::said("a", "Done. p1-b1 now reads: X"));
        "actually, only the first page".clone_into(&mut state.composer);
        state.start_answer(
            &egui::Context::default(),
            String::new(),
            &pdf_agent::tools::DocumentBrief::default(),
        );
        let kinds: Vec<&str> = state
            .turns
            .iter()
            .map(|turn| match turn {
                Turn::Person { .. } => "person",
                Turn::Model { .. } => "model",
                Turn::Results { .. } => "results",
            })
            .collect();
        assert_eq!(kinds, ["person", "model", "results", "person"]);
        let Turn::Results { results } = &state.turns[2] else {
            panic!("results");
        };
        assert_eq!(results.len(), 2);
        assert!(!results[0].is_error && results[1].is_error);
        assert!(state.busy(), "and the new question is on its way");
        assert!(!state.tools.busy());
    }

    #[test]
    fn a_question_that_failed_comes_back_with_its_pictures_and_even_with_no_words() {
        for words in ["describe this", ""] {
            let (send, mut state) = asking_state();
            state.turns.push(Turn::person_with(
                words,
                vec![pdf_agent::connect::Attachment::image(
                    "cover.png",
                    "image/png",
                    vec![1, 2, 3],
                )],
            ));
            send.send(Answer::Chat(
                state.generation,
                Err(ConnectError::Http("500 Internal".to_owned())),
            ))
            .expect("the worker answers");
            state.poll(&egui::Context::default());
            assert!(state.turns.is_empty(), "nothing was said after all");
            assert_eq!(state.composer, words);
            assert_eq!(state.pending.len(), 1, "the picture comes back too");
            assert_eq!(state.pending[0].name, "cover.png");
            assert!(state.may_send(), "so it can be sent again as it was");
        }
    }

    #[test]
    fn an_error_in_the_middle_of_a_run_offers_to_try_again_and_keeps_the_work() {
        let (send, mut state) = asking_state();
        state.turns.push(Turn::person("fix every heading"));
        state.turns.push(Turn::Model {
            text: String::new(),
            calls: vec![ToolCall::asked("a", "replace_text", Json::Null)],
            raw: None,
        });
        state.turns.push(Turn::Results {
            results: vec![ToolResult::said("a", "Done.")],
        });
        send.send(Answer::Chat(
            state.generation,
            Err(ConnectError::Curl("timed out".to_owned())),
        ))
        .expect("the worker answers");
        state.poll(&egui::Context::default());
        assert_eq!(state.turns.len(), 3, "what was done stays");
        assert!(state.composer.is_empty());
        let notice = state.notice.clone().expect("a notice");
        assert_eq!(notice.said, Message::AiCouldNotReach);
        assert_eq!(notice.action, Some(NoticeAction::Retry));

        state.act_on_the_notice(
            &egui::Context::default(),
            &pdf_agent::tools::DocumentBrief::default(),
            NoticeAction::Retry,
        );
        assert!(state.busy(), "the same request goes out again");
        assert_eq!(state.turns.len(), 3, "without a new turn");
    }

    #[test]
    fn a_conversation_too_large_to_send_says_so_and_does_not_blame_the_address() {
        let notice = notice_of(&ConnectError::TooLarge("2200000 bytes of words".to_owned()));
        assert_eq!(notice.said, Message::AiConversationTooLarge);
        let english = notice.said.say(Lang::English);
        assert!(english.contains("too large"), "{english}");
        assert!(
            !english.contains("address") && !english.contains("model"),
            "{english}"
        );
        assert_eq!(notice.detail.as_deref(), Some("2200000 bytes of words"));
    }

    #[test]
    fn a_saved_chat_brings_its_plan_back() {
        let steps = r#"{"steps":[{"text":"Read","status":"done"},{"text":"Write","status":"in_progress"}]}"#;
        let chat = pdf_agent::history::Chat {
            id: "000000000001".to_owned(),
            turns: vec![
                Turn::person("write the report"),
                Turn::Model {
                    text: String::new(),
                    calls: vec![ToolCall::asked(
                        "p1",
                        "update_plan",
                        Json::parse(steps).expect("JSON"),
                    )],
                    raw: None,
                },
                Turn::Results {
                    results: vec![ToolResult::said("p1", "Plan: 1 of 2 steps done.")],
                },
            ],
            ..pdf_agent::history::Chat::default()
        };
        let written = pdf_agent::history::write(&chat);
        let back = pdf_agent::history::read(&written).expect("it reads");
        let mut state = AiState::default();
        state.take_up(&back);
        assert_eq!(state.tools.plan.len(), 2);
        assert_eq!(state.tools.plan[1].text, "Write");
        assert_eq!(
            state.tools.plan[1].state,
            pdf_agent::tools::request::StepState::InProgress
        );
        state.new_chat();
        assert!(state.tools.plan.is_empty(), "a new chat has no plan");
    }

    #[test]
    fn ask_again_is_offered_and_works_after_an_answer_that_used_tools() {
        let mut state = AiState {
            turns: vec![
                Turn::person("fix the typos on page 2"),
                Turn::Model {
                    text: String::new(),
                    calls: vec![ToolCall::asked("a", "replace_text", Json::Null)],
                    raw: None,
                },
                Turn::Results {
                    results: vec![ToolResult::said("a", "Done.")],
                },
                Turn::model("Fixed three typos."),
            ],
            ..AiState::default()
        };
        assert_eq!(
            state.last_question(),
            Some(0),
            "the question, not the results"
        );
        state.go_back_to(0, true);
        assert!(state.turns.is_empty(), "the whole run was taken back");
        assert_eq!(state.composer, "fix the typos on page 2");
        assert!(state.send_now);
    }

    #[test]
    fn forgetting_the_chat_that_is_open_does_not_bring_it_back() {
        let mut state = AiState {
            turns: vec![Turn::person("one"), Turn::model("two")],
            chat_id: "000000000001".to_owned(),
            saved_turns: 2,
            ..AiState::default()
        };
        state.forget_a_chat("000000000002");
        assert_eq!(
            state.turns.len(),
            2,
            "another chat's deletion leaves this one"
        );
        state.forget_a_chat("000000000001");
        assert!(
            state.turns.is_empty(),
            "so the next save has nothing to write"
        );
        assert!(state.chat_id.is_empty());
        state.forget_a_chat("../../elsewhere");
    }

    #[test]
    fn a_new_chat_forgets_what_was_allowed() {
        let mut state = AiState::default();
        state.turns.push(Turn::person("change page 2"));
        state
            .tools
            .allowed_for_chat
            .insert("replace_text".to_owned());
        state
            .notes
            .push((0, Message::AiChangedTheDocument { pages: vec![2] }));
        state.new_chat();
        assert!(state.turns.is_empty());
        assert!(state.notes.is_empty());
        assert!(state.tools.allowed_for_chat.is_empty());
        assert!(!state.tools.busy());
    }

    #[test]
    fn the_snippets_carry_the_servers_own_path() {
        assert_eq!(
            claude_code_line("/opt/panpdf/panpdf-mcp"),
            "claude mcp add panpdf -- /opt/panpdf/panpdf-mcp"
        );
        let windows = client_configuration("C:\\Program Files\\PanPDF\\panpdf-mcp.exe");
        assert!(
            windows.contains("\"command\": \"C:\\\\Program Files\\\\PanPDF\\\\panpdf-mcp.exe\""),
            "{windows}"
        );
        assert!(windows.starts_with('{') && windows.trim_end().ends_with('}'));
    }

    fn a_conversation() -> Vec<Turn> {
        vec![
            Turn::person("one"),
            Turn::model("first answer"),
            Turn::person("three"),
            Turn::model("second answer"),
        ]
    }

    #[test]
    fn going_back_to_a_question_can_be_put_back() {
        let mut state = AiState {
            turns: a_conversation(),
            ..AiState::default()
        };
        assert_eq!(state.last_question(), Some(2));
        state.go_back_to(2, false);
        assert_eq!(state.turns.len(), 2);
        assert_eq!(state.composer, "three");
        assert!(!state.send_now);
        state.put_back();
        assert_eq!(state.turns, a_conversation());
        assert!(state.composer.is_empty());

        state.go_back_to(state.last_question().expect("answered"), true);
        assert_eq!(state.turns.len(), 2);
        assert!(state.send_now, "Ask again sends it");

        let (_tx, rx) = mpsc::channel();
        let mut busy = AiState {
            turns: a_conversation(),
            asking: Some(Asking {
                stop: std::sync::Arc::default(),
                answers: rx,
            }),
            ..AiState::default()
        };
        busy.go_back_to(2, false);
        assert_eq!(busy.turns.len(), 4, "nothing is cut while it is answering");
    }

    #[test]
    fn an_unanswered_question_is_not_asked_again() {
        let state = AiState {
            turns: vec![
                Turn::person("one"),
                Turn::model("two"),
                Turn::person("three"),
            ],
            ..AiState::default()
        };
        assert_eq!(state.last_question(), None);
    }

    #[test]
    fn up_walks_back_through_what_was_asked() {
        use pdf_app::ai_recall::Way;
        let mut state = AiState {
            turns: a_conversation(),
            history: Some(std::sync::Arc::new(vec![pdf_agent::history::Chat {
                id: "old".to_owned(),
                turns: vec![Turn::person("elsewhere")],
                ..pdf_agent::history::Chat::default()
            }])),
            ..AiState::default()
        };
        for want in ["three", "one", "elsewhere"] {
            assert!(state.recall_key(Way::Up, false));
            assert_eq!(state.composer, want);
        }
        assert!(!state.recall_key(Way::Up, false), "nothing older");
        assert!(state.recall_key(Way::Down, false));
        assert!(state.recall_key(Way::Down, false));
        assert_eq!(state.composer, "three");
    }

    fn kept(id: &str, place: &str) -> pdf_agent::history::Chat {
        pdf_agent::history::Chat {
            id: id.to_owned(),
            documents: vec!["x.pdf".to_owned()],
            places: vec![place.to_owned()],
            turns: vec![Turn::person("about x"), Turn::model("x is a report")],
            ..pdf_agent::history::Chat::default()
        }
    }

    #[test]
    fn a_document_brings_back_its_own_chat() {
        let mut state = AiState {
            history: Some(std::sync::Arc::new(vec![kept("kept", "/a/x.pdf")])),
            ..AiState::default()
        };
        state.document_arrived("x.pdf".to_owned(), "/a/x.pdf".to_owned());
        assert_eq!(state.chat_id, "kept");
        assert_eq!(state.turns.len(), 2);

        state.document_arrived("y.pdf".to_owned(), "/b/y.pdf".to_owned());
        assert!(state.turns.is_empty(), "a new chat");
        assert!(state.chat_id.is_empty());
        assert_eq!(state.places, ["/b/y.pdf"]);
        assert_eq!(state.documents, ["y.pdf"]);

        state.document_arrived("x.pdf".to_owned(), "/b/x.pdf".to_owned());
        assert!(state.turns.is_empty(), "the same name elsewhere is not it");
    }

    #[test]
    fn a_chat_carried_to_another_file_is_that_files_too() {
        let mut state = AiState {
            history: Some(std::sync::Arc::new(vec![kept("kept", "/a/x.pdf")])),
            ..AiState::default()
        };
        state.document_arrived("y.pdf".to_owned(), "/b/y.pdf".to_owned());
        state.open_a_chat(&kept("kept", "/a/x.pdf"));
        assert_eq!(state.places, ["/a/x.pdf", "/b/y.pdf"]);
        assert_eq!(state.documents.last().map(String::as_str), Some("y.pdf"));
        state.saved_as("/b/y-edited.pdf".to_owned());
        assert_eq!(state.places.len(), 3);
        let kept_now = pdf_agent::history::Chat {
            places: state.places.clone(),
            ..kept("kept", "/a/x.pdf")
        };
        assert_eq!(
            pdf_agent::history::chat_about(&[kept_now], "/b/y-edited.pdf")
                .map(|chat| chat.id.as_str()),
            Some("kept")
        );
    }
}
