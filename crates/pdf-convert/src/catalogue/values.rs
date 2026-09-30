use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;

use crate::catalogue::choice::Choice;
use crate::catalogue::setting::{Setting, SettingKind};
use crate::catalogue::tool::Tool;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    pub page: usize,
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

pub type Stroke = Vec<(f64, f64)>;

#[derive(Clone, PartialEq)]
pub enum Value {
    Text(String),
    Secret(String),
    Number(f64),
    Flag(bool),
    Choice(Choice),
    Choices(Vec<Choice>),
    Pages(String),
    File { name: String, bytes: Vec<u8> },
    Strokes(Vec<Stroke>),
    Terms(Vec<String>),
    Areas(Vec<Area>),
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(text) => f.debug_tuple("Text").field(text).finish(),
            Self::Secret(_) => f.write_str("Secret(..)"),
            Self::Number(number) => f.debug_tuple("Number").field(number).finish(),
            Self::Flag(flag) => f.debug_tuple("Flag").field(flag).finish(),
            Self::Choice(choice) => f.debug_tuple("Choice").field(choice).finish(),
            Self::Choices(choices) => f.debug_tuple("Choices").field(choices).finish(),
            Self::Pages(pages) => f.debug_tuple("Pages").field(pages).finish(),
            Self::File { name, bytes } => f
                .debug_struct("File")
                .field("name", name)
                .field("bytes", &bytes.len())
                .finish(),
            Self::Strokes(strokes) => f.debug_tuple("Strokes").field(strokes).finish(),
            Self::Terms(terms) => f.debug_tuple("Terms").field(terms).finish(),
            Self::Areas(areas) => f.debug_tuple("Areas").field(areas).finish(),
        }
    }
}

