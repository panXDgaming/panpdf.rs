#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Choice {
    pub provider: String,
    pub base_url: String,
    pub model: String,
    pub effort: String,
    pub mode: String,
}

pub const DEFAULT_MODE: &str = "ask_before_changes";

#[must_use]
pub fn read(text: &str) -> Option<Choice> {
    let line = text.lines().next()?;
    let fields: Vec<&str> = line.split('\t').collect();
    if fields.len() < 3 {
        return None;
    }
    let field = |at: usize| (*fields.get(at).unwrap_or(&"")).to_owned();
    if fields[0].is_empty() {
        return None;
    }
    Some(Choice {
        provider: field(0),
        base_url: field(1),
        model: field(2),
        effort: field(3),
        mode: field(4),
    })
}

#[must_use]
pub fn write(choice: &Choice) -> Option<String> {
    let fields = [
        &choice.provider,
        &choice.base_url,
        &choice.model,
        &choice.effort,
        &choice.mode,
    ];
    if fields
        .iter()
        .any(|field| field.contains(['\t', '\n', '\r']) || field.contains('\u{0}'))
    {
        return None;
    }
    if choice.provider.is_empty() {
        return None;
    }
    Some(format!(
        "{}\t{}\t{}\t{}\t{}\n",
        choice.provider, choice.base_url, choice.model, choice.effort, choice.mode
    ))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Family {
    OpenAi,
    Anthropic,
    Gemini,
    Other,
}

const NOT_FOR_CHAT: [&str; 20] = [
    "embed",
    "tts",
    "whisper",
    "audio",
    "realtime",
    "transcribe",
    "image",
    "dall",
    "moderation",
    "speech",
    "search",
    "computer-use",
    "davinci",
    "babbage",
    "instruct",
    "codex",
    "robotics",
    "aqa",
    "guard",
    "rerank",
];

const NOT_FOR_LOCAL_CHAT: [&str; 8] = [
    "embed",
    "rerank",
    "whisper",
    "tts",
    "speech",
    "transcribe",
    "moderation",
    "guard",
];

const OFF_THE_MAIN_ROAD: [&str; 9] = [
    "mini", "nano", "pro", "preview", "latest", "chat", "lite", "exp", "live",
];

fn bare(id: &str) -> &str {
    id.rsplit('/').next().unwrap_or(id)
}

fn strip_date(id: &str) -> &str {
    let bytes = id.as_bytes();
    let digits = |from: usize, count: usize| {
        bytes
            .get(from..from + count)
            .is_some_and(|part| part.iter().all(u8::is_ascii_digit))
    };
    for at in 0..bytes.len() {
        if at != 0 && bytes[at - 1] != b'-' {
            continue;
        }
        let year_month_day = digits(at, 4)
            && bytes.get(at + 4) == Some(&b'-')
            && digits(at + 5, 2)
            && bytes.get(at + 7) == Some(&b'-')
            && digits(at + 8, 2);
        let eight = digits(at, 8) && !digits(at, 9);
        if year_month_day || eight {
            return &id[..at.saturating_sub(1)];
        }
    }
    id
}

fn version_of(id: &str) -> Vec<u32> {
    let mut parts: Vec<u32> = Vec::new();
    let mut digits = String::new();
    for letter in strip_date(bare(id)).chars().chain(std::iter::once('-')) {
        if letter.is_ascii_digit() {
            digits.push(letter);
            continue;
        }
        if !digits.is_empty()
            && digits.len() < 4
            && let Ok(number) = digits.parse()
        {
            parts.push(number);
        }
        digits.clear();
    }
    parts
}

fn has_a_word(id: &str, words: &[&str]) -> bool {
    let lowered = bare(id).to_ascii_lowercase();
    lowered
        .split(|letter: char| !letter.is_ascii_alphanumeric())
        .any(|part| words.contains(&part))
        || words
            .iter()
            .any(|word| word.len() > 4 && lowered.contains(word))
}

fn dated(id: &str) -> bool {
    strip_date(bare(id)).len() != bare(id).len()
}

#[must_use]
pub fn sensible_model(family: Family, ids: &[String]) -> Option<String> {
    let not_for_chat: &[&str] = match family {
        Family::Other => &NOT_FOR_LOCAL_CHAT,
        Family::OpenAi | Family::Anthropic | Family::Gemini => &NOT_FOR_CHAT,
    };
    let chat: Vec<&String> = ids
        .iter()
        .filter(|id| !has_a_word(id, not_for_chat))
        .collect();
    let named = |word: &str| -> Vec<&String> {
        chat.iter()
            .copied()
            .filter(|id| bare(id).to_ascii_lowercase().contains(word))
            .collect()
    };
    let pool: Vec<&String> = match family {
        Family::OpenAi => chat
            .iter()
            .copied()
            .filter(|id| bare(id).starts_with("gpt-") && !has_a_word(id, &OFF_THE_MAIN_ROAD))
            .collect(),
        Family::Anthropic => named("sonnet"),
        Family::Gemini => named("gemini")
            .into_iter()
            .filter(|id| bare(id).to_ascii_lowercase().contains("flash"))
            .filter(|id| !has_a_word(id, &OFF_THE_MAIN_ROAD))
            .collect(),
        Family::Other => return chat.first().map(|id| (*id).clone()),
    };
    let best = pool.into_iter().max_by(|one, other| {
        version_of(one)
            .cmp(&version_of(other))
            .then_with(|| dated(other).cmp(&dated(one)))
            .then_with(|| other.len().cmp(&one.len()))
            .then_with(|| one.cmp(other))
    });
    best.or_else(|| chat.first().copied()).cloned()
}

#[cfg(test)]
mod tests {
    use super::{Choice, DEFAULT_MODE, Family, read, sensible_model, write};

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|id| (*id).to_owned()).collect()
    }

    #[test]
    fn the_newest_plain_gpt_is_chosen_over_small_and_dated_ones() {
        let listed = ids(&[
            "gpt-4o",
            "gpt-4.1",
            "gpt-4.1-mini",
            "gpt-5",
            "gpt-5-mini",
            "gpt-5-nano",
            "gpt-5-2025-08-07",
            "gpt-5-chat-latest",
            "text-embedding-3-large",
            "whisper-1",
            "o3",
            "dall-e-3",
            "gpt-4o-realtime-preview",
        ]);
        assert_eq!(
            sensible_model(Family::OpenAi, &listed).as_deref(),
            Some("gpt-5")
        );
    }

    #[test]
    fn the_newest_sonnet_is_chosen_and_an_undated_name_wins_its_own_date() {
        let listed = ids(&[
            "claude-opus-4-1-20250805",
            "claude-sonnet-4-20250514",
            "claude-sonnet-4-5-20250929",
            "claude-sonnet-4-5",
            "claude-haiku-4-5-20251001",
            "claude-3-7-sonnet-20250219",
        ]);
        assert_eq!(
            sensible_model(Family::Anthropic, &listed).as_deref(),
            Some("claude-sonnet-4-5")
        );
        let only_dated = ids(&["claude-3-5-haiku-20241022", "claude-sonnet-4-5-20250929"]);
        assert_eq!(
            sensible_model(Family::Anthropic, &only_dated).as_deref(),
            Some("claude-sonnet-4-5-20250929")
        );
    }

    #[test]
    fn the_newest_plain_flash_is_chosen_for_gemini() {
        let listed = ids(&[
            "models/gemini-2.0-flash",
            "models/gemini-2.5-flash",
            "models/gemini-2.5-flash-lite",
            "models/gemini-2.5-pro",
            "models/gemini-2.5-flash-image",
            "models/text-embedding-004",
        ]);
        assert_eq!(
            sensible_model(Family::Gemini, &listed).as_deref(),
            Some("models/gemini-2.5-flash")
        );
    }

    #[test]
    fn a_local_server_gets_its_first_chat_model_and_an_empty_list_gets_none() {
        let listed = ids(&["nomic-embed-text", "llama3.2:3b", "qwen2.5:7b"]);
        assert_eq!(
            sensible_model(Family::Other, &listed).as_deref(),
            Some("llama3.2:3b")
        );
        let tuned = ids(&["nomic-embed-text:latest", "qwen2.5:7b-instruct"]);
        assert_eq!(
            sensible_model(Family::Other, &tuned).as_deref(),
            Some("qwen2.5:7b-instruct"),
            "an instruct model is the ordinary one on a local server"
        );
        assert_eq!(sensible_model(Family::Other, &[]), None);
        assert_eq!(sensible_model(Family::OpenAi, &[]), None);
    }

    #[test]
    fn when_nothing_matches_the_first_chat_model_is_still_better_than_none() {
        let listed = ids(&["text-embedding-3-small", "o3", "o4-mini"]);
        assert_eq!(
            sensible_model(Family::OpenAi, &listed).as_deref(),
            Some("o3")
        );
        assert_eq!(
            sensible_model(Family::Anthropic, &ids(&["claude-opus-4-1"])).as_deref(),
            Some("claude-opus-4-1")
        );
    }

    #[test]
    fn a_version_with_more_parts_is_newer_than_its_prefix() {
        let listed = ids(&["gpt-5", "gpt-5.1", "gpt-4.1"]);
        assert_eq!(
            sensible_model(Family::OpenAi, &listed).as_deref(),
            Some("gpt-5.1")
        );
    }

    fn ollama() -> Choice {
        Choice {
            provider: "Ollama".to_owned(),
            base_url: "http://127.0.0.1:11434/v1".to_owned(),
            model: "llama3".to_owned(),
            effort: "medium".to_owned(),
            mode: DEFAULT_MODE.to_owned(),
        }
    }

    #[test]
    fn a_choice_written_reads_back_as_itself() {
        let line = write(&ollama()).expect("it can be written");
        assert_eq!(read(&line), Some(ollama()));
    }

    #[test]
    fn a_choice_with_no_model_is_kept() {
        let choice = Choice {
            model: String::new(),
            ..ollama()
        };
        let line = write(&choice).expect("it can be written");
        assert_eq!(read(&line), Some(choice));
    }

    #[test]
    fn a_line_from_before_the_last_two_fields_still_reads() {
        assert_eq!(
            read("Ollama\thttp://127.0.0.1:11434/v1\tllama3\n"),
            Some(Choice {
                effort: String::new(),
                mode: String::new(),
                ..ollama()
            })
        );
        assert_eq!(
            read("Ollama\thttp://127.0.0.1:11434/v1\tllama3\thigh\n"),
            Some(Choice {
                effort: "high".to_owned(),
                mode: String::new(),
                ..ollama()
            })
        );
    }

    #[test]
    fn a_line_from_a_newer_version_still_gives_the_fields_this_one_knows() {
        assert_eq!(
            read("Ollama\thttp://x\tm\tlow\task\tsixth\tseventh\n"),
            Some(Choice {
                provider: "Ollama".to_owned(),
                base_url: "http://x".to_owned(),
                model: "m".to_owned(),
                effort: "low".to_owned(),
                mode: "ask".to_owned(),
            })
        );
    }

    #[test]
    fn what_cannot_be_read_faithfully_is_not_read_or_written() {
        assert_eq!(read(""), None);
        assert_eq!(read("Ollama\thttp://x\n"), None);
        assert_eq!(read("\thttp://x\tm\n"), None);
        assert_eq!(
            write(&Choice {
                provider: "Oll\tama".to_owned(),
                ..ollama()
            }),
            None
        );
        assert_eq!(
            write(&Choice {
                provider: String::new(),
                ..ollama()
            }),
            None
        );
    }
}
