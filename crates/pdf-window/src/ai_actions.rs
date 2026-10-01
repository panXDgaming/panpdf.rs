use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::{Duration, Instant};

use eframe::egui;
use pdf_agent::connect::{Picture, ToolCall, ToolResult};
use pdf_agent::desk::{self, Block, MOST_CHARACTERS};
use pdf_agent::tools::{self, request::PlanStep, request::Request};
use pdf_app::Applied;
use pdf_app::ai_permission::{Decision, Mode, Why, decide, may_allow_all, refusal_text};
use pdf_app::ai_run::{Cannot, Run};
use pdf_app::wording::{Done, Lang, Message};

use crate::window_state::Window;

const WAIT: Duration = Duration::from_secs(30);

const MOST_SEARCHED: usize = 120;

const READ_AHEAD: usize = 8;

const MOST_INSERTED_BYTES: u64 = 256 * 1024 * 1024;

const NOT_RUN: &str = "not run: the person stopped the assistant";

const RENUMBERED: &str = "not run: an earlier call in this reply put pages in, took them out or moved them, so the page \
numbers in this call may no longer mean the pages they meant. Ask again with the numbers as they are now.";

pub(crate) struct Pending {
    pub(crate) call: ToolCall,
    pub(crate) request: Request,
    pub(crate) may_allow_for_chat: bool,
    pub(crate) of_this_tool: usize,
}

struct Sent {
    call: ToolCall,
    request: Request,
    was: Option<(usize, [f64; 4])>,
    success: Option<String>,
    changed: Vec<usize>,
}

pub(crate) struct TakingBack {
    left: usize,
    total: usize,
    started: bool,
    revision: Option<u64>,
}

#[derive(Clone)]
struct Named {
    epoch: u64,
    arranged: u64,
    text: String,
    area: [f64; 4],
}

struct Gathering {
    call: String,
    span: (usize, usize),
    next: usize,
    pages: Vec<(usize, Vec<Block>)>,
    characters: usize,
}

#[derive(Default)]
pub(crate) struct Tools {
    pub(crate) queue: VecDeque<ToolCall>,
    pub(crate) results: Vec<ToolResult>,
    pub(crate) ask: Option<Pending>,
    pub(crate) allowed_for_chat: BTreeSet<String>,
    allowed_now: Option<String>,
    allowed_all: Option<String>,
    waiting: Option<(String, usize, Instant)>,
    sent: Option<Sent>,
    landed: Option<Applied>,
    gathering: Option<Gathering>,
    named: BTreeMap<(usize, usize), Named>,
    arranged: u64,
    renumbered: u64,
    numbered_at_take: u64,
    revision_before: Option<u64>,
    skipped: Vec<ToolCall>,
    stopping: bool,
    announced: Option<String>,
    pub(crate) question: Option<Question>,
    pub(crate) plan: Vec<PlanStep>,
    pub(crate) run: Run,
    pub(crate) taking_back: Option<TakingBack>,
}

pub(crate) struct Question {
    pub(crate) call: String,
    pub(crate) asked: String,
    pub(crate) options: Vec<(String, String)>,
    pub(crate) own: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum QuestionReply {
    Said(String),
    Skipped,
}

pub(crate) enum Doing {
    Asking,
    Allowing,
    Tool(String, Box<Request>),
}

impl Tools {
    pub(crate) fn doing(&self) -> Option<Doing> {
        if self.question.is_some() {
            return Some(Doing::Asking);
        }
        if self.ask.is_some() {
            return Some(Doing::Allowing);
        }
        if let Some(sent) = &self.sent {
            return Some(Doing::Tool(
                sent.call.name.clone(),
                Box::new(sent.request.clone()),
            ));
        }
        let call = self.queue.front()?;
        let request = tools::request::parse(&call.name, &call.arguments).ok()?;
        Some(Doing::Tool(call.name.clone(), Box::new(request)))
    }
}

impl Tools {
    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn drop_the_queue(&mut self) {
        self.queue.clear();
        self.results.clear();
        self.skipped.clear();
        self.ask = None;
        self.waiting = None;
        self.sent = None;
        self.landed = None;
        self.gathering = None;
        self.allowed_now = None;
        self.allowed_all = None;
        self.question = None;
        self.stopping = false;
        self.taking_back = None;
    }

    pub(crate) fn busy(&self) -> bool {
        !self.queue.is_empty()
            || self.ask.is_some()
            || self.sent.is_some()
            || self.question.is_some()
            || self.stopping
            || self.taking_back.is_some()
    }

    pub(crate) fn stop(&mut self) -> Option<Vec<ToolResult>> {
        let waiting_on_an_edit = self.sent.is_some();
        let unrun: Vec<ToolCall> = if waiting_on_an_edit {
            self.queue.drain(1..).collect()
        } else {
            self.queue.drain(..).collect()
        };
        self.skipped.extend(unrun);
        self.ask = None;
        self.question = None;
        self.waiting = None;
        self.gathering = None;
        self.allowed_now = None;
        self.allowed_all = None;
        self.taking_back = None;
        if waiting_on_an_edit {
            self.stopping = true;
            return None;
        }
        Some(self.closing_results())
    }

    fn closing_results(&mut self) -> Vec<ToolResult> {
        self.stopping = false;
        let mut results = std::mem::take(&mut self.results);
        results.extend(
            std::mem::take(&mut self.skipped)
                .into_iter()
                .map(|call| ToolResult::failed(call.id, NOT_RUN)),
        );
        results
    }

    pub(crate) fn answer_the_question(&mut self, reply: QuestionReply) {
        let Some(question) = self.question.take() else {
            return;
        };
        let said = match reply {
            QuestionReply::Said(answer) => format!("The person answered: {answer}"),
            QuestionReply::Skipped => {
                "The person skipped this question without answering. Go on with \
                               your own best judgement, or ask in words if you cannot."
                    .to_owned()
            }
        };
        self.answer(ToolResult::said(&question.call, said));
    }

    pub(crate) fn take(&mut self, calls: &[ToolCall]) {
        if self.queue.is_empty() {
            self.numbered_at_take = self.renumbered;
        }
        for call in calls {
            self.queue.push_back(call.clone());
        }
    }

