#!/usr/bin/env python3
import os
import struct
import zipfile
import zlib
HERE = os.path.dirname(os.path.abspath(__file__))
EMU_IN = 914400
NS_MAIN = 'http://schemas.openxmlformats.org/spreadsheetml/2006/main'
NS_R = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships'
NS_A = 'http://schemas.openxmlformats.org/drawingml/2006/main'
NS_C = 'http://schemas.openxmlformats.org/drawingml/2006/chart'
NS_XDR = 'http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing'
REL = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships/'
PKG_REL = 'http://schemas.openxmlformats.org/package/2006/relationships'

def esc(text):
    return str(text).replace('&', '&amp;').replace('<', '&lt;').replace('>', '&gt;').replace('"', '&quot;')

def png(width, height):
    rows = b''
    for y in range(height):
        row = b'\x00'
        for x in range(width):
            if abs(x * height - y * width) < width:
                row += b'\xff\xff\xff'
            else:
                t = x / max(1, width - 1)
                row += bytes((int(68 + (237 - 68) * t), int(114 + (125 - 114) * t), int(196 + (49 - 196) * t)))
        rows += row

    def chunk(kind, data):
        body = kind + data
        return struct.pack('>I', len(data)) + body + struct.pack('>I', zlib.crc32(body) & 4294967295)
    header = struct.pack('>IIBBBBB', width, height, 8, 2, 0, 0, 0)
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', header) + chunk(b'IDAT', zlib.compress(rows, 9)) + chunk(b'IEND', b'')

def col_name(c):
    out = ''
    c += 1
    while c:
        c, r = divmod(c - 1, 26)
        out = chr(65 + r) + out
    return out

def cell(ref, value, style=0):
    s = f' s="{style}"' if style else ''
    if isinstance(value, tuple):
        return f'<c r="{ref}" t="e"{s}><v>{esc(value[1])}</v></c>'
    if isinstance(value, str):
        return f'<c r="{ref}" t="inlineStr"{s}><is><t xml:space="preserve">{esc(value)}</t></is></c>'
    return f'<c r="{ref}"{s}><v>{value}</v></c>'

def sheet_xml(rows, extra_before='', extra_after='', cols='', row_attrs=None, pr=''):
    row_attrs = row_attrs or {}
    body = []
    all_rows = sorted(set(rows) | set(row_attrs))
    for r in all_rows:
        cells = rows.get(r, {})
        attrs = row_attrs.get(r, '')
        inner = ''.join((cell(f'{col_name(c)}{r + 1}', v) for c, v in sorted(cells.items())))
        body.append(f'<row r="{r + 1}"{attrs}>{inner}</row>')
    return f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<worksheet xmlns="{NS_MAIN}" xmlns:r="{NS_R}">{pr}<sheetViews><sheetView workbookViewId="0"/></sheetViews><sheetFormatPr defaultRowHeight="15"/>{cols}<sheetData>{''.join(body)}</sheetData>{extra_before}<pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/>{extra_after}<drawing r:id="rId1"/></worksheet>'

def str_cache(ref, values):
    pts = ''.join((f'<c:pt idx="{k}"><c:v>{esc(v)}</c:v></c:pt>' for k, v in enumerate(values)))
    return f'<c:strRef><c:f>{ref}</c:f><c:strCache><c:ptCount val="{len(values)}"/>{pts}</c:strCache></c:strRef>'

def num_cache(ref, values, code='General'):
    pts = ''.join((f'<c:pt idx="{k}"><c:v>{v}</c:v></c:pt>' for k, v in enumerate(values) if v is not None))
    return f'<c:numRef><c:f>{ref}</c:f><c:numCache><c:formatCode>{esc(code)}</c:formatCode><c:ptCount val="{len(values)}"/>{pts}</c:numCache></c:numRef>'

def rich(text, size=1400, bold=0):
    return f'<c:tx><c:rich><a:bodyPr/><a:lstStyle/><a:p><a:pPr><a:defRPr sz="{size}" b="{bold}"/></a:pPr><a:r><a:rPr lang="en-US" sz="{size}" b="{bold}"/><a:t>{esc(text)}</a:t></a:r></a:p></c:rich></c:tx>'

def title(text, size=1400, bold=0):
    return f'<c:title>{rich(text, size, bold)}<c:overlay val="0"/></c:title><c:autoTitleDeleted val="0"/>'

def solid(rgb):
    return f'<c:spPr><a:solidFill><a:srgbClr val="{rgb}"/></a:solidFill></c:spPr>'

def labels(show_val=0, show_pct=0, show_cat=0, pos=None, size=900):
    p = f'<c:dLblPos val="{pos}"/>' if pos else ''
    return f'<c:dLbls><c:spPr><a:noFill/><a:ln><a:noFill/></a:ln></c:spPr><c:txPr><a:bodyPr/><a:lstStyle/><a:p><a:pPr><a:defRPr sz="{size}"/></a:pPr><a:endParaRPr lang="en-US"/></a:p></c:txPr>{p}<c:showLegendKey val="0"/><c:showVal val="{show_val}"/><c:showCatName val="{show_cat}"/><c:showSerName val="0"/><c:showPercent val="{show_pct}"/><c:showBubbleSize val="0"/></c:dLbls>'

