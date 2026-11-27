# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
# /// script
# requires-python = ">=3.10"
# dependencies = [
#   "xlsxwriter>=3,<4",
#   "xlwt>=1.3,<2",
# ]
# ///
"""Generate spreadsheet fixtures under tests/fixtures/xlsx/."""

from __future__ import annotations

import io
import sys
import zipfile

from _common import (
    FIXED_DT_NAIVE,
    FIXED_ISO_DATE,
    FIXED_ZIP_TIME,
    FIXTURES_ROOT,
    repack_zip_deterministic,
    write_bytes,
)


def manifest_entries() -> list[dict[str, object]]:
    """Return ADR 0011 manifest descriptors for all xlsx/ fixtures."""
    return [
        {
            "path": "xlsx/formulas.xlsx",
            "format": "xlsx",
            "stub": False,
            "generator": "scripts/fixtures/gen_xlsx.py",
            "scenarios": ["S06"],
            "expect": {
                "outcome": "extracted",
                "sheets": [
                    {
                        "name": "Data",
                        "formulas": {},
                        "values": {
                            "A1": "Sample",
                            "B1": "Mass",
                            "A2": "Alpha",
                            "B2": 12,
                            "A3": "Beta",
                            "B3": 18,
                            "A4": "Gamma",
                            "B4": 30,
                        },
                    },
                    {
                        "name": "Summary",
                        "formulas": {
                            "B2": "=SUM(Data!B2:B4)",
                            "B3": "=B2/3",
                            "B4": "=Data!B4",
                        },
                        "values": {
                            "A1": "Metric",
                            "B1": "Value",
                            "A2": "Total Mass",
                            "B2": 60,
                            "A3": "Mean Mass",
                            "B3": 20,
                            "A4": "Peak Sample",
                            "B4": 30,
                        },
                    },
                ],
                "named_ranges": ["SampleMasses"],
            },
            "notes": "XlsxWriter workbook with cached formula values and a named range",
        },
        {
            "path": "xlsx/legacy.xls",
            "format": "xls",
            "stub": False,
            "generator": "scripts/fixtures/gen_xlsx.py",
            "scenarios": ["S06"],
            "expect": {
                "outcome": "extracted",
                "sheets": [
                    {
                        "name": "Data",
                        "formulas": {},
                        "values": {
                            "A1": "Sample",
                            "B1": "Mass",
                            "A2": "Alpha",
                            "B2": 12,
                            "A3": "Beta",
                            "B3": 18,
                        },
                    },
                ],
            },
            "notes": "Legacy BIFF8 workbook written with BSD-licensed xlwt",
        },
        {
            "path": "xlsx/sheet.ods",
            "format": "ods",
            "stub": False,
            "generator": "scripts/fixtures/gen_xlsx.py",
            "scenarios": ["S06"],
            "expect": {
                "outcome": "extracted",
                "sheets": [
                    {
                        "name": "Data",
                        "formulas": {"B4": "=SUM(B2:B3)"},
                        "values": {
                            "A1": "Item",
                            "B1": "Score",
                            "A2": "Quiz 1",
                            "B2": 14,
                            "A3": "Quiz 2",
                            "B3": 16,
                            "A4": "Total",
                            "B4": 30,
                        },
                    }
                ],
            },
            "notes": "Hand-built OpenDocument spreadsheet with stored mimetype entry first",
        },
    ]


def _build_formulas_xlsx() -> bytes:
    import xlsxwriter

    buf = io.BytesIO()
    wb = xlsxwriter.Workbook(buf, {"in_memory": True, "constant_memory": False})
    wb.set_properties(
        {
            "author": "fixture",
            "title": "Mass Balance Workbook",
            "subject": "Chemistry",
            "created": FIXED_DT_NAIVE,
        }
    )

    ws_data = wb.add_worksheet("Data")
    ws_data.write_row(0, 0, ["Sample", "Mass"])
    ws_data.write_row(1, 0, ["Alpha", 12])
    ws_data.write_row(2, 0, ["Beta", 18])
    ws_data.write_row(3, 0, ["Gamma", 30])

    ws_summary = wb.add_worksheet("Summary")
    ws_summary.write_row(0, 0, ["Metric", "Value"])
    ws_summary.write(1, 0, "Total Mass")
    ws_summary.write_formula(1, 1, "=SUM(Data!B2:B4)", value=60)
    ws_summary.write(2, 0, "Mean Mass")
    ws_summary.write_formula(2, 1, "=B2/3", value=20)
    ws_summary.write(3, 0, "Peak Sample")
    ws_summary.write_formula(3, 1, "=Data!B4", value=30)

    wb.define_name("SampleMasses", "=Data!$B$2:$B$4")
    wb.close()
    return repack_zip_deterministic(buf.getvalue())


