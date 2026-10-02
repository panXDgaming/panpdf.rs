#!/usr/bin/env python3
import os
import zipfile
HERE = os.path.dirname(os.path.abspath(__file__))
EMU = 12700
A = 'http://schemas.openxmlformats.org/drawingml/2006/main'
R = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships'
P = 'http://schemas.openxmlformats.org/presentationml/2006/main'
P_NS = f'xmlns:a="{A}" xmlns:r="{R}" xmlns:p="{P}"'
REL = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships/'
PKG_REL = 'http://schemas.openxmlformats.org/package/2006/relationships'
SW, SH = (12192000, 6858000)
BOX_ARABIC = (36, 30, 400, 40)
BOX_HEBREW = (36, 80, 400, 40)
BOX_LIST = (36, 130, 400, 170)
BOX_LTR = (480, 30, 400, 40)
BOX_LEFT = (480, 80, 400, 40)
BOX_NUMBER = (480, 130, 400, 40)
BOX_COLUMNS = (480, 190, 400, 120)
ARABIC = 'مرحبا بكم في PanPDF'
HEBREW = 'שלום עולם, ברוכים הבאים'
BULLETS = ['البند الأول', 'البند الثاني', 'البند الثالث']
NUMBERED = ['الخطوة الأولى', 'الخطوة الثانية']
LTR = 'The word سلام means peace'
LEFT = 'نص على اليسار'
NUMBER = 'عام 2026 هو عام جديد'
COLUMNS = ['العمود الأول يبدأ هنا', 'ويستمر في السطر الثاني', 'العمود الثاني هنا', 'والسطر الأخير']
TABLE = [['الاسم', 'الكمية', 'السعر'], ['FirstCol', 'SecondCol', 'ThirdCol'], ['قلم', '10', '25,000']]

def esc(text):
    return str(text).replace('&', '&amp;').replace('<', '&lt;').replace('>', '&gt;').replace('"', '&quot;')

def run(text, sz=2000):
    return f'<a:r><a:rPr lang="ar-SA" sz="{sz}" dirty="0"/><a:t>{esc(text)}</a:t></a:r>'

def para(text, ppr='', sz=2000):
    return f'<a:p>{ppr}{run(text, sz)}</a:p>'

def box(i, name, rect, paras, body=''):
    x, y, w, h = (v * EMU for v in rect)
    body = body if 'rtlCol' in body else ' rtlCol="0"' + body
    return f'<p:sp><p:nvSpPr><p:cNvPr id="{i}" name="{name}"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{w}" cy="{h}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/><a:ln w="3175"><a:solidFill><a:srgbClr val="BBBBBB"/></a:solidFill></a:ln></p:spPr><p:txBody><a:bodyPr wrap="square"{body}><a:noAutofit/></a:bodyPr><a:lstStyle/>{''.join(paras)}</p:txBody></p:sp>'

def table(i, x, y, rows, widths):
    grid = ''.join((f'<a:gridCol w="{w * EMU}"/>' for w in widths))
    trs = ''
    for r, row in enumerate(rows):
        tcs = ''
        for text in row:
            fill = '1F5FBF' if r == 0 else 'EEF2FA'
            color = ' <a:solidFill><a:srgbClr val="FFFFFF"/></a:solidFill>' if r == 0 else ''
            tcs += f'<a:tc><a:txBody><a:bodyPr/><a:lstStyle/><a:p><a:pPr algn="r" rtl="1"/><a:r><a:rPr lang="ar-SA" sz="1600">{color}</a:rPr><a:t>{esc(text)}</a:t></a:r></a:p></a:txBody><a:tcPr><a:solidFill><a:srgbClr val="{fill}"/></a:solidFill></a:tcPr></a:tc>'
        trs += f'<a:tr h="{30 * EMU}">{tcs}</a:tr>'
    return f'<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="{i}" name="Table"/><p:cNvGraphicFramePr><a:graphicFrameLocks noGrp="1"/></p:cNvGraphicFramePr><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x="{x * EMU}" y="{y * EMU}"/><a:ext cx="{sum(widths) * EMU}" cy="{30 * EMU * len(rows)}"/></p:xfrm><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/table"><a:tbl><a:tblPr rtl="1" firstRow="1" bandRow="1"/><a:tblGrid>{grid}</a:tblGrid>{trs}</a:tbl></a:graphicData></a:graphic></p:graphicFrame>'