def cat_ax(ax, cross, pos='b', delete=0, extra=''):
    return f'<c:catAx><c:axId val="{ax}"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="{delete}"/><c:axPos val="{pos}"/>{extra}<c:numFmt formatCode="General" sourceLinked="1"/><c:majorTickMark val="out"/><c:minorTickMark val="none"/><c:tickLblPos val="nextTo"/><c:crossAx val="{cross}"/><c:crosses val="autoZero"/><c:auto val="1"/><c:lblAlgn val="ctr"/><c:lblOffset val="100"/><c:noMultiLvlLbl val="0"/></c:catAx>'

def val_ax(ax, cross, pos='l', between='between', fmt=None, extra='', grid=True):
    nf = f'<c:numFmt formatCode="{esc(fmt)}" sourceLinked="0"/>' if fmt else '<c:numFmt formatCode="General" sourceLinked="1"/>'
    g = '<c:majorGridlines/>' if grid else ''
    return f'<c:valAx><c:axId val="{ax}"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/><c:axPos val="{pos}"/>{g}{extra}{nf}<c:majorTickMark val="out"/><c:minorTickMark val="none"/><c:tickLblPos val="nextTo"/><c:crossAx val="{cross}"/><c:crosses val="autoZero"/><c:crossBetween val="{between}"/></c:valAx>'

def chart_space(body, legend='b'):
    leg = f'<c:legend><c:legendPos val="{legend}"/><c:overlay val="0"/></c:legend>' if legend else ''
    return f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<c:chartSpace xmlns:c="{NS_C}" xmlns:a="{NS_A}" xmlns:r="{NS_R}"><c:roundedCorners val="0"/><c:chart>{body}{leg}<c:plotVisOnly val="1"/><c:dispBlanksAs val="gap"/></c:chart><c:spPr><a:solidFill><a:schemeClr val="bg1"/></a:solidFill><a:ln w="9525"><a:solidFill><a:schemeClr val="tx1"><a:lumMod val="15000"/><a:lumOff val="85000"/></a:schemeClr></a:solidFill></a:ln></c:spPr><c:txPr><a:bodyPr/><a:lstStyle/><a:p><a:pPr><a:defRPr sz="1000"/></a:pPr><a:endParaRPr lang="en-US"/></a:p></c:txPr></c:chartSpace>'
MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun']
SALES = [120, 135, 160, 150, 185, 210]
COST = [80, 90, 95, 110, 115, 130]
PROFIT = [s - c for s, c in zip(SALES, COST)]
REGIONS = ['ภาคเหนือ', 'ภาคกลาง', 'ภาคใต้', 'ວຽງຈັນ']
SHARE = [35, 30, 20, 15]
AREA = [('Web', [300, 420, 380, 510]), ('Shop', [150, 180, 220, 260])]
SCATTER_X = [1.5, 2.0, 3.2, 4.1, 5.5, 6.3]
SCATTER_Y = [2.1, 3.9, 5.2, 7.8, 9.1, 12.4]
CROPS = [('Rice', 55), ('Coffee', 30), ('Tea', 15)]
PERCENT = [('Laos', [40, 45, 50]), ('Thailand', [35, 30, 30]), ('Other', [25, 25, 20])]

def column_chart():
    months = str_cache('Sales!$A$4:$A$9', MONTHS)
    ser1 = f'<c:ser><c:idx val="0"/><c:order val="0"/><c:tx>{str_cache('Sales!$B$3', ['Sales'])}</c:tx>{solid('4472C4')}<c:invertIfNegative val="0"/>{labels(show_val=1, pos='outEnd')}<c:cat>{months}</c:cat><c:val>{num_cache('Sales!$B$4:$B$9', SALES)}</c:val></c:ser>'
    ser2 = f'<c:ser><c:idx val="1"/><c:order val="1"/><c:tx>{str_cache('Sales!$C$3', ['Cost'])}</c:tx><c:invertIfNegative val="0"/><c:cat>{months}</c:cat><c:val>{num_cache('Sales!$C$4:$C$9', COST)}</c:val></c:ser>'
    body = title('Sales and cost 2026') + '<c:plotArea><c:layout/><c:barChart><c:barDir val="col"/><c:grouping val="clustered"/>' + f'<c:varyColors val="0"/>{ser1}{ser2}<c:gapWidth val="150"/><c:overlap val="-10"/>' + '<c:axId val="101"/><c:axId val="102"/></c:barChart>' + cat_ax(101, 102) + val_ax(102, 101) + '</c:plotArea>'
    return chart_space(body, 'b')

