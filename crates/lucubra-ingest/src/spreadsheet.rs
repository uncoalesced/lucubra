// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Spreadsheet extraction using calamine (SPEC section 10.2, ADR 0011, S06).
//!
//! Emits a summary chunk per sheet (`x:<Sheet>!summary`) carrying dimensions,
//! headers, named ranges and formula count, followed by range chunks
//! (`x:<Sheet>!<A1>:<B2>`) with cached values and formula text.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::BufReader;
use std::path::Path;

use calamine::{Data, Ods, Range, Reader, Xls, Xlsx, open_workbook};

use crate::limits::MAX_FILE_BYTES;
use crate::native::{CHUNK_CHARS, Chunk, Extracted, Failure};
use crate::route::Format;
use crate::zipguard::check_zip;

const ROWS_PER_RANGE: usize = 40;

fn fail(code: &'static str, detail: impl Into<String>) -> Failure {
    Failure {
        code,
        detail: detail.into(),
    }
}

fn col_letters(mut col: u32) -> String {
    let mut out = Vec::new();
    loop {
        out.push((b'A' + (col % 26) as u8) as char);
        if col < 26 {
            break;
        }
        col = col / 26 - 1;
    }
    out.into_iter().rev().collect()
}

fn a1(row: u32, col: u32) -> String {
    format!("{}{}", col_letters(col), row + 1)
}

fn format_data(cell: &Data) -> Option<String> {
    match cell {
        Data::Empty => None,
        Data::String(s) => {
            let trimmed = s.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_owned())
        }
        Data::Int(i) => Some(i.to_string()),
        Data::Float(f) => {
            if f.is_finite()
                && f.fract() == 0.0
                && *f >= (i64::MIN / 2) as f64
                && *f <= (i64::MAX / 2) as f64
            {
                Some((*f as i64).to_string())
            } else {
                Some(f.to_string())
            }
        }
        Data::Bool(b) => Some(if *b { "TRUE" } else { "FALSE" }.into()),
        Data::Error(e) => Some(format!("#ERR:{e}")),
        Data::DateTime(dt) => Some(dt.to_string()),
        Data::DateTimeIso(s) | Data::DurationIso(s) => Some(s.clone()),
    }
}

fn map_open_err(err: impl std::fmt::Display) -> Failure {
    let msg = err.to_string();
    let lower = msg.to_ascii_lowercase();
    if lower.contains("password") || lower.contains("encrypt") {
        fail("encrypted", msg)
    } else {
        fail("corrupt", msg)
    }
}

fn collect_formulas(range: &Range<String>) -> BTreeMap<(u32, u32), String> {
    let mut out = BTreeMap::new();
    let (r0, c0) = range.start().unwrap_or((0, 0));
    for (dr, row) in range.rows().enumerate() {
        for (dc, text) in row.iter().enumerate() {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                let normalized = trimmed.strip_prefix('=').unwrap_or(trimmed).to_owned();
                out.insert((r0 + dr as u32, c0 + dc as u32), normalized);
            }
        }
    }
    out
}

fn collect_values(range: &Range<Data>) -> BTreeMap<(u32, u32), String> {
    let mut out = BTreeMap::new();
    let (r0, c0) = range.start().unwrap_or((0, 0));
    for (dr, row) in range.rows().enumerate() {
        for (dc, cell) in row.iter().enumerate() {
            if let Some(val) = format_data(cell) {
                out.insert((r0 + dr as u32, c0 + dc as u32), val);
            }
        }
    }
    out
}

