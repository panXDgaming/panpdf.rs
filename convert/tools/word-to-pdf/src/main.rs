pub fn main() -> std::process::ExitCode {
    convert_cli::run_bytes_tool("word-to-pdf", word_to_pdf::run)
}
