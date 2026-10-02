pub fn main() -> std::process::ExitCode {
    convert_cli::run_bytes_tool("powerpoint-to-pdf", powerpoint_to_pdf::run)
}