fn push_sheet<R: Reader<BufReader<File>>>(
    wb: &mut R,
    sheet: &str,
    named_ranges: &[(String, String)],
    out: &mut Extracted,
) {
    let values = wb
        .worksheet_range(sheet)
        .map(|r| collect_values(&r))
        .unwrap_or_default();
    let formulas = wb
        .worksheet_formula(sheet)
        .map(|r| collect_formulas(&r))
        .unwrap_or_default();

    let mut coords: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    let mut min_col = u32::MAX;
    let mut max_col = 0u32;
    for &(r, c) in values.keys().chain(formulas.keys()) {
        coords.entry(r).or_default().push(c);
        min_col = min_col.min(c);
        max_col = max_col.max(c);
    }
    for cols in coords.values_mut() {
        cols.sort_unstable();
        cols.dedup();
    }

    let dimensions = match (coords.keys().next(), coords.keys().next_back()) {
        (Some(&r_min), Some(&r_max)) => format!("{}:{}", a1(r_min, min_col), a1(r_max, max_col)),
        _ => "empty".into(),
    };
    let headers: Vec<String> = coords
        .iter()
        .next()
        .map(|(&r, cols)| {
            cols.iter()
                .filter_map(|&c| values.get(&(r, c)).cloned())
                .collect()
        })
        .unwrap_or_default();

    let mut summary = format!(
        "Sheet: {sheet}\nDimensions: {dimensions}\nFormulas: {}",
        formulas.len()
    );
    if !headers.is_empty() {
        summary.push_str(&format!("\nHeaders: {}", headers.join(" | ")));
    }
    if !named_ranges.is_empty() {
        let joined: Vec<String> = named_ranges
            .iter()
            .map(|(n, f)| format!("{n}={f}"))
            .collect();
        summary.push_str(&format!("\nNamed ranges: {}", joined.join(", ")));
    }
    out.chunks.push(Chunk {
        locator: format!("x:{sheet}!summary"),
        text: summary,
    });

    let rows: Vec<u32> = coords.keys().copied().collect();
    if rows.is_empty() {
        return;
    }

    let mut idx = 0usize;
    while idx < rows.len() {
        let first_row = rows[idx];
        let mut last_row = first_row;
        let mut chunk_min_col = u32::MAX;
        let mut chunk_max_col = 0u32;
        let mut body = String::new();
        let mut count = 0usize;

        while idx < rows.len() && count < ROWS_PER_RANGE {
            let r = rows[idx];
            let mut parts = Vec::new();
            for &c in &coords[&r] {
                chunk_min_col = chunk_min_col.min(c);
                chunk_max_col = chunk_max_col.max(c);
                let cell_ref = a1(r, c);
                let val = values.get(&(r, c)).map(String::as_str);
                let form = formulas.get(&(r, c)).map(String::as_str);
                let rendered = match (val, form) {
                    (Some(v), Some(f)) => format!("{cell_ref}: {v} (={f})"),
                    (Some(v), None) => format!("{cell_ref}: {v}"),
                    (None, Some(f)) => format!("{cell_ref}: (={f})"),
                    (None, None) => continue,
                };
                parts.push(rendered);
            }
            let line = parts.join(" | ");
            if !body.is_empty() && body.len() + line.len() + 1 > CHUNK_CHARS {
                break;
            }
            if !body.is_empty() {
                body.push('\n');
            }
            body.push_str(&line);
            last_row = r;
            count += 1;
            idx += 1;
        }
        if !body.is_empty() {
            let loc = format!(
                "x:{sheet}!{}:{}",
                a1(first_row, chunk_min_col),
                a1(last_row, chunk_max_col)
            );
            out.chunks.push(Chunk {
                locator: loc,
                text: body,
            });
        }
    }
}

fn read_all<R: Reader<BufReader<File>>>(mut wb: R) -> Extracted {
    let mut out = Extracted::default();
    let sheets = wb.sheet_names();
    let named: Vec<(String, String)> = wb.defined_names().to_vec();
    out.units_hint = sheets.clone();
    for sheet in &sheets {
        push_sheet(&mut wb, sheet, &named, &mut out);
    }
    out
}

