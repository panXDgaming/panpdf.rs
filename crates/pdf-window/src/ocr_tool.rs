use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};

use eframe::egui;

use pdf_app::ocr_choice::Choice;
use pdf_app::wording::{Command, Done, Fact, Lang, Message};
use pdf_edit::stamp::Only;
use pdf_ocr::Quality;
use pdf_paint::PaintAtomKind;

use crate::dialog;
use crate::format::quiet_icon_button;
use crate::icons::Icon;
use crate::window_state::{OcrDraft, OcrFetch, OcrReading, OcrWhich, PageRead, Window};

const PANEL_WIDTH: f32 = 340.0;

const MOST_WORKERS: usize = 4;

const LIST_HEIGHT: f32 = 200.0;

const ROW_HEIGHT: f32 = 28.0;

const ROW_BAR: f32 = 84.0;

fn choice_file() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("state"))
        })?;
    Some(state.join("panpdf").join("ocr"))
}

fn remembered() -> Choice {
    choice_file()
        .and_then(|file| std::fs::read_to_string(file).ok())
        .and_then(|text| pdf_app::ocr_choice::read(&text))
        .unwrap_or_else(Choice::fresh)
}

fn remember(choice: &Choice) {
    let Some(file) = choice_file() else {
        return;
    };
    if let Some(folder) = file.parent() {
        let _ = std::fs::create_dir_all(folder);
    }
    let temporary = file.with_extension("new");
    if std::fs::write(&temporary, pdf_app::ocr_choice::write(choice)).is_ok() {
        let _ = std::fs::rename(&temporary, &file);
    }
}

pub(crate) fn draft() -> OcrDraft {
    let choice = remembered();
    let mut engine = pdf_ocr::Tesseract::locate().ok();
    if let Some(engine) = engine.as_mut() {
        engine.own = pdf_ocr::store::models_dir(choice.quality);
    }
    let mut draft = OcrDraft {
        engine,
        ticked: Vec::new(),
        here: Vec::new(),
        own: Vec::new(),
        system: Vec::new(),
        list: pdf_app::ocr_languages::catalogue(),
        search: String::new(),
        choice,
        which: OcrWhich::default(),
        range: String::new(),
        reading: None,
        fetching: None,
        trouble: None,
    };
    draft.take_stock();
    draft
}

