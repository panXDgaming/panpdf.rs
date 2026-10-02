#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cannot {
    NothingOfTheirs,
    PersonEdited,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Run {
    expected: Option<u64>,
    steps: usize,
    taken_back: usize,
    person_edited: bool,
}

impl Run {
    #[must_use]
    pub const fn steps(&self) -> usize {
        self.steps
    }

    #[must_use]
    pub const fn steps_taken_back(&self) -> usize {
        self.taken_back
    }

    pub fn before_a_step(&mut self, revision: Option<u64>) {
        if self.steps == 0 && self.taken_back == 0 {
            self.expected = revision;
            return;
        }
        if revision != self.expected {
            self.person_edited = true;
        }
    }

    pub fn landed(&mut self, revision: Option<u64>) {
        self.steps += 1;
        self.taken_back = 0;
        self.expected = revision;
    }

    pub fn took_one_back(&mut self, revision: Option<u64>) {
        self.steps = self.steps.saturating_sub(1);
        self.taken_back += 1;
        self.expected = revision;
    }

    pub fn put_one_back(&mut self, revision: Option<u64>) {
        self.taken_back = self.taken_back.saturating_sub(1);
        self.steps += 1;
        self.expected = revision;
    }

    pub fn may_take_back(&self, revision: Option<u64>) -> Result<usize, Cannot> {
        if self.steps == 0 {
            return Err(Cannot::NothingOfTheirs);
        }
        if self.person_edited || revision != self.expected {
            return Err(Cannot::PersonEdited);
        }
        Ok(self.steps)
    }

    pub fn may_put_back(&self, revision: Option<u64>) -> Result<usize, Cannot> {
        if self.taken_back == 0 {
            return Err(Cannot::NothingOfTheirs);
        }
        if self.person_edited || revision != self.expected {
            return Err(Cannot::PersonEdited);
        }
        Ok(self.taken_back)
    }
}

#[cfg(test)]
mod tests;
