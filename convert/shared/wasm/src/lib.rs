#![forbid(unsafe_code)]

use std::cell::RefCell;

use convert_structure::model::Document;
use convert_structure::{Options, Output, Reader, Request};

pub type Write = fn(&Document, &Request) -> Result<Output, String>;

#[derive(Default)]
struct State {
    input: Vec<u8>,
    settings: Vec<u8>,
    reader: Option<Reader>,
    request: Request,
    todo: Vec<usize>,
    document: Document,
    files: Vec<(Vec<u8>, Vec<u8>)>,
    message: Vec<u8>,
    font_name: Vec<u8>,
    font_input: Vec<u8>,
    fonts: convert_structure::held_fonts::HeldFonts,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

#[must_use]
pub fn input(len: usize) -> *mut u8 {
    STATE.with_borrow_mut(|state| {
        state.input = vec![0; len];
        state.input.as_mut_ptr()
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
pub fn font_name(len: usize) -> *mut u8 {
    STATE.with_borrow_mut(|state| {
        state.font_name = vec![0; len];
        state.font_name.as_mut_ptr()
    })
}

#[must_use]
pub fn font_input(len: usize) -> *mut u8 {
    STATE.with_borrow_mut(|state| {
        state.font_input = vec![0; len];
        state.font_input.as_mut_ptr()
    })
}

#[must_use]
pub fn font_add() -> i32 {
    STATE.with_borrow_mut(|state| {
        let family = String::from_utf8_lossy(&state.font_name).into_owned();
        let bytes = std::mem::take(&mut state.font_input);
        match state.fonts.add(&family, bytes) {
            Ok(()) => 0,
            Err(why) => fail(state, &why),
        }
    })
}

fn fail(state: &mut State, message: &str) -> i32 {
    state.message = message.as_bytes().to_vec();
    -1
}

fn parse_settings(text: &str) -> Result<Request, String> {
    let mut request = Request {
        attachments: "files".to_owned(),
        ..Request::default()
    };
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "pages" => {
                for part in value.split(',').map(str::trim).filter(|p| !p.is_empty()) {
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
                        return Err(format!("'{part}': pages count from 1"));
                    }
                    request.pages.extend((a - 1)..b);
                }
            }
            "password" => request.password = value.as_bytes().to_vec(),
            "name" => {
                let stem = value
                    .trim()
                    .rsplit_once('.')
                    .map_or(value.trim(), |(stem, _)| stem);
                request.attachments = format!("{stem}_files");
            }
            "attachments" => value.trim().clone_into(&mut request.attachments),
            _ => {}
        }
    }
    Ok(request)
}

#[must_use]
pub fn begin() -> i32 {
    STATE.with_borrow_mut(|state| {
        let settings = String::from_utf8_lossy(&state.settings).into_owned();
        let request = match parse_settings(&settings) {
            Ok(request) => request,
            Err(why) => return fail(state, &why),
        };
        let pdf = std::mem::take(&mut state.input);
        let fonts = (!state.fonts.is_empty()).then(|| {
            std::sync::Arc::new(state.fonts.clone())
                as std::sync::Arc<dyn convert_structure::FontProvider>
        });
        let reader = match Reader::open(pdf, &request.password, fonts) {
            Ok(reader) => reader,
            Err(why) => return fail(state, &why),
        };
        let todo: Vec<usize> = if request.pages.is_empty() {
            (0..reader.page_count()).collect()
        } else {
            request.pages.clone()
        };
        if let Some(page) = todo.iter().find(|&&p| p >= reader.page_count()) {
            let why = format!(
                "page {} was asked for and the file has {} pages",
                page + 1,
                reader.page_count()
            );
            return fail(state, &why);
        }
        state.todo = todo.into_iter().rev().collect();
        state.request = request;
        state.reader = Some(reader);
        state.document = Document::default();
        state.files.clear();
        i32::try_from(state.todo.len()).unwrap_or(i32::MAX)
    })
}

#[must_use]
pub fn step(options: &Options) -> i32 {
    STATE.with_borrow_mut(|state| {
        let Some(reader) = state.reader.as_ref() else {
            return fail(state, "begin was not called");
        };
        if let Some(index) = state.todo.pop() {
            let page = reader.read_page(index, options);
            state.document.pages.push(page);
        }
        i32::try_from(state.document.pages.len()).unwrap_or(i32::MAX)
    })
}

#[must_use]
pub fn finish(write: Write) -> i32 {
    STATE.with_borrow_mut(|state| {
        if state.reader.is_none() {
            return fail(state, "begin was not called");
        }
        convert_structure::finish(&mut state.document);
        match write(&state.document, &state.request) {
            Ok(output) => {
                state.files.clear();
                state.files.push((b"main".to_vec(), output.main));
                for (name, bytes) in output.attachments {
                    state.files.push((
                        format!("{}/{name}", state.request.attachments).into_bytes(),
                        bytes,
                    ));
                }
                state.message = output.notes.join("\n").into_bytes();
                state.reader = None;
                state.document = Document::default();
                0
            }
            Err(why) => fail(state, &why),
        }
    })
}

thread_local! {
    static BATCH: RefCell<Option<convert_zip::ZipWriter>> = const { RefCell::new(None) };
}

#[must_use]
pub fn batch_add(extension: &str) -> i32 {
    STATE.with_borrow_mut(|state| {
        let stem = state
            .request
            .attachments
            .strip_suffix("_files")
            .unwrap_or("output")
            .to_owned();
        let files = state.files.clone();
        BATCH.with_borrow_mut(|batch| {
            let zip = batch.get_or_insert_with(convert_zip::ZipWriter::new);
            for (name, data) in &files {
                let name = String::from_utf8_lossy(name);
                let path = if name == "main" {
                    format!("{stem}.{extension}")
                } else {
                    name.into_owned()
                };
                if let Err(error) = zip.add(&path, data, convert_zip::Method::Deflated) {
                    state.message = error.to_string().into_bytes();
                    return -1;
                }
            }
            0
        })
    })
}

#[must_use]
pub fn batch_add_files() -> i32 {
    STATE.with_borrow_mut(|state| {
        let files = state.files.clone();
        BATCH.with_borrow_mut(|batch| {
            let zip = batch.get_or_insert_with(convert_zip::ZipWriter::new);
            for (name, data) in &files {
                let name = String::from_utf8_lossy(name).into_owned();
                let mut path = name.clone();
                let mut n = 2;
                loop {
                    match zip.add(&path, data, convert_zip::Method::Deflated) {
                        Ok(()) => break,
                        Err(convert_zip::ZipError::DuplicateName(_)) => {
                            path = match name.rsplit_once('.') {
                                Some((stem, ext)) => format!("{stem}-{n}.{ext}"),
                                None => format!("{name}-{n}"),
                            };
                            n += 1;
                        }
                        Err(error) => {
                            state.message = error.to_string().into_bytes();
                            return -1;
                        }
                    }
                }
            }
            0
        })
    })
}

#[must_use]
pub fn batch_finish() -> i32 {
    let Some(zip) = BATCH.with_borrow_mut(Option::take) else {
        return STATE.with_borrow_mut(|state| fail(state, "nothing was added to the batch"));
    };
    STATE.with_borrow_mut(|state| match zip.finish() {
        Ok(bytes) => {
            state.files = vec![(b"batch.zip".to_vec(), bytes)];
            0
        }
        Err(error) => fail(state, &error.to_string()),
    })
}

#[must_use]
pub fn count() -> usize {
    STATE.with_borrow(|state| state.files.len())
}

#[must_use]
pub fn name_ptr(at: usize) -> *const u8 {
    STATE.with_borrow(|state| {
        state
            .files
            .get(at)
            .map_or(std::ptr::null(), |f| f.0.as_ptr())
    })
}

#[must_use]
pub fn name_len(at: usize) -> usize {
    STATE.with_borrow(|state| state.files.get(at).map_or(0, |f| f.0.len()))
}

#[must_use]
pub fn data_ptr(at: usize) -> *const u8 {
    STATE.with_borrow(|state| {
        state
            .files
            .get(at)
            .map_or(std::ptr::null(), |f| f.1.as_ptr())
    })
}

#[must_use]
pub fn data_len(at: usize) -> usize {
    STATE.with_borrow(|state| state.files.get(at).map_or(0, |f| f.1.len()))
}

#[must_use]
pub fn message_ptr() -> *const u8 {
    STATE.with_borrow(|state| state.message.as_ptr())
}

#[must_use]
pub fn message_len() -> usize {
    STATE.with_borrow(|state| state.message.len())
}

thread_local! {
    static INPUTS: RefCell<(Vec<u8>, Vec<convert_structure::bytes_tool::Input>)> = const { RefCell::new((Vec::new(), Vec::new())) };
}

#[must_use]
pub fn input_name(len: usize) -> *mut u8 {
    INPUTS.with_borrow_mut(|(name, _)| {
        *name = vec![0; len];
        name.as_mut_ptr()
    })
}

#[must_use]
pub fn input_add() -> i32 {
    let bytes = STATE.with_borrow_mut(|state| std::mem::take(&mut state.input));
    INPUTS.with_borrow_mut(|(name, inputs)| {
        inputs.push(convert_structure::bytes_tool::Input {
            name: String::from_utf8_lossy(&std::mem::take(name)).into_owned(),
            bytes,
        });
        i32::try_from(inputs.len()).unwrap_or(i32::MAX)
    })
}

pub fn reset() {
    INPUTS.with_borrow_mut(|(name, inputs)| {
        name.clear();
        inputs.clear();
    });
}

#[must_use]
pub fn run(tool: convert_structure::bytes_tool::Run) -> i32 {
    let inputs = INPUTS.with_borrow_mut(|(_, inputs)| std::mem::take(inputs));
    STATE.with_borrow_mut(|state| {
        let settings = convert_structure::bytes_tool::Settings::parse(&String::from_utf8_lossy(
            &state.settings,
        ));
        let fonts = convert_structure::bytes_tool::Fonts {
            provider: (!state.fonts.is_empty()).then(|| {
                std::sync::Arc::new(state.fonts.clone())
                    as std::sync::Arc<dyn convert_structure::FontProvider>
            }),
            files: state.fonts.files(),
        };
        match tool(&inputs, &settings, &fonts, &mut |_, _| true) {
            Ok(made) => {
                state.files = made
                    .files
                    .into_iter()
                    .map(|(name, data)| (name.into_bytes(), data))
                    .collect();
                state.message = made.notes.join("\n").into_bytes();
                0
            }
            Err(why) => fail(state, &why),
        }
    })
}

#[macro_export]
macro_rules! export_bytes_tool {
    ($run:path) => {
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
            pub extern "C" fn input_add() -> i32 {
                $crate::input_add()
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn reset() {
                $crate::reset();
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn settings(len: usize) -> *mut u8 {
                $crate::settings(len)
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn font_name(len: usize) -> *mut u8 {
                $crate::font_name(len)
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn font_input(len: usize) -> *mut u8 {
                $crate::font_input(len)
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn font_add() -> i32 {
                $crate::font_add()
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn run() -> i32 {
                $crate::run($run)
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn batch_add() -> i32 {
                $crate::batch_add_files()
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn batch_finish() -> i32 {
                $crate::batch_finish()
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

#[macro_export]
macro_rules! export_tool {
    ($options:expr, $write:path, $extension:expr) => {
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
            pub extern "C" fn font_name(len: usize) -> *mut u8 {
                $crate::font_name(len)
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn font_input(len: usize) -> *mut u8 {
                $crate::font_input(len)
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn font_add() -> i32 {
                $crate::font_add()
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn begin() -> i32 {
                $crate::begin()
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn step() -> i32 {
                $crate::step(&$options)
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn finish() -> i32 {
                $crate::finish($write)
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn batch_add() -> i32 {
                $crate::batch_add($extension)
            }
            #[unsafe(no_mangle)]
            pub extern "C" fn batch_finish() -> i32 {
                $crate::batch_finish()
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

    fn joined(
        inputs: &[convert_structure::bytes_tool::Input],
        settings: &convert_structure::bytes_tool::Settings,
        _fonts: &convert_structure::bytes_tool::Fonts,
        _progress: &mut dyn FnMut(usize, usize) -> bool,
    ) -> Result<convert_structure::bytes_tool::Files, String> {
        let mut out = Vec::new();
        for input in inputs {
            out.extend_from_slice(&input.bytes);
        }
        Ok(convert_structure::bytes_tool::Files {
            files: vec![(settings.get("name").unwrap_or("out").to_owned(), out)],
            notes: vec![format!("{} inputs", inputs.len())],
        })
    }

    #[test]
    fn a_bytes_tool_takes_several_inputs() {
        reset();
        for (name, bytes) in [("a.bin", b"ab".as_slice()), ("b.bin", b"cd".as_slice())] {
            let _ = input(bytes.len());
            STATE.with_borrow_mut(|state| state.input.copy_from_slice(bytes));
            let _ = input_name(name.len());
            INPUTS.with_borrow_mut(|(n, _)| n.copy_from_slice(name.as_bytes()));
            let _ = input_add();
        }
        let text = b"name=joined.bin";
        let _ = settings(text.len());
        STATE.with_borrow_mut(|state| state.settings.copy_from_slice(text));
        assert_eq!(run(joined), 0);
        assert_eq!(count(), 1);
        STATE.with_borrow(|state| {
            assert_eq!(state.files[0].0, b"joined.bin");
            assert_eq!(state.files[0].1, b"abcd");
            assert_eq!(state.message, b"2 inputs");
        });
    }

    #[test]
    fn several_bytes_tool_runs_make_one_zip() {
        for bytes in [b"first".as_slice(), b"second".as_slice()] {
            reset();
            let _ = input(bytes.len());
            STATE.with_borrow_mut(|state| state.input.copy_from_slice(bytes));
            let _ = input_name(5);
            INPUTS.with_borrow_mut(|(n, _)| n.copy_from_slice(b"a.bin"));
            let _ = input_add();
            let text = b"name=same.bin";
            let _ = settings(text.len());
            STATE.with_borrow_mut(|state| state.settings.copy_from_slice(text));
            assert_eq!(run(joined), 0);
            assert_eq!(batch_add_files(), 0);
        }
        assert_eq!(batch_finish(), 0);
        STATE.with_borrow(|state| {
            assert_eq!(state.files.len(), 1);
            assert_eq!(state.files[0].0, b"batch.zip");
            let zip = &state.files[0].1;
            let has = |name: &[u8]| zip.windows(name.len()).any(|w| w == name);
            assert!(zip.starts_with(b"PK\x03\x04"));
            assert!(has(b"same.bin"));
            assert!(has(b"same-2.bin"));
        });
        assert_eq!(batch_finish(), -1);
    }

    #[test]
    fn settings_are_read() {
        let request = parse_settings("pages=1,3-4\npassword=abc\n").unwrap();
        assert_eq!(request.pages, vec![0, 2, 3]);
        assert_eq!(request.password, b"abc");
        assert!(parse_settings("pages=0").is_err());
    }

    #[test]
    fn a_broken_file_fails_with_a_message() {
        let bytes = b"not a pdf";
        let ptr = input(bytes.len());
        assert!(!ptr.is_null());
        STATE.with_borrow_mut(|state| state.input.copy_from_slice(bytes));
        assert_eq!(begin(), -1);
        assert!(message_len() > 0);
    }
}