def line_chart():
    ser = f'<c:ser><c:idx val="0"/><c:order val="0"/><c:tx>{str_cache('Sales!$D$3', ['Profit'])}</c:tx><c:spPr><a:ln w="28575" cap="rnd"><a:solidFill><a:srgbClr val="70AD47"/></a:solidFill><a:round/></a:ln></c:spPr><c:marker><c:symbol val="circle"/><c:size val="6"/><c:spPr><a:solidFill><a:srgbClr val="70AD47"/></a:solidFill><a:ln w="9525"><a:solidFill><a:srgbClr val="FFFFFF"/></a:solidFill></a:ln></c:spPr></c:marker>{labels(show_val=1, pos='t')}<c:cat>{str_cache('Sales!$A$4:$A$9', MONTHS)}</c:cat><c:val>{num_cache('Sales!$D$4:$D$9', PROFIT)}</c:val><c:smooth val="0"/></c:ser>'
    body = title('ກຳໄລ ລາຍເດືອນ (Profit)') + '<c:plotArea><c:layout/><c:lineChart><c:grouping val="standard"/><c:varyColors val="0"/>' + f'{ser}<c:marker val="1"/><c:axId val="201"/><c:axId val="202"/></c:lineChart>' + cat_ax(201, 202, extra=f'<c:title>{rich('Month', 1000, 1)}<c:overlay val="0"/></c:title>') + val_ax(202, 201, extra=f'<c:title>{rich('Kip (thousand)', 1000, 1)}<c:overlay val="0"/></c:title>') + '</c:plotArea>'
    return chart_space(body, None)

def pie_chart():
    ser = f'<c:ser><c:idx val="0"/><c:order val="0"/><c:tx>{str_cache('Sales!$G$20', ['Share'])}</c:tx><c:dPt><c:idx val="3"/><c:bubble3D val="0"/><c:spPr><a:solidFill><a:srgbClr val="7030A0"/></a:solidFill><a:ln w="19050"><a:solidFill><a:schemeClr val="lt1"/></a:solidFill></a:ln></c:spPr></c:dPt>{labels(show_pct=1, pos='bestFit', size=1000)}<c:cat>{str_cache('Sales!$A$21:$A$24', REGIONS)}</c:cat><c:val>{num_cache('Sales!$B$21:$B$24', SHARE)}</c:val></c:ser>'
    body = title('สัดส่วนยอดขาย ตามภาค') + '<c:plotArea><c:layout/><c:pieChart><c:varyColors val="1"/>' + f'{ser}<c:firstSliceAng val="0"/></c:pieChart></c:plotArea>'
    return chart_space(body, 'r')

def bar_chart():
    teams = ['North', 'South', 'East', 'West']
    body = title('Tasks by team (stacked)') + '<c:plotArea><c:layout/><c:barChart><c:barDir val="bar"/>'
    body += '<c:grouping val="stacked"/><c:varyColors val="0"/>'
    for k, (name, vals) in enumerate([('Done', [12, 9, 15, 7]), ('Open', [5, 8, 3, 6]), ('Late', [2, 1, 4, 3])]):
        body += f'<c:ser><c:idx val="{k}"/><c:order val="{k}"/><c:tx>{str_cache(f'More!${col_name(k + 1)}$1', [name])}</c:tx><c:invertIfNegative val="0"/>{(labels(show_val=1, pos='ctr') if k == 0 else '')}<c:cat>{str_cache('More!$A$2:$A$5', teams)}</c:cat><c:val>{num_cache(f'More!${col_name(k + 1)}$2:${col_name(k + 1)}$5', vals)}</c:val></c:ser>'
    body += '<c:gapWidth val="80"/><c:overlap val="100"/><c:axId val="301"/><c:axId val="302"/></c:barChart>'
    body += cat_ax(301, 302, pos='l') + val_ax(302, 301, pos='b') + '</c:plotArea>'
    return chart_space(body, 'r')

def area_chart():
    quarters = ['Q1', 'Q2', 'Q3', 'Q4']
    body = title('Visitors (stacked area)') + '<c:plotArea><c:layout/><c:areaChart><c:grouping val="stacked"/>'
    body += '<c:varyColors val="0"/>'
    for k, (name, vals) in enumerate(AREA):
        col = 'BD'[k]
        body += f'<c:ser><c:idx val="{k}"/><c:order val="{k}"/><c:tx>{str_cache(f'More!${col}$36', [name])}</c:tx><c:cat>{str_cache('More!$A$37:$A$40', quarters)}</c:cat><c:val>{num_cache(f'More!${col}$37:${col}$40', vals)}</c:val></c:ser>'
    body += '<c:axId val="401"/><c:axId val="402"/></c:areaChart>'
    body += cat_ax(401, 402) + val_ax(402, 401, between='midCat', fmt='#,##0') + '</c:plotArea>'
    return chart_space(body, 't')

def scatter_chart():
    xs, ys = (SCATTER_X, SCATTER_Y)
    ser = f'<c:ser><c:idx val="0"/><c:order val="0"/><c:tx>{str_cache('More!$B$42', ['Height (m)'])}</c:tx><c:spPr><a:ln w="19050"><a:solidFill><a:srgbClr val="C00000"/></a:solidFill><a:prstDash val="dash"/></a:ln></c:spPr><c:marker><c:symbol val="diamond"/><c:size val="7"/></c:marker><c:xVal>{num_cache('More!$A$43:$A$48', xs)}</c:xVal><c:yVal>{num_cache('More!$B$43:$B$48', ys, '0.0')}</c:yVal><c:smooth val="0"/></c:ser>'
    body = title('Growth (scatter)') + '<c:plotArea><c:layout/><c:scatterChart><c:scatterStyle val="lineMarker"/><c:varyColors val="0"/>' + f'{ser}<c:axId val="501"/><c:axId val="502"/></c:scatterChart>' + val_ax(501, 502, pos='b', between='midCat', grid=False) + val_ax(502, 501, pos='l', between='midCat') + '</c:plotArea>'
    return chart_space(body, None)

