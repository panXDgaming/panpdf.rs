pub fn main() -> std::process::ExitCode {
    sign_pdf::set_fonts(convert_cli::fonts(None));
    convert_pdftool::cli::run(&sign_pdf::TOOL)
}
