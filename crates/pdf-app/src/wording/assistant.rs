use crate::recent::Ago;

use super::Lang;
use super::edits::count;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Assistant {
    StatusReady(String),
    StatusWorking,
    StatusNeedsYou,
    StatusOffline,
    SuggestSummarise,
    SuggestSpelling,
    SuggestContents,
    SuggestTranslate,
    ConnectFirst,
    ProviderCustom,
    KeyHint,
    KeyOptional,
    KeyPrivacy(String),
    LocalPrivacy,
    Advanced,
    Connect,
    Connecting,
    Done,
    Change,
    ChatAgo(Ago),
    ChatDate { day: u8, month: u8, year: i32 },
    DeleteChat,
    KeepChat,
    Steps(usize),
    StepsFailed(usize),
    StepAsked,
    StepResult,
    StepFailed,
    TargetPage(usize),
    TargetPages { first: usize, last: usize },
    TargetBlock(String),
    TargetQuery(String),
    TargetFile(String),
    TargetField(String),
    TargetCount(usize),
    TargetDocument,
    Before,
    After,
    Removed,
    ShowAll,
    ShowLess,
    ReplaceTextOn(usize),
    AddTextOn(usize),
    Page(usize),
    PageChipOn,
    PageChipOff,
    OpenSettings,
    Attaching,
    ProviderModelSummary { provider: String, model: String },
}

impl Assistant {
    #[must_use]
    pub fn say(&self, lang: Lang) -> String {
        match lang {
            Lang::English => self.english(),
        }
    }

    fn english(&self) -> String {
        match self {
            Self::StatusReady(model) if model.is_empty() => "Connected".to_owned(),
            Self::StatusReady(model) => format!("Connected \u{b7} {model}"),
            Self::StatusWorking => "Working\u{2026}".to_owned(),
            Self::StatusNeedsYou => "Waiting for you".to_owned(),
            Self::StatusOffline => "Not connected".to_owned(),
            Self::SuggestSummarise => "Summarise this document".to_owned(),
            Self::SuggestSpelling => "Fix spelling on this page".to_owned(),
            Self::SuggestContents => "Make a table of contents".to_owned(),
            Self::SuggestTranslate => "Translate this page into Thai".to_owned(),
            Self::ConnectFirst => "Choose a provider and connect to start.".to_owned(),
            Self::ProviderCustom => "Custom".to_owned(),
            Self::KeyHint => "Paste your key here".to_owned(),
            Self::KeyOptional => "API key (optional)".to_owned(),
            Self::KeyPrivacy(provider) => format!(
                "Sent only to {provider}. It stays in memory unless you ask to remember it."
            ),
            Self::LocalPrivacy => {
                "Your questions go to the server at this address, and nowhere else.".to_owned()
            }
            Self::Advanced => "Advanced".to_owned(),
            Self::Connect => "Connect".to_owned(),
            Self::Connecting => "Connecting\u{2026}".to_owned(),
            Self::Done => "Done".to_owned(),
            Self::Change => "Change".to_owned(),
            Self::ChatAgo(ago) => match ago {
                Ago::JustNow => "Just now".to_owned(),
                Ago::Minutes(n) => format!("{n} min ago"),
                Ago::Hours(n) => format!("{n} h ago"),
                Ago::Yesterday => "Yesterday".to_owned(),
                Ago::Days(n) => format!("{n} days ago"),
                Ago::Long => "Over a month ago".to_owned(),
            },
            Self::ChatDate { day, month, year } => {
                format!("{day} {} {year}", month_name(*month))
            }
            Self::DeleteChat => "Delete".to_owned(),
            Self::KeepChat => "Keep".to_owned(),
            Self::Steps(n) => count(*n, "step"),
            Self::StepsFailed(n) => format!("{n} failed"),
            Self::StepAsked => "Asked for".to_owned(),
            Self::StepResult => "Result".to_owned(),
            Self::StepFailed => "This step did not work".to_owned(),
            Self::TargetPage(page) => format!("page {page}"),
            Self::TargetPages { first, last } => format!("pages {first} to {last}"),
            Self::TargetBlock(name) | Self::TargetFile(name) => name.clone(),
            Self::TargetQuery(text) => format!("\u{201c}{text}\u{201d}"),
            Self::TargetField(name) => format!("\u{201c}{name}\u{201d}"),
            Self::TargetCount(n) => format!("{n} pages"),
            Self::TargetDocument => "the document".to_owned(),
            Self::Before => "Before".to_owned(),
            Self::After => "After".to_owned(),
            Self::Removed => "Removed".to_owned(),
            Self::ShowAll => "Show all".to_owned(),
            Self::ShowLess => "Show less".to_owned(),
            Self::ReplaceTextOn(page) => format!("Replace text on page {page}"),
            Self::AddTextOn(page) => format!("Add text on page {page}"),
            Self::Page(page) => format!("Page {page}"),
            Self::PageChipOn => {
                "The text of this page goes with your question. Click to leave it out".to_owned()
            }
            Self::PageChipOff => {
                "Click to send the text of this page with your question".to_owned()
            }
            Self::OpenSettings => "Open settings".to_owned(),
            Self::Attaching => "reading\u{2026}".to_owned(),
            Self::ProviderModelSummary { provider, model } if model.is_empty() => provider.clone(),
            Self::ProviderModelSummary { provider, model } => {
                format!("{provider} \u{b7} {model}")
            }
        }
    }
}

