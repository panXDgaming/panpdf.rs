use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    Cancelled,
    NeedsPassword,
    BadInput(String),
    Refused(String),
    Panicked(String),
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => f.write_str("cancelled"),
            Self::NeedsPassword => {
                f.write_str("the file needs its password, or the password given does not open it")
            }
            Self::BadInput(why) | Self::Refused(why) => f.write_str(why),
            Self::Panicked(why) => write!(f, "the converter stopped unexpectedly: {why}"),
        }
    }
}

impl std::error::Error for Failure {}

const PASSWORD_PHRASES: [&str; 6] = [
    "password is wrong",
    "needs its password",
    "password does not open",
    "password does not authenticate",
    "asks for a password",
    "needs a password",
];

const BAD_INPUT_PHRASES: [&str; 18] = [
    "cannot be read as a pdf",
    "not a pdf",
    "no .html file among the inputs",
    "no pdf was given",
    "no picture was given",
    "no photograph was given",
    "no input file was given",
    "not a word document",
    "no document catalogue",
    "the file is damaged",
    "was asked for and the file has",
    "is not a page",
    "pages count from 1",
    "is not a number",
    "is not between",
    "takes one pdf at a time",
    "give two pdfs",
    "is not a png or a jpeg",
];

pub fn from_engine(message: &str) -> Failure {
    let lowered = message.to_ascii_lowercase();
    if lowered.trim() == "cancelled" {
        Failure::Cancelled
    } else if PASSWORD_PHRASES
        .iter()
        .any(|phrase| lowered.contains(phrase))
    {
        Failure::NeedsPassword
    } else if BAD_INPUT_PHRASES
        .iter()
        .any(|phrase| lowered.contains(phrase))
    {
        Failure::BadInput(message.to_owned())
    } else {
        Failure::Refused(message.to_owned())
    }
}

pub fn merge(failures: &[(String, Failure)], several: bool) -> Failure {
    if failures.iter().any(|(_, f)| *f == Failure::NeedsPassword) {
        return Failure::NeedsPassword;
    }
    let all_bad_input = failures
        .iter()
        .all(|(_, failure)| matches!(failure, Failure::BadInput(_)));
    let lines: Vec<String> = failures
        .iter()
        .map(|(name, failure)| {
            if several {
                format!("{name}: {failure}")
            } else {
                failure.to_string()
            }
        })
        .collect();
    let text = lines.join("\n");
    if all_bad_input {
        Failure::BadInput(text)
    } else {
        Failure::Refused(text)
    }
}
