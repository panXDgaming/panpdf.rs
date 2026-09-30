pub fn main() -> std::process::ExitCode {
    convert_files::cli::run(
        &convert_files::cli::Help {
            tool: "scan-to-pdf",
            what: "photographs of paper to a PDF that looks scanned",
            options: "  --look colour|grey|bw|original   how the page is cleaned (default colour)
  --crop true|false              find the page's edges and cut it out (default true)
  --quality N                    JPEG quality 1-100 (default 80)
  --size a4|letter|fit           page size (default a4)
  --orientation auto|portrait|landscape
  --margin none|small|big|POINTS",
            flags: &[],
        },
        scan_to_pdf::start,
    )
}