const fn month_name(month: u8) -> &'static str {
    match month {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        _ => "Dec",
    }
}

#[cfg(test)]
mod tests {
    use super::{Assistant, Lang};
    use crate::recent::Ago;

    fn every() -> Vec<Assistant> {
        vec![
            Assistant::StatusReady("gpt-5".to_owned()),
            Assistant::StatusReady(String::new()),
            Assistant::StatusWorking,
            Assistant::StatusNeedsYou,
            Assistant::StatusOffline,
            Assistant::SuggestSummarise,
            Assistant::SuggestSpelling,
            Assistant::SuggestContents,
            Assistant::SuggestTranslate,
            Assistant::ConnectFirst,
            Assistant::ProviderCustom,
            Assistant::KeyHint,
            Assistant::KeyOptional,
            Assistant::KeyPrivacy("Gemini".to_owned()),
            Assistant::LocalPrivacy,
            Assistant::Advanced,
            Assistant::Connect,
            Assistant::Connecting,
            Assistant::Done,
            Assistant::Change,
            Assistant::ChatAgo(Ago::JustNow),
            Assistant::ChatAgo(Ago::Minutes(4)),
            Assistant::ChatAgo(Ago::Hours(2)),
            Assistant::ChatAgo(Ago::Yesterday),
            Assistant::ChatAgo(Ago::Days(3)),
            Assistant::ChatAgo(Ago::Long),
            Assistant::ChatDate {
                day: 3,
                month: 11,
                year: 2025,
            },
            Assistant::DeleteChat,
            Assistant::KeepChat,
            Assistant::Steps(1),
            Assistant::Steps(4),
            Assistant::StepsFailed(2),
            Assistant::StepAsked,
            Assistant::StepResult,
            Assistant::StepFailed,
            Assistant::TargetPage(3),
            Assistant::TargetPages { first: 2, last: 5 },
            Assistant::TargetBlock("p2-b3".to_owned()),
            Assistant::TargetQuery("total".to_owned()),
            Assistant::TargetFile("a.pdf".to_owned()),
            Assistant::TargetField("Name".to_owned()),
            Assistant::TargetCount(3),
            Assistant::TargetDocument,
            Assistant::Before,
            Assistant::After,
            Assistant::Removed,
            Assistant::ShowAll,
            Assistant::ShowLess,
            Assistant::ReplaceTextOn(4),
            Assistant::AddTextOn(4),
            Assistant::Page(7),
            Assistant::PageChipOn,
            Assistant::PageChipOff,
            Assistant::OpenSettings,
            Assistant::Attaching,
            Assistant::ProviderModelSummary {
                provider: "Anthropic".to_owned(),
                model: "claude-sonnet-4-5".to_owned(),
            },
            Assistant::ProviderModelSummary {
                provider: "Ollama".to_owned(),
                model: String::new(),
            },
        ]
    }

    #[test]
    fn every_sentence_of_the_assistant_says_something_in_every_language() {
        for said in every() {
            for lang in Lang::ALL {
                assert!(!said.say(*lang).trim().is_empty(), "{said:?}");
            }
        }
    }

    #[test]
    fn a_count_is_a_plural_only_when_it_is_not_one() {
        assert_eq!(Assistant::Steps(1).say(Lang::English), "1 step");
        assert_eq!(Assistant::Steps(4).say(Lang::English), "4 steps");
        assert_eq!(Assistant::TargetCount(3).say(Lang::English), "3 pages");
    }

    #[test]
    fn a_summary_names_the_provider_and_the_model_when_there_is_one() {
        let said = |model: &str| {
            Assistant::ProviderModelSummary {
                provider: "Anthropic".to_owned(),
                model: model.to_owned(),
            }
            .say(Lang::English)
        };
        assert_eq!(
            said("claude-sonnet-4-5"),
            "Anthropic \u{b7} claude-sonnet-4-5"
        );
        assert_eq!(said(""), "Anthropic");
        assert_eq!(
            Assistant::StatusReady("gpt-5".to_owned()).say(Lang::English),
            "Connected \u{b7} gpt-5"
        );
        assert_eq!(
            Assistant::StatusReady(String::new()).say(Lang::English),
            "Connected"
        );
    }

    #[test]
    fn a_page_a_change_and_a_block_are_named_by_their_numbers() {
        assert_eq!(Assistant::Page(7).say(Lang::English), "Page 7");
        assert_eq!(
            Assistant::ReplaceTextOn(4).say(Lang::English),
            "Replace text on page 4"
        );
        assert_eq!(
            Assistant::TargetPages { first: 2, last: 5 }.say(Lang::English),
            "pages 2 to 5"
        );
    }

    #[test]
    fn the_privacy_sentence_names_where_the_key_goes() {
        let said = Assistant::KeyPrivacy("Gemini".to_owned()).say(Lang::English);
        assert!(said.contains("Gemini"), "{said}");
        assert!(said.contains("memory"), "{said}");
    }
}
