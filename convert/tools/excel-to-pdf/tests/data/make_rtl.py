#!/usr/bin/env python3
import os
import sys
import zipfile
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import make_drawings as md
ARABIC = 'مرحبا بالعالم'
HEBREW = 'שלום עולם'
PRICE = 'السعر 250 دولار'
LATIN_INSIDE = 'نظام Windows الجديد'
BRACKETS = '(مرحبا)'
WRAPPED = 'هذه فقرة عربية طويلة تلتف داخل الخلية لتختبر تقسيم الأسطر من اليمين إلى اليسار'
RTL_ORDER = 'Total: 42 items'
MIXED = 'مرحبا PanPDF'
CHART_TITLE = 'المبيعات الشهرية'
MONTHS = ['يناير', 'فبراير', 'مارس']
VALUES = [120, 150, 90]
STYLES = f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<styleSheet xmlns="{md.NS_MAIN}"><fonts count="1"><font><sz val="11"/><name val="Calibri"/><family val="2"/><scheme val="minor"/></font></fonts>\n<fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills>\n<borders count="2"><border><left/><right/><top/><bottom/><diagonal/></border>\n<border><left style="thick"><color rgb="FFC00000"/></left><right/><top/><bottom/><diagonal/></border></borders>\n<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>\n<cellXfs count="7"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/>\n<xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0" applyAlignment="1"><alignment readingOrder="2"/></xf>\n<xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0" applyAlignment="1"><alignment readingOrder="1"/></xf>\n<xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0" applyAlignment="1"><alignment wrapText="1"/></xf>\n<xf numFmtId="0" fontId="0" fillId="0" borderId="1" xfId="0" applyBorder="1"/>\n<xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0" applyAlignment="1"><alignment horizontal="left"/></xf>\n<xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0" applyAlignment="1"><alignment horizontal="right"/></xf></cellXfs>\n<cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>'

def row(r, cells, attrs=''):
    inner = ''.join((md.cell(f'{md.col_name(c)}{r + 1}', v, s) for c, (v, s) in sorted(cells.items())))
    return f'<row r="{r + 1}"{attrs}>{inner}</row>'

def worksheet(rows, view='', cols='', merges='', drawing=False):
    d = '<drawing r:id="rId1"/>' if drawing else ''
    return f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<worksheet xmlns="{md.NS_MAIN}" xmlns:r="{md.NS_R}"><sheetViews><sheetView{view} workbookViewId="0"/></sheetViews><sheetFormatPr defaultRowHeight="15"/>{cols}<sheetData>{''.join(rows)}</sheetData>{merges}<pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/><pageSetup paperSize="9" orientation="portrait"/>{d}</worksheet>'

def chart():
    cats = md.str_cache('Cells!$D$4:$D$6', MONTHS)
    ser = f'<c:ser><c:idx val="0"/><c:order val="0"/><c:tx>{md.str_cache('Cells!$E$3', ['المبيعات'])}</c:tx>{md.solid('4472C4')}<c:invertIfNegative val="0"/><c:cat>{cats}</c:cat><c:val>{md.num_cache('Cells!$E$4:$E$6', VALUES)}</c:val></c:ser>'
    body = md.title(CHART_TITLE) + '<c:plotArea><c:layout/><c:barChart><c:barDir val="col"/><c:grouping val="clustered"/>' + f'<c:varyColors val="0"/>{ser}<c:gapWidth val="150"/>' + '<c:axId val="101"/><c:axId val="102"/></c:barChart>' + md.cat_ax(101, 102) + md.val_ax(102, 101) + '</c:plotArea>'
    return md.chart_space(body, 'b')

