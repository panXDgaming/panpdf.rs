#![forbid(unsafe_code)]

use std::cell::RefCell;
use std::collections::BTreeMap;

#[cfg(not(target_arch = "wasm32"))]
pub mod cli;

#[derive(Clone, Debug, Default)]
pub struct Job {
    pub inputs: Vec<Vec<u8>>,
    pub options: BTreeMap<String, String>,
    pub files: BTreeMap<String, Vec<u8>>,
    pub random: Vec<u8>,
}

impl Job {
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.options.get(key).map(String::as_str)
    }

    #[must_use]
    pub fn flag(&self, key: &str) -> bool {
        self.get(key).is_some_and(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "true" | "yes" | "1" | "on" | ""
            )
        })
    }

    #[must_use]
    pub fn password(&self) -> Vec<u8> {
        self.get("password")
            .map(|p| p.as_bytes().to_vec())
            .unwrap_or_default()
    }

    pub fn pdf(&self) -> Result<&[u8], String> {
        self.inputs
            .first()
            .map(Vec::as_slice)
            .ok_or_else(|| "no input file was given".to_owned())
    }

    pub fn number(&self, key: &str) -> Result<Option<f64>, String> {
        self.get(key)
            .map(|v| {
                v.trim()
                    .parse::<f64>()
                    .map_err(|_| format!("--{key}: '{v}' is not a number"))
            })
            .transpose()
    }

    pub fn pages(&self, key: &str, count: usize) -> Result<Vec<usize>, String> {
        match self.get(key).map(str::trim) {
            None | Some("" | "all") => Ok((0..count).collect()),
            Some(text) => {
                let pages = parse_pages(text, count)?;
                if let Some(page) = pages.iter().find(|&&p| p >= count) {
                    return Err(format!(
                        "page {} was asked for and the file has {count} pages",
                        page + 1
                    ));
                }
                Ok(pages)
            }
        }
    }
}

pub fn parse_pages(text: &str, count: usize) -> Result<Vec<usize>, String> {
    let page = |part: &str, whole: &str| -> Result<usize, String> {
        match part.trim() {
            "last" | "-1" => Ok(count.max(1)),
            other => other
                .parse::<usize>()
                .ok()
                .filter(|&n| n > 0)
                .ok_or_else(|| format!("'{whole}' is not a page or a range of pages")),
        }
    };
    let mut pages = Vec::new();
    for part in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (from, to) = match part.split_once('-').filter(|(a, _)| !a.is_empty()) {
            Some((a, b)) => (page(a, part)?, page(b, part)?),
            None => (page(part, part)?, page(part, part)?),
        };
        if to < from {
            return Err(format!("'{part}': a range runs forwards"));
        }
        pages.extend((from - 1)..to);
    }
    Ok(pages)
}

#[derive(Clone, Debug, Default)]
pub struct Output {
    pub main: Vec<u8>,
    pub attachments: Vec<(String, Vec<u8>)>,
    pub notes: Vec<String>,
}

pub struct Tool {
    pub name: &'static str,
    pub extension: &'static str,
    pub inputs: usize,
    pub file_options: &'static [&'static str],
    pub flags: &'static [&'static str],
    pub help: &'static str,
    pub run: fn(&Job) -> Result<Output, String>,
}

fn parse_settings(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter_map(|line| line.split_once('='))
        .map(|(k, v)| (k.trim().to_owned(), v.to_owned()))
        .collect()
}