impl OcrDraft {
    fn take_stock(&mut self) {
        let quality = self.choice.quality;
        if let Some(engine) = self.engine.as_mut() {
            engine.own = pdf_ocr::store::models_dir(quality);
        }
        self.system = self
            .engine
            .as_ref()
            .and_then(|engine| engine.installed_languages().ok())
            .unwrap_or_default();
        self.own = pdf_ocr::store::models_dir(quality)
            .map(|dir| {
                pdf_ocr::store::languages_in(&dir)
                    .into_iter()
                    .filter(|code| {
                        pdf_ocr::models::model(code, quality)
                            .is_some_and(|model| pdf_ocr::store::have(&dir, model))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut here: Vec<String> = self.own.iter().chain(&self.system).cloned().collect();
        here.sort_unstable();
        here.dedup();
        self.here = here;
        self.ticked = pdf_app::ocr_languages::ticks(&self.choice.languages, &self.here);
    }

    fn tick(&mut self, code: &str, on: bool) {
        let mut ticked: Vec<String> = self
            .ticked
            .iter()
            .filter(|ticked| *ticked != code)
            .cloned()
            .collect();
        if on && self.here.iter().any(|here| here == code) {
            ticked.push(code.to_owned());
        }
        self.ticked = pdf_app::ocr_languages::in_order(&ticked, &self.here);
        self.choice.languages.clone_from(&self.ticked);
    }

    fn chosen(&self) -> Vec<String> {
        self.ticked.clone()
    }

    fn split(&self) -> Option<Message> {
        pdf_app::ocr_languages::one_place(&self.own, &self.system, &self.ticked)
            .err()
            .map(Message::OcrNotInOnePlace)
    }
}

fn has_text(view: &pdf_session::PageView) -> bool {
    view.graph
        .atoms
        .iter()
        .any(|atom| matches!(atom.kind, PaintAtomKind::Text(_)))
}

pub(crate) fn is_a_scan(view: &pdf_session::PageView) -> bool {
    !has_text(view)
        && view
            .graph
            .atoms
            .iter()
            .any(|atom| matches!(atom.kind, PaintAtomKind::Image(_)))
}

fn read_one(
    source: &pdf_bytes::ByteStore,
    page: usize,
    (credential, fonts): (&[u8], Option<Arc<dyn pdf_content::FontProvider>>),
    (engine, languages, skip_text): (&pdf_ocr::Tesseract, &[String], bool),
    cancel: &AtomicBool,
) -> PageRead {
    let view = match pdf_session::interpret_page_for_display(source, page, credential, None, fonts)
    {
        Ok(view) => view,
        Err(error) => return PageRead::Failed(error.to_string()),
    };
    if skip_text && has_text(&view) {
        return PageRead::HadText;
    }
    match pdf_ocr::read_page(
        &view.layers(),
        &view.program.geometry,
        engine,
        languages,
        cancel,
    ) {
        Ok(reading) => PageRead::Read(reading),
        Err(error) => PageRead::Failed(error.to_string()),
    }
}

impl Window {
    pub(crate) fn scan_notice(&mut self, ctx: &egui::Context, area: egui::Rect) {
        if self.ocr_draft.is_some()
            || self.editor.is_busy()
            || self.scan_notice_shut.as_ref() == Some(&self.opened)
            || !self
                .editor
                .leaf(self.focus)
                .is_some_and(|leaf| is_a_scan(&leaf.view))
        {
            return;
        }
        let lang = self.lang;
        let (mut read, mut shut) = (false, false);
        egui::Area::new(egui::Id::new("scan-notice"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::CENTER_TOP)
            .fixed_pos(area.center_top() + egui::vec2(0.0, 12.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .inner_margin(egui::Margin::symmetric(14, 8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 10.0;
                            ui.label(Message::ThisPageIsAScan.say(lang));
                            let label = Message::Command(Command::RecognizeText).say(lang);
                            read = dialog::primary(ui, label.trim_end_matches('\u{2026}'), true)
                                .clicked();
                            shut = quiet_icon_button(ui, Icon::Close, &Message::Close.say(lang))
                                .clicked();
                        });
                    });
            });
        if read {
            self.open_the_ocr_panel();
        }
        if shut {
            self.scan_notice_shut = Some(self.opened.clone());
        }
    }

    pub(crate) fn open_the_ocr_panel(&mut self) {
        if self
            .ocr_draft
            .as_ref()
            .is_some_and(|draft| draft.reading.is_some())
        {
            return;
        }
        self.ocr_draft = Some(draft());
    }

    fn ocr_pages(&self, draft: &OcrDraft) -> Result<Vec<usize>, Message> {
        let count = self.editor.page_count();
        match draft.which {
            OcrWhich::ThisPage => {
                pdf_edit::stamp::pages_of(&(self.focus + 1).to_string(), count, Only::Every)
            }
            OcrWhich::All => pdf_edit::stamp::pages_of("", count, Only::Every),
            OcrWhich::Some => pdf_edit::stamp::pages_of(&draft.range, count, Only::Every),
        }
        .map_err(|error| Message::Refused(error.to_string().into()))
    }

    pub(crate) fn ocr_panel(&mut self, ctx: &egui::Context) {
        if self.ocr_draft.is_none() || !self.has_document() {
            return;
        }
        self.keep_reading();
        self.keep_fetching(ctx);
        let lang = self.lang;
        let count = self.editor.page_count();
        let canvas = self.canvas;
        let pages = self.ocr_draft.as_ref().map(|draft| self.ocr_pages(draft));
        let Some(draft) = self.ocr_draft.as_mut() else {
            return;
        };
        let mut asked = Asked::default();
        let title = Message::Command(Command::RecognizeText).say(lang);
        let why = Message::OcrWhy.say(lang);
        let busy = draft.reading.is_some() || draft.fetching.is_some();
        let close = Message::Close.say(lang);
        let spec = dialog::Spec {
            id: "ocr-panel",
            width: PANEL_WIDTH,
        };
        dialog::panel(ctx, canvas, &spec, |ui, room| {
            let closed = dialog::tool_header(
                ui,
                title.trim_end_matches('\u{2026}'),
                Some(&why),
                (!busy).then_some(close.as_str()),
                lang,
            );
            if closed {
                asked.pressed = Pressed::Close;
            }
            dialog::body(ui, |ui| {
                dialog::scrolling(ui, "ocr-body", room, |ui| {
                    the_choices(ui, draft, (lang, count), &mut asked);
                });
                what_stands_in_the_way(ui, draft, lang, pages.as_ref());
            });
            how_far(ui, draft, lang);
            dialog::body(ui, |ui| {
                the_buttons(ui, draft, (lang, pages.as_ref()), &mut asked);
            });
        });
        if asked.pressed == Pressed::Stop {
            if let Some(reading) = draft.reading.as_ref() {
                reading.cancel.store(true, Ordering::Relaxed);
            }
            if let Some(fetch) = draft.fetching.as_ref() {
                fetch.cancel.store(true, Ordering::Relaxed);
            }
        }
        if let Some(quality) = asked.quality.take() {
            draft.choice.quality = quality;
            draft.take_stock();
            asked.remember = true;
        }
        if asked.remember {
            remember(&draft.choice);
        }
        match asked.pressed {
            Pressed::Close => self.ocr_draft = None,
            Pressed::GetModel(code) => self.fetch_model(ctx, &code),
            Pressed::RemoveModel(code) => self.remove_model(&code),
            Pressed::GetEngine => self.fetch_engine(ctx),
            Pressed::Read => self.start_reading(ctx),
            Pressed::Nothing | Pressed::Stop => {}
        }
    }

    fn fetch_model(&mut self, ctx: &egui::Context, code: &str) {
        let Some(draft) = self.ocr_draft.as_mut() else {
            return;
        };
        let (Some(model), Some(dir)) = (
            pdf_ocr::models::model(code, draft.choice.quality),
            pdf_ocr::store::models_dir(draft.choice.quality),
        ) else {
            draft.trouble = Some(Message::OcrModelFailed(
                pdf_ocr::FetchError::NoHome.to_string(),
            ));
            return;
        };
        draft.trouble = None;
        let (cancel, seen) = (
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicU64::new(0)),
        );
        let (send, answer) = mpsc::channel();
        let (worker_cancel, worker_seen, ctx) =
            (Arc::clone(&cancel), Arc::clone(&seen), ctx.clone());
        let worker = std::thread::spawn(move || {
            let landed = pdf_ocr::store::fetch(
                model,
                &dir,
                &|bytes| {
                    worker_seen.store(bytes, Ordering::Relaxed);
                    ctx.request_repaint();
                },
                &worker_cancel,
            );
            let _ = send.send(landed.map(|_| ()).map_err(|why| why.to_string()));
            ctx.request_repaint();
        });
        draft.fetching = Some(OcrFetch {
            code: Some(code.to_owned()),
            seen,
            total: model.bytes,
            cancel,
            answer,
            worker: Some(worker),
        });
    }

    fn remove_model(&mut self, code: &str) {
        let Some(draft) = self.ocr_draft.as_mut() else {
            return;
        };
        let quality = draft.choice.quality;
        let (Some(model), Some(dir)) = (
            pdf_ocr::models::model(code, quality),
            pdf_ocr::store::models_dir(quality),
        ) else {
            return;
        };
        draft.trouble = match pdf_ocr::store::remove(&dir, model) {
            Ok(_) => None,
            Err(error) => Some(Message::OcrRemoveFailed(error.to_string())),
        };
        draft.take_stock();
        draft.choice.languages.clone_from(&draft.ticked);
        remember(&draft.choice);
    }

    fn fetch_engine(&mut self, ctx: &egui::Context) {
        let Some(draft) = self.ocr_draft.as_mut() else {
            return;
        };
        let Some(into) = pdf_ocr::store::own_engine() else {
            draft.trouble = Some(Message::OcrEngineFailed(
                pdf_ocr::FetchError::NoHome.to_string(),
            ));
            return;
        };
        draft.trouble = None;
        let (cancel, seen) = (
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicU64::new(0)),
        );
        let (send, answer) = mpsc::channel();
        let (worker_cancel, ctx) = (Arc::clone(&cancel), ctx.clone());
        let worker = std::thread::spawn(move || {
            let done = pdf_ocr::setup::install(&into, &|_| {}, &worker_cancel);
            let _ = send.send(done.map(|_| ()).map_err(|why| why.to_string()));
            ctx.request_repaint();
        });
        draft.fetching = Some(OcrFetch {
            code: None,
            seen,
            total: 0,
            cancel,
            answer,
            worker: Some(worker),
        });
    }

    fn keep_fetching(&mut self, ctx: &egui::Context) {
        let Some(draft) = self.ocr_draft.as_mut() else {
            return;
        };
        let Some(fetch) = draft.fetching.as_mut() else {
            return;
        };
        let Ok(answer) = fetch.answer.try_recv() else {
            return;
        };
        let was_a_model = fetch.code.is_some();
        let landed = fetch.code.clone().filter(|_| answer.is_ok());
        if let Some(worker) = fetch.worker.take() {
            let _ = worker.join();
        }
        draft.fetching = None;
        draft.trouble = match answer {
            Ok(()) => None,
            Err(why) if why == pdf_ocr::FetchError::Cancelled.to_string() => None,
            Err(why) if was_a_model => Some(Message::OcrModelFailed(why)),
            Err(why) => Some(Message::OcrEngineFailed(why)),
        };
        if !was_a_model {
            let mut engine = pdf_ocr::Tesseract::locate().ok();
            if let Some(engine) = engine.as_mut() {
                engine.own = pdf_ocr::store::models_dir(draft.choice.quality);
            }
            draft.engine = engine;
        }
        draft.take_stock();
        if let Some(code) = landed {
            draft.tick(&code, true);
            remember(&draft.choice);
        }
        ctx.request_repaint();
    }

    fn start_reading(&mut self, ctx: &egui::Context) {
        let Some(draft) = self.ocr_draft.as_ref() else {
            return;
        };
        let (Some(engine), Some(source)) = (draft.engine.clone(), self.editor.source().cloned())
        else {
            return;
        };
        let pages = match self.ocr_pages(draft) {
            Ok(pages) => pages,
            Err(why) => {
                self.editor.say(why);
                return;
            }
        };
        let reading = start_the_readers(
            ReadJob {
                source,
                credential: self.editor.credential().to_vec(),
                fonts: self.editor.fonts(),
                engine,
                languages: draft.chosen(),
                skip_text: draft.choice.skip_text,
                pages,
                epoch: self.editor.epoch(),
            },
            Some(ctx),
        );
        if let Some(draft) = self.ocr_draft.as_mut() {
            draft.reading = Some(reading);
        }
    }

    fn keep_reading(&mut self) {
        let Some(reading) = self
            .ocr_draft
            .as_mut()
            .and_then(|draft| draft.reading.as_mut())
        else {
            return;
        };
        if !reading_is_over(reading) {
            return;
        }
        let stopped = reading.cancel.load(Ordering::Relaxed);
        if self.editor.is_busy() {
            return;
        }
        let Some(reading) = self
            .ocr_draft
            .as_mut()
            .and_then(|draft| draft.reading.take())
        else {
            return;
        };
        for worker in reading.workers {
            let _ = worker.join();
        }
        if stopped {
            self.editor.say(Done::RecognitionStopped.into());
            return;
        }
        if reading.epoch != self.editor.epoch() {
            self.editor.say(Done::RecognitionOutdated.into());
            return;
        }
        self.write_what_was_read(reading.read);
    }

    fn write_what_was_read(&mut self, read: BTreeMap<usize, PageRead>) {
        let Sorted {
            layers,
            confidence,
            had_text,
            unread,
            failed,
        } = sorted_out(read);
        if layers.is_empty() {
            let said = match failed {
                Some(why) => Message::Refused(why.into()),
                None if had_text > 0 => Done::NothingToRecognize.into(),
                None => Done::NothingChanged.into(),
            };
            self.editor.say(said);
            return;
        }
        let job = self
            .editor
            .begin_text_layers(layers, (confidence, had_text, unread));
        if job.is_none() {
            self.editor.say(Message::AnotherEditIsRunning);
            return;
        }
        self.send(job);
    }
}

pub(crate) struct ReadJob {
    pub(crate) source: pdf_bytes::ByteStore,
    pub(crate) credential: Vec<u8>,
    pub(crate) fonts: Option<Arc<dyn pdf_content::FontProvider>>,
    pub(crate) engine: pdf_ocr::Tesseract,
    pub(crate) languages: Vec<String>,
    pub(crate) skip_text: bool,
    pub(crate) pages: Vec<usize>,
    pub(crate) epoch: u64,
}

pub(crate) fn start_the_readers(job: ReadJob, repaint: Option<&egui::Context>) -> OcrReading {
    let ReadJob {
        source,
        credential,
        fonts,
        engine,
        languages,
        skip_text,
        pages,
        epoch,
    } = job;
    let cancel = Arc::new(AtomicBool::new(false));
    let next = Arc::new(AtomicUsize::new(0));
    let shared_pages = Arc::new(pages.clone());
    let (send, answers) = mpsc::channel();
    let workers = std::thread::available_parallelism()
        .map_or(1, |cores| cores.get().saturating_sub(1))
        .clamp(1, MOST_WORKERS)
        .min(pages.len().max(1));
    let handles = (0..workers)
        .map(|_| {
            let (cancel, next, pages, send) = (
                Arc::clone(&cancel),
                Arc::clone(&next),
                Arc::clone(&shared_pages),
                send.clone(),
            );
            let (source, credential, fonts, engine, languages, repaint) = (
                source.clone(),
                credential.clone(),
                fonts.clone(),
                engine.clone(),
                languages.clone(),
                repaint.cloned(),
            );
            std::thread::spawn(move || {
                loop {
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    let Some(&page) = pages.get(next.fetch_add(1, Ordering::Relaxed)) else {
                        break;
                    };
                    let read = read_one(
                        &source,
                        page,
                        (&credential, fonts.clone()),
                        (&engine, &languages, skip_text),
                        &cancel,
                    );
                    if send.send((page, read)).is_err() {
                        break;
                    }
                    if let Some(ctx) = &repaint {
                        ctx.request_repaint();
                    }
                }
            })
        })
        .collect();
    OcrReading {
        cancel,
        answers,
        workers: handles,
        pages,
        read: BTreeMap::new(),
        epoch,
    }
}

pub(crate) fn reading_is_over(reading: &mut OcrReading) -> bool {
    while let Ok((page, read)) = reading.answers.try_recv() {
        reading.read.insert(page, read);
    }
    let stopped = reading.cancel.load(Ordering::Relaxed);
    let finished = reading
        .workers
        .iter()
        .all(std::thread::JoinHandle::is_finished);
    finished && (stopped || reading.read.len() == reading.pages.len())
}

pub(crate) struct Sorted {
    pub(crate) layers: Vec<(usize, pdf_edit::text_layer::TextLayer)>,
    pub(crate) confidence: u8,
    pub(crate) had_text: usize,
    pub(crate) unread: usize,
    pub(crate) failed: Option<String>,
}

pub(crate) fn sorted_out(read: BTreeMap<usize, PageRead>) -> Sorted {
    let mut layers = Vec::new();
    let (mut had_text, mut unread, mut failed) = (0, 0, None);
    let (mut weight, mut sum) = (0.0_f64, 0.0_f64);
    for (page, outcome) in read {
        match outcome {
            PageRead::Read(reading) => {
                if reading.layer.words.is_empty() {
                    continue;
                }
                #[allow(clippy::cast_precision_loss)]
                let words = reading.layer.words.len() as f64;
                weight += words;
                sum += words * f64::from(reading.confidence.unwrap_or(0.0));
                layers.push((page, reading.layer));
            }
            PageRead::HadText => had_text += 1,
            PageRead::Failed(why) => {
                unread += 1;
                failed.get_or_insert(why);
            }
        }
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let confidence = (sum / weight.max(1.0)).round().clamp(0.0, 100.0) as u8;
    Sorted {
        layers,
        confidence,
        had_text,
        unread,
        failed,
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
enum Pressed {
    #[default]
    Nothing,
    Read,
    Stop,
    Close,
    GetModel(String),
    RemoveModel(String),
    GetEngine,
}

#[derive(Clone, Debug, Default)]
struct Asked {
    pressed: Pressed,
    quality: Option<Quality>,
    remember: bool,
}

fn the_languages(ui: &mut egui::Ui, draft: &mut OcrDraft, lang: Lang, asked: &mut Asked) {
    dialog::caption(ui, &Message::OcrLanguages.say(lang));
    for code in pdf_app::ocr_languages::yours(&draft.here, &draft.ticked) {
        if let Some(language) = pdf_app::ocr_languages::Language::of(&code) {
            one_language(ui, draft, &language, lang, asked);
        }
    }
    if draft.ticked.len() > 1 {
        dialog::small(ui, &Message::OcrLanguagesCost.say(lang));
    }
    ui.add_space(4.0);
    more_languages(ui, draft, lang, asked);
}

fn more_languages(ui: &mut egui::Ui, draft: &mut OcrDraft, lang: Lang, asked: &mut Asked) {
    let count = draft.list.iter().filter(|language| language.reads).count();
    dialog::foldable(
        ui,
        egui::Id::new("ocr-more-languages"),
        &Message::OcrMoreLanguages(count).say(lang),
        |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut draft.search)
                    .hint_text(Message::OcrSearchLanguages.say(lang))
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(4.0);
            let found: Vec<pdf_app::ocr_languages::Language> =
                pdf_app::ocr_languages::search(&draft.list, &draft.search)
                    .into_iter()
                    .copied()
                    .collect();
            if found.is_empty() {
                dialog::small(ui, &Message::OcrNoLanguageMatches.say(lang));
                return;
            }
            egui::ScrollArea::vertical()
                .id_salt("ocr-more-languages-list")
                .max_height(LIST_HEIGHT)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    let mut apart = false;
                    for language in &found {
                        if !language.reads && !apart {
                            apart = true;
                            ui.add_space(4.0);
                            dialog::hairline(ui);
                            ui.add_space(4.0);
                            dialog::small(ui, &Message::OcrNotLanguages.say(lang));
                        }
                        one_language(ui, draft, language, lang, asked);
                    }
                });
        },
    );
}

