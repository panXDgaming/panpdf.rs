pub const A_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
pub const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
pub const P_NS: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";

#[must_use]
pub fn theme(lao: &str) -> String {
    let colors = [
        ("dk1", "<a:sysClr val=\"windowText\" lastClr=\"000000\"/>"),
        ("lt1", "<a:sysClr val=\"window\" lastClr=\"FFFFFF\"/>"),
        ("dk2", "<a:srgbClr val=\"44546A\"/>"),
        ("lt2", "<a:srgbClr val=\"E7E6E6\"/>"),
        ("accent1", "<a:srgbClr val=\"4472C4\"/>"),
        ("accent2", "<a:srgbClr val=\"ED7D31\"/>"),
        ("accent3", "<a:srgbClr val=\"A5A5A5\"/>"),
        ("accent4", "<a:srgbClr val=\"FFC000\"/>"),
        ("accent5", "<a:srgbClr val=\"5B9BD5\"/>"),
        ("accent6", "<a:srgbClr val=\"70AD47\"/>"),
        ("hlink", "<a:srgbClr val=\"0563C1\"/>"),
        ("folHlink", "<a:srgbClr val=\"954F72\"/>"),
    ];
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<a:theme xmlns:a=\"{A_NS}\" name=\"PanPDF\"><a:themeElements><a:clrScheme name=\"Office\">"
    );
    for (name, value) in colors {
        out.push_str(&format!("<a:{name}>{value}</a:{name}>"));
    }
    out.push_str("</a:clrScheme><a:fontScheme name=\"PanPDF\">");
    for kind in ["majorFont", "minorFont"] {
        out.push_str(&format!(
            "<a:{kind}><a:latin typeface=\"Calibri\"/><a:ea typeface=\"\"/><a:cs typeface=\"{lao}\"/></a:{kind}>"
        ));
    }
    out.push_str("</a:fontScheme><a:fmtScheme name=\"Office\"><a:fillStyleLst>");
    for _ in 0..3 {
        out.push_str("<a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>");
    }
    out.push_str("</a:fillStyleLst><a:lnStyleLst>");
    for width in [6350, 12700, 19050] {
        out.push_str(&format!(
            "<a:ln w=\"{width}\" cap=\"flat\" cmpd=\"sng\" algn=\"ctr\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:prstDash val=\"solid\"/><a:miter lim=\"800000\"/></a:ln>"
        ));
    }
    out.push_str("</a:lnStyleLst><a:effectStyleLst>");
    for _ in 0..3 {
        out.push_str("<a:effectStyle><a:effectLst/></a:effectStyle>");
    }
    out.push_str("</a:effectStyleLst><a:bgFillStyleLst>");
    for _ in 0..3 {
        out.push_str("<a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>");
    }
    out.push_str("</a:bgFillStyleLst></a:fmtScheme></a:themeElements><a:objectDefaults/><a:extraClrSchemeLst/></a:theme>");
    out
}

const TREE_START: &str = "<p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>";

#[must_use]
pub fn tree_start() -> &'static str {
    TREE_START
}

#[must_use]
pub fn master() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<p:sldMaster xmlns:a=\"{A_NS}\" xmlns:r=\"{R_NS}\" xmlns:p=\"{P_NS}\"><p:cSld><p:bg><p:bgRef idx=\"1001\"><a:schemeClr val=\"bg1\"/></p:bgRef></p:bg><p:spTree>{TREE_START}</p:spTree></p:cSld><p:clrMap bg1=\"lt1\" tx1=\"dk1\" bg2=\"lt2\" tx2=\"dk2\" accent1=\"accent1\" accent2=\"accent2\" accent3=\"accent3\" accent4=\"accent4\" accent5=\"accent5\" accent6=\"accent6\" hlink=\"hlink\" folHlink=\"folHlink\"/><p:sldLayoutIdLst><p:sldLayoutId id=\"2147483649\" r:id=\"rId1\"/></p:sldLayoutIdLst></p:sldMaster>"
    )
}

#[must_use]
pub fn layout() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<p:sldLayout xmlns:a=\"{A_NS}\" xmlns:r=\"{R_NS}\" xmlns:p=\"{P_NS}\" type=\"blank\" preserve=\"1\"><p:cSld name=\"Blank\"><p:spTree>{TREE_START}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"
    )
}

#[must_use]
pub fn properties() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<p:presentationPr xmlns:a=\"{A_NS}\" xmlns:r=\"{R_NS}\" xmlns:p=\"{P_NS}\"/>"
    )
}