#[derive(Default)]
struct State {
    inputs: Vec<Vec<u8>>,
    settings: Vec<u8>,
    random: Vec<u8>,
    files: Vec<(Vec<u8>, Vec<u8>)>,
    message: Vec<u8>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

#[must_use]
pub fn input(len: usize) -> *mut u8 {
    STATE.with_borrow_mut(|state| {
        state.inputs.push(vec![0; len]);
        state
            .inputs
            .last_mut()
            .map_or(std::ptr::null_mut(), Vec::as_mut_ptr)
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
pub fn random(len: usize) -> *mut u8 {
    STATE.with_borrow_mut(|state| {
        wipe(&mut state.random);
        state.random = vec![0; len];
        state.random.as_mut_ptr()
    })
}

fn wipe(bytes: &mut [u8]) {
    bytes.fill(0);
    std::hint::black_box(bytes);
}

pub fn reset() {
    STATE.with_borrow_mut(|state| {
        wipe(&mut state.random);
        *state = State::default();
    });
}

#[must_use]
pub fn run(tool: &Tool) -> i32 {
    STATE.with_borrow_mut(|state| {
        let options = parse_settings(&String::from_utf8_lossy(&state.settings));
        let inputs = std::mem::take(&mut state.inputs);
        let mut job = Job {
            random: std::mem::take(&mut state.random),
            ..Job::default()
        };
        let mut used = vec![false; inputs.len()];
        for (key, value) in options {
            if let Some(at) = value
                .strip_prefix('@')
                .and_then(|n| n.trim().parse::<usize>().ok())
                && let Some(file) = inputs.get(at)
            {
                job.files.insert(key, file.clone());
                used[at] = true;
                continue;
            }
            job.options.insert(key, value);
        }
        job.inputs = inputs
            .into_iter()
            .zip(used)
            .filter(|(_, used)| !used)
            .map(|(file, _)| file)
            .collect();
        let result = (tool.run)(&job);
        wipe(&mut job.random);
        match result {
            Ok(output) => {
                state.files.clear();
                state.files.push((b"main".to_vec(), output.main));
                for (name, bytes) in output.attachments {
                    state.files.push((name.into_bytes(), bytes));
                }
                state.message = output.notes.join("\n").into_bytes();
                0
            }
            Err(why) => {
                state.files.clear();
                state.message = why.into_bytes();
                -1
            }
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
macro_rules! export_pdf_tool {
    ($tool:path) => {
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
            pub extern "C" fn settings(len: usize) -> *mut u8 {
                $crate::settings(len)
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn random(len: usize) -> *mut u8 {
                $crate::random(len)
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn run() -> i32 {
                $crate::run(&$tool)
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn reset() {
                $crate::reset()
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

    fn echo(job: &Job) -> Result<Output, String> {
        let mut main = job.pdf()?.to_vec();
        main.extend_from_slice(job.files.get("image").map_or(&[][..], Vec::as_slice));
        Ok(Output {
            main,
            attachments: Vec::new(),
            notes: vec![job.get("x").unwrap_or("-").to_owned()],
        })
    }

    static ECHO: Tool = Tool {
        name: "echo",
        extension: "pdf",
        inputs: 1,
        file_options: &["image"],
        flags: &[],
        help: "",
        run: echo,
    };

    #[test]
    fn pages_parse() {
        assert_eq!(parse_pages("1,3-4,last", 9).unwrap(), vec![0, 2, 3, 8]);
        assert!(parse_pages("0", 3).is_err());
        assert!(parse_pages("4-2", 5).is_err());
    }

    #[test]
    fn the_browser_frame_names_files_by_position() {
        reset();
        let a = input(2);
        assert!(!a.is_null());
        STATE.with_borrow_mut(|s| s.inputs[0].copy_from_slice(b"AB"));
        let _ = input(1);
        STATE.with_borrow_mut(|s| s.inputs[1].copy_from_slice(b"C"));
        let text = b"x=hello\nimage=@1\n";
        let _ = settings(text.len());
        STATE.with_borrow_mut(|s| s.settings.copy_from_slice(text));
        assert_eq!(run(&ECHO), 0);
        assert_eq!(count(), 1);
        assert_eq!(data_len(0), 3);
        assert_eq!(message_len(), 5);
    }

    fn random_length(job: &Job) -> Result<Output, String> {
        Ok(Output {
            main: Vec::new(),
            attachments: Vec::new(),
            notes: vec![format!("{}", job.random.len())],
        })
    }

    static RANDOM: Tool = Tool {
        name: "random",
        extension: "pdf",
        inputs: 1,
        file_options: &[],
        flags: &[],
        help: "",
        run: random_length,
    };

    #[test]
    fn random_bytes_reach_one_run_only() {
        reset();
        let _ = random(64);
        assert_eq!(run(&RANDOM), 0);
        let said = || STATE.with_borrow(|s| String::from_utf8(s.message.clone()).unwrap());
        assert_eq!(said(), "64");
        assert!(STATE.with_borrow(|s| s.random.is_empty()));
        assert_eq!(run(&RANDOM), 0);
        assert_eq!(said(), "0");
    }
}
