use super::*;

struct Boxes<'a>(&'a [Segment]);

impl TextPainter for Boxes<'_> {
    fn width(&self, index: usize) -> f32 {
        self.0[index]
            .pieces
            .iter()
            .map(|p| p.text.chars().count() as f32 * p.size * 0.5)
            .sum()
    }

    fn draw(&self, page: &mut Page, index: usize) {
        let w = self.width(index);
        page.rect(0.0, 0.0, w, 5.0).fill();
    }
}

fn render(svg: &str) -> (Drawing, String) {
    let drawing = Drawing::parse(svg.as_bytes()).unwrap();
    let mut canvas = convert_pdf_canvas::Canvas::new();
    let images: Vec<Option<ImageInfo>> = drawing
        .images()
        .iter()
        .map(|b| canvas.add_image(b).ok())
        .collect();
    let page = canvas.add_page(400.0, 400.0);
    drawing.draw(
        canvas.page(page),
        [10.0, 10.0, 300.0, 200.0],
        &Boxes(drawing.segments()),
        &images,
    );
    let content = canvas.page(page).content().to_owned();
    let pdf = canvas.finish().unwrap();
    assert!(pdf.starts_with(b"%PDF"));
    (drawing, content)
}

#[test]
fn sizes_come_from_attributes_and_view_box() {
    assert_eq!(
        size(b"<svg width='480' height='260'/>"),
        Some((480.0, 260.0))
    );
    assert_eq!(
        size(b"<svg width='10mm' viewBox='0 0 2 1'/>").map(|s| s.1.round()),
        Some(19.0)
    );
    assert_eq!(size(b"<svg viewBox='0 0 600 300'/>"), Some((300.0, 150.0)));
    assert_eq!(size(b"<svg width='100%'/>"), Some((300.0, 150.0)));
    assert!(size(b"<html/>").is_none());
    assert!(is_svg(b"\xef\xbb\xbf <?xml version='1.0'?><svg/>"));
    assert!(!is_svg(b"GIF89a"));
}

#[test]
fn shapes_paint_with_their_styles() {
    let (d, content) = render(
        r##"<svg width="300" height="200" viewBox="0 0 300 200">
        <style>.bar { fill: #1f5fbf } g.axis line { stroke: #333 }</style>
        <rect class="bar" x="10" y="10" width="50" height="80" rx="5"/>
        <g class="axis"><line x1="0" y1="100" x2="300" y2="100" stroke-width="2" stroke-dasharray="3 3"/></g>
        <circle cx="150" cy="50" r="20" fill="none" stroke="red" stroke-linecap="round"/>
        <path d="M200 20 h40 v40 z" fill="currentColor" color="green" fill-opacity=".5"/>
        <polygon points="10,150 50,190 90,150" fill-rule="evenodd" style="fill:rgb(255,0,0);stroke:blue"/>
        </svg>"##,
    );
    assert!(d.notes().is_empty(), "{:?}", d.notes());
    assert!(content.contains("0.1216 0.3725 0.749 rg"), "{content}");
    assert!(content.contains("[3 3] 0 d"));
    assert!(content.contains("1 J"));
    assert!(content.contains("0 0.502 0 rg"));
    assert!(
        content.contains("/A128_255 gs") || content.contains("/A127_255 gs"),
        "{content}"
    );
    assert!(content.contains("B*"));
}

#[test]
fn gradients_become_shading_patterns_and_masks() {
    let (_, content) = render(
        r##"<svg viewBox="0 0 100 100">
        <defs>
          <linearGradient id="a"><stop offset="0" stop-color="red"/><stop offset="1" stop-color="blue"/></linearGradient>
          <linearGradient id="b" href="#a" x2="0" y2="1"/>
          <radialGradient id="c"><stop offset="0%" style="stop-color:#fff;stop-opacity:1"/><stop offset="100%" stop-color="#000" stop-opacity="0"/></radialGradient>
        </defs>
        <rect width="50" height="50" fill="url(#a)"/>
        <rect y="50" width="50" height="50" fill="url(#b)" stroke="url(#a)"/>
        <circle cx="75" cy="75" r="20" fill="url(#c)"/>
        <rect x="60" width="10" height="10" fill="url(#missing) orange"/>
        </svg>"##,
    );
    assert!(content.contains("/Pattern cs /P0 scn"), "{content}");
    assert!(content.contains("/Pattern CS"));
    assert!(content.contains("/SM0 gs"));
    assert!(content.contains("1 0.6471 0 rg"));
}

#[test]
fn use_symbol_clip_and_nested_svg() {
    let (d, content) = render(
        r##"<svg viewBox="0 0 100 100">
        <defs>
          <symbol id="s" viewBox="0 0 10 10"><rect width="10" height="10" fill="teal"/></symbol>
          <clipPath id="k"><circle cx="50" cy="50" r="10"/></clipPath>
          <clipPath id="u" clipPathUnits="objectBoundingBox"><rect width=".5" height=".5"/></clipPath>
          <g id="loop"><use href="#loop"/></g>
        </defs>
        <use href="#s" x="5" y="5" width="20" height="20"/>
        <use xlink:href="#loop"/>
        <g clip-path="url(#k)" transform="rotate(45 50 50)"><rect width="100" height="100"/></g>
        <rect x="10" y="60" width="20" height="20" clip-path="url(#u)"/>
        <svg x="60" y="60" width="30" height="30" viewBox="0 0 3 3"><rect width="3" height="3" fill="navy"/></svg>
        <foreignObject/><animate/><rect filter="url(#f)" width="1" height="1"/>
        </svg>"##,
    );
    assert!(content.contains("W n"));
    assert!(content.contains("0 0.502 0.502 rg"));
    assert_eq!(
        d.notes(),
        [
            "animation (the first state is drawn)",
            "filters (drawn unfiltered)",
            "foreignObject"
        ]
    );
}

#[test]
fn text_is_gathered_into_chunks_and_segments() {
    let (d, content) = render(
        r#"<svg viewBox="0 0 300 100" font-family="Noto Sans Lao, sans-serif">
        <text x="150" y="20" font-size="15" text-anchor="middle" font-weight="bold">  ຍອດຂາຍ
           2026 </text>
        <text x="10 40" y="50">AB<tspan fill="red" dy="5">C</tspan> D</text>
        <text x="10" y="80" dominant-baseline="middle">x<tspan visibility="hidden">y</tspan>z</text>
        </svg>"#,
    );
    let segs = d.segments();
    assert_eq!(segs[0].pieces[0].text, "ຍອດຂາຍ 2026");
    assert!(segs[0].pieces[0].bold);
    assert_eq!(segs[0].pieces[0].family, "Noto Sans Lao, sans-serif");
    let texts: Vec<String> = segs
        .iter()
        .map(|s| s.pieces.iter().map(|p| p.text.as_str()).collect())
        .collect();
    assert_eq!(texts, ["ຍອດຂາຍ 2026", "A", "B", "C D", "x", "y", "z"]);
    assert_eq!(segs[3].pieces[0].colour, [255, 0, 0]);
    assert!(content.contains("1 0 0 -1 108.75 20 cm"), "{content}");
}

#[test]
fn embedded_pictures_are_placed() {
    let gif = "R0lGODlhAQABAIAAAP8AAAAAACH5BAEAAAAALAAAAAABAAEAAAICRAEAOw==";
    let svg = format!(
        r#"<svg viewBox="0 0 10 10"><image href="data:image/gif;base64,{gif}" width="10" height="5"/><image href="missing.png"/></svg>"#
    );
    let (d, content) = render(&svg);
    assert_eq!(d.images().len(), 1);
    assert!(content.contains("/Im0 Do"), "{content}");
    assert_eq!(
        d.notes(),
        ["pictures not embedded in the drawing or not found"]
    );
    let embedded = embed_files(br#"<svg><image xlink:href="a.gif"/></svg>"#, &|name| {
        (name == "a.gif").then(|| base64(gif).unwrap())
    });
    assert!(
        String::from_utf8(embedded)
            .unwrap()
            .contains("data:image/gif;base64,R0lG")
    );
}