fn one_language(
    ui: &mut egui::Ui,
    draft: &mut OcrDraft,
    language: &pdf_app::ocr_languages::Language,
    lang: Lang,
    asked: &mut Asked,
) {
    let code = language.code;
    let name = Message::OcrLanguage(code.to_owned()).say(lang);
    let model = pdf_ocr::models::model(code, draft.choice.quality);
    let here = draft.here.iter().any(|have| have == code);
    let own = draft.own.iter().any(|own| own == code);
    let size = egui::vec2(ui.available_width(), ROW_HEIGHT);
    ui.allocate_ui_with_layout(
        size,
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if let Some(model) = model {
                the_row_end(
                    ui,
                    draft,
                    (code, language.reads, model),
                    (here, own),
                    lang,
                    asked,
                );
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                if !language.reads {
                    ui.add(
                        egui::Label::new(egui::RichText::new(&name).color(dialog::weak(ui)))
                            .truncate(),
                    )
                    .on_hover_text(Message::OcrHelperWhy.say(lang));
                    return;
                }
                let mut on = draft.ticked.iter().any(|ticked| ticked == code);
                let tick = ui.add_enabled(here, egui::Checkbox::without_text(&mut on));
                let text = if here {
                    egui::RichText::new(&name)
                } else {
                    egui::RichText::new(&name).color(dialog::weak(ui))
                };
                let label = ui
                    .add(egui::Label::new(text).truncate().sense(if here {
                        egui::Sense::click()
                    } else {
                        egui::Sense::hover()
                    }))
                    .on_hover_text(&name);
                if tick.changed() || label.clicked() {
                    let on = if tick.changed() { on } else { !on };
                    draft.tick(code, on);
                    asked.remember = true;
                }
            });
        },
    );
}