    pub(crate) fn text_before(&self, request: &Request) -> Option<String> {
        let Request::ReplaceText { block, .. } = request else {
            return None;
        };
        let key = parse_block_name(block).ok()?;
        self.named.get(&key).map(|named| named.text.clone())
    }

    pub(crate) fn waits_for_an_edit(&self) -> bool {
        self.sent.is_some()
    }

    pub(crate) fn note_applied(&mut self, applied: &Applied) {
        if self.sent.is_some() && self.landed.is_none() {
            self.landed = Some(applied.clone());
        }
    }

    pub(crate) fn pages_moved(&mut self) {
        self.arranged += 1;
    }

    pub(crate) fn pages_renumbered(&mut self) {
        self.renumbered += 1;
    }

    pub(crate) fn forget_the_names(&mut self) {
        self.named.clear();
        self.arranged += 1;
    }

    pub(crate) fn takes_it_back(&mut self, steps: usize) {
        self.taking_back = Some(TakingBack {
            left: steps,
            total: steps,
            started: false,
            revision: None,
        });
    }

    pub(crate) fn the_person_is_needed_for(&self) -> Option<String> {
        self.ask
            .as_ref()
            .map(|pending| pending.call.id.clone())
            .or_else(|| self.question.as_ref().map(|question| question.call.clone()))
            .filter(|id| self.announced.as_deref() != Some(id.as_str()))
    }

    pub(crate) fn the_person_was_told(&mut self, call: String) {
        self.announced = Some(call);
    }

    fn answer(&mut self, result: ToolResult) {
        self.queue.pop_front();
        self.waiting = None;
        self.gathering = None;
        self.allowed_now = None;
        self.results.push(result);
    }

    fn answer_a_call_that_could_not_be_read(&mut self, call: &ToolCall) -> bool {
        let Some(problem) = &call.problem else {
            return false;
        };
        self.answer(ToolResult::failed(&call.id, problem));
        true
    }

    fn allowed_just_now(&self, call: &ToolCall) -> bool {
        self.allowed_now.as_deref() == Some(call.id.as_str())
            || self.allowed_all.as_deref() == Some(call.name.as_str())
    }

    pub(crate) fn answer_the_card(&mut self, answer: pdf_app::ai_permission::Answer) {
        use pdf_app::ai_permission::Answer;
        let Some(pending) = self.ask.take() else {
            return;
        };
        match answer {
            Answer::Refuse => {
                self.answer(ToolResult::failed(&pending.call.id, refusal_text()));
            }
            Answer::ForThisChat => {
                self.allowed_for_chat.insert(pending.call.name.clone());
                self.allowed_now = Some(pending.call.id.clone());
            }
            Answer::AllOfThem => {
                self.allowed_all = Some(pending.call.name.clone());
                self.allowed_now = Some(pending.call.id.clone());
            }
            Answer::Once => self.allowed_now = Some(pending.call.id.clone()),
        }
    }
}

enum Performed {
    Done(ToolResult),
    NeedPages(Vec<usize>),
    Sent,
    Busy,
    Waiting,
}

impl Window {
    pub(crate) fn document_brief(&self) -> tools::DocumentBrief {
        tools::DocumentBrief {
            file_name: self
                .opened
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            title: self
                .editor
                .facts()
                .map(|facts| facts.info.title)
                .unwrap_or_default(),
            pages: self.editor.page_count(),
        }
    }

    pub(crate) fn whereabouts(&mut self) -> tools::Whereabouts {
        let selected = match self.pointing {
            crate::window_state::Pointing::Block { page, block }
            | crate::window_state::Pointing::Text { page, block, .. } => self
                .editor
                .leaf(page)
                .map(std::sync::Arc::clone)
                .and_then(|leaf| desk::read_block(&leaf.view, page, block))
                .map(|block| {
                    self.remember_the_name(&block);
                    (block.name(), block.text)
                }),
            _ => None,
        };
        tools::Whereabouts {
            page_on_screen: self.focus + 1,
            pages: self.editor.page_count(),
            selected,
            unsaved: self.unsaved(),
        }
    }

    pub(crate) fn text_of_the_page_on_screen(&self, most: usize) -> String {
        self.editor
            .leaf(self.focus)
            .map(|leaf| desk::text_of_page(&leaf.view, self.focus, most))
            .unwrap_or_default()
    }

    pub(crate) fn keep_the_assistant_going(&mut self, ctx: &egui::Context) {
        self.ai.poll(ctx);
        self.advance_tools(ctx);
        self.carry_on_taking_the_run_back();
        self.show_what_needs_the_person(ctx);
        self.ai.save_the_chat();
    }

    pub(crate) fn take_the_run_back(&mut self) {
        if self.running.is_some() || self.editor.is_busy() || self.ai.working() {
            return;
        }
        match self.ai.tools.run.may_take_back(self.editor.revision()) {
            Ok(steps) => self.ai.tools.takes_it_back(steps),
            Err(Cannot::PersonEdited) => self.ai.say_the_run_cannot_be_taken_back(),
            Err(Cannot::NothingOfTheirs) => {}
        }
    }

    pub(crate) fn carry_on_taking_the_run_back(&mut self) {
        let Some(mut taking) = self.ai.tools.taking_back.take() else {
            return;
        };
        if self.running.is_some() || self.editor.is_busy() {
            self.ai.tools.taking_back = Some(taking);
            return;
        }
        let revision = self.editor.revision();
        if std::mem::take(&mut taking.started) {
            if revision == taking.revision {
                return;
            }
            self.ai.tools.run.took_one_back(revision);
            taking.left -= 1;
        }
        if taking.left == 0 {
            self.ai.say_the_run_was_taken_back(taking.total);
            return;
        }
        if self.ai.tools.run.may_take_back(revision).is_err() {
            self.ai.say_the_run_cannot_be_taken_back();
            return;
        }
        if self.walk_history(true) {
            taking.started = true;
            taking.revision = revision;
            self.ai.tools.taking_back = Some(taking);
        }
    }