def doughnut_chart():
    names = [n for n, _ in CROPS]
    ser = f'<c:ser><c:idx val="0"/><c:order val="0"/><c:tx>{str_cache('More!$B$50', ['Crops'])}</c:tx>{labels(show_val=1, size=900)}<c:cat>{str_cache('More!$A$51:$A$53', names)}</c:cat><c:val>{num_cache('More!$B$51:$B$53', [v for _, v in CROPS])}</c:val></c:ser>'
    body = title('Crops (doughnut)') + '<c:plotArea><c:layout/><c:doughnutChart><c:varyColors val="1"/>' + f'{ser}<c:firstSliceAng val="90"/><c:holeSize val="55"/></c:doughnutChart></c:plotArea>'
    return chart_space(body, 'b')

def radar_chart():
    ser = f'<c:ser><c:idx val="0"/><c:order val="0"/><c:tx>{str_cache('More!$B$55', ['Skill'])}</c:tx><c:cat>{str_cache('More!$A$56:$A$58', ['A', 'B', 'C'])}</c:cat><c:val>{num_cache('More!$B$56:$B$58', [3, 4, 5])}</c:val></c:ser>'
    body = title('Skills (radar, not drawn)') + '<c:plotArea><c:layout/><c:radarChart><c:radarStyle val="marker"/><c:varyColors val="0"/>' + f'{ser}<c:axId val="601"/><c:axId val="602"/></c:radarChart>' + cat_ax(601, 602) + val_ax(602, 601) + '</c:plotArea>'
    return chart_space(body, None)

def percent_chart():
    body = title('Share by year (100%)') + '<c:plotArea><c:layout/><c:barChart><c:barDir val="col"/>'
    body += '<c:grouping val="percentStacked"/><c:varyColors val="0"/>'
    years = ['2024', '2025', '2026']
    for k, (name, vals) in enumerate(PERCENT):
        body += f'<c:ser><c:idx val="{k}"/><c:order val="{k}"/><c:tx>{str_cache(f'More!$A${k + 62}', [name])}</c:tx><c:invertIfNegative val="0"/><c:cat>{str_cache('More!$D$61:$F$61', years)}</c:cat><c:val>{num_cache(f'More!$D${k + 62}:$F${k + 62}', vals)}</c:val></c:ser>'
    body += '<c:gapWidth val="60"/><c:overlap val="100"/><c:axId val="701"/><c:axId val="702"/></c:barChart>'
    body += cat_ax(701, 702) + val_ax(702, 701, fmt='0%') + '</c:plotArea>'
    return chart_space(body, 'r')

def marker(tag, col, row, col_off=0, row_off=0):
    return f'<xdr:{tag}><xdr:col>{col}</xdr:col><xdr:colOff>{col_off}</xdr:colOff><xdr:row>{row}</xdr:row><xdr:rowOff>{row_off}</xdr:rowOff></xdr:{tag}>'

def two_cell(frm, to, obj, client=''):
    return f'<xdr:twoCellAnchor editAs="oneCell">{marker('from', *frm)}{marker('to', *to)}{obj}<xdr:clientData{client}/></xdr:twoCellAnchor>'

def frame(id_, name, rid):
    return f'<xdr:graphicFrame macro=""><xdr:nvGraphicFramePr><xdr:cNvPr id="{id_}" name="{name}"/><xdr:cNvGraphicFramePr/></xdr:nvGraphicFramePr><xdr:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/></xdr:xfrm><a:graphic><a:graphicData uri="{NS_C}"><c:chart r:id="{rid}"/></a:graphicData></a:graphic></xdr:graphicFrame>'

def xfrm(x, y, w, h, rot=0, tag='a:xfrm'):
    r = f' rot="{rot}"' if rot else ''
    return f'<{tag}{r}><a:off x="{x}" y="{y}"/><a:ext cx="{w}" cy="{h}"/></{tag}>'
STYLE_ACCENT = '<xdr:style><a:lnRef idx="2"><a:schemeClr val="accent2"><a:shade val="50000"/></a:schemeClr></a:lnRef><a:fillRef idx="1"><a:schemeClr val="accent2"/></a:fillRef><a:effectRef idx="0"><a:schemeClr val="accent2"/></a:effectRef><a:fontRef idx="minor"><a:schemeClr val="lt1"/></a:fontRef></xdr:style>'

