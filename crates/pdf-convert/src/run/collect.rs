use crate::run::Outcome;
use crate::run::failure::{self, Failure};

#[derive(Default)]
pub struct Collected {
    pub files: Vec<(String, Vec<u8>)>,
    notes: Vec<String>,
    failures: Vec<(String, Failure)>,
}

impl Collected {
    pub fn fail(&mut self, name: &str, failure: Failure) -> Result<(), Failure> {
        if failure == Failure::Cancelled {
            return Err(failure);
        }
        self.failures.push((name.to_owned(), failure));
        Ok(())
    }

    pub fn note(&mut self, note: impl Into<String>) {
        self.notes.push(note.into());
    }

    pub fn notes_of(&mut self, from: &str, notes: Vec<String>, several: bool) {
        for note in notes {
            self.notes.push(if several {
                format!("{from}: {note}")
            } else {
                note
            });
        }
    }

    pub fn finish(mut self, several: bool) -> Result<Outcome, Failure> {
        if self.files.is_empty() && !self.failures.is_empty() {
            return Err(failure::merge(&self.failures, several));
        }
        for (name, failure) in self.failures {
            self.notes.push(format!("{name}: {failure}"));
        }
        Ok(Outcome {
            files: self.files,
            notes: self.notes,
        })
    }
}
