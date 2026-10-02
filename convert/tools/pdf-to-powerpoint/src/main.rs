pub fn main() -> std::process::ExitCode {
    convert_cli::run("pdf-to-powerpoint", "pptx", pdf_to_powerpoint::convert)
}
