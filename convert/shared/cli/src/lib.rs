#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

use convert_structure::{FontProvider, Output, Request};

pub type Convert = fn(
    Vec<u8>,
    &Request,
    Option<Arc<dyn FontProvider>>,
    &mut dyn FnMut(usize, usize) -> bool,
) -> Result<Output, String>;

pub fn parse_pages(text: &str) -> Result<Vec<usize>, String> {
    let mut pages = Vec::new();
    for part in text
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        let (from, to) = match part.split_once('-') {
            Some((a, b)) => (a.trim(), b.trim()),
            None => (part, part),
        };
        let from: usize = from
            .parse()
            .map_err(|_| format!("'{part}' is not a page or a range of pages"))?;
        let to: usize = to
            .parse()
            .map_err(|_| format!("'{part}' is not a page or a range of pages"))?;
        if from == 0 || to < from {
            return Err(format!(
                "'{part}': pages count from 1, and a range runs forwards"
            ));
        }
        pages.extend((from - 1)..to);
    }
    Ok(pages)
}

fn json_field(chunk: &str, key: &str) -> Option<String> {
    let rest = chunk
        .split(&format!("\"{key}\""))
        .nth(1)?
        .split(':')
        .nth(1)?;
    Some(rest.split('"').nth(1)?.to_owned())
}