fn rounds(normal: usize) -> usize {
    std::env::var("FUZZ_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(normal)
}

#[test]
fn fuzz_does_not_panic() {
    let seeds = [
        include_str!("tests/sample.svg"),
        r##"<svg viewBox="0 0 10 10"><defs><linearGradient id="g" href="#g"/></defs><path d="M0 0 A5 5 0 1 1 10 10 Q 1 1 2 2 T 5 5 S 1 1 2 2 C 1 1 1 1 1 1z" fill="url(#g)" stroke="url(#g)"/><text x="1 2 3" dx="1em 2%" y="5">abc<tspan x="1">d</tspan></text></svg>"##,
    ];
    let mut state = 0x9E37_79B9_u32;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    };
    let alphabet: &[u8] =
        b"<>/=\"' #.-0123456789eEmMlLhHvVcCsSqQtTaAzZ,()%svgrectpathgtextuseidhrefurlfill";
    for seed in seeds {
        let bytes = seed.as_bytes();
        for cut in (0..bytes.len()).step_by(7) {
            let _ = Drawing::parse(&bytes[..cut]).map(|d| render_quietly(&d));
        }
        for _ in 0..rounds(400) {
            let mut b = bytes.to_vec();
            for _ in 0..(next() % 8 + 1) {
                let at = next() as usize % b.len();
                match next() % 3 {
                    0 => b[at] = alphabet[next() as usize % alphabet.len()],
                    1 => {
                        b.remove(at);
                    }
                    _ => b.insert(at, alphabet[next() as usize % alphabet.len()]),
                }
            }
            let _ = size(&b);
            if let Ok(d) = Drawing::parse(&b) {
                render_quietly(&d);
            }
        }
    }
}

fn render_quietly(d: &Drawing) {
    let mut canvas = convert_pdf_canvas::Canvas::new();
    let page = canvas.add_page(100.0, 100.0);
    d.draw(
        canvas.page(page),
        [0.0, 0.0, 100.0, 100.0],
        &Boxes(d.segments()),
        &[],
    );
    let _ = canvas.finish();
}