def shape(id_, name, geom, box, paras, style=STYLE_ACCENT, sppr_extra='', body='', hidden=False, txbox=False):
    h = ' hidden="1"' if hidden else ''
    tb = ' txBox="1"' if txbox else ''
    return f'<xdr:sp macro="" textlink=""><xdr:nvSpPr><xdr:cNvPr id="{id_}" name="{name}"{h}/><xdr:cNvSpPr{tb}/></xdr:nvSpPr><xdr:spPr>{xfrm(*box)}<a:prstGeom prst="{geom}"><a:avLst/></a:prstGeom>{sppr_extra}</xdr:spPr>{style}<xdr:txBody><a:bodyPr vertOverflow="clip" horzOverflow="clip" wrap="square" rtlCol="0"{body}/><a:lstStyle/>{paras}</xdr:txBody></xdr:sp>'

def para(runs, align=None):
    a = f'<a:pPr algn="{align}"/>' if align else ''
    out = ''
    for text, props in runs:
        out += f'<a:r><a:rPr lang="en-US"{props}/><a:t>{esc(text)}</a:t></a:r>'
    return f'<a:p>{a}{out}</a:p>'

def emu(inches):
    return int(inches * EMU_IN)

def drawing1():
    parts = []
    parts.append(two_cell((5, 1), (12, 16), frame(2, 'Chart 1', 'rId1')))
    parts.append(two_cell((5, 17), (12, 33), frame(3, 'Chart 2', 'rId2')))
    parts.append(two_cell((13, 1), (19, 16), frame(4, 'Chart 3', 'rId3')))
    rect = shape(5, 'Rectangle 4', 'rect', (0, emu(2.0), emu(3.2), emu(0.6)), para([('Total: 1,234 kip', ' sz="1400" b="1"')], 'ctr'), body=' anchor="ctr"')
    parts.append(two_cell((0, 10), (4, 13), rect))
    box_style = '<a:solidFill><a:schemeClr val="lt1"/></a:solidFill><a:ln w="9525" cmpd="sng"><a:solidFill><a:schemeClr val="lt1"><a:shade val="50000"/></a:schemeClr></a:solidFill></a:ln>'
    tb_style = '<xdr:style><a:lnRef idx="0"><a:scrgbClr r="0" g="0" b="0"/></a:lnRef><a:fillRef idx="0"><a:scrgbClr r="0" g="0" b="0"/></a:fillRef><a:effectRef idx="0"><a:scrgbClr r="0" g="0" b="0"/></a:effectRef><a:fontRef idx="minor"><a:schemeClr val="dk1"/></a:fontRef></xdr:style>'
    paras = para([('ສະບາຍດີ ', ' sz="1400" b="1"'), ('ນີ້ແມ່ນກ່ອງຂໍ້ຄວາມ ທີ່ມີຫຼາຍແຖວ ແລະ ຕັດແຖວເອງ', ' sz="1200"')]) + para([('สวัสดี ', ' sz="1200" i="1"'), ('นี่คือกล่องข้อความภาษาไทย', ' sz="1200"')]) + '<a:p><a:r><a:rPr lang="en-US" sz="1100"><a:solidFill><a:srgbClr val="C00000"/></a:solidFill></a:rPr>' + '<a:t>Red run in the text box</a:t></a:r></a:p>'
    tb = shape(6, 'TextBox 5', 'rect', (0, emu(2.8), emu(3.2), emu(1.4)), paras, style=tb_style, sppr_extra=box_style, txbox=True)
    parts.append(f'<xdr:oneCellAnchor>{marker('from', 0, 15)}<xdr:ext cx="{emu(3.2)}" cy="{emu(1.4)}"/>{tb}<xdr:clientData/></xdr:oneCellAnchor>')
    pic = f'<xdr:pic><xdr:nvPicPr><xdr:cNvPr id="7" name="Picture 6"/><xdr:cNvPicPr><a:picLocks noChangeAspect="1"/></xdr:cNvPicPr></xdr:nvPicPr><xdr:blipFill><a:blip r:embed="rId4"/><a:stretch><a:fillRect/></a:stretch></xdr:blipFill><xdr:spPr>{xfrm(emu(0.2), emu(4.6), emu(1.6), emu(0.8))}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:ln w="12700"><a:solidFill><a:srgbClr val="404040"/></a:solidFill></a:ln></xdr:spPr></xdr:pic>'
    parts.append(f'<xdr:absoluteAnchor><xdr:pos x="{emu(0.2)}" y="{emu(4.6)}"/><xdr:ext cx="{emu(1.6)}" cy="{emu(0.8)}"/>{pic}<xdr:clientData/></xdr:absoluteAnchor>')
    ell = f'<xdr:sp macro="" textlink=""><xdr:nvSpPr><xdr:cNvPr id="9" name="Oval 8"/><xdr:cNvSpPr/></xdr:nvSpPr><xdr:spPr>{xfrm(0, 0, 1000000, 600000)}<a:prstGeom prst="ellipse"><a:avLst/></a:prstGeom><a:solidFill><a:srgbClr val="FFC000"/></a:solidFill><a:ln w="12700"><a:solidFill><a:srgbClr val="7F6000"/></a:solidFill></a:ln></xdr:spPr><xdr:txBody><a:bodyPr anchor="ctr"/><a:lstStyle/>{para([('Group label', ' sz="1000"')], 'ctr')}</xdr:txBody></xdr:sp>'
    arr = f'<xdr:sp macro="" textlink=""><xdr:nvSpPr><xdr:cNvPr id="10" name="Arrow 9"/><xdr:cNvSpPr/></xdr:nvSpPr><xdr:spPr>{xfrm(1100000, 150000, 900000, 300000)}<a:prstGeom prst="rightArrow"><a:avLst/></a:prstGeom><a:solidFill><a:srgbClr val="5B9BD5"/></a:solidFill><a:ln><a:noFill/></a:ln></xdr:spPr></xdr:sp>'
    grp = f'<xdr:grpSp><xdr:nvGrpSpPr><xdr:cNvPr id="8" name="Group 7"/><xdr:cNvGrpSpPr/></xdr:nvGrpSpPr><xdr:grpSpPr><a:xfrm><a:off x="{emu(2.2)}" y="{emu(4.6)}"/><a:ext cx="{emu(2.4)}" cy="{emu(0.7)}"/><a:chOff x="0" y="0"/><a:chExt cx="2000000" cy="600000"/></a:xfrm></xdr:grpSpPr>{ell}{arr}</xdr:grpSp>'
    parts.append(f'<xdr:oneCellAnchor>{marker('from', 3, 23)}<xdr:ext cx="{emu(2.4)}" cy="{emu(0.7)}"/>{grp}<xdr:clientData/></xdr:oneCellAnchor>')
    cxn = f'<xdr:cxnSp macro=""><xdr:nvCxnSpPr><xdr:cNvPr id="11" name="Straight Arrow Connector 10"/><xdr:cNvCxnSpPr/></xdr:nvCxnSpPr><xdr:spPr>{xfrm(emu(3.3), emu(2.3), emu(0.9), emu(0.6))}<a:prstGeom prst="straightConnector1"><a:avLst/></a:prstGeom><a:ln w="19050"><a:solidFill><a:srgbClr val="C00000"/></a:solidFill><a:tailEnd type="triangle"/></a:ln></xdr:spPr></xdr:cxnSp>'
    parts.append(two_cell((4, 11), (5, 14), cxn))
    return wsdr(parts)