impl Value {
    #[must_use]
    pub fn fits(&self, kind: SettingKind) -> bool {
        matches!(
            (self, kind),
            (Self::Text(_), SettingKind::Text { .. })
                | (Self::Secret(_), SettingKind::Secret)
                | (Self::Number(_), SettingKind::Number { .. })
                | (Self::Flag(_), SettingKind::Flag { .. })
                | (Self::Choice(_), SettingKind::Choice { .. })
                | (Self::Choices(_), SettingKind::Several { .. })
                | (Self::Pages(_), SettingKind::Pages { .. })
                | (Self::File { .. }, SettingKind::Picture)
                | (Self::Strokes(_), SettingKind::Strokes)
                | (Self::Terms(_), SettingKind::Terms)
                | (Self::Areas(_), SettingKind::Areas)
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Why {
    WrongKind,
    NotOneOf,
    NotANumber,
    OutOfRange { value: f64, least: f64, most: f64 },
    NoSuchPage,
    BadPoint,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fault {
    pub setting: Setting,
    pub why: Why,
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let key = self.setting.key();
        match &self.why {
            Why::WrongKind => write!(f, "{key}: this is not the kind of value it takes"),
            Why::NotOneOf => write!(f, "{key}: this is not one of the choices it offers"),
            Why::NotANumber => write!(f, "{key}: this is not a number"),
            Why::OutOfRange { value, least, most } => {
                write!(f, "{key}: {value} is not between {least} and {most}")
            }
            Why::NoSuchPage => write!(f, "{key}: pages count from 1"),
            Why::BadPoint => write!(f, "{key}: a point of the drawing is not a number"),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Values {
    entries: BTreeMap<Setting, Value>,
}

impl Values {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, setting: Setting, value: Value) {
        self.entries.insert(setting, value);
    }

    #[must_use]
    pub fn with(mut self, setting: Setting, value: Value) -> Self {
        self.set(setting, value);
        self
    }

    pub fn unset(&mut self, setting: Setting) -> Option<Value> {
        self.entries.remove(&setting)
    }

    #[must_use]
    pub fn is_set(&self, setting: Setting) -> bool {
        self.entries.contains_key(&setting)
    }

    #[must_use]
    pub fn explicit(&self, setting: Setting) -> Option<&Value> {
        self.entries.get(&setting)
    }

    pub fn iter(&self) -> impl Iterator<Item = (Setting, &Value)> {
        self.entries
            .iter()
            .map(|(setting, value)| (*setting, value))
    }

    #[must_use]
    pub fn get(&self, setting: Setting) -> Option<Cow<'_, Value>> {
        if let Some(value) = self.entries.get(&setting) {
            return Some(Cow::Borrowed(value));
        }
        let value = match setting.kind() {
            SettingKind::Choice { default, .. } => Value::Choice(default),
            SettingKind::Number { default, .. } => Value::Number(default),
            SettingKind::Flag { default } => Value::Flag(default),
            SettingKind::Text { default } if !default.is_empty() => Value::Text(default.to_owned()),
            SettingKind::Pages { default } if !default.is_empty() => {
                Value::Pages(default.to_owned())
            }
            _ => return None,
        };
        Some(Cow::Owned(value))
    }

    #[must_use]
    pub fn choice(&self, setting: Setting) -> Option<Choice> {
        match self.get(setting)?.as_ref() {
            Value::Choice(choice) => Some(*choice),
            _ => None,
        }
    }

    #[must_use]
    pub fn choices(&self, setting: Setting) -> Vec<Choice> {
        match self.entries.get(&setting) {
            Some(Value::Choices(choices)) => choices.clone(),
            _ => Vec::new(),
        }
    }

    #[must_use]
    pub fn flag(&self, setting: Setting) -> bool {
        matches!(self.get(setting).as_deref(), Some(Value::Flag(true)))
    }

    #[must_use]
    pub fn number(&self, setting: Setting) -> Option<f64> {
        match self.get(setting)?.as_ref() {
            Value::Number(number) => Some(*number),
            _ => None,
        }
    }

    #[must_use]
    pub fn text(&self, setting: Setting) -> Option<Cow<'_, str>> {
        match self.get(setting)? {
            Cow::Borrowed(Value::Text(text) | Value::Secret(text) | Value::Pages(text)) => {
                Some(Cow::Borrowed(text.as_str()))
            }
            Cow::Owned(Value::Text(text) | Value::Secret(text) | Value::Pages(text)) => {
                Some(Cow::Owned(text))
            }
            _ => None,
        }
    }

    #[must_use]
    pub fn faults(&self, tool: Tool) -> Vec<Fault> {
        let mut faults = Vec::new();
        for &setting in tool.settings() {
            let Some(value) = self.entries.get(&setting) else {
                continue;
            };
            if let Some(why) = misfit(setting, value) {
                faults.push(Fault { setting, why });
            }
        }
        faults
    }
}

fn misfit(setting: Setting, value: &Value) -> Option<Why> {
    let kind = setting.kind();
    if !value.fits(kind) {
        return Some(Why::WrongKind);
    }
    match (value, kind) {
        (Value::Choice(choice), SettingKind::Choice { choices, .. }) => {
            (!choices.contains(choice)).then_some(Why::NotOneOf)
        }
        (Value::Choices(chosen), SettingKind::Several { choices }) => chosen
            .iter()
            .any(|choice| !choices.contains(choice))
            .then_some(Why::NotOneOf),
        (Value::Number(number), SettingKind::Number { least, most, .. }) => {
            if !number.is_finite() {
                Some(Why::NotANumber)
            } else if *number < least || *number > most {
                Some(Why::OutOfRange {
                    value: *number,
                    least,
                    most,
                })
            } else {
                None
            }
        }
        (Value::Areas(areas), SettingKind::Areas) => areas.iter().find_map(|area| {
            if area.page == 0 {
                Some(Why::NoSuchPage)
            } else if ![area.x0, area.y0, area.x1, area.y1]
                .iter()
                .all(|n| n.is_finite())
            {
                Some(Why::NotANumber)
            } else {
                None
            }
        }),
        (Value::Strokes(strokes), SettingKind::Strokes) => strokes
            .iter()
            .flatten()
            .any(|(x, y)| !x.is_finite() || !y.is_finite())
            .then_some(Why::BadPoint),
        _ => None,
    }
}
