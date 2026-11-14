// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Format routing by magic bytes, then extension (ADR 0011 section 2).

use std::fs::File;
use std::io::Read;
use std::path::Path;

use lucubra_core::{Error, Result};

/// Bytes read from the start of a file for routing. DICOM's marker sits at
/// offset 128, and a PDF header may follow up to 1,024 bytes of junk.
pub const HEAD_LEN: usize = 1024;

/// A routed format. `as_str` gives the identifier used in events, the fixture
/// manifest and `lucubra doctor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Format {
    Pdf,
    Docx,
    Pptx,
    Xlsx,
    Xlsm,
    Xls,
    Ods,
    Epub,
    Csv,
    Tsv,
    Md,
    Txt,
    Html,
    Ipynb,
    Code,
    Image,
    Dxf,
    KicadSch,
    KicadPcb,
    KicadPro,
    Spice,
    LtspiceAsc,
    Gerber,
    Dicom,
    Dwg,
    Blend,
    Rvt,
    Skp,
    Unknown,
}

/// How a format is extracted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Extractor {
    /// Built into the Rust engine.
    Native,
    /// A Python worker.
    Worker,
    /// An external tool the user installs (Tier 2).
    Tool(&'static str),
    /// Known, but no reader in v1.
    Unsupported,
    /// Not extracted.
    None,
}

impl Format {
    /// Every format, in declaration order.
    pub const ALL: [Format; 29] = [
        Format::Pdf,
        Format::Docx,
        Format::Pptx,
        Format::Xlsx,
        Format::Xlsm,
        Format::Xls,
        Format::Ods,
        Format::Epub,
        Format::Csv,
        Format::Tsv,
        Format::Md,
        Format::Txt,
        Format::Html,
        Format::Ipynb,
        Format::Code,
        Format::Image,
        Format::Dxf,
        Format::KicadSch,
        Format::KicadPcb,
        Format::KicadPro,
        Format::Spice,
        Format::LtspiceAsc,
        Format::Gerber,
        Format::Dicom,
        Format::Dwg,
        Format::Blend,
        Format::Rvt,
        Format::Skp,
        Format::Unknown,
    ];

    /// The ADR 0011 identifier.
    pub fn as_str(self) -> &'static str {
        match self {
            Format::Pdf => "pdf",
            Format::Docx => "docx",
            Format::Pptx => "pptx",
            Format::Xlsx => "xlsx",
            Format::Xlsm => "xlsm",
            Format::Xls => "xls",
            Format::Ods => "ods",
            Format::Epub => "epub",
            Format::Csv => "csv",
            Format::Tsv => "tsv",
            Format::Md => "md",
            Format::Txt => "txt",
            Format::Html => "html",
            Format::Ipynb => "ipynb",
            Format::Code => "code",
            Format::Image => "image",
            Format::Dxf => "dxf",
            Format::KicadSch => "kicad_sch",
            Format::KicadPcb => "kicad_pcb",
            Format::KicadPro => "kicad_pro",
            Format::Spice => "spice",
            Format::LtspiceAsc => "ltspice_asc",
            Format::Gerber => "gerber",
            Format::Dicom => "dicom",
            Format::Dwg => "dwg",
            Format::Blend => "blend",
            Format::Rvt => "rvt",
            Format::Skp => "skp",
            Format::Unknown => "unknown",
        }
    }

    /// Parses an identifier.
    pub fn parse(id: &str) -> Option<Format> {
        Format::ALL.into_iter().find(|f| f.as_str() == id)
    }

    /// Which extractor handles the format.
    pub fn extractor(self) -> Extractor {
        match self {
            Format::Pdf | Format::Docx | Format::Pptx | Format::Dxf | Format::Dicom => {
                Extractor::Worker
            }
            Format::Dwg => Extractor::Tool("oda"),
            Format::Blend => Extractor::Tool("blender"),
            Format::Rvt | Format::Skp => Extractor::Unsupported,
            Format::Unknown => Extractor::None,
            _ => Extractor::Native,
        }
    }
}

