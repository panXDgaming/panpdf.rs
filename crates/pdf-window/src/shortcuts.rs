use eframe::egui::{Context, Key, KeyboardShortcut, Modifiers};

use pdf_app::wording::Command;

pub(crate) const NEW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);
pub(crate) const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
pub(crate) const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
pub(crate) const SAVE_A_COPY: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::S);
pub(crate) const PRINT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::P);
pub(crate) const PROPERTIES: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::D);
pub(crate) const ASSISTANT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::J);

const UNDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
const REDO: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Y);
const CUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::X);
const COPY: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::C);
const PASTE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::V);
const PASTE_IN_PLACE: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::V);
const DELETE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::Delete);
const FIND: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::F);
const ZOOM_IN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Plus);
const ZOOM_OUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Minus);
const FRAMES: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::F2);

pub(crate) const fn of(command: Command) -> Option<KeyboardShortcut> {
    Some(match command {
        Command::NewDocument => NEW,
        Command::Open | Command::OpenDocument => OPEN,
        Command::Save => SAVE,
        Command::SaveAs => SAVE_A_COPY,
        Command::Print => PRINT,
        Command::DocumentProperties => PROPERTIES,
        Command::Undo => UNDO,
        Command::Redo => REDO,
        Command::Cut => CUT,
        Command::Copy => COPY,
        Command::Paste => PASTE,
        Command::PasteInPlace => PASTE_IN_PLACE,
        Command::Delete => DELETE,
        Command::Find => FIND,
        Command::ZoomIn => ZOOM_IN,
        Command::ZoomOut => ZOOM_OUT,
        Command::ShowFrames => FRAMES,
        Command::AiAssistant | Command::Assistant => ASSISTANT,
        _ => return None,
    })
}

pub(crate) fn hint(ctx: &Context, command: Command) -> Option<String> {
    of(command).map(|shortcut| ctx.format_shortcut(&shortcut))
}

#[cfg(test)]
mod tests {
    use eframe::egui::{Context, ModifierNames};

    use super::{ASSISTANT, NEW, OPEN, SAVE, SAVE_A_COPY, hint, of};
    use pdf_app::wording::Command;

    const SHOWN: [Command; 19] = [
        Command::NewDocument,
        Command::OpenDocument,
        Command::Save,
        Command::SaveAs,
        Command::Print,
        Command::DocumentProperties,
        Command::Undo,
        Command::Redo,
        Command::Cut,
        Command::Copy,
        Command::Paste,
        Command::PasteInPlace,
        Command::Delete,
        Command::Find,
        Command::ZoomIn,
        Command::ZoomOut,
        Command::ShowFrames,
        Command::AiAssistant,
        Command::Assistant,
    ];

    #[test]
    fn no_two_different_things_are_offered_on_the_same_keys() {
        for (at, first) in SHOWN.iter().enumerate() {
            for second in &SHOWN[at + 1..] {
                let (one, other) = (of(*first), of(*second));
                let same_thing =
                    matches!((first, second), (Command::AiAssistant, Command::Assistant));
                assert!(
                    same_thing || one != other,
                    "{first:?} and {second:?} share {one:?}"
                );
            }
        }
    }

    #[test]
    fn a_menu_word_and_its_toolbar_button_carry_the_same_keys() {
        assert_eq!(of(Command::Open), of(Command::OpenDocument));
        assert_eq!(of(Command::AiAssistant), of(Command::Assistant));
        assert_eq!(of(Command::Open), Some(OPEN));
        assert_eq!(of(Command::Assistant), Some(ASSISTANT));
        assert_eq!(of(Command::NewDocument), Some(NEW));
        assert_eq!(of(Command::Pen), None);
    }

    #[test]
    fn a_command_with_no_keys_shows_no_hint() {
        let ctx = Context::default();
        assert_eq!(hint(&ctx, Command::Pen), None);
        assert_eq!(hint(&ctx, Command::Export), None);
    }

    #[test]
    fn the_hints_are_written_by_the_keyboard_and_not_typed_in() {
        let mac = |shortcut: &eframe::egui::KeyboardShortcut| {
            shortcut.format(&ModifierNames::SYMBOLS, true)
        };
        let elsewhere = |shortcut: &eframe::egui::KeyboardShortcut| {
            shortcut.format(&ModifierNames::NAMES, false)
        };
        assert_eq!(mac(&SAVE), "\u{2318}S");
        assert_eq!(elsewhere(&SAVE), "Ctrl+S");
        assert_eq!(elsewhere(&SAVE_A_COPY), "Ctrl+Shift+S");
        assert_ne!(
            mac(&SAVE_A_COPY),
            elsewhere(&SAVE_A_COPY),
            "a Mac does not read Ctrl"
        );
        let ctx = Context::default();
        let said = hint(&ctx, Command::Save).expect("Save has keys");
        assert!(said.ends_with('S'), "{said}");
        assert!(!said.is_empty());
    }
}
