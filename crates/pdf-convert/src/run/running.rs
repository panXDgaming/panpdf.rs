use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;

use crate::catalogue::{Tool, Values};
use crate::run::{Context, Failure, Input, Outcome, Progress, run};

const STACK: usize = 64 * 1024 * 1024;

type Finished = Result<Outcome, Failure>;

struct Seen {
    latest: Progress,
    file: Option<(usize, usize)>,
}

struct Shared {
    seen: Mutex<Seen>,
    result: Mutex<Option<Finished>>,
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Shared {
    fn note(&self, progress: Progress) {
        let mut seen = locked(&self.seen);
        if let Progress::File { index, of } = progress {
            seen.file = Some((index, of));
        }
        seen.latest = progress;
    }

    fn finish(&self, result: Finished) {
        *locked(&self.result) = Some(result);
    }
}

pub struct Running {
    shared: Arc<Shared>,
    cancel: Arc<AtomicBool>,
}

impl Running {
    #[must_use]
    pub fn progress(&self) -> Progress {
        locked(&self.shared.seen).latest
    }

    #[must_use]
    pub fn fraction(&self) -> Option<f64> {
        let seen = locked(&self.shared.seen);
        match seen.latest {
            Progress::Starting => Some(0.0),
            Progress::Step { done, total } if total > 0 => {
                let (index, of) = seen.file.unwrap_or((0, 1));
                Some((count(index) + count(done) / count(total)) / count(of.max(1)))
            }
            Progress::File { index, of } if of > 0 => Some(count(index) / count(of)),
            Progress::Writing => seen.file.map_or(Some(1.0), |(index, of)| {
                Some(count(index + 1) / count(of.max(1)))
            }),
            _ => None,
        }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    #[must_use]
    pub fn finished(&self) -> Option<Result<Outcome, Failure>> {
        locked(&self.shared.result).take()
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn count(number: usize) -> f64 {
    f64::from(u32::try_from(number).unwrap_or(u32::MAX))
}

pub(crate) fn panic_text(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "no reason was given".to_owned())
}

#[must_use]
pub fn start(tool: Tool, inputs: Vec<Input>, values: Values, context: Context) -> Running {
    launch(tool.slug(), move |progress, cancel| {
        run(tool, inputs, &values, &context, progress, cancel)
    })
}

pub(crate) fn launch<Work>(name: &str, work: Work) -> Running
where
    Work: FnOnce(&mut dyn FnMut(Progress), &AtomicBool) -> Finished + Send + 'static,
{
    let shared = Arc::new(Shared {
        seen: Mutex::new(Seen {
            latest: Progress::Starting,
            file: None,
        }),
        result: Mutex::new(None),
    });
    let cancel = Arc::new(AtomicBool::new(false));
    let worker = (Arc::clone(&shared), Arc::clone(&cancel));
    let spawned = thread::Builder::new()
        .name(format!("pdf-convert {name}"))
        .stack_size(STACK)
        .spawn(move || {
            let (shared, cancel) = worker;
            let caught = catch_unwind(AssertUnwindSafe(|| {
                work(&mut |progress| shared.note(progress), &cancel)
            }));
            shared
                .finish(caught.unwrap_or_else(|payload| {
                    Err(Failure::Panicked(panic_text(payload.as_ref())))
                }));
        });
    if let Err(error) = spawned {
        shared.finish(Err(Failure::Refused(format!(
            "the conversion could not be started: {error}"
        ))));
    }
    Running { shared, cancel }
}