    fn show_what_needs_the_person(&mut self, ctx: &egui::Context) {
        let Some(call) = self.ai.tools.the_person_is_needed_for() else {
            return;
        };
        if !self.ai.open {
            if self.home || self.tools.is_some() {
                return;
            }
            let now = ctx.input(|input| input.time);
            self.open_ai_panel(now);
        }
        self.ai.tools.the_person_was_told(call);
        ctx.request_repaint();
    }

    pub(crate) fn advance_tools(&mut self, ctx: &egui::Context) {
        self.collect_a_sent_edit();
        if self.ai.tools.stopping && self.ai.tools.sent.is_none() {
            self.finish_the_stop();
            return;
        }
        if self.ai.tools.ask.is_some() || self.ai.tools.question.is_some() || self.ai.busy() {
            return;
        }
        while let Some(call) = self.ai.tools.queue.front().cloned() {
            if self.ai.tools.sent.is_some() {
                return;
            }
            if self.ai.tools.answer_a_call_that_could_not_be_read(&call) {
                continue;
            }
            let decision = if self.ai.tools.allowed_just_now(&call) {
                Decision::Run
            } else {
                self.decide_about(&call)
            };
            match decision {
                Decision::Refuse(why) => {
                    let said = match why {
                        Why::ToolsAreOff => {
                            "This chat is in Chat only, so no tool may be used. The person can \
                             change that beside the model's name. Answer with what you know."
                        }
                        Why::UnknownTool => "there is no tool of that name",
                    };
                    self.ai.tools.answer(ToolResult::failed(&call.id, said));
                    continue;
                }
                Decision::Ask { may_allow_for_chat } => {
                    match pdf_agent::tools::request::parse(&call.name, &call.arguments) {
                        Ok(request) => {
                            let of_this_tool = if may_allow_all(&call.name) {
                                self.ai
                                    .tools
                                    .queue
                                    .iter()
                                    .filter(|other| other.name == call.name)
                                    .count()
                            } else {
                                1
                            };
                            self.ai.tools.ask = Some(Pending {
                                call,
                                request,
                                may_allow_for_chat,
                                of_this_tool,
                            });
                            ctx.request_repaint();
                            return;
                        }
                        Err(why) => {
                            self.ai.tools.answer(ToolResult::failed(&call.id, why));
                            continue;
                        }
                    }
                }
                Decision::Run => {}
            }
            self.ai.tools.revision_before = self.editor.revision();
            match self.perform(&call) {
                Performed::Done(result) => self.ai.tools.answer(result),
                Performed::Sent | Performed::Waiting => return,
                Performed::Busy => {
                    ctx.request_repaint();
                    return;
                }
                Performed::NeedPages(pages) => {
                    if let Some((page, why)) = pages
                        .iter()
                        .find_map(|page| self.failed.get(page).map(|why| (*page, why.clone())))
                    {
                        let said = format!("page {} cannot be read: {why}", page + 1);
                        self.ai.tools.answer(ToolResult::failed(&call.id, said));
                        continue;
                    }
                    let first = pages.first().copied().unwrap_or(0);
                    let since = match self.ai.tools.waiting.take() {
                        Some((id, was, since)) if id == call.id && was == first => since,
                        _ => Instant::now(),
                    };
                    if since.elapsed() > WAIT {
                        let said = format!("page {} could not be read in time", first + 1);
                        self.ai.tools.answer(ToolResult::failed(&call.id, said));
                        continue;
                    }
                    for page in pages {
                        self.ask_the_painter_for(page);
                    }
                    self.ai.tools.waiting = Some((call.id.clone(), first, since));
                    ctx.request_repaint();
                    return;
                }
            }
        }
        self.finish_the_round(ctx);
    }

    fn decide_about(&self, call: &ToolCall) -> Decision {
        let destructive = tools::facts(&call.name).is_some_and(|facts| facts.destructive)
            || tools::request::parse(&call.name, &call.arguments)
                .is_ok_and(|request| request.is_destructive());
        decide(
            self.ai.mode,
            &call.name,
            tools::facts(&call.name).map(|facts| (facts.read_only, destructive)),
            self.ai.tools.allowed_for_chat.contains(&call.name),
        )
    }

    fn finish_the_round(&mut self, ctx: &egui::Context) {
        self.ai.tools.allowed_all = None;
        if self.ai.tools.results.is_empty() {
            return;
        }
        let results = std::mem::take(&mut self.ai.tools.results);
        self.ai.round_done(results);
        let brief = self.document_brief();
        self.ai.start_round(ctx, &brief);
    }

    fn finish_the_stop(&mut self) {
        let results = self.ai.tools.closing_results();
        self.ai.keep_what_was_done(results);
    }

    fn collect_a_sent_edit(&mut self) {
        let Some(applied) = self.ai.tools.landed.take() else {
            return;
        };
        let Some(sent) = self.ai.tools.sent.take() else {
            return;
        };
        let now = match (&applied, &sent.request) {
            (Applied::Changed { .. }, Request::ReplaceText { .. }) => sent
                .was
                .and_then(|(page, area)| self.block_over(page, area)),
            _ => None,
        };
        if let Some(block) = &now {
            self.remember_the_name(block);
        }
        if let Applied::Changed { page, .. } = &applied {
            let revision = self.editor.revision();
            match &sent.request {
                Request::Undo => self.ai.tools.run.took_one_back(revision),
                Request::Redo => self.ai.tools.run.put_one_back(revision),
                _ => self.ai.tools.run.landed(revision),
            }
            let pages = if sent.changed.is_empty() {
                vec![*page]
            } else {
                sent.changed.clone()
            };
            if !matches!(sent.request, Request::SetProperties(_)) {
                self.ai.say_the_document_changed(&pages);
            }
        }
        let mut result = result_of(&sent.call.id, (&applied, &sent.request), now.as_ref());
        if let (Some(success), Applied::Changed { .. }) = (&sent.success, &applied) {
            result.text.clone_from(success);
        }
        self.ai.tools.answer(result);
    }