fn the_row_end(
    ui: &mut egui::Ui,
    draft: &OcrDraft,
    (code, reads, model): (&str, bool, &pdf_ocr::models::Model),
    (here, own): (bool, bool),
    lang: Lang,
    asked: &mut Asked,
) {
    let weak = dialog::weak(ui);
    let bytes = Message::OcrSize(model.bytes).say(lang);
    let size_label = |ui: &mut egui::Ui| {
        ui.label(egui::RichText::new(&bytes).size(12.0).color(weak));
    };
    if !reads {
        size_label(ui);
        return;
    }
    if let Some(fetch) = draft
        .fetching
        .as_ref()
        .filter(|fetch| fetch.code.as_deref() == Some(code))
    {
        if quiet_icon_button(ui, Icon::Close, &Message::OcrStop.say(lang)).clicked() {
            asked.pressed = Pressed::Stop;
        }
        let share = fetch_share(fetch);
        ui.allocate_ui(egui::vec2(ROW_BAR, 8.0), |ui| dialog::bar(ui, share))
            .response
            .on_hover_text(Message::OcrGettingModel(code.to_owned()).say(lang));
        return;
    }
    let free = draft.fetching.is_none();
    if own {
        let hover = Message::OcrRemoveModel {
            code: code.to_owned(),
            bytes: model.bytes,
        }
        .say(lang);
        ui.add_enabled_ui(free, |ui| {
            if quiet_icon_button(ui, Icon::Delete, &hover).clicked() {
                asked.pressed = Pressed::RemoveModel(code.to_owned());
            }
        });
        size_label(ui);
    } else if here {
        ui.label(
            egui::RichText::new(Message::OcrFromTheSystem.say(lang))
                .size(12.0)
                .color(weak),
        );
    } else {
        let hover = format!(
            "{}\n{}",
            Message::OcrDownloadModel {
                code: code.to_owned(),
                bytes: model.bytes,
            }
            .say(lang),
            model.url()
        );
        ui.add_enabled_ui(free, |ui| {
            if quiet_icon_button(ui, Icon::Download, &hover).clicked() {
                asked.pressed = Pressed::GetModel(code.to_owned());
            }
        });
        size_label(ui);
    }
}