def drawing2():
    parts = []
    parts.append(two_cell((6, 0), (12, 13), frame(2, 'Chart 1', 'rId1')))
    parts.append(two_cell((6, 16), (12, 29), frame(3, 'Chart 2', 'rId2')))
    parts.append(two_cell((13, 0), (19, 13), frame(4, 'Chart 3', 'rId3')))
    parts.append(two_cell((13, 16), (19, 29), frame(5, 'Chart 4', 'rId4')))
    parts.append(two_cell((20, 0), (26, 13), frame(6, 'Chart 5', 'rId5')))
    across = shape(7, 'Rectangle 6', 'roundRect', (0, 0, emu(2), emu(1.5)), para([('Across the break', ' sz="1200" b="1"')], 'ctr'), body=' anchor="t"')
    parts.append(two_cell((0, 12), (4, 19), across))
    parts.append(two_cell((0, 30), (3, 33), shape(8, 'Hidden rows', 'rect', (0, 0, emu(2), emu(0.4)), para([('HIDDEN-ROW-SHAPE', '')]))))
    parts.append(two_cell((0, 20), (3, 22), shape(9, 'Hidden', 'rect', (0, 0, emu(2), emu(0.4)), para([('HIDDEN-FLAG-SHAPE', '')]), hidden=True)))
    parts.append(two_cell((0, 23), (3, 25), shape(10, 'No print', 'rect', (0, 0, emu(2), emu(0.4)), para([('NOPRINT-SHAPE', '')])), client=' fPrintsWithSheet="0"'))
    return wsdr(parts)

def drawing3():
    return f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<xdr:wsDr xmlns:xdr="{NS_XDR}" xmlns:a="{NS_A}" xmlns:r="{NS_R}" xmlns:c="{NS_C}"><xdr:absoluteAnchor><xdr:pos x="0" y="0"/><xdr:ext cx="8666000" cy="6293000"/>{frame(2, 'Chart 1', 'rId1')}<xdr:clientData/></xdr:absoluteAnchor></xdr:wsDr>'

def wsdr(parts):
    return f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<xdr:wsDr xmlns:xdr="{NS_XDR}" xmlns:a="{NS_A}" xmlns:r="{NS_R}" xmlns:c="{NS_C}">{''.join(parts)}</xdr:wsDr>'

def rels(items):
    body = ''.join((f'<Relationship Id="{i}" Type="{REL}{t}" Target="{target}"/>' for i, t, target in items))
    return f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<Relationships xmlns="{PKG_REL}">{body}</Relationships>'
