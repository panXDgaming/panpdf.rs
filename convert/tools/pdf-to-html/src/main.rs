pub fn main() -> std::process::ExitCode {
    convert_cli::run("pdf-to-html", "html", pdf_to_html::convert)
}