fn json_number(chunk: &str, key: &str) -> Option<u32> {
    let rest = chunk
        .split(&format!("\"{key}\""))
        .nth(1)?
        .split(':')
        .nth(1)?;
    rest.trim()
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

fn packaged_faces(directory: &Path, manifest: &str) -> Vec<pdf_content::PackagedFace> {
    let Some(list) = manifest.split("\"faces\"").nth(1) else {
        return Vec::new();
    };
    let mut faces = Vec::new();
    for chunk in list.split('{').skip(1) {
        let (Some(file), Some(sha256)) = (json_field(chunk, "file"), json_field(chunk, "sha256"))
        else {
            continue;
        };
        faces.push(pdf_content::PackagedFace {
            path: directory.join(file),
            face_index: json_number(chunk, "face_index").unwrap_or(0),
            sha256,
        });
    }
    faces
}

#[must_use]
pub fn font_package(given: Option<&Path>) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(given) = given {
        candidates.push(given.to_path_buf());
    }
    if let Some(dir) = std::env::var_os("PANPDF_FONTS") {
        candidates.push(PathBuf::from(dir));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        candidates.push(dir.join("fonts"));
        if let Some(up) = dir.parent() {
            candidates.push(up.join("fonts"));
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        for up in [4, 5] {
            if let Some(workspace) = exe.ancestors().nth(up) {
                candidates.push(workspace.join("panpdf.rs/fonts"));
            }
        }
    }
    candidates
        .into_iter()
        .find(|dir| dir.join("manifest.json").is_file())
}

#[derive(Debug)]
struct FamilyFirst {
    packaged: Arc<pdf_content::SystemFontProvider>,
    host: Arc<pdf_content::SystemFontProvider>,
}

impl FontProvider for FamilyFirst {
    fn primary_face(
        &self,
        request: &pdf_content::FontRequest,
    ) -> Option<pdf_content::SubstitutedFace> {
        self.packaged
            .primary_face(request)
            .filter(|face| pdf_content::outline_match::is_same_family(face, &request.family))
            .or_else(|| self.host.primary_face(request))
            .or_else(|| self.packaged.primary_face(request))
    }

    fn fallback_face(
        &self,
        request: &pdf_content::FontRequest,
        character: char,
    ) -> Option<pdf_content::SubstitutedFace> {
        let reference = if ('\u{0E80}'..='\u{0EFF}').contains(&character) {
            Some("Noto Sans Lao")
        } else if character.is_ascii_alphabetic() {
            Some("DejaVu Sans")
        } else {
            None
        };
        reference
            .and_then(|family| {
                let mut named = request.clone();
                family.clone_into(&mut named.family);
                self.packaged
                    .primary_face(&named)
                    .filter(|face| pdf_content::outline_match::is_same_family(face, family))
            })
            .or_else(|| self.host.fallback_face(request, character))
            .or_else(|| self.packaged.fallback_face(request, character))
    }

    fn description(&self) -> String {
        format!(
            "{} then {}",
            self.packaged.description(),
            self.host.description()
        )
    }
}

#[must_use]
pub fn fonts(given: Option<&Path>) -> Option<Arc<dyn FontProvider>> {
    if given.is_some_and(|path| path == Path::new("none")) {
        return None;
    }
    let host = Arc::new(pdf_content::SystemFontProvider::discover());
    let packaged = font_package(given).and_then(|root| {
        let manifest = std::fs::read_to_string(root.join("manifest.json")).ok()?;
        let faces = packaged_faces(&root.join("packaged"), &manifest);
        let (provider, problems) = pdf_content::SystemFontProvider::from_package(&faces);
        for problem in problems {
            eprintln!("font package: {problem}");
        }
        (!provider.faces().is_empty()).then(|| Arc::new(provider))
    });
    Some(match packaged {
        Some(packaged) => Arc::new(FamilyFirst { packaged, host }),
        None => host,
    })
}

#[must_use]
pub fn font_files(given: Option<&Path>) -> Vec<convert_structure::bytes_tool::FontFile> {
    if given.is_some_and(|path| path == Path::new("none")) {
        return Vec::new();
    }
    let Some(root) = font_package(given) else {
        return Vec::new();
    };
    let Ok(manifest) = std::fs::read_to_string(root.join("manifest.json")) else {
        return Vec::new();
    };
    let faces = packaged_faces(&root.join("packaged"), &manifest);
    let (provider, _) = pdf_content::SystemFontProvider::from_package(&faces);
    provider
        .faces()
        .iter()
        .filter_map(|face| {
            let bytes = std::fs::read(&face.path).ok()?;
            Some(convert_structure::bytes_tool::FontFile {
                family: face.family.clone(),
                bytes: Arc::from(bytes),
            })
        })
        .collect()
}

#[must_use]
pub fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[must_use]
pub fn peak_memory() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|line| line.starts_with("VmHWM:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb * 1024)
}

struct Arguments {
    inputs: Vec<PathBuf>,
    output: Option<PathBuf>,
    out_dir: Option<PathBuf>,
    zip: Option<PathBuf>,
    pages: Vec<usize>,
    password: Vec<u8>,
    fonts: Option<PathBuf>,
    stats: bool,
    quiet: bool,
}

fn usage(tool: &str, extension: &str) -> String {
    format!(
        "usage: {tool} IN.pdf OUT.{extension} [options]\n       {tool} IN.pdf... --out-dir DIR [options]\n       {tool} IN.pdf... --zip OUT.zip [options]\noptions: --pages 1,3-5  --password PW  --fonts DIR|none  --stats  --quiet"
    )
}

fn arguments(tool: &str, extension: &str) -> Result<Arguments, String> {
    let mut args = std::env::args_os().skip(1);
    let mut positional = Vec::new();
    let mut parsed = Arguments {
        inputs: Vec::new(),
        output: None,
        out_dir: None,
        zip: None,
        pages: Vec::new(),
        password: Vec::new(),
        fonts: None,
        stats: false,
        quiet: false,
    };
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or_else(|| usage(tool, extension));
        match arg.to_str() {
            Some("--pages") => parsed.pages = parse_pages(&value()?.to_string_lossy())?,
            Some("--password") => {
                parsed.password = value()?.to_string_lossy().into_owned().into_bytes()
            }
            Some("--fonts") => parsed.fonts = Some(PathBuf::from(value()?)),
            Some("--out-dir") => parsed.out_dir = Some(PathBuf::from(value()?)),
            Some("--zip") => parsed.zip = Some(PathBuf::from(value()?)),
            Some("--stats") => parsed.stats = true,
            Some("--quiet") => parsed.quiet = true,
            Some("-h" | "--help") => return Err(usage(tool, extension)),
            _ => positional.push(PathBuf::from(arg)),
        }
    }
    if parsed.out_dir.is_some() || parsed.zip.is_some() {
        if positional.is_empty() || (parsed.out_dir.is_some() && parsed.zip.is_some()) {
            return Err(usage(tool, extension));
        }
        parsed.inputs = positional;
    } else {
        let [input, output] =
            <[PathBuf; 2]>::try_from(positional).map_err(|_| usage(tool, extension))?;
        parsed.inputs = vec![input];
        parsed.output = Some(output);
    }
    Ok(parsed)
}