    fn block_over(&self, page: usize, area: [f64; 4]) -> Option<Block> {
        let leaf = self.editor.leaf(page)?;
        (0..leaf.view.index.blocks.len())
            .filter_map(|index| desk::read_block(&leaf.view, page, index))
            .filter(|block| desk::overlaps(block.area, area))
            .max_by(|one, other| {
                desk::overlap(one.area, area).total_cmp(&desk::overlap(other.area, area))
            })
    }

    fn perform(&mut self, call: &ToolCall) -> Performed {
        let request = match pdf_agent::tools::request::parse(&call.name, &call.arguments) {
            Ok(request) => request,
            Err(why) => return Performed::Done(ToolResult::failed(&call.id, why)),
        };
        if request.counts_pages() && self.ai.tools.renumbered != self.ai.tools.numbered_at_take {
            return Performed::Done(ToolResult::failed(&call.id, RENUMBERED));
        }
        let pages = self.editor.page_count();
        match &request {
            Request::DocumentInfo => self.document_info(call),
            Request::ReadText { first, last } => self.read_text(call, (*first, *last), pages),
            Request::FindText {
                text,
                match_case,
                first,
                last,
            } => self.find_text(call, (text, *match_case), (*first, *last), pages),
            Request::RenderPage { page, dpi } => self.render_page(call, *page, *dpi),
            Request::ListFonts { name } => Performed::Done(ToolResult::said(
                &call.id,
                tools::list_fonts_named(name.as_deref()).text,
            )),
            Request::ReplaceText { block, find, text } => {
                self.replace_text(call, request.clone(), (block, find.as_deref(), text))
            }
            Request::AddText {
                page, area, text, ..
            } => self.add_text(call, request.clone(), (*page, *area, text.clone())),
            Request::WritePages {
                from_page,
                markdown,
                replace,
                size,
                family,
                margin,
                theme,
            } => self.write_pages(
                call,
                request.clone(),
                (*from_page, markdown, *replace),
                (*size, family, *margin, theme),
            ),
            Request::SetProperties(edit) => {
                let job = self.editor.begin_describe(edit.clone());
                self.send_for(call, request.clone(), job, None)
            }
            Request::FillField { name, value } => {
                self.fill_field(call, request.clone(), name, value)
            }
            Request::AddBlankPage { after, size } => {
                self.put_a_blank_page(call, request.clone(), *after, *size, pages)
            }
            Request::DeletePages(_) | Request::MovePages { .. } | Request::RotatePages { .. } => {
                self.a_page_command(call, request.clone(), pages)
            }
            Request::InsertPages { .. } => self.insert_pages(call, request.clone(), pages),
            Request::Undo | Request::Redo => self.step_history(call, request),
            Request::AskPerson { question, options } => {
                self.ai.tools.question = Some(Question {
                    call: call.id.clone(),
                    asked: question.clone(),
                    options: options.clone(),
                    own: String::new(),
                });
                Performed::Waiting
            }
            Request::UpdatePlan { steps } => {
                self.ai.tools.plan.clone_from(steps);
                Performed::Done(ToolResult::said(
                    &call.id,
                    pdf_agent::context::plan_said(steps),
                ))
            }
        }
    }

    fn document_info(&mut self, call: &ToolCall) -> Performed {
        let sizes = self.shown_page_sizes();
        let set_aside = self.editor.restrictions_set_aside();
        let Some(source) = self.editor.source().cloned() else {
            return self.no_source(call);
        };
        let credential = self.editor.credential().to_vec();
        Performed::Done(said(
            &call.id,
            pdf_agent::about::describe(&source, &credential, sizes, set_aside)
                .map(|answer| (answer.text, None)),
        ))
    }

    fn no_source(&self, call: &ToolCall) -> Performed {
        if self.editor.is_busy() {
            return Performed::Busy;
        }
        Performed::Done(ToolResult::failed(
            &call.id,
            "there is no document open in this window",
        ))
    }

    fn a_page_command(&mut self, call: &ToolCall, request: Request, pages: usize) -> Performed {
        let wanted: &[usize] = match &request {
            Request::DeletePages(wanted)
            | Request::MovePages { pages: wanted, .. }
            | Request::RotatePages { pages: wanted, .. } => wanted,
            _ => return Performed::Done(ToolResult::failed(&call.id, "not a page command")),
        };
        if let Some(said) = no_such_page(wanted, pages) {
            return Performed::Done(ToolResult::failed(&call.id, said));
        }
        let job = match &request {
            Request::DeletePages(wanted) => self.editor.begin_remove_pages(wanted),
            Request::MovePages { pages: wanted, to } => self.editor.begin_move_pages(wanted, *to),
            Request::RotatePages {
                pages: wanted,
                quarter_turns,
            } => self.editor.begin_rotate_pages(wanted, *quarter_turns),
            _ => return Performed::Done(ToolResult::failed(&call.id, "not a page command")),
        };
        let mut changed = wanted.to_vec();
        if let Request::MovePages { to, .. } = &request {
            changed.push(*to);
        }
        changed.sort_unstable();
        changed.dedup();
        let sent = self.send_for(call, request, job, None);
        self.sent_changing(changed, None);
        sent
    }

    fn sent_changing(&mut self, pages: Vec<usize>, success: Option<String>) {
        if let Some(sent) = self.ai.tools.sent.as_mut() {
            sent.changed = pages;
            sent.success = success;
        }
    }

    fn step_history(&mut self, call: &ToolCall, request: Request) -> Performed {
        let back = matches!(request, Request::Undo);
        if self.running.is_some() || self.editor.is_busy() {
            return Performed::Busy;
        }
        let revision = self.editor.revision();
        let allowed = if back {
            self.ai.tools.run.may_take_back(revision)
        } else {
            self.ai.tools.run.may_put_back(revision)
        };
        if let Err(cannot) = allowed {
            return Performed::Done(ToolResult::failed(&call.id, why_not_walked(cannot, back)));
        }
        if self.walk_history(back) {
            self.ai.tools.sent = Some(Sent {
                call: call.clone(),
                request,
                was: None,
                success: None,
                changed: Vec::new(),
            });
            Performed::Sent
        } else {
            Performed::Done(ToolResult::said(
                &call.id,
                if back {
                    "There is nothing to undo."
                } else {
                    "There is nothing to redo."
                },
            ))
        }
    }

