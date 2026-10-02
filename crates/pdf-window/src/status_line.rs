use pdf_app::wording::{Message, Tone};

pub(crate) const QUIET_AFTER: f64 = 6.0;

pub(crate) const FADE: f64 = 0.8;

pub(crate) struct StatusLine {
    seen: Message,
    since: f64,
}

impl Default for StatusLine {
    fn default() -> Self {
        Self {
            seen: Message::Quiet,
            since: 0.0,
        }
    }
}

impl StatusLine {
    pub(crate) fn watch(&mut self, said: &Message, now: f64) {
        if self.seen != *said {
            self.seen = said.clone();
            self.since = now;
        }
    }

    pub(crate) fn age(&self, now: f64) -> f64 {
        (now - self.since).max(0.0)
    }

    pub(crate) fn strength(&self, now: f64) -> f32 {
        if self.seen == Message::Quiet {
            return 0.0;
        }
        strength(self.seen.tone(), self.age(now))
    }

    pub(crate) fn changes_after(&self, now: f64) -> Option<f64> {
        if self.seen == Message::Quiet {
            return None;
        }
        changes_after(self.seen.tone(), self.age(now))
    }
}

pub(crate) fn strength(tone: Tone, age: f64) -> f32 {
    match tone {
        Tone::Trouble => 1.0,
        Tone::Information => {
            let fading = ((age - QUIET_AFTER) / FADE).clamp(0.0, 1.0);
            #[expect(
                clippy::cast_possible_truncation,
                reason = "a share between nought and one"
            )]
            let fading = fading as f32;
            1.0 - fading
        }
    }
}

pub(crate) fn changes_after(tone: Tone, age: f64) -> Option<f64> {
    match tone {
        Tone::Information if age < QUIET_AFTER => Some(QUIET_AFTER - age),
        Tone::Information if age < QUIET_AFTER + FADE => Some(0.0),
        Tone::Trouble | Tone::Information => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{FADE, QUIET_AFTER, StatusLine, changes_after, strength};
    use pdf_app::wording::{Message, Tone};

    fn saved() -> Message {
        Message::SavedTo {
            name: "invoice.pdf".to_owned(),
            bytes: 2048,
        }
    }

    fn refused() -> Message {
        Message::AnotherEditIsRunning
    }

    #[test]
    fn an_informational_message_stays_for_six_seconds_and_then_fades_to_nothing() {
        assert!((strength(Tone::Information, 0.0) - 1.0).abs() < 1e-6);
        assert!((strength(Tone::Information, QUIET_AFTER) - 1.0).abs() < 1e-6);
        let halfway = strength(Tone::Information, QUIET_AFTER + FADE / 2.0);
        assert!((halfway - 0.5).abs() < 1e-3, "{halfway}");
        assert!(strength(Tone::Information, QUIET_AFTER + FADE).abs() < 1e-6);
        assert!(strength(Tone::Information, 600.0).abs() < 1e-6);
    }

    #[test]
    fn a_refusal_stays_as_loud_as_it_was_until_something_else_is_said() {
        for age in [0.0, QUIET_AFTER, QUIET_AFTER + FADE, 600.0] {
            assert!((strength(Tone::Trouble, age) - 1.0).abs() < 1e-6, "{age}");
            assert_eq!(changes_after(Tone::Trouble, age), None);
        }
    }

    #[test]
    fn the_window_is_woken_when_a_message_is_due_to_fade_and_not_before() {
        assert_eq!(changes_after(Tone::Information, 1.0), Some(5.0));
        assert_eq!(changes_after(Tone::Information, QUIET_AFTER), Some(0.0));
        assert_eq!(
            changes_after(Tone::Information, QUIET_AFTER + FADE / 2.0),
            Some(0.0),
            "while it fades it wants every frame"
        );
        assert_eq!(
            changes_after(Tone::Information, QUIET_AFTER + FADE),
            None,
            "once it is gone nothing more is due"
        );
    }

    #[test]
    fn the_clock_starts_when_a_message_is_said_and_again_when_it_is_said_anew() {
        let mut line = StatusLine::default();
        line.watch(&saved(), 10.0);
        assert!((line.age(13.0) - 3.0).abs() < 1e-9);
        line.watch(&saved(), 14.0);
        assert!(
            (line.age(14.0) - 4.0).abs() < 1e-9,
            "the same words seen again do not start the clock again"
        );
        line.watch(&refused(), 15.0);
        assert!(line.age(15.0).abs() < 1e-9);
        line.watch(&saved(), 30.0);
        assert!(line.age(30.0).abs() < 1e-9, "said anew after another");
    }

    #[test]
    fn what_is_said_as_quiet_shows_nothing_and_asks_for_nothing() {
        let mut line = StatusLine::default();
        line.watch(&Message::Quiet, 5.0);
        assert!(line.strength(5.0).abs() < 1e-6);
        assert_eq!(line.changes_after(5.0), None);
        line.watch(&saved(), 6.0);
        assert!((line.strength(6.0) - 1.0).abs() < 1e-6);
        assert!(line.strength(6.0 + QUIET_AFTER + FADE).abs() < 1e-6);
        line.watch(&Message::Quiet, 20.0);
        assert!(line.strength(20.0).abs() < 1e-6);
    }

    #[test]
    fn a_clock_that_runs_backwards_does_not_make_a_message_stronger_than_full() {
        let mut line = StatusLine::default();
        line.watch(&saved(), 10.0);
        assert!(line.age(5.0).abs() < 1e-9);
        assert!((line.strength(5.0) - 1.0).abs() < 1e-6);
    }
}
