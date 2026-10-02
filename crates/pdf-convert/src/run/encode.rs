use crate::catalogue::{Area, Choice, Setting, SettingKind, Stroke, Tool, Value, Values};
use crate::run::failure::Failure;
use crate::run::random;

#[derive(Default)]
pub struct Prepared {
    pub options: Vec<(&'static str, String)>,
    pub files: Vec<(&'static str, Vec<u8>)>,
    pub notes: Vec<String>,
}

pub fn prepare(tool: Tool, values: &Values) -> Result<Prepared, Failure> {
    match tool {
        Tool::Sign => sign(values),
        Tool::Protect => protect(values),
        _ => Ok(Prepared {
            options: pairs(tool, values),
            ..Prepared::default()
        }),
    }
}

pub fn pairs(tool: Tool, values: &Values) -> Vec<(&'static str, String)> {
    let mut pairs = Vec::new();
    for &setting in tool.settings() {
        if setting == Setting::Signature || !applies(values, setting) {
            continue;
        }
        if let Some(text) = encoded(setting, values) {
            pairs.push((setting.key(), text));
        }
    }
    if tool == Tool::Ocr
        && !pairs
            .iter()
            .any(|(key, _)| *key == Setting::Languages.key())
    {
        pairs.push((Setting::Languages.key(), "eng".to_owned()));
    }
    pairs
}

pub fn requested_pages(values: &Values) -> Result<Vec<usize>, Failure> {
    let Some(text) = values
        .text(Setting::Pages)
        .and_then(|text| pages_text(&text))
    else {
        return Ok(Vec::new());
    };
    convert_files::Settings(vec![("pages".to_owned(), text)])
        .pages()
        .map_err(Failure::BadInput)
}

pub fn secret(values: &Values, setting: Setting) -> Vec<u8> {
    values
        .text(setting)
        .map(|text| text.into_owned().into_bytes())
        .unwrap_or_default()
}

fn applies(values: &Values, setting: Setting) -> bool {
    match setting.when() {
        None => true,
        Some((on, choice)) => values.choice(on) == Some(choice),
    }
}

fn encoded(setting: Setting, values: &Values) -> Option<String> {
    let non_empty = |text: String| (!text.is_empty()).then_some(text);
    match setting.kind() {
        SettingKind::Choice { .. } => non_empty(values.choice(setting)?.value().to_owned()),
        SettingKind::Several { .. } => {
            let chosen = values.choices(setting);
            let words: Vec<&str> = chosen.iter().map(|choice| choice.value()).collect();
            non_empty(words.join(","))
        }
        SettingKind::Number { .. } => values.number(setting).map(|number| number.to_string()),
        SettingKind::Flag { .. } => Some(values.flag(setting).to_string()),
        SettingKind::Text { .. } | SettingKind::Secret => {
            non_empty(values.text(setting)?.into_owned())
        }
        SettingKind::Pages { default } => {
            let text = keyword_lowercased(values.text(setting)?.trim());
            if default.is_empty() {
                pages_text(&text)
            } else {
                non_empty(text)
            }
        }
        SettingKind::Picture => None,
        SettingKind::Strokes => match values.explicit(setting)? {
            Value::Strokes(strokes) => non_empty(strokes_text(strokes)),
            _ => None,
        },
        SettingKind::Terms => match values.explicit(setting)? {
            Value::Terms(terms) => non_empty(terms_text(terms)),
            _ => None,
        },
        SettingKind::Areas => match values.explicit(setting)? {
            Value::Areas(areas) => non_empty(areas_text(areas)),
            _ => None,
        },
    }
}

fn keyword_lowercased(text: &str) -> String {
    if text.eq_ignore_ascii_case("all") || text.eq_ignore_ascii_case("last") {
        text.to_ascii_lowercase()
    } else {
        text.to_owned()
    }
}

fn pages_text(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty() && !text.eq_ignore_ascii_case("all")).then(|| text.to_owned())
}

fn strokes_text(strokes: &[Stroke]) -> String {
    let drawn: Vec<String> = strokes
        .iter()
        .filter(|stroke| !stroke.is_empty())
        .map(|stroke| {
            let points: Vec<String> = stroke.iter().map(|(x, y)| format!("{x},{y}")).collect();
            points.join(" ")
        })
        .collect();
    drawn.join("; ")
}

fn terms_text(terms: &[String]) -> String {
    let words: Vec<&str> = terms
        .iter()
        .flat_map(|term| term.split('|'))
        .map(str::trim)
        .filter(|word| !word.is_empty())
        .collect();
    words.join("|")
}

fn areas_text(areas: &[Area]) -> String {
    let boxes: Vec<String> = areas
        .iter()
        .map(|a| format!("{}:{},{},{},{}", a.page, a.x0, a.y0, a.x1, a.y1))
        .collect();
    boxes.join("; ")
}

fn has(options: &[(&'static str, String)], key: &str) -> bool {
    options.iter().any(|(found, _)| *found == key)
}

fn signature_way(values: &Values) -> Choice {
    if values.is_set(Setting::Signature) {
        return values.choice(Setting::Signature).unwrap_or(Choice::Drawn);
    }
    if values.is_set(Setting::SignaturePicture) {
        Choice::FromPicture
    } else if values.is_set(Setting::Drawing) {
        Choice::Drawn
    } else if values.is_set(Setting::TypedName) {
        Choice::Typed
    } else {
        Choice::Drawn
    }
}

fn sign(values: &Values) -> Result<Prepared, Failure> {
    let way = signature_way(values);
    let mut chosen = values.clone();
    chosen.set(Setting::Signature, Value::Choice(way));
    let mut prepared = Prepared {
        options: pairs(Tool::Sign, &chosen),
        ..Prepared::default()
    };
    match way {
        Choice::FromPicture => match values.explicit(Setting::SignaturePicture) {
            Some(Value::File { bytes, .. }) if !bytes.is_empty() => {
                prepared.files.push(("image", bytes.clone()));
            }
            _ => {
                return Err(Failure::BadInput(
                    "choose the picture of the signature".to_owned(),
                ));
            }
        },
        Choice::Typed => {
            let named = prepared
                .options
                .iter()
                .any(|(key, text)| *key == "text" && !text.trim().is_empty());
            if !named {
                return Err(Failure::BadInput("type the name to sign with".to_owned()));
            }
        }
        _ => {
            if !has(&prepared.options, "draw") {
                return Err(Failure::BadInput("draw the signature first".to_owned()));
            }
        }
    }
    Ok(prepared)
}

fn protect(values: &Values) -> Result<Prepared, Failure> {
    let mut prepared = Prepared {
        options: pairs(Tool::Protect, values),
        ..Prepared::default()
    };
    let options = &prepared.options;
    if !has(options, "password") && !has(options, "owner-password") && !has(options, "deny") {
        return Err(Failure::BadInput(
            "give a password to lock the file with".to_owned(),
        ));
    }
    if has(options, "deny") && !has(options, "owner-password") {
        prepared
            .options
            .push(("owner-password", random::owner_password()));
        prepared.notes.push(
            "the limits are locked with an owner password made at random and not kept, so nobody can lift them"
                .to_owned(),
        );
    }
    Ok(prepared)
}
