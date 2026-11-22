// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Native Tier 1 extractors (SPEC section 10.2, ADR 0011).
//!
//! Each extractor turns a file into chunks with stable locators:
//! `h:<section>` or `h:<section>.<part>` for text, `c:<cell>` for notebooks,
//! `r:<first>-<last>` for delimited rows (line numbers, header is line 1).

use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::limits::{MAX_FILE_BYTES, MAX_TEXT_BYTES};
use crate::route::Format;

/// Target chunk size in characters (about 400 to 600 tokens).
pub const CHUNK_CHARS: usize = 2000;
/// Data rows per delimited-text chunk.
pub const ROWS_PER_CHUNK: usize = 50;
/// Characters of a notebook cell output kept.
const OUTPUT_CHARS: usize = 500;

/// One chunk of extracted text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    /// Locator within the file, such as `h:2` or `p:12`.
    pub locator: String,
    /// Text, LF line endings.
    pub text: String,
}

/// The result of a successful extraction.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Extracted {
    /// Extractor name and version.
    pub extractor: String,
    /// Document title metadata when present.
    pub title: Option<String>,
    /// Document subject metadata when present.
    pub subject: Option<String>,
    /// Total page count when reported by the extractor (e.g., PDF).
    pub pages: Option<u32>,
    /// Total slide count when reported by the extractor (e.g., PPTX).
    pub slides: Option<u32>,
    /// Chunks in document order.
    pub chunks: Vec<Chunk>,
    /// Pages, slides or images needing vision (1-based).
    pub needs_vision: Vec<u32>,
    /// Headings or outline entries that may name units.
    pub units_hint: Vec<String>,
    /// True when text past the cap was dropped.
    pub truncated: bool,
}

/// A failed extraction with an ADR 0011 code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// Failure code.
    pub code: &'static str,
    /// Reason.
    pub detail: String,
}

fn failure(code: &'static str, detail: impl Into<String>) -> Failure {
    Failure {
        code,
        detail: detail.into(),
    }
}

fn extractor_name() -> String {
    format!("lucubra-native@{}", env!("CARGO_PKG_VERSION"))
}

/// Decodes UTF-8, drops a BOM and normalises line endings to LF.
pub fn decode_text(bytes: &[u8]) -> Result<String, Failure> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let text = std::str::from_utf8(bytes)
        .map_err(|e| failure("corrupt", format!("not UTF-8 text: {e}")))?;
    Ok(text.replace("\r\n", "\n").replace('\r', "\n"))
}

/// Splits text into pieces of at most `CHUNK_CHARS` characters, at paragraph
/// breaks where possible, then at line breaks, then at character boundaries.
pub(crate) fn split_text(text: &str) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut current = String::new();
    let flush = |current: &mut String, pieces: &mut Vec<String>| {
        if !current.trim().is_empty() {
            pieces.push(std::mem::take(current));
        } else {
            current.clear();
        }
    };
    for para in text.split("\n\n") {
        let para = para.trim_matches('\n');
        if para.trim().is_empty() {
            continue;
        }
        let para_len = para.chars().count();
        if para_len <= CHUNK_CHARS {
            if current.chars().count() + para_len + 2 > CHUNK_CHARS {
                flush(&mut current, &mut pieces);
            }
            if !current.is_empty() {
                current.push_str("\n\n");
            }
            current.push_str(para);
            continue;
        }
        flush(&mut current, &mut pieces);
        for line in para.split('\n') {
            let mut rest: &str = line;
            while rest.chars().count() > CHUNK_CHARS {
                let cut = rest
                    .char_indices()
                    .nth(CHUNK_CHARS)
                    .map_or(rest.len(), |(i, _)| i);
                flush(&mut current, &mut pieces);
                pieces.push(rest[..cut].to_owned());
                rest = &rest[cut..];
            }
            if current.chars().count() + rest.chars().count() + 1 > CHUNK_CHARS {
                flush(&mut current, &mut pieces);
            }
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(rest);
        }
        flush(&mut current, &mut pieces);
    }
    flush(&mut current, &mut pieces);
    pieces
}

/// Adds one section's pieces with `h:<section>` and `h:<section>.<part>`.
fn push_section(out: &mut Vec<Chunk>, section: usize, text: &str) {
    for (part, piece) in split_text(text).into_iter().enumerate() {
        let locator = if part == 0 {
            format!("h:{section}")
        } else {
            format!("h:{section}.{part}")
        };
        out.push(Chunk {
            locator,
            text: piece,
        });
    }
}

