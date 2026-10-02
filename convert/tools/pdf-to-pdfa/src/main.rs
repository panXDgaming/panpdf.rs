pub fn main() -> std::process::ExitCode {
    convert_pdftool::cli::run(&pdf_to_pdfa::TOOL)
}
