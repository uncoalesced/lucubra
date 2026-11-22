// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! EPUB extraction using zipguard, quick-xml and HTML stripping (SPEC 10.2, ADR 0011).

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::Event;
use zip::ZipArchive;

use crate::native::{Chunk, Extracted, Failure, split_text, strip_html};
use crate::zipguard::check_zip;

fn fail(code: &'static str, detail: impl Into<String>) -> Failure {
    Failure {
        code,
        detail: detail.into(),
    }
}

fn read_entry(archive: &mut ZipArchive<BufReader<File>>, name: &str) -> Result<String, Failure> {
    let mut entry = archive
        .by_name(name)
        .map_err(|e| fail("corrupt", format!("missing EPUB entry {name}: {e}")))?;
    let mut buf = Vec::new();
    entry
        .read_to_end(&mut buf)
        .map_err(|e| fail("corrupt", format!("reading EPUB entry {name}: {e}")))?;
    let text = std::str::from_utf8(buf.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&buf))
        .map_err(|e| fail("corrupt", format!("EPUB entry {name} is not UTF-8: {e}")))?;
    Ok(text.replace("\r\n", "\n").replace('\r', "\n"))
}

fn attr_value(e: &quick_xml::events::BytesStart<'_>, target: &str) -> Option<String> {
    for attr in e.attributes().flatten() {
        if attr.key.local_name().as_ref() == target {
            return attr
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .ok()
                .map(|s| s.into_owned());
        }
    }
    None
}

fn container_opf(xml: &str) -> Result<String, Failure> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    loop {
        match reader.read_event() {
            Ok(Event::Empty(e) | Event::Start(e)) if e.local_name().as_ref() == "rootfile" => {
                if let Some(path) = attr_value(&e, "full-path") {
                    return Ok(path);
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(fail("corrupt", format!("invalid container.xml: {e}"))),
            _ => {}
        }
    }
    Err(fail("corrupt", "container.xml has no rootfile full-path"))
}

fn parse_opf(xml: &str) -> Result<(BTreeMap<String, String>, Vec<String>), Failure> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut manifest = BTreeMap::new();
    let mut spine = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Empty(e) | Event::Start(e)) => {
                let name = e.local_name();
                if name.as_ref() == "item" {
                    if let (Some(id), Some(href)) = (attr_value(&e, "id"), attr_value(&e, "href")) {
                        manifest.insert(id, href);
                    }
                } else if name.as_ref() == "itemref"
                    && let Some(idref) = attr_value(&e, "idref")
                {
                    spine.push(idref);
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(fail("corrupt", format!("invalid OPF package: {e}"))),
            _ => {}
        }
    }
    Ok((manifest, spine))
}

fn resolve_href(opf_path: &str, href: &str) -> String {
    let href = href.split('#').next().unwrap_or(href);
    let mut parts: Vec<&str> = match opf_path.rsplit_once('/') {
        Some((dir, _)) => dir.split('/').filter(|p| !p.is_empty()).collect(),
        None => Vec::new(),
    };
    for seg in href.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

/// Extracts an EPUB file into chapter chunks (`h:<chapter>`).
pub fn extract(path: &Path) -> Result<Extracted, Failure> {
    check_zip(path)?;
    let file = File::open(path).map_err(|e| fail("corrupt", e.to_string()))?;
    let mut archive = ZipArchive::new(BufReader::new(file))
        .map_err(|e| fail("corrupt", format!("damaged EPUB: {e}")))?;

    let container = read_entry(&mut archive, "META-INF/container.xml")?;
    let opf_path = container_opf(&container)?;
    let opf_xml = read_entry(&mut archive, &opf_path)?;
    let (manifest, spine) = parse_opf(&opf_xml)?;

    let mut out = Extracted::default();
    let mut chapter = 0usize;
    for idref in spine {
        let Some(href) = manifest.get(&idref) else {
            continue;
        };
        let entry_path = resolve_href(&opf_path, href);
        let Ok(xhtml) = read_entry(&mut archive, &entry_path) else {
            continue;
        };
        let (cleaned, hints) = strip_html(&xhtml);
        out.units_hint.extend(hints);
        if cleaned.trim().is_empty() {
            continue;
        }
        chapter += 1;
        for (part, piece) in split_text(&cleaned).into_iter().enumerate() {
            let locator = if part == 0 {
                format!("h:{chapter}")
            } else {
                format!("h:{chapter}.{part}")
            };
            out.chunks.push(Chunk {
                locator,
                text: piece,
            });
        }
    }
    if out.chunks.is_empty() {
        return Err(fail("corrupt", "EPUB has no readable spine chapters"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use zip::CompressionMethod;
    use zip::write::SimpleFileOptions;

    use super::*;

    fn write_test_epub(path: &Path) {
        let file = File::create(path).unwrap();
        let mut w = zip::ZipWriter::new(file);
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        w.start_file("mimetype", stored).unwrap();
        w.write_all(b"application/epub+zip").unwrap();
        let files: &[(&str, &str)] = &[
            (
                "META-INF/container.xml",
                r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
            ),
            (
                "OEBPS/content.opf",
                r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <manifest>
    <item id="ch1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
    <item id="ch2" href="sub/ch2.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="ch1"/>
    <itemref idref="ch2"/>
  </spine>
</package>"#,
            ),
            (
                "OEBPS/ch1.xhtml",
                "<html><body><h1>Chapter 1 Thermodynamics</h1><p>Entropy rises in closed systems.</p></body></html>",
            ),
            (
                "OEBPS/sub/ch2.xhtml",
                "<html><body><h2>Chapter 2 Equilibrium</h2><p>Gibbs free energy reaches a minimum.</p></body></html>",
            ),
        ];
        for (name, body) in files {
            w.start_file(*name, deflated).unwrap();
            w.write_all(body.as_bytes()).unwrap();
        }
        w.finish().unwrap();
    }

    #[test]
    fn extracts_epub_chapters_and_headings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("book.epub");
        write_test_epub(&path);
        let out = extract(&path).unwrap();
        assert_eq!(
            out.units_hint,
            ["Chapter 1 Thermodynamics", "Chapter 2 Equilibrium"]
        );
        assert_eq!(out.chunks.len(), 2);
        assert_eq!(out.chunks[0].locator, "h:1");
        assert!(out.chunks[0].text.contains("Entropy rises"));
        assert_eq!(out.chunks[1].locator, "h:2");
        assert!(out.chunks[1].text.contains("Gibbs free energy"));
    }

    #[test]
    fn rejects_corrupt_epub() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.epub");
        std::fs::write(&path, b"not an epub").unwrap();
        assert_eq!(extract(&path).unwrap_err().code, "corrupt");
    }
}
