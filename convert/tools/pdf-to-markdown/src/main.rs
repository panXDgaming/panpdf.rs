pub fn main() -> std::process::ExitCode {
    convert_cli::run("pdf-to-markdown", "md", pdf_to_markdown::convert)
}