fn fetch_share(fetch: &OcrFetch) -> Option<f32> {
    if fetch.total == 0 {
        return None;
    }
    let seen = fetch.seen.load(Ordering::Relaxed);
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        reason = "a share of a download, which a bar draws to the pixel"
    )]
    let share = (seen as f64 / fetch.total as f64) as f32;
    Some(share.clamp(0.0, 1.0))
}

fn the_accuracy(ui: &mut egui::Ui, draft: &mut OcrDraft, lang: Lang, asked: &mut Asked) {
    dialog::caption(ui, &Message::OcrAccuracy.say(lang));
    let mut quality = draft.choice.quality;
    let options: Vec<(Quality, String)> = Quality::ALL
        .into_iter()
        .map(|quality| (quality, Message::OcrQuality(quality).say(lang)))
        .collect();
    if dialog::segments(ui, "ocr-accuracy", &mut quality, &options) {
        asked.quality = Some(quality);
    }
    ui.add_space(4.0);
    let rates: Vec<String> =
        pdf_app::ocr_languages::how_well(&draft.chosen(), draft.choice.quality)
            .iter()
            .map(|said| said.say(lang))
            .collect();
    let line = dialog::weak(ui);
    let tradeoff = Message::OcrQualityTradeoff(draft.choice.quality).say(lang);
    let shown =
        ui.add(egui::Label::new(egui::RichText::new(tradeoff).size(11.5).color(line)).wrap());
    if !rates.is_empty() {
        shown.on_hover_text(rates.join("\n"));
    }
}

