use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Instant;

use pdf_app::tools::{named_after, placed, reports_each_file, unused};
use pdf_app::wording::{Lang, Tools};
use pdf_convert::Tool;
use pdf_convert::Values;
use pdf_convert::run::{Context, Failure, Input, Outcome, Progress, Running, start};

pub(crate) enum Origin {
    Document(Vec<u8>),
    File(PathBuf),
}

pub(crate) struct Order {
    pub(crate) tool: Tool,
    pub(crate) values: Values,
    pub(crate) sources: Vec<(String, Origin)>,
}

enum Phase {
    Starting(Receiver<Result<Running, Failure>>),
    Going(Running),
}

pub(crate) struct Working {
    started: Instant,
    phase: Phase,
    pub(crate) stopping: bool,
    pub(crate) files: usize,
    pub(crate) pictures: bool,
}

pub(crate) enum Poll {
    Going,
    Finished(Result<Outcome, Failure>),
}

pub(crate) fn begin(order: Order) -> Working {
    let (send, receive) = channel();
    let files = if reports_each_file(order.tool) {
        order.sources.len()
    } else {
        1
    };
    let pictures = !order.tool.starts_from_a_pdf();
    let spawned = std::thread::Builder::new()
        .name("tool-start".to_owned())
        .spawn(move || {
            let _ = send.send(prepared(order));
        });
    let phase = match spawned {
        Ok(_) => Phase::Starting(receive),
        Err(error) => {
            let (send, receive) = channel();
            let _ = send.send(Err(Failure::Refused(error.to_string())));
            Phase::Starting(receive)
        }
    };
    Working {
        started: Instant::now(),
        phase,
        stopping: false,
        files,
        pictures,
    }
}

fn prepared(order: Order) -> Result<Running, Failure> {
    let Order {
        tool,
        values,
        sources,
    } = order;
    let mut inputs = Vec::with_capacity(sources.len());
    for (name, origin) in sources {
        let bytes = match origin {
            Origin::Document(bytes) => bytes,
            Origin::File(path) => std::fs::read(&path)
                .map_err(|error| Failure::BadInput(format!("{}: {error}", path.display())))?,
        };
        inputs.push(Input::new(name, bytes));
    }
    let fonts = pdf_cli::font_provider();
    Ok(start(tool, inputs, values, Context { fonts }))
}

impl Working {
    pub(crate) fn poll(&mut self) -> Poll {
        match &self.phase {
            Phase::Starting(receive) => match receive.try_recv() {
                Ok(Ok(running)) => {
                    if self.stopping {
                        running.cancel();
                    }
                    self.phase = Phase::Going(running);
                    Poll::Going
                }
                Ok(Err(failure)) => Poll::Finished(Err(failure)),
                Err(TryRecvError::Empty) => Poll::Going,
                Err(TryRecvError::Disconnected) => Poll::Finished(Err(if self.stopping {
                    Failure::Cancelled
                } else {
                    Failure::Refused("the work stopped before it began".to_owned())
                })),
            },
            Phase::Going(running) => running.finished().map_or(Poll::Going, Poll::Finished),
        }
    }

    pub(crate) fn stop(&mut self) {
        self.stopping = true;
        match &self.phase {
            Phase::Starting(_) => {
                let (_, closed) = channel();
                self.phase = Phase::Starting(closed);
            }
            Phase::Going(running) => running.cancel(),
        }
    }

    pub(crate) fn seconds_tenths(&self) -> u32 {
        let tenths = self.started.elapsed().as_millis() / 100;
        u32::try_from(tenths).unwrap_or(u32::MAX)
    }

