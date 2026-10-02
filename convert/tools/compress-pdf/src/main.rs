pub fn main() -> std::process::ExitCode {
    convert_files::cli::run(
        &convert_files::cli::Help {
            tool: "compress-pdf",
            what: "a PDF made smaller (writes NAME-compressed.pdf)",
            options: "  --level low|recommended|extreme   (default recommended)
      low: lossless (unused objects dropped, duplicates merged, object streams)
      recommended: + pictures above 150 dpi scaled to 150 dpi, JPEG quality 75
      extreme: + pictures above 100 dpi scaled to 100 dpi, JPEG quality 50
  --password PW",
            flags: &[],
        },
        compress_pdf::start,
    )
}
