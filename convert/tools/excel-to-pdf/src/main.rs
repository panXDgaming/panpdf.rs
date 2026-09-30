pub fn main() -> std::process::ExitCode {
    convert_cli::run_bytes_tool("excel-to-pdf", excel_to_pdf::run)
}
