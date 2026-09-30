use std::path::{Path, PathBuf};
use std::process::ExitCode;

pub fn main() -> ExitCode {
    let mut inputs: Vec<PathBuf> = Vec::new();
    let mut out: Option<PathBuf> = None;
    let mut out_dir: Option<PathBuf> = None;
    let mut fonts_dir: Option<PathBuf> = None;
    let mut quiet = false;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--out" | "-o") => out = args.next().map(PathBuf::from),
            Some("--out-dir") => out_dir = args.next().map(PathBuf::from),
            Some("--fonts") => fonts_dir = args.next().map(PathBuf::from),
            Some("--quiet" | "-q") => quiet = true,
            Some("--help" | "-h") => {
                println!(
                    "usage: html-to-pdf PAGE.html... [--out OUT.pdf | --out-dir DIR] [--fonts DIR|none]"
                );
                return ExitCode::SUCCESS;
            }
            _ => inputs.push(PathBuf::from(arg)),
        }
    }
    if inputs.is_empty() || (out.is_some() && inputs.len() > 1) {
        eprintln!(
            "usage: html-to-pdf PAGE.html... [--out OUT.pdf | --out-dir DIR] [--fonts DIR|none]"
        );
        return ExitCode::from(2);
    }
    if let Some(dir) = &out_dir
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        eprintln!("html-to-pdf: {}: {e}", dir.display());
        return ExitCode::from(1);
    }
    let fonts = convert_cli::fonts(fonts_dir.as_deref());
    let mut failed = 0;
    for input in &inputs {
        let target = out.clone().unwrap_or_else(|| {
            let name = input.with_extension("pdf");
            match &out_dir {
                Some(dir) => dir.join(name.file_name().unwrap_or_default()),
                None => name,
            }
        });
        let folder = input.parent().map(Path::to_path_buf).unwrap_or_default();
        let assets = move |name: &str| -> Option<Vec<u8>> {
            let name = name.split(['?', '#']).next().unwrap_or(name);
            let name = html_to_pdf::percent_decode(name.strip_prefix("file://").unwrap_or(name));
            std::fs::read(folder.join(name)).ok()
        };
        let started = std::time::Instant::now();
        let result = std::fs::read(input)
            .map_err(|e| format!("{}: {e}", input.display()))
            .and_then(|bytes| html_to_pdf::convert(&bytes, &assets, fonts.clone()));
        match result {
            Ok((pdf, notes)) => {
                if let Err(e) = std::fs::write(&target, pdf) {
                    eprintln!("html-to-pdf: {}: {e}", target.display());
                    failed += 1;
                    continue;
                }
                if !quiet {
                    for note in notes {
                        eprintln!("html-to-pdf: {}: {note}", input.display());
                    }
                    eprintln!(
                        "html-to-pdf: {} -> {} ({:.2} s)",
                        input.display(),
                        target.display(),
                        started.elapsed().as_secs_f64()
                    );
                }
            }
            Err(e) => {
                eprintln!("html-to-pdf: FAILED {}: {e}", input.display());
                failed += 1;
            }
        }
    }
    if failed > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