    fn shown_page_sizes(&self) -> Vec<[f64; 2]> {
        let geometries: Vec<pdf_content::PageGeometry> = (0..self.editor.page_count())
            .filter_map(|page| self.editor.geometry(page).copied())
            .collect();
        desk::shown_sizes(&geometries)
    }

    fn read_text(
        &mut self,
        call: &ToolCall,
        (first, last): (Option<usize>, Option<usize>),
        pages: usize,
    ) -> Performed {
        let (first, last) = match tools::page_span(first, last, pages) {
            Ok(span) => span,
            Err(why) => return Performed::Done(ToolResult::failed(&call.id, why)),
        };
        match self.gather_pages(call, (first, last), MOST_CHARACTERS) {
            Ok(gathered) => Performed::Done(said(&call.id, read_reply((first, last), &gathered))),
            Err(waiting) => waiting,
        }
    }

    fn find_text(
        &mut self,
        call: &ToolCall,
        (text, match_case): (&str, bool),
        (first, last): (Option<usize>, Option<usize>),
        pages: usize,
    ) -> Performed {
        let (first, last) = match tools::page_span(first, last, pages) {
            Ok(span) => span,
            Err(why) => return Performed::Done(ToolResult::failed(&call.id, why)),
        };
        let stop = last.min(first + MOST_SEARCHED - 1);
        match self.gather_pages(call, (first, stop), usize::MAX) {
            Ok(gathered) => {
                let answer =
                    tools::format_hits((text, match_case), (first, stop, pages), &mut |page| {
                        Ok(taken(&gathered, page))
                    });
                let more = (stop < last).then(|| {
                    format!(
                        "\nOnly pages {} to {} were searched, which is as many as one search \
                         reads. Search on from page {} with first_page.",
                        first + 1,
                        stop + 1,
                        stop + 2
                    )
                });
                Performed::Done(said(&call.id, answer.map(|answer| (answer.text, more))))
            }
            Err(waiting) => waiting,
        }
    }

    fn gather_pages(
        &mut self,
        call: &ToolCall,
        span: (usize, usize),
        room: usize,
    ) -> Result<Vec<(usize, Vec<Block>)>, Performed> {
        let gathering = match self.ai.tools.gathering.take() {
            Some(gathering) if gathering.call == call.id && gathering.span == span => gathering,
            _ => Gathering {
                call: call.id.clone(),
                span,
                next: span.0,
                pages: Vec::new(),
                characters: 0,
            },
        };
        let mut gathering = gathering;
        while gathering.next <= span.1 {
            let page = gathering.next;
            if let Some(why) = self.failed.get(&page) {
                let said = format!("page {} cannot be read: {why}", page + 1);
                return Err(Performed::Done(ToolResult::failed(&call.id, said)));
            }
            let Some(leaf) = self.editor.leaf(page) else {
                self.ai.tools.gathering = Some(gathering);
                let wanted: Vec<usize> = (page..=span.1.min(page + READ_AHEAD - 1))
                    .filter(|at| self.editor.leaf(*at).is_none() && !self.failed.contains_key(at))
                    .collect();
                return Err(Performed::NeedPages(wanted));
            };
            let blocks: Vec<Block> = (0..leaf.view.index.blocks.len())
                .filter_map(|index| desk::read_block(&leaf.view, page, index))
                .collect();
            let size = tools::reading_size(&blocks);
            if gathering.characters > 0 && gathering.characters.saturating_add(size) > room {
                break;
            }
            gathering.characters = gathering.characters.saturating_add(size.min(room));
            for block in &blocks {
                self.remember_the_name(block);
            }
            gathering.pages.push((page, blocks));
            gathering.next += 1;
        }
        Ok(gathering.pages)
    }

    fn render_page(&mut self, call: &ToolCall, page: usize, dpi: f64) -> Performed {
        if page >= self.editor.page_count() {
            let said = format!(
                "there is no page {}: the document has {}",
                page + 1,
                self.editor.page_count()
            );
            return Performed::Done(ToolResult::failed(&call.id, said));
        }
        if let Some(why) = self.failed.get(&page) {
            let said = format!("page {} cannot be read: {why}", page + 1);
            return Performed::Done(ToolResult::failed(&call.id, said));
        }
        let Some(leaf) = self.editor.leaf(page).map(std::sync::Arc::clone) else {
            return Performed::NeedPages(vec![page]);
        };
        match desk::picture_of(&leaf.view, dpi) {
            Ok((png, width, height)) => {
                let mut result = ToolResult::said(
                    &call.id,
                    format!("Page {} as shown, {width} x {height} pixels.", page + 1),
                );
                result.picture = Some(Picture {
                    media_type: "image/png".to_owned(),
                    base64: pdf_agent::json::base64(&png),
                });
                Performed::Done(result)
            }
            Err(why) => Performed::Done(ToolResult::failed(
                &call.id,
                format!("page {} cannot be drawn: {why}", page + 1),
            )),
        }
    }

    fn replace_text(
        &mut self,
        call: &ToolCall,
        request: Request,
        (name, find, text): (&str, Option<&str>, &str),
    ) -> Performed {
        let key = match parse_block_name(name) {
            Ok(key) => key,
            Err(why) => return Performed::Done(ToolResult::failed(&call.id, why)),
        };
        if key.0 < self.editor.page_count() && self.editor.leaf(key.0).is_none() {
            return Performed::NeedPages(vec![key.0]);
        }
        let (page, block) = match self.block_named(name) {
            Ok(found) => found,
            Err(why) => return Performed::Done(ToolResult::failed(&call.id, why)),
        };
        let range = match find {
            None => self.editor.whole_block(page, block),
            Some(find) => match self.editor.block_reading(page, block) {
                Some(reading) => match desk::range_within(&reading, find) {
                    Ok(range) => Some(range),
                    Err(problem) => {
                        let said = format!("{name}: {problem}");
                        return Performed::Done(ToolResult::failed(&call.id, said));
                    }
                },
                None => None,
            },
        };
        let Some(range) = range else {
            let said = format!("{name} cannot be read as text");
            return Performed::Done(ToolResult::failed(&call.id, said));
        };
        let was = self
            .ai
            .tools
            .named
            .get(&key)
            .map(|named| (page, named.area));
        let job = self
            .editor
            .begin_edit(page, block, range, &text.replace("\r\n", "\n"));
        self.send_for(call, request, job, was)
    }