def slide(shapes):
    return f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<p:sld {P_NS}><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>{''.join(shapes)}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>'
RTL = '<a:pPr algn="r" rtl="1"/>'
BULLET = '<a:pPr marL="342900" indent="-342900" algn="r" rtl="1"><a:buFont typeface="Arial"/><a:buChar char="&#8226;"/></a:pPr>'
NUM = '<a:pPr marL="457200" indent="-457200" algn="r" rtl="1"><a:buFont typeface="+mj-lt"/><a:buAutoNum type="arabicPeriod"/></a:pPr>'
SLIDES = [slide([box(2, 'Arabic', BOX_ARABIC, [para(ARABIC, RTL)]), box(3, 'Hebrew', BOX_HEBREW, [para(HEBREW, RTL)]), box(4, 'List', BOX_LIST, [para(t, BULLET) for t in BULLETS] + [para(t, NUM) for t in NUMBERED]), box(5, 'Ltr', BOX_LTR, [para(LTR)]), box(6, 'Left', BOX_LEFT, [para(LEFT, '<a:pPr algn="l" rtl="1"/>')]), box(7, 'Number', BOX_NUMBER, [para(NUMBER, RTL)]), box(8, 'Columns', BOX_COLUMNS, [para(t, RTL) for t in COLUMNS], body=' numCol="2" spcCol="182880" rtlCol="1"')]), slide([table(2, 60, 60, TABLE, [150, 150, 150])])]
THEME = f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<a:theme xmlns:a="{A}" name="RTL"><a:themeElements>\n<a:clrScheme name="RTL"><a:dk1><a:srgbClr val="1D2433"/></a:dk1><a:lt1><a:srgbClr val="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="123C7A"/></a:dk2><a:lt2><a:srgbClr val="EEF2FA"/></a:lt2>\n<a:accent1><a:srgbClr val="1F5FBF"/></a:accent1><a:accent2><a:srgbClr val="D0343F"/></a:accent2><a:accent3><a:srgbClr val="F0A020"/></a:accent3><a:accent4><a:srgbClr val="1F7A3A"/></a:accent4>\n<a:accent5><a:srgbClr val="7A3A9A"/></a:accent5><a:accent6><a:srgbClr val="3A9A9A"/></a:accent6><a:hlink><a:srgbClr val="0563C1"/></a:hlink><a:folHlink><a:srgbClr val="954F72"/></a:folHlink></a:clrScheme>\n<a:fontScheme name="RTL"><a:majorFont><a:latin typeface="Liberation Sans"/><a:ea typeface=""/><a:cs typeface="DejaVu Sans"/></a:majorFont>\n<a:minorFont><a:latin typeface="Liberation Sans"/><a:ea typeface=""/><a:cs typeface="DejaVu Sans"/></a:minorFont></a:fontScheme>\n<a:fmtScheme name="RTL"><a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:fillStyleLst>\n<a:lnStyleLst><a:ln w="9525"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="19050"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="28575"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln></a:lnStyleLst>\n<a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst>\n<a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:bgFillStyleLst></a:fmtScheme>\n</a:themeElements></a:theme>'
MASTER = f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<p:sldMaster {P_NS}><p:cSld><p:bg><p:bgRef idx="1001"><a:schemeClr val="bg1"/></p:bgRef></p:bg><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/></p:spTree></p:cSld>\n<p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/>\n<p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst>\n<p:txStyles><p:titleStyle><a:lvl1pPr algn="l" rtl="0"><a:defRPr sz="4400"><a:solidFill><a:schemeClr val="tx1"/></a:solidFill><a:latin typeface="+mj-lt"/><a:cs typeface="+mj-cs"/></a:defRPr></a:lvl1pPr></p:titleStyle>\n<p:bodyStyle><a:lvl1pPr algn="l" rtl="0"><a:defRPr sz="2800"><a:solidFill><a:schemeClr val="tx1"/></a:solidFill><a:latin typeface="+mn-lt"/><a:cs typeface="+mn-cs"/></a:defRPr></a:lvl1pPr></p:bodyStyle>\n<p:otherStyle><a:lvl1pPr algn="l" rtl="0"><a:defRPr sz="1800"><a:solidFill><a:schemeClr val="tx1"/></a:solidFill></a:defRPr></a:lvl1pPr></p:otherStyle></p:txStyles></p:sldMaster>'
LAYOUT = f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<p:sldLayout {P_NS} type="blank" preserve="1"><p:cSld name="Blank"><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>'
DEFAULT_TEXT = ''.join((f'<a:lvl{n}pPr marL="{(n - 1) * 457200}" algn="l" defTabSz="914400" rtl="0" eaLnBrk="1" latinLnBrk="0" hangingPunct="1"><a:defRPr sz="1800" kern="1200"><a:solidFill><a:schemeClr val="tx1"/></a:solidFill><a:latin typeface="+mn-lt"/><a:cs typeface="+mn-cs"/></a:defRPr></a:lvl{n}pPr>' for n in range(1, 4)))