def main():
    files = {}
    files['_rels/.rels'] = f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<Relationships xmlns="{md.PKG_REL}"><Relationship Id="rId1" Type="{md.REL}officeDocument" Target="xl/workbook.xml"/></Relationships>'
    files['xl/workbook.xml'] = f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<workbook xmlns="{md.NS_MAIN}" xmlns:r="{md.NS_R}"><sheets><sheet name="Cells" sheetId="1" r:id="rId1"/><sheet name="Mirrored" sheetId="2" r:id="rId2"/></sheets></workbook>'
    files['xl/_rels/workbook.xml.rels'] = md.rels([('rId1', 'worksheet', 'worksheets/sheet1.xml'), ('rId2', 'worksheet', 'worksheets/sheet2.xml'), ('rId3', 'styles', 'styles.xml'), ('rId4', 'theme', 'theme/theme1.xml')])
    files['xl/styles.xml'] = STYLES
    files['xl/theme/theme1.xml'] = md.THEME
    b = [('LEFT', 0), (ARABIC, 0), (HEBREW, 0), (PRICE, 0), (LATIN_INSIDE, 0), (BRACKETS, 0), (WRAPPED, 3), (RTL_ORDER, 1), (MIXED, 2), (MIXED, 0), (1234.5, 0)]
    rows = [row(0, {1: ('RTL cells', 0)})]
    for k, (v, s) in enumerate(b):
        cells = {1: (v, s)}
        if k == 1:
            cells[3] = ('الشهر', 0)
            cells[4] = ('المبيعات', 0)
        if 2 <= k <= 4:
            cells[3] = (MONTHS[k - 2], 0)
            cells[4] = (VALUES[k - 2], 0)
        rows.append(row(1 + k, cells, ' ht="48" customHeight="1"' if s == 3 else ''))
    files['xl/worksheets/sheet1.xml'] = worksheet(rows, cols='<cols><col min="2" max="2" width="40" customWidth="1"/></cols>', drawing=True)
    files['xl/worksheets/_rels/sheet1.xml.rels'] = md.rels([('rId1', 'drawing', '../drawings/drawing1.xml')])
    files['xl/drawings/drawing1.xml'] = md.wsdr([md.two_cell((0, 14), (4, 30), md.frame(2, 'Chart 1', 'rId1'))])
    files['xl/drawings/_rels/drawing1.xml.rels'] = md.rels([('rId1', 'chart', '../charts/chart1.xml')])
    files['xl/charts/chart1.xml'] = chart()
    rows = [row(0, {0: ('العمود أ', 0), 1: ('العمود ب', 0), 2: ('העמודה ג', 0)}), row(1, {0: ('ColA', 0), 1: ('ColB', 0), 2: ('ColC', 0)}), row(2, {0: (42, 0), 1: ('خلية بحد أيسر', 4)}), row(3, {0: ('نص مدمج في خليتين', 0)}), row(4, {0: ('AlignL', 5), 1: ('AlignR', 6), 2: (7, 5)})]
    files['xl/worksheets/sheet2.xml'] = worksheet(rows, view=' rightToLeft="1"', cols='<cols><col min="1" max="3" width="16" customWidth="1"/></cols>', merges='<mergeCells count="1"><mergeCell ref="A4:B4"/></mergeCells>')
    ct = {'/xl/workbook.xml': 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml', '/xl/styles.xml': 'application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml', '/xl/theme/theme1.xml': 'application/vnd.openxmlformats-officedocument.theme+xml', '/xl/worksheets/sheet1.xml': 'application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml', '/xl/worksheets/sheet2.xml': 'application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml', '/xl/drawings/drawing1.xml': 'application/vnd.openxmlformats-officedocument.drawing+xml', '/xl/charts/chart1.xml': 'application/vnd.openxmlformats-officedocument.drawingml.chart+xml'}
    overrides = ''.join((f'<Override PartName="{k}" ContentType="{v}"/>' for k, v in ct.items()))
    files['[Content_Types].xml'] = f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>{overrides}</Types>'
    out = os.path.join(HERE, 'rtl.xlsx')
    order = ['[Content_Types].xml', '_rels/.rels'] + sorted((k for k in files if k not in ('[Content_Types].xml', '_rels/.rels')))
    with zipfile.ZipFile(out, 'w', zipfile.ZIP_DEFLATED) as z:
        for name in order:
            info = zipfile.ZipInfo(name, date_time=(2026, 9, 25, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, files[name].encode('utf-8'))
    print(out)
if __name__ == '__main__':
    main()
