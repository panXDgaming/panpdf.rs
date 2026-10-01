#![forbid(unsafe_code)]

use std::cell::RefCell;

#[cfg(not(target_arch = "wasm32"))]
pub mod cli;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Input {
    pub name: String,
    pub bytes: Vec<u8>,
}

impl Input {
    #[must_use]
    pub fn stem(&self) -> &str {
        let name = self.name.rsplit(['/', '\\']).next().unwrap_or(&self.name);
        match name.rsplit_once('.') {
            Some((stem, _)) if !stem.is_empty() => stem,
            _ if name.is_empty() => "output",
            _ => name,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings(pub Vec<(String, String)>);

impl Settings {
    #[must_use]
    pub fn parse(text: &str) -> Self {
        Self(
            text.lines()
                .filter_map(|line| line.split_once('='))
                .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
                .collect(),
        )
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    #[must_use]
    pub fn text<'a>(&'a self, key: &str, default: &'a str) -> &'a str {
        self.get(key).unwrap_or(default)
    }

    #[must_use]
    pub fn flag(&self, key: &str) -> bool {
        self.get(key)
            .is_some_and(|v| matches!(v, "1" | "true" | "yes" | "on" | ""))
    }

    pub fn number(&self, key: &str, default: f64) -> Result<f64, String> {
        match self.get(key) {
            None => Ok(default),
            Some(v) => v
                .parse::<f64>()
                .ok()
                .filter(|n| n.is_finite())
                .ok_or_else(|| format!("{key}: '{v}' is not a number")),
        }
    }

    pub fn pages(&self) -> Result<Vec<usize>, String> {
        let Some(text) = self.get("pages") else {
            return Ok(Vec::new());
        };
        let mut pages = Vec::new();
        for part in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            let (a, b) = part.split_once('-').unwrap_or((part, part));
            let a: usize = a
                .trim()
                .parse()
                .map_err(|_| format!("'{part}' is not a page"))?;
            let b: usize = b
                .trim()
                .parse()
                .map_err(|_| format!("'{part}' is not a page"))?;
            if a == 0 || b < a {
                return Err(format!(
                    "'{part}': pages count from 1, and a range runs forwards"
                ));
            }
            pages.extend((a - 1)..b);
        }
        Ok(pages)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub files: Vec<(String, Vec<u8>)>,
    pub notes: Vec<String>,
}

pub trait Job {
    fn total(&self) -> usize;
    fn step(&mut self) -> Result<(), String>;
    fn finish(self: Box<Self>) -> Result<Outcome, String>;
}

pub type Start = fn(Vec<Input>, &Settings) -> Result<Box<dyn Job>, String>;

pub struct Done(pub Outcome);

impl Job for Done {
    fn total(&self) -> usize {
        0
    }
    fn step(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn finish(self: Box<Self>) -> Result<Outcome, String> {
        Ok(self.0)
    }
}

pub fn run_to_end(
    start: Start,
    inputs: Vec<Input>,
    settings: &Settings,
) -> Result<Outcome, String> {
    let mut job: Box<dyn Job> = start(inputs, settings)?;
    for _ in 0..job.total() {
        job.step()?;
    }
    job.finish()
}

#[must_use]
pub fn human_size(bytes: usize) -> String {
    let b = bytes as f64;
    if b >= 1024.0 * 1024.0 {
        format!("{:.2} MB", b / (1024.0 * 1024.0))
    } else if b >= 1024.0 {
        format!("{:.1} KB", b / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

#[derive(Default)]
struct State {
    inputs: Vec<Input>,
    names: Vec<Vec<u8>>,
    settings: Vec<u8>,
    job: Option<Box<dyn Job>>,
    done: usize,
    files: Vec<(Vec<u8>, Vec<u8>)>,
    message: Vec<u8>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
    static RANDOM: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

#[must_use]
pub fn random(len: usize) -> *mut u8 {
    RANDOM.with_borrow_mut(|bytes| {
        bytes.fill(0);
        *bytes = vec![0; len];
        bytes.as_mut_ptr()
    })
}

#[must_use]
pub fn take_random() -> Vec<u8> {
    RANDOM.with_borrow_mut(std::mem::take)
}

fn fail(state: &mut State, message: &str) -> i32 {
    state.message = message.as_bytes().to_vec();
    -1
}

#[must_use]
pub fn input(len: usize) -> *mut u8 {
    STATE.with_borrow_mut(|state| {
        state.inputs.push(Input {
            name: String::new(),
            bytes: vec![0; len],
        });
        state.names.push(Vec::new());
        state
            .inputs
            .last_mut()
            .map_or(std::ptr::null_mut(), |i| i.bytes.as_mut_ptr())
    })
}

#[must_use]
pub fn input_name(len: usize) -> *mut u8 {
    STATE.with_borrow_mut(|state| {
        let Some(last) = state.names.last_mut() else {
            return std::ptr::null_mut();
        };
        *last = vec![0; len];
        last.as_mut_ptr()
    })
}

#[must_use]
pub fn settings(len: usize) -> *mut u8 {
    STATE.with_borrow_mut(|state| {
        state.settings = vec![0; len];
        state.settings.as_mut_ptr()
    })
}

#[must_use]
pub fn begin(start: Start) -> i32 {
    STATE.with_borrow_mut(|state| {
        let settings = Settings::parse(&String::from_utf8_lossy(&state.settings));
        let names = std::mem::take(&mut state.names);
        let inputs = std::mem::take(&mut state.inputs)
            .into_iter()
            .zip(names)
            .map(|(mut input, name)| {
                if input.name.is_empty() {
                    input.name = String::from_utf8_lossy(&name).into_owned();
                }
                input
            })
            .collect();
        state.files.clear();
        state.done = 0;
        let status = match start(inputs, &settings) {
            Ok(job) => {
                let total = job.total();
                state.job = Some(job);
                i32::try_from(total).unwrap_or(i32::MAX)
            }
            Err(why) => fail(state, &why),
        };
        RANDOM.with_borrow_mut(|bytes| {
            bytes.fill(0);
            bytes.clear();
        });
        status
    })
}

#[must_use]
pub fn step() -> i32 {
    STATE.with_borrow_mut(|state| {
        let Some(job) = state.job.as_mut() else {
            return fail(state, "begin was not called");
        };
        if state.done < job.total() {
            if let Err(why) = job.step() {
                state.job = None;
                return fail(state, &why);
            }
            state.done += 1;
        }
        i32::try_from(state.done).unwrap_or(i32::MAX)
    })
}

#[must_use]
pub fn finish() -> i32 {
    STATE.with_borrow_mut(|state| {
        let Some(job) = state.job.take() else {
            return fail(state, "begin was not called");
        };
        match job.finish() {
            Ok(outcome) => {
                state.files = outcome
                    .files
                    .into_iter()
                    .map(|(name, data)| (name.into_bytes(), data))
                    .collect();
                state.message = outcome.notes.join("\n").into_bytes();
                0
            }
            Err(why) => fail(state, &why),
        }
    })
}

#[must_use]
pub fn count() -> usize {
    STATE.with_borrow(|state| state.files.len())
}

#[must_use]
pub fn name_ptr(at: usize) -> *const u8 {
    STATE.with_borrow(|s| s.files.get(at).map_or(std::ptr::null(), |f| f.0.as_ptr()))
}

#[must_use]
pub fn name_len(at: usize) -> usize {
    STATE.with_borrow(|s| s.files.get(at).map_or(0, |f| f.0.len()))
}

#[must_use]
pub fn data_ptr(at: usize) -> *const u8 {
    STATE.with_borrow(|s| s.files.get(at).map_or(std::ptr::null(), |f| f.1.as_ptr()))
}

#[must_use]
pub fn data_len(at: usize) -> usize {
    STATE.with_borrow(|s| s.files.get(at).map_or(0, |f| f.1.len()))
}

#[must_use]
pub fn message_ptr() -> *const u8 {
    STATE.with_borrow(|s| s.message.as_ptr())
}

#[must_use]
pub fn message_len() -> usize {
    STATE.with_borrow(|s| s.message.len())
}

#[macro_export]
macro_rules! export_files_tool {
    ($start:path) => {
        #[cfg(target_arch = "wasm32")]
        #[allow(
            unsafe_code,
            reason = "naming an export is the one unsafe attribute a bundle needs"
        )]
        mod browser_exports {
            #[unsafe(no_mangle)]
            pub extern "C" fn input(len: usize) -> *mut u8 {
                $crate::input(len)
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn input_name(len: usize) -> *mut u8 {
                $crate::input_name(len)
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn settings(len: usize) -> *mut u8 {
                $crate::settings(len)
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn random(len: usize) -> *mut u8 {
                $crate::random(len)
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn begin() -> i32 {
                $crate::begin($start)
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn step() -> i32 {
                $crate::step()
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn finish() -> i32 {
                $crate::finish()
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn count() -> usize {
                $crate::count()
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn name_ptr(at: usize) -> *const u8 {
                $crate::name_ptr(at)
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn name_len(at: usize) -> usize {
                $crate::name_len(at)
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn data_ptr(at: usize) -> *const u8 {
                $crate::data_ptr(at)
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn data_len(at: usize) -> usize {
                $crate::data_len(at)
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn message_ptr() -> *const u8 {
                $crate::message_ptr()
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn message_len() -> usize {
                $crate::message_len()
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn echo(inputs: Vec<Input>, settings: &Settings) -> Result<Box<dyn Job>, String> {
        let suffix = settings.text("suffix", "x").to_owned();
        Ok(Box::new(Done(Outcome {
            files: inputs
                .into_iter()
                .map(|i| (format!("{}.{suffix}", i.stem()), i.bytes))
                .collect(),
            notes: vec!["done".into()],
        })))
    }

    #[test]
    fn settings_read() {
        let s = Settings::parse("a=1\nflag=\nb = two\na=3\n");
        assert_eq!(s.get("a"), Some("3"));
        assert!(s.flag("flag"));
        assert!(!s.flag("missing"));
        assert_eq!(s.number("a", 0.0).unwrap(), 3.0);
        assert!(s.number("b", 0.0).is_err());
        let p = Settings::parse("pages=1,3-4");
        assert_eq!(p.pages().unwrap(), vec![0, 2, 3]);
        assert!(Settings::parse("pages=0").pages().is_err());
    }

    fn random_length(_inputs: Vec<Input>, _settings: &Settings) -> Result<Box<dyn Job>, String> {
        let taken = take_random().len();
        Ok(Box::new(Done(Outcome {
            files: Vec::new(),
            notes: vec![taken.to_string()],
        })))
    }

    #[test]
    fn random_bytes_reach_one_job_only() {
        let _ = random(64);
        assert_eq!(begin(random_length), 0);
        assert_eq!(finish(), 0);
        STATE.with_borrow(|s| assert_eq!(s.message, b"64"));
        let _ = random(64);
        assert_eq!(begin(echo), 0);
        assert!(take_random().is_empty());
        let _ = finish();
        assert_eq!(begin(random_length), 0);
        assert_eq!(finish(), 0);
        STATE.with_borrow(|s| assert_eq!(s.message, b"0"));
    }

    #[test]
    fn stems() {
        let input = |name: &str| Input {
            name: name.into(),
            bytes: Vec::new(),
        };
        assert_eq!(input("a/b/photo.jpg").stem(), "photo");
        assert_eq!(input("noext").stem(), "noext");
        assert_eq!(input("").stem(), "output");
    }

    #[test]
    fn the_browser_calls_run_a_job() {
        let ptr = input(3);
        assert!(!ptr.is_null());
        STATE.with_borrow_mut(|s| {
            s.inputs[0].bytes.copy_from_slice(b"abc");
            s.inputs[0].name = "in.pdf".into();
            s.settings = b"suffix=out".to_vec();
        });
        assert_eq!(begin(echo), 0);
        assert_eq!(step(), 0);
        assert_eq!(finish(), 0);
        assert_eq!(count(), 1);
        assert_eq!(name_len(0), "in.out".len());
        assert_eq!(data_len(0), 3);
        assert_eq!(message_len(), 4);
    }
}
