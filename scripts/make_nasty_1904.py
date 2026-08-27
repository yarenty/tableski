#!/usr/bin/env python3
"""Regenerate fixtures/corpus/nasty_1904.xlsx: a handcrafted workbook exercising what
writer libraries won't produce — the 1904 date system, a formula with a numeric cached
value, and a formula whose cached result is a #DIV/0! error."""
import zipfile

FILES = {
    "[Content_Types].xml": """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
<Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>
</Types>""",
    "_rels/.rels": """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>""",
    "xl/workbook.xml": """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<workbookPr date1904="1"/>
<sheets><sheet name="Nasty" sheetId="1" r:id="rId1"/></sheets>
</workbook>""",
    "xl/_rels/workbook.xml.rels": """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
</Relationships>""",
    "xl/styles.xml": """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<fonts count="1"><font/></fonts><fills count="1"><fill/></fills><borders count="1"><border/></borders>
<cellStyleXfs count="1"><xf/></cellStyleXfs>
<cellXfs count="2"><xf numFmtId="0"/><xf numFmtId="14" applyNumberFormat="1"/></cellXfs>
</styleSheet>""",
    # Row 1: headers (inline strings). Row 2: date serial 100 (1904 system -> 1904-04-10),
    # formula with numeric cached value 2, formula with cached #DIV/0! error.
    "xl/worksheets/sheet1.xml": """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<sheetData>
<row r="1">
<c r="A1" t="inlineStr"><is><t>when</t></is></c>
<c r="B1" t="inlineStr"><is><t>calc</t></is></c>
<c r="C1" t="inlineStr"><is><t>broken</t></is></c>
</row>
<row r="2">
<c r="A2" s="1"><v>100</v></c>
<c r="B2"><f>1+1</f><v>2</v></c>
<c r="C2" t="e"><f>1/0</f><v>#DIV/0!</v></c>
</row>
<row r="3">
<c r="A3" s="1"><v>101</v></c>
<c r="B3"><f>2+2</f><v>4</v></c>
<c r="C3" t="e"><f>1/0</f><v>#DIV/0!</v></c>
</row>
</sheetData>
</worksheet>""",
}

with zipfile.ZipFile("fixtures/corpus/nasty_1904.xlsx", "w", zipfile.ZIP_DEFLATED) as z:
    for name, content in FILES.items():
        z.writestr(name, content)
print("wrote fixtures/corpus/nasty_1904.xlsx")
