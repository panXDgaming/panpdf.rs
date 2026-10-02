pub fn main() -> std::process::ExitCode {
    convert_cli::run("pdf-to-excel", "xlsx", pdf_to_excel::convert)
}
