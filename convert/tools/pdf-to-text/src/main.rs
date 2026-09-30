pub fn main() -> std::process::ExitCode {
    convert_cli::run("pdf-to-text", "txt", pdf_to_text::convert)
}