fn the_pages(
    ui: &mut egui::Ui,
    draft: &mut OcrDraft,
    (lang, count): (Lang, usize),
    asked: &mut Asked,
) {
    dialog::caption(ui, &Message::StampPages.say(lang));
    let options = [
        (OcrWhich::ThisPage, Message::OcrThisPage.say(lang)),
        (OcrWhich::All, Message::AllPages.say(lang)),
        (OcrWhich::Some, Message::SomePages.say(lang)),
    ];
    dialog::segments(ui, "ocr-pages", &mut draft.which, &options);
    if draft.which == OcrWhich::Some {
        ui.add_space(6.0);
        ui.add(
            egui::TextEdit::singleline(&mut draft.range)
                .hint_text(format!("1-{count}"))
                .desired_width(f32::INFINITY),
        );
    }
    ui.add_space(6.0);
    if ui
        .checkbox(&mut draft.choice.skip_text, Message::OcrSkipText.say(lang))
        .changed()
    {
        asked.remember = true;
    }
}

fn the_recogniser_itself(ui: &mut egui::Ui, draft: &OcrDraft, lang: Lang, asked: &mut Asked) {
    let installable = pdf_ocr::setup::possible();
    dialog::card(ui, |ui| {
        let said = if installable {
            Message::OcrNeedsRecogniser
        } else {
            Message::OcrCannotInstallRecogniser
        };
        dialog::note(ui, dialog::Tone::Calm, &said.say(lang));
        if installable {
            ui.add_space(8.0);
            if draft
                .fetching
                .as_ref()
                .is_some_and(|fetch| fetch.code.is_none())
            {
                dialog::small(ui, &Message::OcrGettingEngine.say(lang));
                ui.add_space(4.0);
                dialog::bar(ui, None);
            } else {
                let free = draft.fetching.is_none();
                let words = Message::OcrGetEngine.say(lang);
                if dialog::primary(ui, &words, free).clicked() {
                    asked.pressed = Pressed::GetEngine;
                }
            }
        }
        ui.add_space(6.0);
        let details = if installable {
            Message::OcrNotInstalled
        } else {
            Message::OcrEngineElsewhere
        };
        dialog::foldable(
            ui,
            egui::Id::new("ocr-engine-details"),
            &Fact::Details.say(lang),
            |ui| dialog::small(ui, &details.say(lang)),
        );
    });
}