fn heading(line: &str) -> Option<(usize, &str)> {
    let level = line.bytes().take_while(|&b| b == b'#').count();
    if (1..=6).contains(&level) && line[level..].starts_with(' ') {
        Some((level, line[level..].trim()))
    } else {
        None
    }
}

fn markdown(text: &str) -> Extracted {
    let mut out = Extracted::default();
    let mut section = 0usize;
    let mut body = String::new();
    let mut fenced = false;
    for line in text.split('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
        }
        if !fenced && let Some((level, title)) = heading(line) {
            push_section(&mut out.chunks, section, &body);
            body.clear();
            section += 1;
            if level <= 2 && !title.is_empty() {
                out.units_hint.push(title.to_owned());
            }
        }
        body.push_str(line);
        body.push('\n');
    }
    push_section(&mut out.chunks, section, &body);
    out
}

fn plain(text: &str) -> Extracted {
    let mut out = Extracted::default();
    push_section(&mut out.chunks, 0, text);
    out
}

/// Tags whose contents are dropped.
const HTML_SKIP: &[&str] = &["script", "style", "head", "noscript", "template"];
/// Tags that end a line of text.
const HTML_BLOCK: &[&str] = &[
    "p",
    "div",
    "br",
    "li",
    "tr",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "section",
    "article",
    "table",
    "ul",
    "ol",
    "pre",
    "blockquote",
];

fn decode_entities(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

pub(crate) fn strip_html(text: &str) -> (String, Vec<String>) {
    let mut out_text = String::new();
    let mut hints = Vec::new();
    let mut skipping: Option<String> = None;
    let mut heading_text: Option<String> = None;
    let mut rest = text;
    while let Some(lt) = rest.find('<') {
        let before = &rest[..lt];
        if skipping.is_none() {
            out_text.push_str(before);
            if let Some(h) = heading_text.as_mut() {
                h.push_str(before);
            }
        }
        let Some(gt) = rest[lt..].find('>') else {
            rest = "";
            break;
        };
        let tag = &rest[lt + 1..lt + gt];
        rest = &rest[lt + gt + 1..];
        if tag.starts_with('!') {
            continue;
        }
        let closing = tag.starts_with('/');
        let name = tag
            .trim_start_matches('/')
            .chars()
            .take_while(char::is_ascii_alphanumeric)
            .collect::<String>()
            .to_ascii_lowercase();
        if let Some(skipped) = &skipping {
            if closing && &name == skipped {
                skipping = None;
            }
            continue;
        }
        if !closing && HTML_SKIP.contains(&name.as_str()) && !tag.ends_with('/') {
            skipping = Some(name);
            continue;
        }
        if matches!(name.as_str(), "h1" | "h2") {
            if closing {
                if let Some(h) = heading_text.take() {
                    let h = decode_entities(h.trim());
                    if !h.is_empty() {
                        hints.push(h);
                    }
                }
            } else {
                heading_text = Some(String::new());
            }
        }
        if HTML_BLOCK.contains(&name.as_str()) {
            out_text.push('\n');
        }
    }
    if skipping.is_none() {
        out_text.push_str(rest);
    }
    let decoded = decode_entities(&out_text);
    let mut cleaned = String::new();
    let mut blank = 0;
    for line in decoded.split('\n').map(str::trim) {
        if line.is_empty() {
            blank += 1;
            continue;
        }
        if !cleaned.is_empty() {
            cleaned.push_str(if blank > 0 { "\n\n" } else { "\n" });
        }
        blank = 0;
        cleaned.push_str(line);
    }
    (cleaned, hints)
}

fn html(text: &str) -> Extracted {
    let (cleaned, hints) = strip_html(text);
    let mut out = plain(&cleaned);
    out.units_hint = hints;
    out
}

fn rows_chunk(header: &str, rows: &[String], first: usize) -> Chunk {
    let mut text = header.to_owned();
    for row in rows {
        text.push('\n');
        text.push_str(row);
    }
    let locator = if rows.is_empty() {
        "r:1-1".to_owned()
    } else {
        format!("r:{first}-{}", first + rows.len() - 1)
    };
    Chunk { locator, text }
}

fn delimited(bytes: &[u8], delimiter: u8) -> Result<Extracted, Failure> {
    let text = decode_text(bytes)?;
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .flexible(true)
        .from_reader(text.as_bytes());
    let sep = if delimiter == b'\t' { "\t" } else { ", " };
    let mut header: Option<String> = None;
    let mut out = Extracted::default();
    let mut rows: Vec<String> = Vec::new();
    let mut first = 2usize;
    for (index, record) in reader.records().enumerate() {
        let record = record.map_err(|e| failure("corrupt", format!("row {}: {e}", index + 1)))?;
        let line = record.iter().collect::<Vec<_>>().join(sep);
        let Some(head) = &header else {
            header = Some(line);
            continue;
        };
        rows.push(line);
        if rows.len() == ROWS_PER_CHUNK {
            out.chunks.push(rows_chunk(head, &rows, first));
            first += rows.len();
            rows.clear();
        }
    }
    if let Some(head) = &header
        && (!rows.is_empty() || out.chunks.is_empty())
    {
        out.chunks.push(rows_chunk(head, &rows, first));
    }
    Ok(out)
}

fn source_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts.iter().filter_map(Value::as_str).collect(),
        _ => String::new(),
    }
}