    fn write_pages(
        &mut self,
        call: &ToolCall,
        request: Request,
        (from_page, markdown, replace): (usize, &str, bool),
        (size, family, margin, theme): (f64, &str, f64, &str),
    ) -> Performed {
        use pdf_agent::composing::{Faces, Setting, Sheet, compose, placing, theme as themes};
        let pages = self.editor.page_count();
        if from_page >= pages {
            let said = format!(
                "there is no page {}: the document has {pages}",
                from_page + 1
            );
            return Performed::Done(ToolResult::failed(&call.id, said));
        }
        let Some([wide, high]) = self
            .shown_page_sizes()
            .get(from_page)
            .copied()
            .filter(|[wide, high]| *wide > 0.0 && *high > 0.0)
        else {
            return Performed::Done(ToolResult::failed(
                &call.id,
                "that page has no size this program can write on",
            ));
        };
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a point size, far inside f32"
        )]
        let parts = pdf_agent::markup::laying_out::parts(markdown, size as f32);
        if parts.is_empty() {
            return Performed::Done(ToolResult::failed(
                &call.id,
                "there is nothing to write in that markdown",
            ));
        }
        let Some(fonts) = self.editor.fonts() else {
            return Performed::Done(ToolResult::failed(
                &call.id,
                "no fonts were found on this machine",
            ));
        };
        if !replace && self.editor.leaf(from_page).is_none() {
            return Performed::NeedPages(vec![from_page]);
        }
        let start = if replace {
            None
        } else {
            self.bottom_of_everything(from_page)
                .map(|below| below + size)
        };
        let theme = themes::named(theme).unwrap_or_else(themes::default_theme);
        let setting = Setting {
            sheet: Sheet {
                wide,
                high,
                margin: margin.min(wide / 3.0).min(high / 3.0),
            },
            from_page,
            start,
            family,
            theme,
            body: size,
        };
        let composed = match compose(&parts, &setting, &Faces(fonts)) {
            Ok(composed) => composed,
            Err(why) => {
                return Performed::Done(ToolResult::failed(
                    &call.id,
                    format!("nothing was written: {why}"),
                ));
            }
        };
        let commands = match placing::as_commands(&composed.marks, &|page| {
            self.editor.geometry(page).copied()
        }) {
            Ok(commands) => commands,
            Err(why) => {
                return Performed::Done(ToolResult::failed(
                    &call.id,
                    format!("nothing was written: {why}"),
                ));
            }
        };
        let said = written_in_words(&composed, (family, theme.name), replace);
        let job = self.editor.begin_commands(
            commands,
            Done::Wrote {
                pieces: composed.pieces,
                pages: composed.pages,
            },
        );
        let sent = self.send_for(call, request, job, None);
        self.sent_changing(
            (from_page..from_page + composed.pages).collect(),
            Some(said),
        );
        sent
    }

    fn bottom_of_everything(&self, page: usize) -> Option<f64> {
        let leaf = self.editor.leaf(page)?;
        (0..leaf.view.index.blocks.len())
            .filter_map(|index| desk::read_block(&leaf.view, page, index))
            .map(|block| block.area[3])
            .chain(
                leaf.overlay
                    .objects
                    .iter()
                    .map(|object| object.box_pixels[3]),
            )
            .max_by(f64::total_cmp)
    }

    fn add_text(
        &mut self,
        call: &ToolCall,
        request: Request,
        (page, area, text): (usize, [f64; 4], String),
    ) -> Performed {
        let Request::AddText { style, .. } = &request else {
            return Performed::Done(ToolResult::failed(&call.id, "this is not new text"));
        };
        let style = pdf_app::NewTextStyle {
            family: style.family.clone(),
            size: style.size,
            bold: style.bold,
            italic: style.italic,
            fill: style.fill,
            paragraph: pdf_edit::ParagraphLayout::default(),
        };
        if page >= self.editor.page_count() {
            let said = format!(
                "there is no page {}: the document has {}",
                page + 1,
                self.editor.page_count()
            );
            return Performed::Done(ToolResult::failed(&call.id, said));
        }
        let Some(leaf) = self.editor.leaf(page).map(std::sync::Arc::clone) else {
            return Performed::NeedPages(vec![page]);
        };
        let frame = match desk::to_user(&leaf.view, area) {
            Ok(frame) => frame,
            Err(why) => return Performed::Done(ToolResult::failed(&call.id, why)),
        };
        let job = self.editor.begin_place_text(page, frame, &text, &style);
        self.send_for(call, request, job, None)
    }

    fn fill_field(
        &mut self,
        call: &ToolCall,
        request: Request,
        name: &str,
        value: &pdf_agent::json::Json,
    ) -> Performed {
        let Some(source) = self.editor.source().cloned() else {
            return self.no_source(call);
        };
        let credential = self.editor.credential().to_vec();
        let command = match pdf_agent::about::field_to_fill(&source, &credential, name, value) {
            Ok(command) => command,
            Err(why) => return Performed::Done(ToolResult::failed(&call.id, why)),
        };
        let pdf_edit::Command::FillField {
            page_index,
            widget,
            value,
        } = command
        else {
            return Performed::Done(ToolResult::failed(&call.id, "that field cannot be filled"));
        };
        let job = self.editor.begin_fill_field(page_index, widget, value);
        self.send_for(call, request, job, None)
    }

    fn put_a_blank_page(
        &mut self,
        call: &ToolCall,
        request: Request,
        after: usize,
        size: Option<[f64; 2]>,
        pages: usize,
    ) -> Performed {
        if after > pages {
            let said = format!("there is no page {after}: the document has {pages}");
            return Performed::Done(ToolResult::failed(&call.id, said));
        }
        let (beside, before) = tools::beside_after(after);
        let size = match size {
            Some(size) => size,
            None => match self.shown_page_sizes().get(beside).copied() {
                Some(size) => size,
                None => {
                    return Performed::Done(ToolResult::failed(
                        &call.id,
                        "the page beside this one has no size: pass width and height",
                    ));
                }
            },
        };
        let job = self.editor.begin_add_page(beside, before, size);
        let sent = self.send_for(call, request, job, None);
        self.sent_changing(vec![after], None);
        sent
    }

    fn insert_pages(&mut self, call: &ToolCall, request: Request, pages: usize) -> Performed {
        let Request::InsertPages {
            from,
            pages: chosen,
            after,
            password,
        } = &request
        else {
            return Performed::Done(ToolResult::failed(&call.id, "this is not an insertion"));
        };
        if *after > pages {
            let said = format!("there is no page {after}: the document has {pages}");
            return Performed::Done(ToolResult::failed(&call.id, said));
        }
        match std::fs::metadata(from) {
            Ok(facts) if facts.is_file() && facts.len() <= MOST_INSERTED_BYTES => {}
            Ok(facts) if facts.is_file() => {
                let said = format!(
                    "{} is too large to take pages from: the most is {} MB",
                    from.display(),
                    MOST_INSERTED_BYTES / (1024 * 1024)
                );
                return Performed::Done(ToolResult::failed(&call.id, said));
            }
            Ok(_) => {
                let said = format!("{} is not a file", from.display());
                return Performed::Done(ToolResult::failed(&call.id, said));
            }
            Err(error) => {
                let said = format!("{} cannot be read: {error}", from.display());
                return Performed::Done(ToolResult::failed(&call.id, said));
            }
        }
        let bytes: std::sync::Arc<[u8]> = match std::fs::read(from) {
            Ok(bytes) => bytes.into(),
            Err(error) => {
                let said = format!("{} cannot be read: {error}", from.display());
                return Performed::Done(ToolResult::failed(&call.id, said));
            }
        };
        let other =
            pdf_bytes::ByteStore::new(pdf_bytes::SourceId::new(1), std::sync::Arc::clone(&bytes));
        let password = password.as_deref().unwrap_or_default().as_bytes().to_vec();
        if pdf_edit::info::lock(&other, &password) == pdf_edit::info::Lock::Refused {
            let said = if password.is_empty() {
                format!(
                    "{} is protected by a password: give it as `password`",
                    from.display()
                )
            } else {
                format!("the password given does not open {}", from.display())
            };
            return Performed::Done(ToolResult::failed(&call.id, said));
        }
        let available = match pdf_session::Session::new(other, &password).page_count() {
            Ok(available) => available,
            Err(error) => {
                let said = format!("{} has no pages this can read: {error}", from.display());
                return Performed::Done(ToolResult::failed(&call.id, said));
            }
        };
        let wanted: Vec<usize> = match chosen {
            Some(chosen) => chosen.clone(),
            None => (0..available).collect(),
        };
        if let Some(missing) = wanted.iter().find(|page| **page >= available) {
            let said = format!(
                "{} has no page {}: it has {available}",
                from.display(),
                missing + 1
            );
            return Performed::Done(ToolResult::failed(&call.id, said));
        }
        let (beside, before) = tools::beside_after(*after);
        let job = self.editor.begin_insert_pages(
            (beside, before),
            (bytes, pdf_edit::Password(password)),
            &wanted,
        );
        let put = (*after..*after + wanted.len()).collect();
        let sent = self.send_for(call, request, job, None);
        self.sent_changing(put, None);
        sent
    }

    fn send_for(
        &mut self,
        call: &ToolCall,
        request: Request,
        job: Option<pdf_app::EditJob>,
        was: Option<(usize, [f64; 4])>,
    ) -> Performed {
        let Some(job) = job else {
            return Performed::Busy;
        };
        self.ai
            .tools
            .run
            .before_a_step(self.ai.tools.revision_before);
        self.ai.tools.sent = Some(Sent {
            call: call.clone(),
            request,
            was,
            success: None,
            changed: Vec::new(),
        });
        self.send(Some(job));
        Performed::Sent
    }

    fn ask_the_painter_for(&mut self, page: usize) {
        let epoch = self.editor.epoch();
        if self.editor.leaf(page).is_some()
            || self.painter.reading(page, epoch)
            || self.failed.contains_key(&page)
        {
            return;
        }
        let Some(source) = self.editor.source().cloned() else {
            return;
        };
        let credential = self.editor.credential().to_vec();
        let grouping = self.editor.grouping(page);
        self.painter.read(
            page,
            source,
            &credential,
            grouping,
            self.editor.fonts(),
            epoch,
        );
    }

    fn remember_the_name(&mut self, block: &Block) {
        self.ai.tools.named.insert(
            (block.page, block.index),
            Named {
                epoch: self.editor.epoch(),
                arranged: self.ai.tools.arranged,
                text: block.text.clone(),
                area: block.area,
            },
        );
    }

    fn block_named(&self, name: &str) -> Result<(usize, usize), String> {
        let (page, block) = parse_block_name(name)?;
        let named = self.ai.tools.named.get(&(page, block)).ok_or_else(|| {
            format!("{name} has not been read yet: call read_text or find_text for that page first")
        })?;
        let now: Option<Vec<Block>> = (named.epoch != self.editor.epoch())
            .then(|| {
                self.editor.leaf(page).map(|leaf| {
                    (0..leaf.view.index.blocks.len())
                        .filter_map(|index| desk::read_block(&leaf.view, page, index))
                        .collect()
                })
            })
            .flatten();
        the_same_block(
            name,
            (page, block),
            named,
            (self.ai.tools.arranged, self.editor.epoch()),
            now.as_deref(),
        )
    }
}