fn how_far(ui: &mut egui::Ui, draft: &OcrDraft, lang: Lang) {
    let Some(reading) = &draft.reading else {
        return;
    };
    let total = reading.pages.len();
    let done = reading.read.len();
    #[expect(
        clippy::cast_precision_loss,
        reason = "a share of the pages, which a bar draws to the pixel"
    )]
    let share = done as f32 / total.max(1) as f32;
    ui.add_space(8.0);
    dialog::small(ui, &Message::OcrProgress { done, total }.say(lang));
    ui.add_space(4.0);
    dialog::bar(ui, Some(share));
}

fn the_choices(
    ui: &mut egui::Ui,
    draft: &mut OcrDraft,
    (lang, count): (Lang, usize),
    asked: &mut Asked,
) {
    if draft.engine.is_none() {
        the_recogniser_itself(ui, draft, lang, asked);
        ui.add_space(8.0);
    }
    ui.add_enabled_ui(draft.reading.is_none(), |ui| {
        the_languages(ui, draft, lang, asked);
        dialog::divide(ui);
        the_accuracy(ui, draft, lang, asked);
        dialog::divide(ui);
        the_pages(ui, draft, (lang, count), asked);
    });
}

fn blocked(
    draft: &OcrDraft,
    pages: Option<&Result<Vec<usize>, Message>>,
) -> Option<(dialog::Tone, Message)> {
    if let Some(Err(why)) = pages {
        return Some((dialog::Tone::Trouble, why.clone()));
    }
    if draft.engine.is_none() {
        return Some((dialog::Tone::Calm, Message::OcrNeedsRecogniser));
    }
    if draft.chosen().is_empty() {
        return Some((dialog::Tone::Warning, Message::OcrNoLanguage));
    }
    draft.split().map(|split| (dialog::Tone::Warning, split))
}

