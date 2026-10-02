use std::sync::atomic::Ordering;

use pdf_agent::connect::{ToolCall, ToolResult};
use pdf_agent::recognizing::{
    self, Asked, said_no_engine, said_no_language, said_not_in_one_place,
};
use pdf_agent::taking;
use pdf_agent::tools::request::Request;
use pdf_app::wording::{Done, Lang, Message};

use super::Performed;
use super::editing::failed;
use crate::ocr_tool::{ReadJob, Sorted, draft, reading_is_over, sorted_out, start_the_readers};
use crate::window_state::{OcrReading, Window};

pub(super) struct Recognizing {
    call: ToolCall,
    request: Request,
    reading: OcrReading,
    languages: Vec<String>,
}

impl Recognizing {
    pub(super) fn cancel(&self) {
        self.reading.cancel.store(true, Ordering::Relaxed);
    }
}

impl Drop for Recognizing {
    fn drop(&mut self) {
        self.cancel();
    }
}

pub(super) struct Stock<'a> {
    pub(super) here: &'a [String],
    pub(super) own: &'a [String],
    pub(super) system: &'a [String],
    pub(super) ticked: &'a [String],
}

pub(super) fn the_languages(asked: &[String], stock: &Stock<'_>) -> Result<Vec<String>, String> {
    let languages = if asked.is_empty() {
        stock.ticked.to_vec()
    } else {
        let missing: Vec<String> = asked
            .iter()
            .filter(|code| !stock.here.contains(code))
            .cloned()
            .collect();
        if !missing.is_empty() {
            return Err(said_no_language(&missing, stock.here));
        }
        asked.to_vec()
    };
    if languages.is_empty() {
        return Err(said_no_language(&[], stock.here));
    }
    pdf_app::ocr_languages::one_place(stock.own, stock.system, &languages)
        .map_err(|apart| said_not_in_one_place(&apart))?;
    Ok(languages)
}

impl Window {
    pub(super) fn recognize_pages(
        &mut self,
        call: &ToolCall,
        request: Request,
        asked: &Asked,
        count: usize,
    ) -> Performed {
        let pages = match taking::pages_named(&asked.pages, count) {
            Ok(pages) => pages,
            Err(why) => return failed(call, why),
        };
        let Some(source) = self.editor.source().cloned() else {
            return self.no_source(call);
        };
        if self.editor.is_busy() {
            return Performed::Busy;
        }
        let stock = draft();
        let Some(engine) = stock.engine.clone() else {
            return failed(
                call,
                said_no_engine(&Message::OcrNotInstalled.say(Lang::English)),
            );
        };
        let languages = match the_languages(
            &asked.languages,
            &Stock {
                here: &stock.here,
                own: &stock.own,
                system: &stock.system,
                ticked: &stock.ticked,
            },
        ) {
            Ok(languages) => languages,
            Err(why) => return failed(call, why),
        };
        let reading = start_the_readers(
            ReadJob {
                source,
                credential: self.editor.credential().to_vec(),
                fonts: self.editor.fonts(),
                engine,
                languages: languages.clone(),
                skip_text: asked.skip_text,
                pages,
                epoch: self.editor.epoch(),
            },
            None,
        );
        self.ai.tools.recognizing = Some(Recognizing {
            call: call.clone(),
            request,
            reading,
            languages,
        });
        Performed::Waiting
    }

    pub(super) fn keep_recognizing(&mut self) {
        let Some(mut recognizing) = self.ai.tools.recognizing.take() else {
            return;
        };
        if !reading_is_over(&mut recognizing.reading) || self.editor.is_busy() {
            self.ai.tools.recognizing = Some(recognizing);
            return;
        }
        let stopped = recognizing.reading.cancel.load(Ordering::Relaxed);
        let epoch = recognizing.reading.epoch;
        let call = recognizing.call.clone();
        let request = recognizing.request.clone();
        let languages = recognizing.languages.clone();
        let read = std::mem::take(&mut recognizing.reading.read);
        for worker in std::mem::take(&mut recognizing.reading.workers) {
            let _ = worker.join();
        }
        drop(recognizing);
        if stopped {
            self.ai.tools.answer(ToolResult::failed(
                &call.id,
                "The person stopped the reading; nothing was written.",
            ));
            return;
        }
        if epoch != self.editor.epoch() {
            self.ai.tools.answer(ToolResult::failed(
                &call.id,
                format!(
                    "{}. Ask again if it is still wanted.",
                    Done::RecognitionOutdated.say(Lang::English)
                ),
            ));
            return;
        }
        let Sorted {
            layers,
            confidence,
            had_text,
            unread,
            failed: failure,
        } = sorted_out(read);
        if layers.is_empty() {
            let said = recognizing::said_nothing_to_read(had_text, unread, failure.as_deref());
            let result = if failure.is_some() {
                ToolResult::failed(&call.id, said)
            } else {
                ToolResult::said(&call.id, said)
            };
            self.ai.tools.answer(result);
            return;
        }
        let pages: Vec<usize> = layers.iter().map(|(page, _)| *page).collect();
        let said = recognizing::said_read(&pages, (confidence, had_text, unread), &languages);
        let job = self
            .editor
            .begin_text_layers(layers, (confidence, had_text, unread));
        if let Performed::Sent = self.send_for(&call, request, job, None) {
            self.sent_changing(pages, Some(said));
        } else {
            self.ai.tools.answer(ToolResult::failed(
                &call.id,
                "The editor was busy, so nothing was written: ask again.",
            ));
        }
    }
}

#[cfg(test)]
mod tests;
