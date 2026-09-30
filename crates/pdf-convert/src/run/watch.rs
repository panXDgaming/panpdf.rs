use std::sync::atomic::{AtomicBool, Ordering};

use crate::run::{Failure, Progress};

pub struct Watch<'a> {
    progress: &'a mut dyn FnMut(Progress),
    cancel: &'a AtomicBool,
}

impl<'a> Watch<'a> {
    pub fn new(progress: &'a mut dyn FnMut(Progress), cancel: &'a AtomicBool) -> Self {
        Self { progress, cancel }
    }

    pub fn report(&mut self, progress: Progress) {
        (self.progress)(progress);
    }

    pub fn stopped(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub fn check(&self) -> Result<(), Failure> {
        if self.stopped() {
            Err(Failure::Cancelled)
        } else {
            Ok(())
        }
    }

    pub fn step(&mut self, done: usize, total: usize) -> bool {
        self.report(Progress::Step { done, total });
        !self.stopped()
    }
}