fn notebook(bytes: &[u8]) -> Result<Extracted, Failure> {
    let text = decode_text(bytes)?;
    let doc: Value = serde_json::from_str(&text)
        .map_err(|e| failure("corrupt", format!("notebook is not JSON: {e}")))?;
    let cells = doc
        .get("cells")
        .and_then(Value::as_array)
        .ok_or_else(|| failure("corrupt", "notebook has no cells array"))?;
    let mut out = Extracted::default();
    for (i, cell) in cells.iter().enumerate() {
        let kind = cell
            .get("cell_type")
            .and_then(Value::as_str)
            .unwrap_or("code");
        let mut body = source_text(cell.get("source").unwrap_or(&Value::Null));
        if kind == "markdown" {
            for line in body.split('\n') {
                if let Some((level, title)) = heading(line)
                    && level <= 2
                {
                    out.units_hint.push(title.to_owned());
                }
            }
        }
        if let Some(outputs) = cell.get("outputs").and_then(Value::as_array) {
            for output in outputs {
                let shown = output
                    .get("text")
                    .or_else(|| output.get("data").and_then(|d| d.get("text/plain")))
                    .map(source_text)
                    .unwrap_or_default();
                if shown.trim().is_empty() {
                    continue;
                }
                let trimmed: String = shown.chars().take(OUTPUT_CHARS).collect();
                body.push_str("\n[output]\n");
                body.push_str(trimmed.trim_end());
            }
        }
        for (part, piece) in split_text(&body).into_iter().enumerate() {
            let locator = if part == 0 {
                format!("c:{}", i + 1)
            } else {
                format!("c:{}.{part}", i + 1)
            };
            out.chunks.push(Chunk {
                locator,
                text: piece,
            });
        }
    }
    Ok(out)
}

/// Caps the total chunk text at `MAX_TEXT_BYTES`.
fn cap(mut out: Extracted) -> Extracted {
    let mut total = 0usize;
    let mut keep = out.chunks.len();
    for (i, chunk) in out.chunks.iter().enumerate() {
        total += chunk.text.len();
        if total > MAX_TEXT_BYTES {
            keep = i;
            out.truncated = true;
            break;
        }
    }
    out.chunks.truncate(keep);
    out
}

fn short_timestamp(raw: &str) -> String {
    let token = raw.split_whitespace().next().unwrap_or("");
    token.split(['.', ',']).next().unwrap_or(token).to_owned()
}