def rels(items):
    body = ''.join((f'<Relationship Id="{i}" Type="{REL}{t}" Target="{target}"/>' for i, t, target in items))
    return f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<Relationships xmlns="{PKG_REL}">{body}</Relationships>'

def main():
    parts = {}
    n = len(SLIDES)
    parts['ppt/presentation.xml'] = f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<p:presentation {P_NS}><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rIdM"/></p:sldMasterIdLst><p:sldIdLst>{''.join((f'<p:sldId id="{256 + i}" r:id="rIdS{i + 1}"/>' for i in range(n)))}</p:sldIdLst><p:sldSz cx="{SW}" cy="{SH}"/><p:notesSz cx="6858000" cy="9144000"/><p:defaultTextStyle>{DEFAULT_TEXT}</p:defaultTextStyle></p:presentation>'
    parts['ppt/_rels/presentation.xml.rels'] = rels([('rIdM', 'slideMaster', 'slideMasters/slideMaster1.xml'), ('rIdT', 'theme', 'theme/theme1.xml')] + [(f'rIdS{i + 1}', 'slide', f'slides/slide{i + 1}.xml') for i in range(n)])
    parts['ppt/theme/theme1.xml'] = THEME
    parts['ppt/slideMasters/slideMaster1.xml'] = MASTER
    parts['ppt/slideMasters/_rels/slideMaster1.xml.rels'] = rels([('rId1', 'slideLayout', '../slideLayouts/slideLayout1.xml'), ('rId2', 'theme', '../theme/theme1.xml')])
    parts['ppt/slideLayouts/slideLayout1.xml'] = LAYOUT
    parts['ppt/slideLayouts/_rels/slideLayout1.xml.rels'] = rels([('rId1', 'slideMaster', '../slideMasters/slideMaster1.xml')])
    for i, s in enumerate(SLIDES):
        parts[f'ppt/slides/slide{i + 1}.xml'] = s
        parts[f'ppt/slides/_rels/slide{i + 1}.xml.rels'] = rels([('rId1', 'slideLayout', '../slideLayouts/slideLayout1.xml')])
    slides_ct = ''.join((f'<Override PartName="/ppt/slides/slide{i + 1}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>' for i in range(n)))
    parts['[Content_Types].xml'] = f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/><Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/><Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/><Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/>{slides_ct}</Types>'
    parts['_rels/.rels'] = rels([('rId1', 'officeDocument', 'ppt/presentation.xml')])
    out = os.path.join(HERE, 'rtl.pptx')
    order = ['[Content_Types].xml', '_rels/.rels'] + sorted((k for k in parts if k not in ('[Content_Types].xml', '_rels/.rels')))
    with zipfile.ZipFile(out, 'w', zipfile.ZIP_DEFLATED) as z:
        for name in order:
            info = zipfile.ZipInfo(name, date_time=(2026, 9, 25, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, parts[name].encode('utf-8'))
    print(out)
if __name__ == '__main__':
    main()