THEME = f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<a:theme xmlns:a="{NS_A}" name="Office Theme"><a:themeElements>\n<a:clrScheme name="Office"><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1>\n<a:dk2><a:srgbClr val="44546A"/></a:dk2><a:lt2><a:srgbClr val="E7E6E6"/></a:lt2><a:accent1><a:srgbClr val="4472C4"/></a:accent1>\n<a:accent2><a:srgbClr val="ED7D31"/></a:accent2><a:accent3><a:srgbClr val="A5A5A5"/></a:accent3><a:accent4><a:srgbClr val="FFC000"/></a:accent4>\n<a:accent5><a:srgbClr val="5B9BD5"/></a:accent5><a:accent6><a:srgbClr val="70AD47"/></a:accent6><a:hlink><a:srgbClr val="0563C1"/></a:hlink>\n<a:folHlink><a:srgbClr val="954F72"/></a:folHlink></a:clrScheme>\n<a:fontScheme name="Office"><a:majorFont><a:latin typeface="Calibri Light"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont>\n<a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme>\n<a:fmtScheme name="Office"><a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill>\n<a:gradFill rotWithShape="1"><a:gsLst><a:gs pos="0"><a:schemeClr val="phClr"><a:tint val="67000"/></a:schemeClr></a:gs><a:gs pos="100000"><a:schemeClr val="phClr"><a:shade val="94000"/></a:schemeClr></a:gs></a:gsLst><a:lin ang="5400000" scaled="0"/></a:gradFill>\n<a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:fillStyleLst>\n<a:lnStyleLst><a:ln w="6350"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="12700"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln>\n<a:ln w="19050"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln></a:lnStyleLst>\n<a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst>\n<a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:bgFillStyleLst>\n</a:fmtScheme></a:themeElements></a:theme>'
STYLES = f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<styleSheet xmlns="{NS_MAIN}"><fonts count="1"><font><sz val="11"/><name val="Calibri"/><family val="2"/><scheme val="minor"/></font></fonts>\n<fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills>\n<borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders>\n<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>\n<cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/></cellXfs>\n<cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>'