fn subtitle(text: &str) -> Extracted {
    let mut cues: Vec<(String, String, String)> = Vec::new();
    for block in text.split("\n\n") {
        let lines: Vec<&str> = block
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        if lines.is_empty() {
            continue;
        }
        let head = lines[0];
        if head.starts_with("WEBVTT")
            || head.starts_with("NOTE")
            || head.starts_with("STYLE")
            || head.starts_with("REGION")
        {
            continue;
        }
        let Some(arrow_idx) = lines.iter().position(|l| l.contains("-->")) else {
            continue;
        };
        let (left, right) = lines[arrow_idx].split_once("-->").unwrap_or(("", ""));
        let start = short_timestamp(left.trim());
        let end = short_timestamp(right.trim());
        let raw_body = lines[arrow_idx + 1..].join(" ");
        let (clean, _) = strip_html(&raw_body);
        let body = clean.split_whitespace().collect::<Vec<_>>().join(" ");
        if !body.is_empty() {
            cues.push((start, end, body));
        }
    }
    if cues.is_empty() {
        return plain(text);
    }
    let mut out = Extracted::default();
    let mut chunk_start = String::new();
    let mut chunk_end = String::new();
    let mut chunk_text = String::new();
    for (start, end, body) in cues {
        let entry = format!("[{start}] {body}");
        if !chunk_text.is_empty()
            && chunk_text.chars().count() + entry.chars().count() + 1 > CHUNK_CHARS
        {
            out.chunks.push(Chunk {
                locator: format!("t:{chunk_start}-{chunk_end}"),
                text: std::mem::take(&mut chunk_text),
            });
            chunk_start.clear();
        }
        if chunk_start.is_empty() {
            chunk_start = start;
        }
        chunk_end = end;
        if !chunk_text.is_empty() {
            chunk_text.push('\n');
        }
        chunk_text.push_str(&entry);
    }
    if !chunk_text.is_empty() {
        out.chunks.push(Chunk {
            locator: format!("t:{chunk_start}-{chunk_end}"),
            text: chunk_text,
        });
    }
    out
}

/// Formats handled natively by this module.
pub fn handles(format: Format) -> bool {
    format.extractor() == crate::route::Extractor::Native
}

