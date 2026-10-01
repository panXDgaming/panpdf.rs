use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use pdf_convert::run::{Context, Failure, Input, Outcome, Saved, save_beside, start};
use pdf_convert::{Choice, Setting, SettingKind, Tool, Value, Values};

use crate::json::Json;
use crate::tools::{Args, expand};

pub const MOST_FILES: usize = 20;

pub const MOST_PICTURE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct Asked {
    pub tool: Tool,
    pub files: Vec<PathBuf>,
    pub values: Values,
    pub pictures: Vec<(Setting, PathBuf)>,
    pub open_result: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Source {
    Document,
    File(PathBuf),
}

#[must_use]
pub fn slugs() -> Vec<&'static str> {
    Tool::ALL
        .into_iter()
        .filter(|tool| *tool != Tool::Protect)
        .map(Tool::slug)
        .collect()
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

fn keys_of(tool: Tool) -> String {
    let named: Vec<&str> = tool
        .settings()
        .iter()
        .filter(|setting| !matches!(setting.kind(), SettingKind::Strokes | SettingKind::Areas))
        .map(|setting| setting.key())
        .collect();
    if named.is_empty() {
        "no options".to_owned()
    } else {
        named.join(", ")
    }
}

fn choices_of(setting: Setting) -> String {
    setting
        .choices()
        .iter()
        .map(|choice| choice.value())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

fn words(value: &Json, key: &str) -> Result<Vec<String>, String> {
    let wrong = || format!("`{key}` is a list of words, or words separated by commas");
    let listed: Vec<String> = match value {
        Json::Text(text) => text
            .split(',')
            .map(|word| word.trim().to_owned())
            .filter(|word| !word.is_empty())
            .collect(),
        Json::List(items) => items
            .iter()
            .map(|item| item.as_str().map(|word| word.trim().to_owned()))
            .collect::<Option<Vec<String>>>()
            .ok_or_else(wrong)?,
        _ => return Err(wrong()),
    };
    Ok(listed)
}

fn one_option(
    tool: Tool,
    setting: Setting,
    value: &Json,
    taken: &mut (Values, Vec<(Setting, PathBuf)>),
) -> Result<(), String> {
    let key = setting.key();
    let text = || {
        value
            .as_str()
            .ok_or_else(|| format!("`{key}` is text"))
            .map(str::to_owned)
    };
    let made = match setting.kind() {
        SettingKind::Choice { .. } => {
            let word = text()?;
            let choice = setting.choice_named(&word).ok_or_else(|| {
                format!(
                    "`{key}` is not {word:?}: for {} it is one of {}",
                    tool.slug(),
                    choices_of(setting)
                )
            })?;
            Value::Choice(choice)
        }
        SettingKind::Several { .. } => {
            let mut chosen: Vec<Choice> = Vec::new();
            for word in words(value, key)? {
                chosen.push(setting.choice_named(&word).ok_or_else(|| {
                    format!(
                        "`{key}` does not take {word:?}: it takes {}",
                        choices_of(setting)
                    )
                })?);
            }
            Value::Choices(chosen)
        }
        SettingKind::Number { .. } => Value::Number(
            value
                .as_f64()
                .ok_or_else(|| format!("`{key}` is a number"))?,
        ),
        SettingKind::Flag { .. } => match value {
            Json::Bool(on) => Value::Flag(*on),
            _ => return Err(format!("`{key}` is true or false")),
        },
        SettingKind::Text { .. } => Value::Text(text()?),
        SettingKind::Secret => Value::Secret(text()?),
        SettingKind::Pages { .. } => Value::Pages(text()?),
        SettingKind::Terms => Value::Terms(words(value, key)?),
        SettingKind::Picture => {
            let path = text()?;
            if path.trim().is_empty() {
                return Err(format!("`{key}` is the path of a picture file"));
            }
            taken.1.push((setting, expand(&path)));
            return Ok(());
        }
        SettingKind::Strokes => {
            return Err(format!(
                "`{key}` is a drawing, which words cannot give: sign with how: type and `text`, \
                 or how: image and `image`"
            ));
        }
        SettingKind::Areas => {
            return Err(format!(
                "`{key}` is a set of boxes, which words cannot give: name the words to remove \
                 with `search`"
            ));
        }
    };
    taken.0.set(setting, made);
    Ok(())
}

fn options_in(
    tool: Tool,
    options: Option<&Json>,
) -> Result<(Values, Vec<(Setting, PathBuf)>), String> {
    let mut taken = (Values::new(), Vec::new());
    let members = match options {
        None | Some(Json::Null) => return Ok(taken),
        Some(Json::Object(members)) => members,
        Some(_) => return Err("`options` is an object, like {\"pages\": \"1-3\"}".to_owned()),
    };
    for (key, value) in members {
        if matches!(value, Json::Null) {
            continue;
        }
        let setting = tool
            .settings()
            .iter()
            .copied()
            .find(|setting| setting.key() == key)
            .ok_or_else(|| {
                format!(
                    "`{key}` is not an option of {}: it takes {}",
                    tool.slug(),
                    keys_of(tool)
                )
            })?;
        one_option(tool, setting, value, &mut taken)?;
    }
    Ok(taken)
}

fn needs_more(tool: Tool, taken: &mut (Values, Vec<(Setting, PathBuf)>)) -> Result<(), String> {
    let (values, pictures) = (&mut taken.0, &taken.1);
    match tool {
        Tool::Redact => {
            let words = matches!(
                values.explicit(Setting::Search),
                Some(Value::Terms(terms)) if terms.iter().any(|term| !term.is_empty())
            );
            if !words && !values.flag(Setting::UseMarks) {
                return Err(
                    "redact-pdf needs `search`, the words to remove, or `annotations: true` to \
                     remove what the file's own redaction marks cover"
                        .to_owned(),
                );
            }
        }
        Tool::Sign => {
            let has_text = values
                .text(Setting::TypedName)
                .is_some_and(|name| !name.trim().is_empty());
            let has_picture = pictures
                .iter()
                .any(|(setting, _)| *setting == Setting::SignaturePicture);
            let how = if values.is_set(Setting::Signature) {
                values.choice(Setting::Signature)
            } else if has_text {
                Some(Choice::Typed)
            } else if has_picture {
                Some(Choice::FromPicture)
            } else {
                None
            };
            match how {
                Some(Choice::Typed) if has_text => {
                    values.set(Setting::Signature, Value::Choice(Choice::Typed));
                }
                Some(Choice::Typed) => {
                    return Err("a typed signature needs `text`, the name to sign with".to_owned());
                }
                Some(Choice::FromPicture) if has_picture => {
                    values.set(Setting::Signature, Value::Choice(Choice::FromPicture));
                }
                Some(Choice::FromPicture) => {
                    return Err(
                        "a signature from a picture needs `image`, the picture file".to_owned()
                    );
                }
                _ => {
                    return Err(
                        "sign-pdf signs with a typed name (`text`) or a picture file (`image`); \
                         a drawn signature is the person's to make in the Tools room"
                            .to_owned(),
                    );
                }
            }
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn asked_for(args: &Args) -> Result<Asked, String> {
    let slug = args
        .required("tool")
        .map_err(|_| format!("`tool` is needed: one of {}", slugs().join(", ")))?;
    let tool = Tool::from_slug(slug).ok_or_else(|| {
        format!(
            "{slug:?} is not a tool: it is one of {}",
            slugs().join(", ")
        )
    })?;
    if tool == Tool::Protect {
        return Err(
            "protecting with a password is its own tool, protect_document: it always asks the person"
                .to_owned(),
        );
    }
    let files = named_files(args)?;
    let (values, pictures) = {
        let mut taken = options_in(tool, args.0.get("options"))?;
        needs_more(tool, &mut taken)?;
        taken
    };
    let asked = Asked {
        tool,
        files,
        values,
        pictures,
        open_result: args.flag("open_result"),
    };
    asked.check()?;
    Ok(asked)
}

pub(crate) fn protection_for(args: &Args) -> Result<Asked, String> {
    let password = args.required("password")?.to_owned();
    if password.is_empty() {
        return Err("`password` is empty: the person says what it is".to_owned());
    }
    let mut values = Values::new().with(Setting::NewPassword, Value::Secret(password));
    if let Some(owner) = args.text("owner_password") {
        values.set(Setting::OwnerPassword, Value::Secret(owner.to_owned()));
    }
    if args.has("deny") {
        let listed = args
            .0
            .get("deny")
            .ok_or_else(|| "`deny` is a list".to_owned())?;
        let mut chosen: Vec<Choice> = Vec::new();
        for word in words(listed, "deny")? {
            chosen.push(Setting::Forbid.choice_named(&word).ok_or_else(|| {
                format!(
                    "`deny` does not take {word:?}: it takes {}",
                    choices_of(Setting::Forbid)
                )
            })?);
        }
        values.set(Setting::Forbid, Value::Choices(chosen));
    }
    if let Some(open_with) = args.text("document_password") {
        values.set(Setting::FilePassword, Value::Secret(open_with.to_owned()));
    }
    let asked = Asked {
        tool: Tool::Protect,
        files: named_files(args)?,
        values,
        pictures: Vec::new(),
        open_result: args.flag("open_result"),
    };
    asked.check()?;
    Ok(asked)
}

fn named_files(args: &Args) -> Result<Vec<PathBuf>, String> {
    if !args.has("files") {
        return Ok(Vec::new());
    }
    let listed = args
        .0
        .get("files")
        .and_then(Json::as_list)
        .ok_or("`files` is a list of file paths")?;
    if listed.len() > MOST_FILES {
        return Err(format!(
            "`files` names {} files: the most one call takes is {MOST_FILES}",
            listed.len()
        ));
    }
    listed
        .iter()
        .map(|item| match item.as_str().map(str::trim) {
            Some(path) if !path.is_empty() => Ok(expand(path)),
            _ => Err("`files` holds something that is not a path".to_owned()),
        })
        .collect()
}

impl Asked {
    pub fn sources(&self) -> Result<Vec<Source>, String> {
        let slug = self.tool.slug();
        let files = |files: &[PathBuf]| files.iter().cloned().map(Source::File).collect();
        let sources: Vec<Source> = if self.tool == Tool::Compare {
            match self.files.len() {
                1 => {
                    let mut both = vec![Source::Document];
                    both.extend(files(&self.files));
                    both
                }
                2 => files(&self.files),
                _ => {
                    return Err(
                        "compare-pdf compares two files: give `files` with the other one (the \
                         open document is the first), or with both"
                            .to_owned(),
                    );
                }
            }
        } else if self.files.is_empty() {
            if !self.tool.starts_from_a_pdf() {
                return Err(format!(
                    "{slug} makes a PDF from files that are not one: name them in `files`"
                ));
            }
            vec![Source::Document]
        } else {
            files(&self.files)
        };
        if !self.tool.inputs().allows(sources.len()) {
            return Err(format!(
                "{slug} does not take {} file{}",
                sources.len(),
                plural(sources.len())
            ));
        }
        for source in &sources {
            if let Source::File(path) = source {
                let name = path
                    .file_name()
                    .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
                if !self.tool.takes(&name) {
                    return Err(format!(
                        "{} is not a file {slug} takes: it takes {}",
                        path.display(),
                        self.tool
                            .accepts()
                            .iter()
                            .flat_map(|kind| kind.extensions())
                            .map(|ending| format!(".{ending}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
            }
        }
        Ok(sources)
    }

    fn check(&self) -> Result<(), String> {
        self.sources()?;
        match self.values.faults(self.tool).first() {
            Some(fault) => Err(fault.to_string()),
            None => Ok(()),
        }
    }

    pub fn values_with_pictures(&self) -> Result<Values, String> {
        let mut values = self.values.clone();
        for (setting, path) in &self.pictures {
            let facts = std::fs::metadata(path)
                .map_err(|error| format!("{} cannot be read: {error}", path.display()))?;
            if !facts.is_file() || facts.len() > MOST_PICTURE_BYTES {
                return Err(format!(
                    "{} is not a picture file this can use",
                    path.display()
                ));
            }
            let bytes = std::fs::read(path)
                .map_err(|error| format!("{} cannot be read: {error}", path.display()))?;
            let name = path
                .file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
            values.set(*setting, Value::File { name, bytes });
        }
        Ok(values)
    }

    #[must_use]
    pub const fn destroys(&self) -> bool {
        matches!(self.tool, Tool::Redact)
    }

    #[must_use]
    pub fn counts_pages(&self) -> bool {
        self.values.is_set(Setting::Pages) || self.values.is_set(Setting::SignPages)
    }
}

#[must_use]
pub fn why(failure: &Failure) -> String {
    match failure {
        Failure::Cancelled => {
            "The conversion was stopped before it finished; nothing was written.".to_owned()
        }
        Failure::NeedsPassword => {
            "A file needs its password, or the password given does not open it. Ask the person \
             for it and pass it as the `password` option."
                .to_owned()
        }
        Failure::BadInput(why) => format!("The converter could not use what it was given: {why}"),
        Failure::Refused(why) => format!("The converter refused: {why}"),
        Failure::Panicked(why) => format!("The converter stopped unexpectedly: {why}"),
    }
}

fn size_in_words(bytes: usize) -> String {
    const MEGABYTE: usize = 1024 * 1024;
    if bytes < 1024 {
        format!("{bytes} bytes")
    } else if bytes < MEGABYTE {
        format!("{} KB", bytes.div_ceil(1024))
    } else {
        let tenths = bytes / (MEGABYTE / 10);
        format!("{}.{} MB", tenths / 10, tenths % 10)
    }
}

#[must_use]
pub fn said_made(saved: &Saved, outcome: &Outcome) -> String {
    let mut said = match saved.files.as_slice() {
        [one] => {
            let size = outcome
                .files
                .first()
                .map_or_else(String::new, |(_, bytes)| {
                    format!(" ({})", size_in_words(bytes.len()))
                });
            format!("Made {}{size}.", one.display())
        }
        many => {
            let names: Vec<String> = many
                .iter()
                .take(5)
                .map(|path| {
                    path.file_name()
                        .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
                })
                .collect();
            let more = if many.len() > 5 {
                format!(" and {} more", many.len() - 5)
            } else {
                String::new()
            };
            format!(
                "Made {} files in the new folder {}: {}{more}.",
                many.len(),
                saved.folder.display(),
                names.join(", ")
            )
        }
    };
    if !outcome.notes.is_empty() {
        said.push_str(" The converter says: ");
        said.push_str(&outcome.notes.join("; "));
        if !said.ends_with('.') {
            said.push('.');
        }
    }
    said.push_str(" The document that is open was not changed, and no file was written over.");
    said
}

#[must_use]
pub fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || "document.pdf".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

pub fn run_to_the_end(
    tool: Tool,
    inputs: Vec<Input>,
    values: Values,
    fonts: Option<Arc<dyn pdf_content::FontProvider>>,
) -> Result<Outcome, Failure> {
    let running = start(tool, inputs, values, Context { fonts });
    loop {
        if let Some(done) = running.finished() {
            return done;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub fn put_beside(
    tool: Option<Tool>,
    outcome: &Outcome,
    inputs: &[&str],
    base: &Path,
) -> Result<Saved, String> {
    save_beside(tool, outcome, inputs, (base, None), &mut |path, bytes| {
        pdf_convert::run::write_new_file(path, bytes)
    })
}

#[cfg(test)]
mod tests;