struct Done {
    name: String,
    output: Output,
    pages: usize,
    seconds: f64,
}

fn batch_names(inputs: &[PathBuf], extension: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::with_capacity(inputs.len());
    for input in inputs {
        let stem = input.file_stem().map_or_else(
            || "output".to_owned(),
            |stem| stem.to_string_lossy().into_owned(),
        );
        let mut name = format!("{stem}.{extension}");
        let mut n = 2;
        while names.contains(&name) {
            name = format!("{stem}-{n}.{extension}");
            n += 1;
        }
        names.push(name);
    }
    names
}

fn stem_of(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}

fn write_one(folder: &Path, name: &str, output: &Output, attachments: &str) -> Result<(), String> {
    let path = folder.join(name);
    std::fs::write(&path, &output.main).map_err(|error| format!("{}: {error}", path.display()))?;
    for (file, bytes) in &output.attachments {
        let path = folder.join(attachments).join(file);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("{}: {error}", parent.display()))?;
        }
        std::fs::write(&path, bytes).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    Ok(())
}

#[must_use]
pub fn run(tool: &str, extension: &str, convert: Convert) -> ExitCode {
    let args = match arguments(tool, extension) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    let names = match &args.output {
        Some(output) => vec![output.file_name().map_or_else(
            || format!("output.{extension}"),
            |n| n.to_string_lossy().into_owned(),
        )],
        None => batch_names(&args.inputs, extension),
    };
    let folder = match (&args.output, &args.out_dir) {
        (Some(output), _) => output.parent().map_or_else(PathBuf::new, Path::to_path_buf),
        (None, Some(dir)) => dir.clone(),
        (None, None) => PathBuf::new(),
    };
    if let Some(dir) = &args.out_dir
        && let Err(error) = std::fs::create_dir_all(dir)
    {
        eprintln!("{tool}: {}: {error}", dir.display());
        return ExitCode::from(1);
    }
    let fonts = fonts(args.fonts.as_deref());
    let count = args.inputs.len();
    let mut zip = args.zip.as_ref().map(|_| convert_zip::ZipWriter::new());
    let mut failed = 0;
    for (at, (input, name)) in args.inputs.iter().zip(&names).enumerate() {
        let started = Instant::now();
        let attachments = format!("{}_files", stem_of(name));
        let request = Request {
            pages: args.pages.clone(),
            password: args.password.clone(),
            attachments: attachments.clone(),
        };
        let quiet = args.quiet;
        let mut pages = 0;
        let mut progress = |done: usize, total: usize| {
            pages = total;
            if !quiet && total > 1 {
                if count > 1 {
                    eprint!(
                        "\r{tool}: file {} of {count}, page {done} of {total}   ",
                        at + 1
                    );
                } else {
                    eprint!("\r{tool}: page {done} of {total}   ");
                }
                if done == total {
                    eprintln!();
                }
            }
            true
        };
        let result = std::fs::read(input)
            .map_err(|error| format!("{}: {error}", input.display()))
            .and_then(|pdf| convert(pdf, &request, fonts.clone(), &mut progress));
        let done = match result {
            Ok(output) => Done {
                name: name.clone(),
                output,
                pages,
                seconds: started.elapsed().as_secs_f64(),
            },
            Err(why) => {
                failed += 1;
                eprintln!("{tool}: FAILED {}: {why}", input.display());
                if args.stats {
                    eprintln!(
                        "{{\"tool\":\"{tool}\",\"file\":{},\"ok\":false,\"error\":{}}}",
                        json_string(&input.to_string_lossy()),
                        json_string(&why)
                    );
                }
                continue;
            }
        };
        let written = match zip.as_mut() {
            Some(zip) => {
                let mut result = zip
                    .add(&done.name, &done.output.main, convert_zip::Method::Deflated)
                    .map_err(|error| error.to_string());
                for (file, bytes) in &done.output.attachments {
                    if result.is_ok() {
                        result = zip
                            .add(
                                &format!("{attachments}/{file}"),
                                bytes,
                                convert_zip::Method::Stored,
                            )
                            .map_err(|error| error.to_string());
                    }
                }
                result
            }
            None => write_one(&folder, &done.name, &done.output, &attachments),
        };
        if let Err(why) = written {
            failed += 1;
            eprintln!("{tool}: FAILED {}: {why}", input.display());
            continue;
        }
        if !args.quiet {
            if count > 1 {
                eprintln!(
                    "{tool}: ok {} -> {} ({} pages, {:.2} s)",
                    input.display(),
                    done.name,
                    done.pages,
                    done.seconds
                );
            }
            for note in &done.output.notes {
                eprintln!("{tool}: note: {}: {note}", input.display());
            }
        }
        if args.stats {
            eprintln!(
                "{{\"tool\":\"{tool}\",\"file\":{},\"ok\":true,\"pages\":{},\"seconds\":{:.3},\"peak_bytes\":{},\"output_bytes\":{},\"notes\":{},\"dropped\":{}}}",
                json_string(&input.to_string_lossy()),
                done.pages,
                done.seconds,
                peak_memory().unwrap_or(0),
                done.output.main.len(),
                done.output.notes.len(),
                json_string(&done.output.dropped)
            );
        }
    }
    if let (Some(zip), Some(path)) = (zip, &args.zip) {
        match zip.finish() {
            Ok(bytes) => {
                if let Err(error) = std::fs::write(path, bytes) {
                    eprintln!("{tool}: {}: {error}", path.display());
                    return ExitCode::from(1);
                }
            }
            Err(error) => {
                eprintln!("{tool}: {}: {error}", path.display());
                return ExitCode::from(1);
            }
        }
    }
    if failed > 0 {
        if count > 1 {
            eprintln!("{tool}: {failed} of {count} files failed");
        }
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

fn bytes_usage(tool: &str) -> String {
    format!(
        "usage: {tool} IN... [--out FILE | --out-dir DIR | --zip OUT.zip] [--set KEY=VALUE]... [--fonts DIR|none] [--stats] [--quiet]"
    )
}

#[must_use]
pub fn run_bytes_tool(tool: &str, run: convert_structure::bytes_tool::Run) -> ExitCode {
    use convert_structure::bytes_tool::{Input, Settings};
    let mut args = std::env::args_os().skip(1);
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut out: Option<PathBuf> = None;
    let mut out_dir: Option<PathBuf> = None;
    let mut zip_path: Option<PathBuf> = None;
    let mut settings = Settings::default();
    let mut fonts_dir: Option<PathBuf> = None;
    let mut stats = false;
    let mut quiet = false;
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or_else(|| bytes_usage(tool));
        let parsed: Result<(), String> = match arg.to_str() {
            Some("--out") => value().map(|v| out = Some(PathBuf::from(v))),
            Some("--out-dir") => value().map(|v| out_dir = Some(PathBuf::from(v))),
            Some("--zip") => value().map(|v| zip_path = Some(PathBuf::from(v))),
            Some("--fonts") => value().map(|v| fonts_dir = Some(PathBuf::from(v))),
            Some("--set") => value().and_then(|v| {
                let v = v.to_string_lossy().into_owned();
                let (key, val) = v
                    .split_once('=')
                    .ok_or_else(|| format!("--set {v}: KEY=VALUE"))?;
                settings.set(key.trim(), val.trim());
                Ok(())
            }),
            Some("--stats") => {
                stats = true;
                Ok(())
            }
            Some("--quiet") => {
                quiet = true;
                Ok(())
            }
            Some("-h" | "--help") => Err(bytes_usage(tool)),
            _ => {
                paths.push(PathBuf::from(arg));
                Ok(())
            }
        };
        if let Err(message) = parsed {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    }
    if paths.is_empty()
        || [out.is_some(), out_dir.is_some(), zip_path.is_some()]
            .iter()
            .filter(|x| **x)
            .count()
            > 1
    {
        eprintln!("{}", bytes_usage(tool));
        return ExitCode::from(2);
    }
    let started = Instant::now();
    let mut inputs = Vec::with_capacity(paths.len());
    for path in &paths {
        match std::fs::read(path) {
            Ok(bytes) => inputs.push(Input {
                name: path
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
                bytes,
            }),
            Err(error) => {
                eprintln!("{tool}: {}: {error}", path.display());
                return ExitCode::from(1);
            }
        }
    }
    let fonts = convert_structure::bytes_tool::Fonts {
        provider: fonts(fonts_dir.as_deref()),
        files: font_files(fonts_dir.as_deref()),
    };
    let mut progress = |done: usize, total: usize| {
        if !quiet && total > 1 {
            eprint!("\r{tool}: {done} of {total}   ");
            if done == total {
                eprintln!();
            }
        }
        true
    };
    let made = match run(&inputs, &settings, &fonts, &mut progress) {
        Ok(made) => made,
        Err(why) => {
            eprintln!("{tool}: {why}");
            return ExitCode::from(1);
        }
    };
    let written: Result<(), String> = if let Some(path) = &zip_path {
        let mut zip = convert_zip::ZipWriter::new();
        made.files
            .iter()
            .try_for_each(|(name, bytes)| {
                zip.add(name, bytes, convert_zip::Method::Deflated)
                    .map_err(|e| e.to_string())
            })
            .and_then(|()| zip.finish().map_err(|e| e.to_string()))
            .and_then(|bytes| {
                std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
            })
    } else if let Some(path) = &out {
        match made.files.as_slice() {
            [(_, bytes)] => {
                std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
            }
            files => Err(format!(
                "{} files were made; use --out-dir or --zip",
                files.len()
            )),
        }
    } else {
        let folder = out_dir.unwrap_or_default();
        std::fs::create_dir_all(if folder.as_os_str().is_empty() {
            Path::new(".")
        } else {
            &folder
        })
        .map_err(|e| format!("{}: {e}", folder.display()))
        .and_then(|()| {
            made.files.iter().try_for_each(|(name, bytes)| {
                let path = folder.join(name);
                std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))
            })
        })
    };
    if let Err(why) = written {
        eprintln!("{tool}: {why}");
        return ExitCode::from(1);
    }
    if !quiet {
        for note in &made.notes {
            eprintln!("{tool}: note: {note}");
        }
    }
    if stats {
        let bytes: usize = made.files.iter().map(|(_, b)| b.len()).sum();
        eprintln!(
            "{{\"tool\":\"{tool}\",\"ok\":true,\"inputs\":{},\"outputs\":{},\"seconds\":{:.3},\"peak_bytes\":{},\"output_bytes\":{bytes}}}",
            inputs.len(),
            made.files.len(),
            started.elapsed().as_secs_f64(),
            peak_memory().unwrap_or(0)
        );
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_lists() {
        assert_eq!(parse_pages("1,3-5, 9").unwrap(), vec![0, 2, 3, 4, 8]);
        assert!(parse_pages("0").is_err());
        assert!(parse_pages("5-3").is_err());
        assert!(parse_pages("x").is_err());
        assert!(parse_pages("").unwrap().is_empty());
    }

    #[test]
    fn manifest_faces() {
        let manifest = r#"{"faces": [{"file": "A.ttf", "sha256": "ab", "face_index": 2}, {"file": "B.ttf", "sha256": "cd"}]}"#;
        let faces = packaged_faces(Path::new("/x"), manifest);
        assert_eq!(faces.len(), 2);
        assert_eq!(faces[0].face_index, 2);
        assert_eq!(faces[1].path, Path::new("/x/B.ttf"));
    }
}