/// Extracts a file with a native extractor.
pub fn extract_native(path: &Path, format: Format) -> Result<Extracted, Failure> {
    if !handles(format) {
        return Err(failure(
            "unsupported",
            format!("no native extractor for {}", format.as_str()),
        ));
    }
    let meta = fs::metadata(path).map_err(|e| failure("corrupt", e.to_string()))?;
    if meta.len() > MAX_FILE_BYTES {
        return Err(failure(
            "limit_exceeded",
            format!("{} bytes is over the input limit", meta.len()),
        ));
    }
    let mut out = if format == Format::Image {
        Extracted {
            needs_vision: vec![1],
            ..Extracted::default()
        }
    } else if matches!(
        format,
        Format::Xlsx | Format::Xlsm | Format::Xls | Format::Ods
    ) {
        crate::spreadsheet::extract(path, format)?
    } else if format == Format::Epub {
        crate::epub::extract(path)?
    } else {
        let bytes = fs::read(path).map_err(|e| failure("corrupt", e.to_string()))?;
        match format {
            Format::Md => markdown(&decode_text(&bytes)?),
            Format::Txt | Format::Code => plain(&decode_text(&bytes)?),
            Format::Html => html(&decode_text(&bytes)?),
            Format::Csv => delimited(&bytes, b',')?,
            Format::Tsv => delimited(&bytes, b'\t')?,
            Format::Ipynb => notebook(&bytes)?,
            Format::Subtitle => subtitle(&decode_text(&bytes)?),
            Format::KicadSch
            | Format::KicadPcb
            | Format::KicadPro
            | Format::Spice
            | Format::LtspiceAsc
            | Format::Gerber => crate::eda::extract(&decode_text(&bytes)?, format)?,
            _ => unreachable!("checked by handles"),
        }
    };
    out.extractor = extractor_name();
    Ok(cap(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn locators(out: &Extracted) -> Vec<&str> {
        out.chunks.iter().map(|c| c.locator.as_str()).collect()
    }

    #[test]
    fn native_markdown_sections_and_hints() {
        let text = "Intro line\n\n# Unit 1 Cells\nCells are small.\n\n```\n# not a heading\n```\n## Membranes\nLipids.\n### Detail\nMore.\n";
        let out = markdown(text);
        assert_eq!(locators(&out), ["h:0", "h:1", "h:2", "h:3"]);
        assert_eq!(out.units_hint, ["Unit 1 Cells", "Membranes"]);
        assert!(out.chunks[1].text.contains("# not a heading"));
    }

    #[test]
    fn native_long_text_splits_into_parts() {
        let para = "word ".repeat(300);
        let text = format!("{para}\n\n{para}\n\n{para}");
        let out = plain(&text);
        assert_eq!(locators(&out), ["h:0", "h:0.1", "h:0.2"]);
        assert!(
            out.chunks
                .iter()
                .all(|c| c.text.chars().count() <= CHUNK_CHARS)
        );
        let one_line = "x".repeat(CHUNK_CHARS * 2 + 10);
        let out = plain(&one_line);
        assert_eq!(out.chunks.len(), 3);
        assert_eq!(
            out.chunks.iter().map(|c| c.text.len()).sum::<usize>(),
            one_line.len()
        );
    }

    #[test]
    fn native_decode_strips_bom_and_crlf() {
        assert_eq!(decode_text(b"\xEF\xBB\xBFa\r\nb\rc").unwrap(), "a\nb\nc");
        assert_eq!(decode_text(&[0xFF, 0xFE]).unwrap_err().code, "corrupt");
    }

    #[test]
    fn native_html_drops_scripts_and_keeps_headings() {
        let page = "<html><head><title>T</title><style>p{}</style></head><body><h1>Unit 2 Energy</h1><script>var x = '<p>';</script><p>ATP &amp; ADP</p><p>Second</p></body></html>";
        let out = html(page);
        assert_eq!(out.units_hint, ["Unit 2 Energy"]);
        let text = &out.chunks[0].text;
        assert!(text.contains("ATP & ADP"));
        assert!(!text.contains("var x"));
        assert!(!text.contains("p{}"));
    }

    #[test]
    fn native_csv_rows_carry_line_numbers() {
        let mut data = String::from("name,score\n");
        for i in 0..120 {
            data.push_str(&format!("s{i},{i}\n"));
        }
        let out = delimited(data.as_bytes(), b',').unwrap();
        assert_eq!(locators(&out), ["r:2-51", "r:52-101", "r:102-121"]);
        assert!(out.chunks[2].text.starts_with("name, score\ns100, 100"));
        let tsv = delimited("a\tb\n1\t2\n".as_bytes(), b'\t').unwrap();
        assert_eq!(tsv.chunks[0].text, "a\tb\n1\t2");
        let header_only = delimited(b"a,b\n", b',').unwrap();
        assert_eq!(locators(&header_only), ["r:1-1"]);
    }

    #[test]
    fn native_notebook_cells_and_outputs() {
        let nb = r##"{"cells": [
            {"cell_type": "markdown", "source": ["# Unit 3 Waves\n", "Intro"]},
            {"cell_type": "code", "source": "print(2 + 2)", "outputs": [{"output_type": "stream", "text": ["4\n"]}]},
            {"cell_type": "code", "source": "", "outputs": []}
        ]}"##;
        let out = notebook(nb.as_bytes()).unwrap();
        assert_eq!(locators(&out), ["c:1", "c:2"]);
        assert_eq!(out.units_hint, ["Unit 3 Waves"]);
        assert_eq!(out.chunks[1].text, "print(2 + 2)\n[output]\n4");
        assert_eq!(notebook(b"{}").unwrap_err().code, "corrupt");
    }

    #[test]
    fn native_extract_file_sets_extractor_and_vision() {
        let dir = tempfile::tempdir().unwrap();
        let md = dir.path().join("a.md");
        fs::write(&md, "# Unit 1\nText\n").unwrap();
        let out = extract_native(&md, Format::Md).unwrap();
        assert!(out.extractor.starts_with("lucubra-native@"));
        let img = dir.path().join("a.png");
        fs::write(&img, b"\x89PNG\r\n\x1a\n").unwrap();
        assert_eq!(
            extract_native(&img, Format::Image).unwrap().needs_vision,
            [1]
        );
        assert_eq!(
            extract_native(&md, Format::Pdf).unwrap_err().code,
            "unsupported"
        );
    }

    #[test]
    fn native_subtitle_vtt_and_srt_keep_timestamps() {
        let vtt = "WEBVTT\n\n00:00:01.000 --> 00:00:04.200\n<v Prof>Today we study entropy.</v>\n\n00:01:05.500 --> 00:01:12.000\nSecond law of thermodynamics.\n";
        let out = subtitle(vtt);
        assert_eq!(out.chunks.len(), 1);
        assert_eq!(out.chunks[0].locator, "t:00:00:01-00:01:12");
        assert!(
            out.chunks[0]
                .text
                .contains("[00:00:01] Today we study entropy.")
        );
        assert!(
            out.chunks[0]
                .text
                .contains("[00:01:05] Second law of thermodynamics.")
        );

        let srt = "1\n00:00:10,000 --> 00:00:15,500\nKirchhoff current law.\n";
        let srt_out = subtitle(srt);
        assert_eq!(srt_out.chunks[0].locator, "t:00:00:10-00:00:15");
        assert!(srt_out.chunks[0].text.contains("Kirchhoff current law."));
    }
}
