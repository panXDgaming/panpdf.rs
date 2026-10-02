use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use crate::{Job, Output, Tool};

fn usage(tool: &Tool) -> String {
    let inputs = match tool.inputs {
        1 => "IN.pdf".to_owned(),
        n => (1..=n)
            .map(|i| format!("IN{i}.pdf"))
            .collect::<Vec<_>>()
            .join(" "),
    };
    let batch = if tool.inputs == 1 {
        format!("\n       {} IN.pdf... --out-dir DIR [options]", tool.name)
    } else {
        String::new()
    };
    format!(
        "usage: {} {inputs} OUT.{} [options]{batch}\noptions:\n{}\n  --quiet  --stats",
        tool.name, tool.extension, tool.help
    )
}

struct Arguments {
    positional: Vec<PathBuf>,
    out_dir: Option<PathBuf>,
    options: BTreeMap<String, String>,
    files: BTreeMap<String, Vec<u8>>,
    quiet: bool,
    stats: bool,
}

fn arguments(tool: &Tool) -> Result<Arguments, String> {
    let mut args = std::env::args_os().skip(1);
    let mut parsed = Arguments {
        positional: Vec::new(),
        out_dir: None,
        options: BTreeMap::new(),
        files: BTreeMap::new(),
        quiet: false,
        stats: false,
    };
    while let Some(arg) = args.next() {
        let text = arg.to_string_lossy().into_owned();
        let Some(option) = text.strip_prefix("--") else {
            if text == "-h" {
                return Err(usage(tool));
            }
            parsed.positional.push(PathBuf::from(arg));
            continue;
        };
        let (key, inline) = match option.split_once('=') {
            Some((k, v)) => (k.to_owned(), Some(v.to_owned())),
            None => (option.to_owned(), None),
        };
        match key.as_str() {
            "help" => return Err(usage(tool)),
            "quiet" => parsed.quiet = true,
            "stats" => parsed.stats = true,
            _ if tool.flags.contains(&key.as_str()) && inline.is_none() => {
                parsed.options.insert(key, "true".to_owned());
            }
            _ => {
                let value = match inline {
                    Some(value) => value,
                    None => args
                        .next()
                        .map(|v| v.to_string_lossy().into_owned())
                        .ok_or_else(|| format!("--{key} needs a value\n{}", usage(tool)))?,
                };
                if key == "out-dir" {
                    parsed.out_dir = Some(PathBuf::from(value));
                } else if tool.file_options.contains(&key.as_str()) {
                    let bytes = std::fs::read(&value)
                        .map_err(|error| format!("--{key} {value}: {error}"))?;
                    parsed.files.insert(key, bytes);
                } else {
                    parsed.options.insert(key, value);
                }
            }
        }
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

fn write_output(path: &Path, output: &Output) -> Result<(), String> {
    std::fs::write(path, &output.main).map_err(|e| format!("{}: {e}", path.display()))?;
    if output.attachments.is_empty() {
        return Ok(());
    }
    let stem = path
        .file_stem()
        .map_or_else(|| "output".into(), |s| s.to_string_lossy().into_owned());
    let folder = path
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join(format!("{stem}_files"));
    std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    for (name, bytes) in &output.attachments {
        let file = folder.join(name);
        std::fs::write(&file, bytes).map_err(|e| format!("{}: {e}", file.display()))?;
    }
    Ok(())
}

#[must_use]
pub fn run(tool: &Tool) -> ExitCode {
    let args = match arguments(tool) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    let jobs: Vec<(Vec<PathBuf>, PathBuf)> = match &args.out_dir {
        Some(dir) if tool.inputs == 1 && !args.positional.is_empty() => {
            if let Err(error) = std::fs::create_dir_all(dir) {
                eprintln!("{}: {}: {error}", tool.name, dir.display());
                return ExitCode::from(1);
            }
            let mut names: Vec<String> = Vec::new();
            args.positional
                .iter()
                .map(|input| {
                    let stem = input
                        .file_stem()
                        .map_or_else(|| "output".into(), |s| s.to_string_lossy().into_owned());
                    let mut name = format!("{stem}.{}", tool.extension);
                    let mut n = 2;
                    while names.contains(&name) {
                        name = format!("{stem}-{n}.{}", tool.extension);
                        n += 1;
                    }
                    names.push(name.clone());
                    (vec![input.clone()], dir.join(name))
                })
                .collect()
        }
        None if args.positional.len() == tool.inputs + 1 => {
            let mut positional = args.positional.clone();
            let output = positional.pop().unwrap_or_default();
            vec![(positional, output)]
        }
        _ => {
            eprintln!("{}", usage(tool));
            return ExitCode::from(2);
        }
    };
    let mut failed = 0;
    for (inputs, output) in &jobs {
        let started = Instant::now();
        let mut job = Job {
            inputs: Vec::new(),
            options: args.options.clone(),
            files: args.files.clone(),
            random: Vec::new(),
        };
        let mut read_error = None;
        for input in inputs {
            match std::fs::read(input) {
                Ok(bytes) => job.inputs.push(bytes),
                Err(error) => read_error = Some(format!("{}: {error}", input.display())),
            }
        }
        let name = inputs
            .first()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let result = match read_error {
            Some(why) => Err(why),
            None => (tool.run)(&job),
        };
        let result = result.and_then(|out| write_output(output, &out).map(|()| out));
        match result {
            Ok(out) => {
                if !args.quiet {
                    for note in &out.notes {
                        eprintln!("{}: {note}", tool.name);
                    }
                    if jobs.len() > 1 {
                        eprintln!("{}: ok {name} -> {}", tool.name, output.display());
                    }
                }
                if args.stats {
                    eprintln!(
                        "{{\"tool\":\"{}\",\"file\":\"{}\",\"ok\":true,\"seconds\":{:.3},\"peak_bytes\":{},\"output_bytes\":{},\"notes\":{}}}",
                        tool.name,
                        name.replace('\\', "\\\\").replace('"', "\\\""),
                        started.elapsed().as_secs_f64(),
                        peak_memory().unwrap_or(0),
                        out.main.len(),
                        out.notes.len()
                    );
                }
            }
            Err(why) => {
                failed += 1;
                eprintln!("{}: FAILED {name}: {why}", tool.name);
            }
        }
    }
    if failed > 0 {
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}
