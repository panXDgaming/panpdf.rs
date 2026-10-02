use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;

use pdf_agent::connect::{ToolCall, ToolResult};
use pdf_agent::converting::{self, Asked, Source};
use pdf_agent::desk;
use pdf_agent::taking::{self, Pictures, Split};
use pdf_agent::tools::request::Request;
use pdf_app::ai_permission::WrittenTo;
use pdf_app::wording::Message;
use pdf_convert::Tool;
use pdf_convert::run::{Outcome, Saved};

use super::editing::{failed, said};
use super::{Performed, Tools};
use crate::tools_run::{self, Order, Origin, Poll, Working};
use crate::window_state::Window;

pub(super) struct Converting {
    call: ToolCall,
    working: Working,
    tool: Tool,
    names: Vec<String>,
    base: (PathBuf, PathBuf),
    open_result: bool,
}

pub(super) struct Making {
    call: ToolCall,
    handle: JoinHandle<Result<Made, String>>,
}

struct Made {
    saved: Saved,
    said: String,
    bytes: u64,
    pictures: bool,
}

impl Tools {
    pub(super) fn files_in_flight(&self) -> bool {
        self.converting.is_some() || self.making.is_some() || self.recognizing.is_some()
    }

    pub(super) fn stop_the_files(&mut self) {
        if let Some(converting) = self.converting.as_mut() {
            converting.working.stop();
        }
        if let Some(recognizing) = self.recognizing.as_ref() {
            recognizing.cancel();
        }
    }
}

