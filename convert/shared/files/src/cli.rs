use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use crate::{Input, Outcome, Settings, Start, human_size};

pub struct Help {
    pub tool: &'static str,
    pub what: &'static str,
    pub options: &'static str,
    pub flags: &'static [&'static str],
}

fn usage(help: &Help) -> String {
    format!(
        "{} -- {}\nusage: {} IN... [-o OUT] [options]\n{}\n  --stats            time, memory and sizes as JSON on stderr\n  --quiet            no progress",
        help.tool, help.what, help.tool, help.options
    )
}

struct Arguments {
    inputs: Vec<PathBuf>,
    out: Option<PathBuf>,
    settings: Settings,
    stats: bool,
    quiet: bool,
}

fn arguments(help: &Help) -> Result<Arguments, String> {
    let mut args = std::env::args_os().skip(1);
    let mut parsed = Arguments {
        inputs: Vec::new(),
        out: None,
        settings: Settings::default(),
        stats: false,
        quiet: false,
    };
    while let Some(arg) = args.next() {
        let text = arg.to_string_lossy().into_owned();
        match text.as_str() {
            "-o" | "--out" => {
                parsed.out = Some(PathBuf::from(args.next().ok_or_else(|| usage(help))?));
            }
            "--stats" => parsed.stats = true,
            "--quiet" => parsed.quiet = true,
            "-h" | "--help" => return Err(usage(help)),
            _ => {
                if let Some(key) = text.strip_prefix("--") {
                    let (key, value) = match key.split_once('=') {
                        Some((k, v)) => (k.to_owned(), v.to_owned()),
                        None if help.flags.contains(&key) => (key.to_owned(), "true".to_owned()),
                        None => {
                            let value = args
                                .next()
                                .ok_or_else(|| format!("--{key} needs a value\n{}", usage(help)))?;
                            (key.to_owned(), value.to_string_lossy().into_owned())
                        }
                    };
                    parsed.settings.0.push((key, value));
                } else {
                    parsed.inputs.push(PathBuf::from(arg));
                }
            }
        }
    }
    if parsed.inputs.is_empty() {
        return Err(usage(help));
    }
    Ok(parsed)
}

#[must_use]
pub fn peak_memory() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|line| line.starts_with("VmHWM:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb * 1024)
}

pub fn write_outcome(outcome: &Outcome, out: &Path) -> Result<Vec<PathBuf>, String> {
    let is_zip = out
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"));
    if is_zip {
        let mut zip = convert_zip::ZipWriter::new();
        for (name, bytes) in &outcome.files {
            zip.add(name, bytes, convert_zip::Method::Deflated)
                .map_err(|e| e.to_string())?;
        }
        let bytes = zip.finish().map_err(|e| e.to_string())?;
        std::fs::write(out, bytes).map_err(|e| format!("{}: {e}", out.display()))?;
        return Ok(vec![out.to_path_buf()]);
    }
    if outcome.files.len() == 1 && !out.is_dir() && !out.to_string_lossy().ends_with('/') {
        if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(out, &outcome.files[0].1).map_err(|e| format!("{}: {e}", out.display()))?;
        return Ok(vec![out.to_path_buf()]);
    }
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut written = Vec::new();
    for (name, bytes) in &outcome.files {
        let path = out.join(name);
        std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        written.push(path);
    }
    Ok(written)
}

fn json(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[must_use]
pub fn run(help: &Help, start: Start) -> ExitCode {
    let args = match arguments(help) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    let started = Instant::now();
    let mut inputs = Vec::new();
    for path in &args.inputs {
        match std::fs::read(path) {
            Ok(bytes) => inputs.push(Input {
                name: path
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
                bytes,
            }),
            Err(error) => {
                eprintln!("{}: {}: {error}", help.tool, path.display());
                return ExitCode::from(1);
            }
        }
    }
    let input_bytes: usize = inputs.iter().map(|i| i.bytes.len()).sum();
    let out = args.out.clone().unwrap_or_else(|| {
        args.inputs[0]
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
    });
    let result = start(inputs, &args.settings).and_then(|mut job| {
        let total = job.total();
        for done in 0..total {
            job.step()?;
            if !args.quiet && total > 1 {
                eprint!("\r{}: {} of {total}   ", help.tool, done + 1);
            }
        }
        if !args.quiet && total > 1 {
            eprintln!();
        }
        job.finish()
    });
    let outcome = match result {
        Ok(outcome) => outcome,
        Err(why) => {
            eprintln!("{}: {why}", help.tool);
            if args.stats {
                eprintln!(
                    "{{\"tool\":\"{}\",\"ok\":false,\"error\":{}}}",
                    help.tool,
                    json(&why)
                );
            }
            return ExitCode::from(1);
        }
    };
    let written = match write_outcome(&outcome, &out) {
        Ok(written) => written,
        Err(why) => {
            eprintln!("{}: {why}", help.tool);
            return ExitCode::from(1);
        }
    };
    let output_bytes: usize = outcome.files.iter().map(|f| f.1.len()).sum();
    if !args.quiet {
        for note in &outcome.notes {
            eprintln!("{}: {note}", help.tool);
        }
        eprintln!(
            "{}: {} file(s), {} -> {} in {:.2} s",
            help.tool,
            written.len(),
            human_size(input_bytes),
            human_size(output_bytes),
            started.elapsed().as_secs_f64()
        );
    }
    if args.stats {
        eprintln!(
            "{{\"tool\":\"{}\",\"ok\":true,\"inputs\":{},\"input_bytes\":{input_bytes},\"files\":{},\"output_bytes\":{output_bytes},\"seconds\":{:.3},\"peak_bytes\":{}}}",
            help.tool,
            args.inputs.len(),
            outcome.files.len(),
            started.elapsed().as_secs_f64(),
            peak_memory().unwrap_or(0)
        );
    }
    ExitCode::SUCCESS
}