    pub(crate) fn how_far(&self) -> Option<f32> {
        let Phase::Going(running) = &self.phase else {
            return None;
        };
        if self.stopping {
            return None;
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a share of the work between nothing and all of it"
        )]
        running
            .fraction()
            .map(|fraction| fraction.clamp(0.0, 1.0) as f32)
    }

    pub(crate) fn words(&self, lang: Lang) -> String {
        if self.stopping {
            return Tools::Stopping.say(lang);
        }
        let Phase::Going(running) = &self.phase else {
            return Tools::Starting.say(lang);
        };
        progress_line(
            running.progress(),
            running.fraction(),
            (self.files, self.pictures),
            lang,
        )
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "a count of files and a share of the work between nothing and all of it"
)]
pub(crate) fn progress_line(
    progress: Progress,
    fraction: Option<f64>,
    (files, pictures): (usize, bool),
    lang: Lang,
) -> String {
    let say = |sentence: Tools| sentence.say(lang);
    let percent = |fraction: f64| {
        say(Tools::Percent(
            (fraction.clamp(0.0, 1.0) * 100.0).round() as u32
        ))
    };
    let which_file = |index: usize| {
        (files > 1).then(|| {
            say(Tools::FileOf {
                index: index.min(files - 1),
                of: files,
            })
        })
    };
    let mut parts: Vec<String> = Vec::new();
    match progress {
        Progress::Starting => return say(Tools::Starting),
        Progress::Writing => return say(Tools::Writing),
        Progress::Busy => return say(Tools::WorkingOnIt),
        Progress::File { index, .. } => {
            parts.extend(which_file(index));
        }
        Progress::Step { done, total } => {
            if let Some(fraction) = fraction {
                let index = (fraction * files as f64 - done as f64 / total.max(1) as f64)
                    .round()
                    .max(0.0) as usize;
                parts.extend(which_file(index));
            }
            parts.push(say(if pictures {
                Tools::PictureOf { done, total }
            } else {
                Tools::PageOf { done, total }
            }));
        }
    }
    if let Some(fraction) = fraction {
        parts.push(percent(fraction));
    }
    if parts.is_empty() {
        return say(Tools::WorkingOnIt);
    }
    parts.join(" \u{00b7} ")
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Saved {
    pub(crate) folder: PathBuf,
    pub(crate) files: Vec<PathBuf>,
    pub(crate) tucked_in: bool,
}

pub(crate) fn documents_folder() -> Option<PathBuf> {
    let home = crate::chooser::home_folder()?;
    let documents = home.join("Documents");
    Some(if documents.is_dir() { documents } else { home })
}

pub(crate) fn is_inside(name: &str) -> bool {
    let path = Path::new(name);
    !name.is_empty()
        && !name.contains('\\')
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

pub(crate) fn save_beside(
    tool: Tool,
    outcome: &Outcome,
    inputs: &[&str],
    (original, base): (&Path, &Path),
) -> Result<Saved, String> {
    let renamed: Vec<String> = outcome
        .files
        .iter()
        .map(|(name, _)| named_after(tool, name, inputs))
        .collect();
    if let Some(bad) = renamed.iter().find(|name| !is_inside(name)) {
        return Err(format!("{bad} is not a name this can write"));
    }
    let layout = placed(&renamed, inputs);
    let mut tried = vec![base.to_path_buf()];
    if let Some(documents) = documents_folder()
        && documents != base
    {
        tried.push(documents);
    }
    let mut last = String::new();
    for folder in tried {
        match write_all(
            &folder,
            layout.folder.as_deref(),
            &renamed,
            outcome,
            original,
        ) {
            Ok(saved) => return Ok(saved),
            Err(why) => last = why,
        }
    }
    Err(last)
}

fn write_all(
    folder: &Path,
    tucked_in: Option<&str>,
    names: &[String],
    outcome: &Outcome,
    original: &Path,
) -> Result<Saved, String> {
    let there = |name: &str| std::fs::symlink_metadata(folder.join(name)).is_ok();
    let target = match tucked_in {
        Some(name) => {
            let made = folder.join(unused(name, &there, false));
            std::fs::create_dir(&made).map_err(|error| format!("{}: {error}", made.display()))?;
            made
        }
        None => folder.to_path_buf(),
    };
    let mut files = Vec::with_capacity(names.len());
    for (name, (_, bytes)) in names.iter().zip(&outcome.files) {
        let inside = |wanted: &str| std::fs::symlink_metadata(target.join(wanted)).is_ok();
        let wanted = if tucked_in.is_some() {
            name.clone()
        } else {
            unused(name, &inside, true)
        };
        let path = target.join(&wanted);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("{}: {error}", parent.display()))?;
        }
        crate::save_file::save(original, &path, bytes, None)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        files.push(path);
    }
    Ok(Saved {
        folder: target,
        files,
        tucked_in: tucked_in.is_some(),
    })
}

pub(crate) fn write_a_copy(original: &Path, path: &Path, bytes: &[u8]) -> Result<Saved, String> {
    crate::save_file::save(original, path, bytes, None)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(Saved {
        folder: path.parent().map_or_else(PathBuf::new, Path::to_path_buf),
        files: vec![path.to_path_buf()],
        tucked_in: false,
    })
}

pub(crate) fn show_in_folder(path: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = std::process::Command::new("open");
        command.arg("-R").arg(path);
        command
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = std::process::Command::new("explorer");
        command.arg(format!("/select,{}", path.display()));
        command
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(path.parent().unwrap_or(path));
        command
    };
    command.spawn().map(drop)
}

#[cfg(test)]
mod tests;