fn many(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

impl Window {
    pub(super) fn perform_filing(
        &mut self,
        call: &ToolCall,
        request: &Request,
        pages: usize,
    ) -> Performed {
        match request {
            Request::Convert(asked) => self.convert(call, asked),
            Request::ExtractPages(asked) => self.extract_pages(call, &asked.pages, pages),
            Request::SplitDocument(split) => self.split_document(call, split, pages),
            Request::ExportPictures(asked) => self.export_pictures(call, asked, pages),
            Request::SaveCopy { path } => self.save_copy(call, path.as_deref()),
            Request::OcrPages(asked) => self.recognize_pages(call, request.clone(), asked, pages),
            _ => failed(call, "that is not one of the tools that write files"),
        }
    }

    fn has_a_file(&self) -> bool {
        !self.untitled && !self.opened.as_os_str().is_empty()
    }

    fn the_name_to_go_by(&self) -> PathBuf {
        if self.has_a_file() {
            self.opened.clone()
        } else {
            PathBuf::from("document.pdf")
        }
    }

    pub(super) fn where_files_go(&self, first: Option<&Path>) -> (PathBuf, PathBuf) {
        let there = |path: &Path| path.parent().map(Path::to_path_buf);
        let (original, folder) = match first {
            Some(path) => (path.to_path_buf(), there(path)),
            None if self.has_a_file() => (self.opened.clone(), there(&self.opened)),
            None => (PathBuf::new(), None),
        };
        let folder = folder
            .filter(|folder| !folder.as_os_str().is_empty())
            .or_else(tools_run::documents_folder)
            .unwrap_or_else(|| PathBuf::from("."));
        (original, folder)
    }

    pub(crate) fn the_place_for(&self, request: &Request) -> Option<WrittenTo> {
        let folder = |first: Option<&Path>| {
            Some(WrittenTo::Folder(
                self.where_files_go(first).1.display().to_string(),
            ))
        };
        match request {
            Request::Convert(asked) => {
                let first = asked
                    .sources()
                    .ok()
                    .and_then(|sources| match sources.first() {
                        Some(Source::File(path)) => Some(path.clone()),
                        _ => None,
                    });
                folder(first.as_deref())
            }
            Request::ExtractPages(_) | Request::SplitDocument(_) | Request::ExportPictures(_) => {
                folder(None)
            }
            Request::SaveCopy { path: Some(path) } => {
                Some(WrittenTo::File(path.display().to_string()))
            }
            Request::SaveCopy { path: None } if self.has_a_file() => Some(WrittenTo::File(
                crate::save_file::unused_copy(&self.opened)
                    .display()
                    .to_string(),
            )),
            _ => None,
        }
    }

    fn the_bytes_for(&mut self, call: &ToolCall) -> Result<(Arc<[u8]>, Vec<u8>), Performed> {
        if self.editor.is_busy() {
            return Err(Performed::Busy);
        }
        match self.editor.export() {
            Ok(export) => Ok((Arc::from(export.bytes), self.editor.credential().to_vec())),
            Err(why) => Err(failed(
                call,
                format!("the document cannot be written out just now: {why}"),
            )),
        }
    }

    fn convert(&mut self, call: &ToolCall, asked: &Asked) -> Performed {
        let sources = match asked.sources() {
            Ok(sources) => sources,
            Err(why) => return failed(call, why),
        };
        let mut values = match asked.values_with_pictures() {
            Ok(values) => values,
            Err(why) => return failed(call, why),
        };
        let wants_document = sources.contains(&Source::Document);
        let exported = if wants_document {
            match self.the_bytes_for(call) {
                Ok((bytes, _)) => Some(bytes.to_vec()),
                Err(performed) => return performed,
            }
        } else {
            None
        };
        let credential = self.editor.credential().to_vec();
        let document = converting::name_of(&self.the_name_to_go_by());
        let first = match sources.first() {
            Some(Source::File(path)) => Some(path.clone()),
            _ => None,
        };
        let base = self.where_files_go(first.as_deref());
        let mut exported = exported;
        let mut inputs = Vec::with_capacity(sources.len());
        let mut names = Vec::with_capacity(sources.len());
        for (at, source) in sources.iter().enumerate() {
            match source {
                Source::Document => {
                    values.give_the_credential(asked.tool, at, &credential);
                    names.push(document.clone());
                    inputs.push((
                        document.clone(),
                        Origin::Document(exported.take().unwrap_or_default()),
                    ));
                }
                Source::File(path) => {
                    let name = converting::name_of(path);
                    names.push(name.clone());
                    inputs.push((name, Origin::File(path.clone())));
                }
            }
        }
        let working = tools_run::begin(Order {
            tool: asked.tool,
            values,
            sources: inputs,
        });
        self.ai.tools.converting = Some(Converting {
            call: call.clone(),
            working,
            tool: asked.tool,
            names,
            base,
            open_result: asked.open_result,
        });
        Performed::Waiting
    }

    pub(super) fn keep_converting(&mut self) {
        let Some(mut converting) = self.ai.tools.converting.take() else {
            return;
        };
        let result = match converting.working.poll() {
            Poll::Going => {
                self.ai.tools.converting = Some(converting);
                return;
            }
            Poll::Finished(result) => result,
        };
        let call = converting.call.clone();
        let result = match result {
            Ok(outcome) if outcome.made_nothing() => ToolResult::failed(
                &call.id,
                format!(
                    "The converter made no file: {}",
                    if outcome.notes.is_empty() {
                        "it gave no reason".to_owned()
                    } else {
                        outcome.notes.join("; ")
                    }
                ),
            ),
            Ok(outcome) => self.the_conversion_is_made(converting, &outcome),
            Err(failure) => ToolResult::failed(&call.id, converting::why(&failure)),
        };
        self.ai.tools.answer(result);
    }

    fn the_conversion_is_made(&mut self, converting: Converting, outcome: &Outcome) -> ToolResult {
        let Converting {
            call,
            tool,
            names,
            base,
            open_result,
            ..
        } = converting;
        let inputs: Vec<&str> = names.iter().map(String::as_str).collect();
        let saved = match tools_run::save_beside(tool, outcome, &inputs, (&base.0, &base.1)) {
            Ok(saved) => saved,
            Err(why) => {
                return ToolResult::failed(
                    &call.id,
                    format!("The result could not be written, and nothing was changed: {why}"),
                );
            }
        };
        let bytes: u64 = outcome
            .files
            .iter()
            .map(|(_, bytes)| bytes.len() as u64)
            .sum();
        self.tell_what_was_written(&saved, bytes, tool == Tool::PdfToImage);
        let mut text = converting::said_made(&saved, outcome);
        if open_result {
            text.push_str(&self.open_what_was_made(&saved));
        }
        ToolResult::said(&call.id, text)
    }

    fn open_what_was_made(&mut self, saved: &Saved) -> String {
        let pdf = match saved.files.as_slice() {
            [one]
                if one
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf")) =>
            {
                one.clone()
            }
            _ => {
                return " It was not opened in the window: only a single PDF can be.".to_owned();
            }
        };
        if self.ai.tools.queue.len() > 1 {
            return " It was not opened in the window, because other calls in this reply \
                    would then run on a different document: ask for that in a reply of its own."
                .to_owned();
        }
        self.open(&pdf);
        self.ai.tools.forget_the_names();
        " The window is opening it now (the person is asked first if this document has changes \
         they have not saved); the document this chat is about will be the new one."
            .to_owned()
    }

    fn tell_what_was_written(&mut self, saved: &Saved, bytes: u64, pictures: bool) {
        let said = match saved.files.as_slice() {
            [] => return,
            [one] => Message::SavedTo {
                name: one.display().to_string(),
                bytes,
            },
            [first, ..] if pictures => Message::WrotePictures {
                files: saved.files.len(),
                first: first.display().to_string(),
                bytes,
            },
            [first, ..] => Message::WroteFiles {
                files: saved.files.len(),
                first: first.display().to_string(),
                bytes,
            },
        };
        self.editor.say(said);
    }

    fn start_making(
        &mut self,
        call: &ToolCall,
        work: impl FnOnce() -> Result<Made, String> + Send + 'static,
    ) -> Performed {
        let handle = std::thread::spawn(work);
        self.ai.tools.making = Some(Making {
            call: call.clone(),
            handle,
        });
        Performed::Waiting
    }

    pub(super) fn keep_making(&mut self) {
        let Some(making) = self.ai.tools.making.take() else {
            return;
        };
        if !making.handle.is_finished() {
            self.ai.tools.making = Some(making);
            return;
        }
        let Making { call, handle } = making;
        let result = match handle.join() {
            Ok(Ok(made)) => {
                self.tell_what_was_written(&made.saved, made.bytes, made.pictures);
                ToolResult::said(&call.id, made.said)
            }
            Ok(Err(why)) => ToolResult::failed(&call.id, why),
            Err(_) => ToolResult::failed(&call.id, "the work stopped before it was done"),
        };
        self.ai.tools.answer(result);
    }

    fn extract_pages(&mut self, call: &ToolCall, spec: &str, count: usize) -> Performed {
        let pages = match taking::pages_named(spec, count) {
            Ok(pages) => pages,
            Err(why) => return failed(call, why),
        };
        let (bytes, credential) = match self.the_bytes_for(call) {
            Ok(held) => held,
            Err(performed) => return performed,
        };
        let name = self.the_name_to_go_by();
        let base = self.where_files_go(None);
        self.start_making(call, move || {
            let outcome = taking::extract_outcome(&name, &bytes, &credential, &pages)?;
            let (saved, total) = save_these(&outcome, &name, &base)?;
            let said = taking::said_extracted(&pages, &converting::said_made(&saved, &outcome));
            Ok(Made {
                saved,
                said,
                bytes: total,
                pictures: false,
            })
        })
    }

    fn split_document(&mut self, call: &ToolCall, split: &Split, count: usize) -> Performed {
        let groups = match split.groups(count) {
            Ok(groups) => groups,
            Err(why) => return failed(call, why),
        };
        let (bytes, credential) = match self.the_bytes_for(call) {
            Ok(held) => held,
            Err(performed) => return performed,
        };
        let name = self.the_name_to_go_by();
        let base = self.where_files_go(None);
        self.start_making(call, move || {
            let outcome = taking::split_outcome(&name, (&bytes, &credential), &groups)?;
            let (saved, total) = save_these(&outcome, &name, &base)?;
            let said = taking::said_split(&groups, &converting::said_made(&saved, &outcome));
            Ok(Made {
                saved,
                said,
                bytes: total,
                pictures: false,
            })
        })
    }

    fn export_pictures(&mut self, call: &ToolCall, asked: &Pictures, count: usize) -> Performed {
        let pages = match taking::pages_named(&asked.pages, count) {
            Ok(pages) => pages,
            Err(why) => return failed(call, why),
        };
        if pages.len() > taking::MOST_FILES {
            return failed(
                call,
                format!(
                    "that is {} pictures: the most one call writes is {}",
                    pages.len(),
                    taking::MOST_FILES
                ),
            );
        }
        if self.editor.is_busy() {
            return Performed::Busy;
        }
        let Some(source) = self.editor.source().cloned() else {
            return self.no_source(call);
        };
        let credential = self.editor.credential().to_vec();
        let fonts = self.editor.fonts();
        let name = self.the_name_to_go_by();
        let base = self.where_files_go(None);
        let dpi = asked.dpi;
        self.start_making(call, move || {
            let names = taking::picture_names(&name, &pages);
            let mut files = Vec::with_capacity(pages.len());
            for (page, file) in pages.iter().zip(names) {
                let view = pdf_session::interpret_page_for_display(
                    &source,
                    *page,
                    &credential,
                    None,
                    fonts.clone(),
                )
                .map_err(|error| format!("page {} cannot be read: {error}", page + 1))?;
                let picture = desk::page_png(&view, dpi)
                    .map_err(|why| format!("page {} cannot be drawn: {why}", page + 1))?;
                files.push((file, picture));
            }
            let outcome = Outcome {
                files,
                notes: Vec::new(),
            };
            let (saved, total) = save_these(&outcome, &name, &base)?;
            let said = taking::said_pictures(&pages, dpi, &converting::said_made(&saved, &outcome));
            Ok(Made {
                saved,
                said,
                bytes: total,
                pictures: true,
            })
        })
    }

    fn save_copy(&mut self, call: &ToolCall, path: Option<&Path>) -> Performed {
        let destination = match path {
            Some(path) => path.to_path_buf(),
            None if self.has_a_file() => crate::save_file::unused_copy(&self.opened),
            None => {
                return failed(
                    call,
                    "this document has no file of its own yet, so there is no \"beside the \
                     original\": give `path`, a full path for the copy",
                );
            }
        };
        let (bytes, _) = match self.the_bytes_for(call) {
            Ok(held) => held,
            Err(performed) => return performed,
        };
        let original = if self.has_a_file() {
            self.opened.clone()
        } else {
            PathBuf::new()
        };
        match tools_run::write_a_copy(&original, &destination, &bytes) {
            Ok(saved) => {
                self.tell_what_was_written(&saved, bytes.len() as u64, false);
                said(
                    call,
                    format!(
                        "Wrote a copy of the document as it is now, {} byte{}, to {}. It is a new \
                         file: the window still shows the document under its own name, and the \
                         person's own Save is theirs to press.",
                        bytes.len(),
                        many(bytes.len()),
                        destination.display()
                    ),
                )
            }
            Err(why) => failed(
                call,
                format!(
                    "No copy was written: {why}. A copy goes to a path no file has yet and is \
                     never written over one."
                ),
            ),
        }
    }
}

fn save_these(
    outcome: &Outcome,
    name: &Path,
    (original, base): &(PathBuf, PathBuf),
) -> Result<(Saved, u64), String> {
    let document = converting::name_of(name);
    let saved = tools_run::put_beside(None, outcome, &[document.as_str()], (original, base))
        .map_err(|why| format!("The files could not be written, and nothing was changed: {why}"))?;
    let total = outcome
        .files
        .iter()
        .map(|(_, bytes)| bytes.len() as u64)
        .sum();
    Ok((saved, total))
}

#[cfg(test)]
mod tests;