fn what_stands_in_the_way(
    ui: &mut egui::Ui,
    draft: &OcrDraft,
    lang: Lang,
    pages: Option<&Result<Vec<usize>, Message>>,
) {
    if let Some(trouble) = &draft.trouble {
        ui.add_space(8.0);
        dialog::note(ui, dialog::Tone::Trouble, &trouble.say(lang));
    }
    if draft.engine.is_none() {
        return;
    }
    if let Some((tone, why)) = blocked(draft, pages) {
        ui.add_space(8.0);
        dialog::note(ui, tone, &why.say(lang));
    }
}

fn the_buttons(
    ui: &mut egui::Ui,
    draft: &OcrDraft,
    (lang, pages): (Lang, Option<&Result<Vec<usize>, Message>>),
    asked: &mut Asked,
) {
    let reason = blocked(draft, pages).map(|(_, why)| why.say(lang));
    dialog::footer(ui, |ui| {
        if draft.reading.is_some() || draft.fetching.is_some() {
            if dialog::secondary(ui, &Message::OcrStop.say(lang)).clicked() {
                asked.pressed = Pressed::Stop;
            }
            return;
        }
        let count = pages
            .and_then(|pages| pages.as_ref().ok())
            .map_or(0, Vec::len);
        let ready = count > 0 && reason.is_none();
        let start = dialog::primary(ui, &Message::OcrStart(count).say(lang), ready);
        if let Some(reason) = &reason {
            start.on_disabled_hover_text(reason);
        } else if start.clicked() {
            asked.pressed = Pressed::Read;
        }
        if dialog::secondary(ui, &Message::Close.say(lang)).clicked() {
            asked.pressed = Pressed::Close;
        }
    });
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicU64};
    use std::sync::{Arc, mpsc};

    use pdf_app::wording::Message;

    use super::{blocked, draft, fetch_share};
    use crate::dialog::Tone;
    use crate::window_state::OcrFetch;

    fn fetching(seen: u64, total: u64) -> OcrFetch {
        let (_send, answer) = mpsc::channel();
        OcrFetch {
            code: Some("lao".to_owned()),
            seen: Arc::new(AtomicU64::new(seen)),
            total,
            cancel: Arc::new(AtomicBool::new(false)),
            answer,
            worker: None,
        }
    }

    #[test]
    fn a_download_shows_the_share_of_it_that_has_arrived() {
        let share = fetch_share(&fetching(50, 200)).expect("the size is known");
        assert!((share - 0.25).abs() < 1e-6, "{share}");
        assert_eq!(
            fetch_share(&fetching(0, 0)),
            None,
            "an unknown size only pulses"
        );
        let over = fetch_share(&fetching(900, 200)).expect("the size is known");
        assert!(
            (over - 1.0).abs() < 1e-6,
            "a bar never runs past its end: {over}"
        );
    }

    #[test]
    fn a_missing_recogniser_is_the_first_thing_said_to_stand_in_the_way() {
        let mut stock = draft();
        stock.engine = None;
        let (tone, why) = blocked(&stock, None).expect("nothing can be read without one");
        assert_eq!(tone, Tone::Calm, "it is a step to take, not a fault");
        assert_eq!(why, Message::OcrNeedsRecogniser);
    }

    #[test]
    fn a_page_range_that_names_nothing_is_trouble_before_anything_else() {
        let mut stock = draft();
        stock.engine = None;
        let range = Err(Message::Refused("no such page".to_owned().into()));
        let (tone, _) = blocked(&stock, Some(&range)).expect("the range is refused");
        assert_eq!(tone, Tone::Trouble);
    }
}