const CODE_EXTENSIONS: &[&str] = &[
    "c", "h", "cc", "cpp", "cxx", "hpp", "hh", "java", "kt", "scala", "py", "rs", "go", "js",
    "mjs", "ts", "tsx", "jsx", "rb", "php", "swift", "m", "r", "jl", "sql", "sh", "ps1", "lua",
    "hs", "ml", "cs", "fs", "vb", "pl", "dart", "v", "vhd", "vhdl", "sv", "asm", "s", "tex", "bib",
    "yaml", "yml", "toml", "json", "xml", "css",
];

const GERBER_EXTENSIONS: &[&str] = &[
    "gbr", "ger", "gtl", "gbl", "gts", "gbs", "gto", "gbo", "gtp", "gbp", "gko", "gm1", "g1", "g2",
];

const OLE_MAGIC: &[u8] = &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// True when `head` is UTF-8, allowing a sequence cut off at the end.
fn is_utf8_prefix(head: &[u8]) -> bool {
    match std::str::from_utf8(head) {
        Ok(_) => true,
        Err(e) => e.error_len().is_none(),
    }
}

/// Routes a file from its name and the first `HEAD_LEN` bytes of its contents.
pub fn route(name: &Path, head: &[u8]) -> Format {
    let ext = name
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let ext = ext.as_str();
    let head = &head[..head.len().min(HEAD_LEN)];

    // Magic bytes first.
    if contains(head, b"%PDF-") {
        return Format::Pdf;
    }
    if head.starts_with(b"PK\x03\x04") {
        return match ext {
            "docx" => Format::Docx,
            "pptx" => Format::Pptx,
            "xlsx" => Format::Xlsx,
            "xlsm" => Format::Xlsm,
            "ods" => Format::Ods,
            "epub" => Format::Epub,
            _ => Format::Unknown,
        };
    }
    if head.starts_with(OLE_MAGIC) {
        return match ext {
            "xls" => Format::Xls,
            "rvt" | "rfa" | "rte" | "rft" => Format::Rvt,
            _ => Format::Unknown,
        };
    }
    if head.starts_with(b"AC10") {
        return Format::Dwg;
    }
    if head.starts_with(b"BLENDER") {
        return Format::Blend;
    }
    if head.starts_with(b"\x89PNG\r\n\x1a\n")
        || head.starts_with(&[0xFF, 0xD8, 0xFF])
        || head.starts_with(b"GIF87a")
        || head.starts_with(b"GIF89a")
        || (head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP")
    {
        return Format::Image;
    }
    if head.len() >= 132 && &head[128..132] == b"DICM" {
        return Format::Dicom;
    }

    // Then the extension.
    match ext {
        "rvt" | "rfa" | "rte" | "rft" => return Format::Rvt,
        "skp" => return Format::Skp,
        // A DWG name without DWG magic is not a drawing.
        "dwg" | "dwt" => return Format::Unknown,
        _ => {}
    }
    if !is_utf8_prefix(head) {
        return Format::Unknown;
    }
    match ext {
        "csv" => Format::Csv,
        "tsv" | "tab" => Format::Tsv,
        "md" | "markdown" => Format::Md,
        "txt" | "text" => Format::Txt,
        "html" | "htm" | "xhtml" => Format::Html,
        "ipynb" => Format::Ipynb,
        "dxf" if contains(head, b"SECTION") => Format::Dxf,
        "kicad_sch" => Format::KicadSch,
        "kicad_pcb" => Format::KicadPcb,
        "kicad_pro" => Format::KicadPro,
        "cir" | "sp" | "spi" | "net" | "spice" => Format::Spice,
        "asc" if contains(head, b"SHEET") || contains(head, b"Version") => Format::LtspiceAsc,
        e if GERBER_EXTENSIONS.contains(&e) && contains(head, b"%FS") => Format::Gerber,
        e if CODE_EXTENSIONS.contains(&e) => Format::Code,
        _ => Format::Unknown,
    }
}

/// Reads the head of a file and routes it.
pub fn route_file(path: &Path) -> Result<Format> {
    let file = File::open(path).map_err(|e| Error::io(format!("opening {}", path.display()), e))?;
    let mut head = Vec::with_capacity(HEAD_LEN);
    file.take(HEAD_LEN as u64)
        .read_to_end(&mut head)
        .map_err(|e| Error::io(format!("reading {}", path.display()), e))?;
    Ok(route(path, &head))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(name: &str, head: &[u8]) -> &'static str {
        route(Path::new(name), head).as_str()
    }

    #[test]
    fn route_identifiers_round_trip() {
        for f in Format::ALL {
            assert_eq!(Format::parse(f.as_str()), Some(f));
        }
        assert_eq!(Format::parse("nope"), None);
    }

    #[test]
    fn route_magic_wins_over_extension() {
        assert_eq!(r("notes.docx", b"%PDF-1.7\n"), "pdf");
        assert_eq!(r("scan.txt", b"\x89PNG\r\n\x1a\n...."), "image");
        assert_eq!(r("a.pdf", b"PK\x03\x04rest"), "unknown");
        assert_eq!(r("x.bin", b"AC1032\0\0"), "dwg");
        assert_eq!(r("x", b"BLENDER-v300"), "blend");
        assert_eq!(r("x.dwg", b"not a drawing"), "unknown");
    }

    #[test]
    fn route_containers_by_extension() {
        assert_eq!(r("a.docx", b"PK\x03\x04"), "docx");
        assert_eq!(r("a.PPTX", b"PK\x03\x04"), "pptx");
        assert_eq!(r("a.xlsm", b"PK\x03\x04"), "xlsm");
        assert_eq!(r("a.epub", b"PK\x03\x04"), "epub");
        assert_eq!(r("a.xls", OLE_MAGIC), "xls");
        assert_eq!(r("a.rvt", OLE_MAGIC), "rvt");
        assert_eq!(r("a.doc", OLE_MAGIC), "unknown");
    }

    #[test]
    fn route_images_and_dicom() {
        assert_eq!(r("a.jpg", &[0xFF, 0xD8, 0xFF, 0xE0]), "image");
        assert_eq!(r("a.webp", b"RIFF\0\0\0\0WEBPVP8 "), "image");
        let mut dicom = vec![0u8; 128];
        dicom.extend_from_slice(b"DICM");
        assert_eq!(r("scan.dcm", &dicom), "dicom");
    }

    #[test]
    fn route_text_formats() {
        assert_eq!(r("a.csv", b"a,b\n1,2\n"), "csv");
        assert_eq!(r("a.tsv", b"a\tb\n"), "tsv");
        assert_eq!(r("a.md", "# Unit 1\n\u{0939}\u{093F}".as_bytes()), "md");
        assert_eq!(r("a.ipynb", b"{\"cells\": []}"), "ipynb");
        assert_eq!(r("Sample.java", b"class A {}"), "code");
        assert_eq!(r("d.dxf", b"  0\nSECTION\n"), "dxf");
        assert_eq!(r("d.dxf", b"garbage"), "unknown");
        assert_eq!(
            r("amp.kicad_sch", b"(kicad_sch (version 20231120)"),
            "kicad_sch"
        );
        assert_eq!(r("rc.cir", b"* RC\nR1 in out 1k\n"), "spice");
        assert_eq!(r("f.asc", b"Version 4\nSHEET 1 880 680\n"), "ltspice_asc");
        assert_eq!(r("b.gtl", b"%FSLAX46Y46*%\n"), "gerber");
        assert_eq!(r("m.skp", b"\xFF\xFE\xFF\x0ES\0k\0"), "skp");
    }

    #[test]
    fn route_rejects_binary_text_and_unknown() {
        assert_eq!(r("a.txt", &[0xFF, 0xFE, 0x00, 0x80, 0x81]), "unknown");
        assert_eq!(r("a.zzz", b"hello"), "unknown");
        // A UTF-8 sequence cut at the end of the head is still text.
        let mut cut = b"abc".to_vec();
        cut.extend_from_slice(&"\u{0939}".as_bytes()[..2]);
        assert_eq!(r("a.txt", &cut), "txt");
    }

    #[test]
    fn route_extractors() {
        assert_eq!(Format::Pdf.extractor(), Extractor::Worker);
        assert_eq!(Format::Xlsx.extractor(), Extractor::Native);
        assert_eq!(Format::Dwg.extractor(), Extractor::Tool("oda"));
        assert_eq!(Format::Blend.extractor(), Extractor::Tool("blender"));
        assert_eq!(Format::Skp.extractor(), Extractor::Unsupported);
        assert_eq!(Format::Unknown.extractor(), Extractor::None);
    }
}
