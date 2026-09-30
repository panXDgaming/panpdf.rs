pub fn main() -> std::process::ExitCode {
    if let Some(fonts) = convert_cli::fonts(None) {
        pdf_to_image::use_fonts(fonts);
    }
    convert_files::cli::run(
        &convert_files::cli::Help {
            tool: "pdf-to-image",
            what: "PDF pages to JPG or PNG, or the pictures a PDF holds",
            options: "  --mode pages|extract   draw each page (default), or take out the pictures
  --format jpg|png       picture format (default jpg for pages, png for extract)
  --dpi N                resolution of drawn pages (default 150)
  --quality N            JPEG quality 1-100 (default 85)
  --pages 1,3-5          which pages
  --password PW          the PDF's password",
            flags: &[],
        },
        pdf_to_image::start,
    )
}
