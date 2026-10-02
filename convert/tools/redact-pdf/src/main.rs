pub fn main() -> std::process::ExitCode {
    redact_pdf::set_fonts(convert_cli::fonts(None));
    convert_pdftool::cli::run(&redact_pdf::TOOL)
}
