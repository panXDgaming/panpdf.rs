pub fn main() -> std::process::ExitCode {
    compare_pdf::set_fonts(convert_cli::fonts(None));
    convert_pdftool::cli::run(&compare_pdf::TOOL)
}
