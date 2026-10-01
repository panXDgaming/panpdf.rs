pub fn main() -> std::process::ExitCode {
    convert_cli::run("pdf-to-word", "docx", pdf_to_word::convert)
}
