pub fn main() -> std::process::ExitCode {
    convert_files::cli::run(
        &convert_files::cli::Help {
            tool: "ocr-pdf",
            what: "scanned pages made searchable (writes NAME-ocr.pdf; needs Tesseract)",
            options: "  --languages lao+tha+eng   Tesseract languages (default lao+tha+eng; Lao and Thai are never read together)
  --pages 1,3-5             which pages
  --force                   read pages that already have text too
  --password PW
Tesseract is looked for beside the program (ocr/), in PANPDF_TESSERACT, and on PATH;
its languages in PANPDF_TESSDATA.",
            flags: &["force"],
        },
        ocr_pdf::start,
    )
}