/// Extracts an `xlsx`, `xlsm`, `xls`, or `ods` workbook.
pub fn extract(path: &Path, format: Format) -> Result<Extracted, Failure> {
    if format == Format::Xls {
        let meta = fs::metadata(path).map_err(|e| fail("corrupt", e.to_string()))?;
        if meta.len() > MAX_FILE_BYTES {
            return Err(fail(
                "limit_exceeded",
                format!("{} bytes is over the input limit", meta.len()),
            ));
        }
    } else {
        check_zip(path)?;
    }

    let extracted = match format {
        Format::Xlsx | Format::Xlsm => {
            read_all(open_workbook::<Xlsx<_>, _>(path).map_err(map_open_err)?)
        }
        Format::Xls => read_all(open_workbook::<Xls<_>, _>(path).map_err(map_open_err)?),
        Format::Ods => read_all(open_workbook::<Ods<_>, _>(path).map_err(map_open_err)?),
        _ => return Err(fail("unsupported", "not a spreadsheet format")),
    };
    Ok(extracted)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use zip::CompressionMethod;
    use zip::write::SimpleFileOptions;

    use super::*;

    fn write_minimal_xlsx(path: &Path) {
        let file = File::create(path).unwrap();
        let mut w = zip::ZipWriter::new(file);
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        let entries: &[(&str, &str)] = &[
            (
                "[Content_Types].xml",
                r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
  <Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
  <Override PartName="/xl/worksheets/sheet2.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
</Types>"#,
            ),
            (
                "_rels/.rels",
                r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>"#,
            ),
            (
                "xl/workbook.xml",
                r#"<?xml version="1.0" encoding="UTF-8"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <sheets>
    <sheet name="Data" sheetId="1" r:id="rId1"/>
    <sheet name="Summary" sheetId="2" r:id="rId2"/>
  </sheets>
  <definedNames>
    <definedName name="ScoreRange">Data!$A$2:$A$3</definedName>
  </definedNames>
</workbook>"#,
            ),
            (
                "xl/_rels/workbook.xml.rels",
                r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/>
</Relationships>"#,
            ),
            (
                "xl/worksheets/sheet1.xml",
                r#"<?xml version="1.0" encoding="UTF-8"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <sheetData>
    <row r="1"><c r="A1" t="inlineStr"><is><t>Value</t></is></c></row>
    <row r="2"><c r="A2"><v>10</v></c></row>
    <row r="3"><c r="A3"><v>25</v></c></row>
  </sheetData>
</worksheet>"#,
            ),
            (
                "xl/worksheets/sheet2.xml",
                r#"<?xml version="1.0" encoding="UTF-8"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <sheetData>
    <row r="1"><c r="A1" t="inlineStr"><is><t>Metric</t></is></c><c r="B1" t="inlineStr"><is><t>Result</t></is></c></row>
    <row r="2"><c r="A2" t="inlineStr"><is><t>Total</t></is></c><c r="B2"><f>SUM(Data!A2:A3)</f><v>35</v></c></row>
  </sheetData>
</worksheet>"#,
            ),
        ];
        for (name, body) in entries {
            w.start_file(*name, opts).unwrap();
            w.write_all(body.as_bytes()).unwrap();
        }
        w.finish().unwrap();
    }

    #[test]
    fn col_letters_cover_single_and_double_letters() {
        assert_eq!(col_letters(0), "A");
        assert_eq!(col_letters(25), "Z");
        assert_eq!(col_letters(26), "AA");
        assert_eq!(a1(1, 2), "C2");
    }

    #[test]
    fn extracts_xlsx_summary_values_formulas_and_named_ranges() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("formulas.xlsx");
        write_minimal_xlsx(&path);
        let out = extract(&path, Format::Xlsx).unwrap();
        assert_eq!(out.units_hint, ["Data", "Summary"]);
        let summary = out
            .chunks
            .iter()
            .find(|c| c.locator == "x:Summary!summary")
            .unwrap();
        assert!(summary.text.contains("Formulas: 1"));
        assert!(summary.text.contains("ScoreRange=Data!$A$2:$A$3"));
        let range = out
            .chunks
            .iter()
            .find(|c| c.locator == "x:Summary!A1:B2")
            .unwrap();
        assert!(range.text.contains("B2: 35 (=SUM(Data!A2:A3))"));
    }

    #[test]
    fn rejects_corrupt_xls() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.xls");
        fs::write(&path, b"not a workbook").unwrap();
        let err = extract(&path, Format::Xls).unwrap_err();
        assert_eq!(err.code, "corrupt");
    }
}