fn parse_block_name(name: &str) -> Result<(usize, usize), String> {
    let bad = || format!("{name:?} is not a block name: they look like p3-b12");
    let rest = name.trim().strip_prefix('p').ok_or_else(bad)?;
    let (page, block) = rest.split_once("-b").ok_or_else(bad)?;
    let page: usize = page.parse().map_err(|_| bad())?;
    let block: usize = block.parse().map_err(|_| bad())?;
    if page == 0 || block == 0 {
        return Err(bad());
    }
    Ok((page - 1, block - 1))
}

fn the_same_block(
    name: &str,
    (page, block): (usize, usize),
    named: &Named,
    (arranged, epoch): (u64, u64),
    now: Option<&[Block]>,
) -> Result<(usize, usize), String> {
    if named.arranged != arranged {
        return Err(format!(
            "{name} was read before the pages were moved about, and its page number no \
             longer means the page it meant: read_text again and use the name it gives now"
        ));
    }
    if named.epoch == epoch {
        return Ok((page, block));
    }
    let Some(now) = now else {
        return Err(format!(
            "{name} is on a page that has not been read since it changed: read_text page {} again",
            page + 1
        ));
    };
    let same: Vec<usize> = now
        .iter()
        .filter(|block| block.text == named.text && desk::overlaps(block.area, named.area))
        .map(|block| block.index)
        .collect();
    match same[..] {
        [only] => Ok((page, only)),
        [] => Err(format!(
            "{name} is no longer on the page as it was read: read_text page {} again",
            page + 1
        )),
        _ => Err(format!(
            "{name} cannot be told apart from another block since the page changed: \
             read_text page {} again",
            page + 1
        )),
    }
}