def main():
    ct = {'/xl/workbook.xml': 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml', '/xl/styles.xml': 'application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml', '/xl/theme/theme1.xml': 'application/vnd.openxmlformats-officedocument.theme+xml', '/xl/worksheets/sheet1.xml': 'application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml', '/xl/worksheets/sheet2.xml': 'application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml', '/xl/chartsheets/sheet3.xml': 'application/vnd.openxmlformats-officedocument.spreadsheetml.chartsheet+xml'}
    files = {}
    files['_rels/.rels'] = f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<Relationships xmlns="{PKG_REL}"><Relationship Id="rId1" Type="{REL}officeDocument" Target="xl/workbook.xml"/></Relationships>'
    files['xl/workbook.xml'] = f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<workbook xmlns="{NS_MAIN}" xmlns:r="{NS_R}"><sheets><sheet name="Sales" sheetId="1" r:id="rId1"/><sheet name="More" sheetId="2" r:id="rId2"/><sheet name="Share" sheetId="3" r:id="rId3"/></sheets></workbook>'
    files['xl/_rels/workbook.xml.rels'] = rels([('rId1', 'worksheet', 'worksheets/sheet1.xml'), ('rId2', 'worksheet', 'worksheets/sheet2.xml'), ('rId3', 'chartsheet', 'chartsheets/sheet3.xml'), ('rId4', 'styles', 'styles.xml'), ('rId5', 'theme', 'theme/theme1.xml')])
    files['xl/styles.xml'] = STYLES
    files['xl/theme/theme1.xml'] = THEME
    rows = {0: {0: 'Monthly sales (drawings test)'}, 2: {0: 'Month', 1: 'Sales', 2: 'Cost', 3: 'Profit'}}
    for k, m in enumerate(MONTHS):
        rows[3 + k] = {0: m, 1: SALES[k], 2: COST[k], 3: PROFIT[k]}
    rows[19] = {0: 'Region', 1: 'Share'}
    for k, r in enumerate(REGIONS):
        rows[20 + k] = {0: r, 1: SHARE[k]}
    files['xl/worksheets/sheet1.xml'] = sheet_xml(rows, extra_after='<pageSetup paperSize="9" orientation="landscape" fitToWidth="1" fitToHeight="1"/>', pr='<sheetPr><pageSetUpPr fitToPage="1"/></sheetPr>')
    files['xl/worksheets/_rels/sheet1.xml.rels'] = rels([('rId1', 'drawing', '../drawings/drawing1.xml')])
    files['xl/drawings/drawing1.xml'] = drawing1()
    files['xl/drawings/_rels/drawing1.xml.rels'] = rels([('rId1', 'chart', '../charts/chart1.xml'), ('rId2', 'chart', '../charts/chart2.xml'), ('rId3', 'chart', '../charts/chart3.xml'), ('rId4', 'image', '../media/image1.png')])
    files['xl/charts/chart1.xml'] = column_chart()
    files['xl/charts/chart2.xml'] = line_chart()
    files['xl/charts/chart3.xml'] = pie_chart()
    rows = {0: {0: 'Team', 1: 'Done', 2: 'Open', 3: 'Late'}}
    for k, (t, a, b, c) in enumerate([('North', 12, 5, 2), ('South', 9, 8, 1), ('East', 15, 3, 4), ('West', 7, 6, 3)]):
        rows[1 + k] = {0: t, 1: a, 2: b, 3: c}
    rows[35] = {0: 'Quarter', 1: 'Web', 3: 'Shop'}
    for k, q in enumerate(['Q1', 'Q2', 'Q3', 'Q4']):
        rows[36 + k] = {0: q, 1: AREA[0][1][k], 3: AREA[1][1][k]}
    rows[41] = {0: 'x', 1: 'Height (m)'}
    for k, (x, y) in enumerate(zip(SCATTER_X, SCATTER_Y)):
        rows[42 + k] = {0: x, 1: y}
    rows[49] = {0: 'Crop', 1: 'Crops'}
    for k, (n, v) in enumerate(CROPS):
        rows[50 + k] = {0: n, 1: v}
    rows[54] = {0: 'Axis', 1: 'Skill'}
    for k, (n, v) in enumerate(zip('ABC', [3, 4, 5])):
        rows[55 + k] = {0: n, 1: v}
    rows[60] = {0: 'Country', 3: '2024', 4: '2025', 5: '2026'}
    for k, (n, vals) in enumerate(PERCENT):
        rows[61 + k] = {0: n, 3: vals[0], 4: vals[1], 5: vals[2]}
    rows[66] = {0: 'End of sheet', 1: ('e', '#DIV/0!')}
    hidden_rows = {r: ' hidden="1" ht="15" customHeight="1"' for r in (30, 31, 32)}
    files['xl/worksheets/sheet2.xml'] = sheet_xml(rows, cols='<cols><col min="3" max="3" width="9.140625" hidden="1" customWidth="1"/></cols>', extra_after='<pageSetup paperSize="9" orientation="landscape" scale="100" firstPageNumber="5" useFirstPageNumber="1" errors="dash"/><headerFooter differentFirst="1"><oddHeader>&amp;RPage &amp;P of &amp;N</oddHeader><firstHeader>&amp;CFirst page header</firstHeader></headerFooter><rowBreaks count="1" manualBreakCount="1"><brk id="15" max="16383" man="1"/></rowBreaks>', row_attrs=hidden_rows)
    files['xl/worksheets/_rels/sheet2.xml.rels'] = rels([('rId1', 'drawing', '../drawings/drawing2.xml')])
    files['xl/drawings/drawing2.xml'] = drawing2()
    files['xl/drawings/_rels/drawing2.xml.rels'] = rels([('rId1', 'chart', '../charts/chart4.xml'), ('rId2', 'chart', '../charts/chart5.xml'), ('rId3', 'chart', '../charts/chart6.xml'), ('rId4', 'chart', '../charts/chart7.xml'), ('rId5', 'chart', '../charts/chart8.xml')])
    files['xl/charts/chart4.xml'] = bar_chart()
    files['xl/charts/chart5.xml'] = area_chart()
    files['xl/charts/chart6.xml'] = scatter_chart()
    files['xl/charts/chart7.xml'] = doughnut_chart()
    files['xl/charts/chart8.xml'] = radar_chart()
    files['xl/chartsheets/sheet3.xml'] = f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<chartsheet xmlns="{NS_MAIN}" xmlns:r="{NS_R}"><sheetPr/><sheetViews><sheetView zoomToFit="1" workbookViewId="0"/></sheetViews><pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/><pageSetup paperSize="9" orientation="landscape"/><headerFooter><oddFooter>&amp;CChart sheet &amp;A</oddFooter></headerFooter><drawing r:id="rId1"/></chartsheet>'
    files['xl/chartsheets/_rels/sheet3.xml.rels'] = rels([('rId1', 'drawing', '../drawings/drawing3.xml')])
    files['xl/drawings/drawing3.xml'] = drawing3()
    files['xl/drawings/_rels/drawing3.xml.rels'] = rels([('rId1', 'chart', '../charts/chart9.xml')])
    files['xl/charts/chart9.xml'] = percent_chart()
    for n in range(1, 4):
        ct[f'/xl/drawings/drawing{n}.xml'] = 'application/vnd.openxmlformats-officedocument.drawing+xml'
    for n in range(1, 10):
        ct[f'/xl/charts/chart{n}.xml'] = 'application/vnd.openxmlformats-officedocument.drawingml.chart+xml'
    overrides = ''.join((f'<Override PartName="{k}" ContentType="{v}"/>' for k, v in ct.items()))
    files['[Content_Types].xml'] = f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/>{overrides}</Types>'
    out = os.path.join(HERE, 'drawings.xlsx')
    order = ['[Content_Types].xml', '_rels/.rels'] + sorted((k for k in files if k not in ('[Content_Types].xml', '_rels/.rels')))
    with zipfile.ZipFile(out, 'w', zipfile.ZIP_DEFLATED) as z:
        for name in order:
            info = zipfile.ZipInfo(name, date_time=(2026, 9, 24, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, files[name].encode('utf-8'))
        info = zipfile.ZipInfo('xl/media/image1.png', date_time=(2026, 9, 24, 0, 0, 0))
        z.writestr(info, png(96, 48))
    print(out)
if __name__ == '__main__':
    main()
