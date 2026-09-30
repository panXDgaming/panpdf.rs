pub fn main() -> std::process::ExitCode {
    convert_files::cli::run(
        &convert_files::cli::Help {
            tool: "image-to-pdf",
            what: "JPG, PNG and GIF pictures to PDF, a page each",
            options: "  --size a4|letter|fit           page size (default a4; fit = the picture's own size)
  --orientation auto|portrait|landscape   (default auto: follows each picture)
  --margin none|small|big|POINTS (default none; small 20 pt, big 50 pt)
  --merge true|false             one PDF for all (default) or one per picture",
            flags: &[],
        },
        image_to_pdf::start,
    )
}