def _build_legacy_xls() -> bytes:
    import xlwt

    wb = xlwt.Workbook()
    ws = wb.add_sheet("Data")
    ws.write(0, 0, "Sample")
    ws.write(0, 1, "Mass")
    ws.write(1, 0, "Alpha")
    ws.write(1, 1, 12)
    ws.write(2, 0, "Beta")
    ws.write(2, 1, 18)
    buf = io.BytesIO()
    wb.save(buf)
    return buf.getvalue()


def _build_sheet_ods() -> bytes:
    mimetype = b"application/vnd.oasis.opendocument.spreadsheet"
    manifest_xml = (
        b'<?xml version="1.0" encoding="UTF-8"?>\n'
        b'<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" '
        b'manifest:version="1.2">\n'
        b'  <manifest:file-entry manifest:full-path="/" '
        b'manifest:media-type="application/vnd.oasis.opendocument.spreadsheet"/>\n'
        b'  <manifest:file-entry manifest:full-path="content.xml" '
        b'manifest:media-type="text/xml"/>\n'
        b'  <manifest:file-entry manifest:full-path="meta.xml" manifest:media-type="text/xml"/>\n'
        b"</manifest:manifest>\n"
    )
    meta_xml = (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<office:document-meta xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" '
        'xmlns:dc="http://purl.org/dc/elements/1.1/" '
        'xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0" office:version="1.2">\n'
        "  <office:meta>\n"
        "    <dc:title>Score Sheet</dc:title>\n"
        "    <dc:creator>fixture</dc:creator>\n"
        f"    <meta:creation-date>{FIXED_ISO_DATE}</meta:creation-date>\n"
        f"    <dc:date>{FIXED_ISO_DATE}</dc:date>\n"
        "  </office:meta>\n"
        "</office:document-meta>\n"
    ).encode()
    content_xml = (
        b'<?xml version="1.0" encoding="UTF-8"?>\n'
        b"<office:document-content "
        b'xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" '
        b'xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" '
        b'xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" '
        b'xmlns:of="urn:oasis:names:tc:opendocument:xmlns:of:1.2" office:version="1.2">\n'
        b"  <office:body>\n"
        b"    <office:spreadsheet>\n"
        b'      <table:table table:name="Data">\n'
        b"        <table:table-row>\n"
        b'          <table:table-cell office:value-type="string">'
        b"<text:p>Item</text:p></table:table-cell>\n"
        b'          <table:table-cell office:value-type="string">'
        b"<text:p>Score</text:p></table:table-cell>\n"
        b"        </table:table-row>\n"
        b"        <table:table-row>\n"
        b'          <table:table-cell office:value-type="string">'
        b"<text:p>Quiz 1</text:p></table:table-cell>\n"
        b'          <table:table-cell office:value-type="float" office:value="14">'
        b"<text:p>14</text:p></table:table-cell>\n"
        b"        </table:table-row>\n"
        b"        <table:table-row>\n"
        b'          <table:table-cell office:value-type="string">'
        b"<text:p>Quiz 2</text:p></table:table-cell>\n"
        b'          <table:table-cell office:value-type="float" office:value="16">'
        b"<text:p>16</text:p></table:table-cell>\n"
        b"        </table:table-row>\n"
        b"        <table:table-row>\n"
        b'          <table:table-cell office:value-type="string">'
        b"<text:p>Total</text:p></table:table-cell>\n"
        b'          <table:table-cell table:formula="of:=SUM([.B2:.B3])" '
        b'office:value-type="float" office:value="30">'
        b"<text:p>30</text:p></table:table-cell>\n"
        b"        </table:table-row>\n"
        b"      </table:table>\n"
        b"    </office:spreadsheet>\n"
        b"  </office:body>\n"
        b"</office:document-content>\n"
    )

    out = io.BytesIO()
    with zipfile.ZipFile(out, "w", compression=zipfile.ZIP_DEFLATED) as zout:
        entries = [
            ("mimetype", mimetype, zipfile.ZIP_STORED),
            ("META-INF/manifest.xml", manifest_xml, zipfile.ZIP_DEFLATED),
            ("content.xml", content_xml, zipfile.ZIP_DEFLATED),
            ("meta.xml", meta_xml, zipfile.ZIP_DEFLATED),
        ]
        for name, data, comp_type in entries:
            info = zipfile.ZipInfo(name, date_time=FIXED_ZIP_TIME)
            info.compress_type = comp_type
            info.external_attr = 0o644 << 16
            zout.writestr(info, data)
    return out.getvalue()


def generate() -> None:
    """Write all xlsx/ fixtures to tests/fixtures/xlsx/."""
    write_bytes(FIXTURES_ROOT / "xlsx" / "formulas.xlsx", _build_formulas_xlsx())
    write_bytes(FIXTURES_ROOT / "xlsx" / "legacy.xls", _build_legacy_xls())
    write_bytes(FIXTURES_ROOT / "xlsx" / "sheet.ods", _build_sheet_ods())


def main() -> int:
    generate()
    return 0


if __name__ == "__main__":
    sys.exit(main())