fn read_reply(
    (first, last): (usize, usize),
    gathered: &[(usize, Vec<Block>)],
) -> Result<(String, Option<String>), String> {
    let reached = gathered.last().map_or(first, |(page, _)| *page);
    let answer = tools::format_pages((first, reached), &mut |page| Ok(taken(gathered, page)))?;
    let more = (reached < last).then(|| {
        format!(
            "\n(The reply is full. Continue with first_page: {}.)",
            reached + 2
        )
    });
    Ok((answer.text, more))
}

fn taken(gathered: &[(usize, Vec<Block>)], page: usize) -> Vec<Block> {
    gathered
        .iter()
        .find(|(at, _)| *at == page)
        .map(|(_, blocks)| blocks.clone())
        .unwrap_or_default()
}

fn said(call: &str, answer: Result<(String, Option<String>), String>) -> ToolResult {
    match answer {
        Ok((text, more)) => ToolResult::said(call, format!("{text}{}", more.unwrap_or_default())),
        Err(why) => ToolResult::failed(call, why),
    }
}

fn result_of(
    call: &str,
    (applied, request): (&Applied, &Request),
    now: Option<&Block>,
) -> ToolResult {
    match applied {
        Applied::Changed { page, .. } => ToolResult::said(call, what_changed(request, *page, now)),
        Applied::Unchanged => ToolResult::said(
            call,
            match request {
                Request::Undo => "There is nothing to undo.",
                Request::Redo => "There is nothing to redo.",
                _ => "Nothing changed.",
            },
        ),
        Applied::Refused(reason) => {
            ToolResult::failed(call, Message::Refused(reason.clone()).say(Lang::English))
        }
    }
}

fn what_changed(request: &Request, page: usize, now: Option<&Block>) -> String {
    match request {
        Request::ReplaceText { .. } => now.map_or_else(
            || "Done. The block could not be read back: read_text that page again.".to_owned(),
            |block| format!("Done. {} now reads: {}", block.name(), block.text),
        ),
        Request::AddText { style, .. } => format!(
            "Written on page {} in {}, {} pt.",
            page + 1,
            style.family,
            style.size
        ),
        Request::SetProperties(_) => "Properties set.".to_owned(),
        Request::FillField { name, .. } => format!("{name} is filled."),
        Request::AddBlankPage { after, .. } => format!("A blank page is now page {}.", after + 1),
        Request::DeletePages(pages) => {
            format!("Took out {} page{}.", pages.len(), plural(pages.len()))
        }
        Request::MovePages { .. } => "Moved.".to_owned(),
        Request::RotatePages { .. } => "Turned.".to_owned(),
        Request::InsertPages { pages, after, .. } => match pages {
            Some(pages) => format!(
                "Put in {} page{} after page {after}.",
                pages.len(),
                plural(pages.len())
            ),
            None => format!("Put the file's pages in after page {after}."),
        },
        Request::Undo => "Took back the last change.".to_owned(),
        Request::Redo => "Put the change back.".to_owned(),
        _ => "Done.".to_owned(),
    }
}

fn written_in_words(
    composed: &pdf_agent::composing::Composed,
    (family, theme): (&str, &str),
    replace: bool,
) -> String {
    let left_out = if composed.left_out.is_empty() {
        String::new()
    } else {
        format!(
            " Left out, as no face on this machine draws them: {}.",
            composed.left_out
        )
    };
    let family = if family.is_empty() {
        "the faces chosen for each script"
    } else {
        family
    };
    let covers = if replace {
        " What was on the first page is covered, not removed: it is still in the file underneath."
    } else {
        ""
    };
    format!(
        "Written: {} pieces of text over {} page{} in {family}, theme {theme}, as one step the \
         person can undo.{left_out}{covers}",
        composed.pieces,
        composed.pages,
        plural(composed.pages)
    )
}

fn why_not_walked(cannot: Cannot, back: bool) -> &'static str {
    match (cannot, back) {
        (Cannot::NothingOfTheirs, true) => {
            "There is nothing of yours to undo in this run: the changes made before it are the \
             person's own, and only they take those back."
        }
        (Cannot::NothingOfTheirs, false) => "There is nothing of yours to redo in this run.",
        (Cannot::PersonEdited, _) => {
            "The person has changed the document since your last step, so undo and redo are \
             theirs now: do not take back what they did."
        }
    }
}

fn no_such_page(wanted: &[usize], pages: usize) -> Option<String> {
    wanted
        .iter()
        .find(|page| **page >= pages)
        .map(|page| format!("there is no page {}: the document has {pages}", page + 1))
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

pub(crate) const fn tools_are_offered(mode: Mode) -> bool {
    !matches!(mode, Mode::ChatOnly)
}

#[cfg(test)]
mod tests;
