// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Native EDA extractors: KiCad, SPICE, LTspice and Gerber (SPEC 10.2, ADR 0011).

use std::collections::BTreeSet;

use serde_json::Value;

use crate::native::{Chunk, Extracted, Failure, split_text};
use crate::route::Format;

fn fail(code: &'static str, detail: impl Into<String>) -> Failure {
    Failure {
        code,
        detail: detail.into(),
    }
}

#[derive(Debug)]
enum Sexp {
    Atom(String),
    List(Vec<Sexp>),
}

impl Sexp {
    fn as_atom(&self) -> Option<&str> {
        match self {
            Sexp::Atom(s) => Some(s.as_str()),
            Sexp::List(_) => None,
        }
    }

    fn as_list(&self) -> Option<&[Sexp]> {
        match self {
            Sexp::List(items) => Some(items.as_slice()),
            Sexp::Atom(_) => None,
        }
    }

    fn head(&self) -> Option<&str> {
        self.as_list()?.first()?.as_atom()
    }
}

fn parse_sexps(input: &str) -> Result<Vec<Sexp>, Failure> {
    let bytes = input.as_bytes();
    let mut i = 0usize;
    let mut stack: Vec<Vec<Sexp>> = vec![Vec::new()];

    while i < bytes.len() {
        match bytes[i] {
            b if b.is_ascii_whitespace() => i += 1,
            b'(' => {
                stack.push(Vec::new());
                i += 1;
            }
            b')' => {
                if stack.len() <= 1 {
                    return Err(fail("corrupt", "unexpected ')' in S-expression"));
                }
                let list = stack.pop().expect("len > 1");
                stack.last_mut().expect("non-empty").push(Sexp::List(list));
                i += 1;
            }
            b'"' => {
                i += 1;
                let mut out = String::new();
                while i < bytes.len() {
                    let ch = input[i..].chars().next().expect("valid utf-8");
                    i += ch.len_utf8();
                    if ch == '"' {
                        break;
                    }
                    if ch == '\\' && i < bytes.len() {
                        let esc = input[i..].chars().next().expect("valid utf-8");
                        i += esc.len_utf8();
                        match esc {
                            'n' => out.push('\n'),
                            'r' => out.push('\r'),
                            't' => out.push('\t'),
                            other => out.push(other),
                        }
                    } else {
                        out.push(ch);
                    }
                }
                stack.last_mut().expect("non-empty").push(Sexp::Atom(out));
            }
            _ => {
                let start = i;
                while i < bytes.len()
                    && !bytes[i].is_ascii_whitespace()
                    && bytes[i] != b'('
                    && bytes[i] != b')'
                {
                    i += 1;
                }
                let atom = &input[start..i];
                stack
                    .last_mut()
                    .expect("non-empty")
                    .push(Sexp::Atom(atom.to_owned()));
            }
        }
    }
    if stack.len() != 1 {
        return Err(fail("corrupt", "unclosed '(' in S-expression"));
    }
    Ok(stack.pop().expect("root exists"))
}

fn push_text(out: &mut Extracted, body: &str) {
    for (part, piece) in split_text(body).into_iter().enumerate() {
        let locator = if part == 0 {
            "h:0".to_owned()
        } else {
            format!("h:0.{part}")
        };
        out.chunks.push(Chunk {
            locator,
            text: piece,
        });
    }
}

fn find_property<'a>(items: &'a [Sexp], key: &str) -> Option<&'a str> {
    for child in items {
        let Some(sub) = child.as_list() else {
            continue;
        };
        if sub.first().and_then(Sexp::as_atom) == Some("property")
            && sub.get(1).and_then(Sexp::as_atom) == Some(key)
        {
            return sub.get(2).and_then(Sexp::as_atom);
        }
    }
    None
}

fn kicad_sch(text: &str) -> Result<Extracted, Failure> {
    let nodes = parse_sexps(text)?;
    let root = nodes
        .iter()
        .find(|n| n.head() == Some("kicad_sch"))
        .and_then(Sexp::as_list)
        .ok_or_else(|| fail("corrupt", "missing (kicad_sch ...) root"))?;

    let mut components = Vec::new();
    let mut nets = BTreeSet::new();
    let mut sheets = Vec::new();

    for child in root {
        let Some(items) = child.as_list() else {
            continue;
        };
        match child.head() {
            Some("symbol") => {
                let reference = find_property(items, "Reference").unwrap_or_default();
                let value = find_property(items, "Value").unwrap_or_default();
                let lib_id = items
                    .iter()
                    .find(|n| n.head() == Some("lib_id"))
                    .and_then(Sexp::as_list)
                    .and_then(|l| l.get(1))
                    .and_then(Sexp::as_atom)
                    .unwrap_or_default();
                if !reference.is_empty() || !value.is_empty() {
                    let mut desc = if reference.is_empty() {
                        value.to_owned()
                    } else if value.is_empty() {
                        reference.to_owned()
                    } else {
                        format!("{reference}: {value}")
                    };
                    if !lib_id.is_empty() {
                        desc.push_str(&format!(" ({lib_id})"));
                    }
                    components.push(desc);
                }
            }
            Some("label" | "global_label" | "hierarchical_label" | "net") => {
                if let Some(name) = items.get(1).and_then(Sexp::as_atom)
                    && !name.is_empty()
                {
                    nets.insert(name.to_owned());
                }
            }
            Some("sheet") => {
                let name = find_property(items, "Sheetname").unwrap_or("sheet");
                let file = find_property(items, "Sheetfile").unwrap_or_default();
                if file.is_empty() {
                    sheets.push(name.to_owned());
                } else {
                    sheets.push(format!("{name} ({file})"));
                }
            }
            _ => {}
        }
    }

    let mut lines = vec!["Format: KiCad schematic".to_owned()];
    if !components.is_empty() {
        lines.push(format!("Components: {}", components.join("; ")));
    }
    if !nets.is_empty() {
        let joined: Vec<&str> = nets.iter().map(String::as_str).collect();
        lines.push(format!("Nets: {}", joined.join(", ")));
    }
    if !sheets.is_empty() {
        lines.push(format!("Sheets: {}", sheets.join(", ")));
    }
    let mut out = Extracted {
        units_hint: sheets,
        ..Extracted::default()
    };
    push_text(&mut out, &lines.join("\n"));
    Ok(out)
}

fn kicad_pcb(text: &str) -> Result<Extracted, Failure> {
    let nodes = parse_sexps(text)?;
    let root = nodes
        .iter()
        .find(|n| n.head() == Some("kicad_pcb"))
        .and_then(Sexp::as_list)
        .ok_or_else(|| fail("corrupt", "missing (kicad_pcb ...) root"))?;

    let mut components = Vec::new();
    let mut nets = BTreeSet::new();

    for child in root {
        let Some(items) = child.as_list() else {
            continue;
        };
        match child.head() {
            Some("net") => {
                if let Some(name) = items.get(2).and_then(Sexp::as_atom)
                    && !name.is_empty()
                {
                    nets.insert(name.to_owned());
                }
            }
            Some("footprint") => {
                let mut reference = find_property(items, "Reference").map(str::to_owned);
                let mut value = find_property(items, "Value").map(str::to_owned);
                for sub in items {
                    let Some(slist) = sub.as_list() else {
                        continue;
                    };
                    if slist.first().and_then(Sexp::as_atom) == Some("fp_text") {
                        let kind = slist.get(1).and_then(Sexp::as_atom);
                        let val = slist.get(2).and_then(Sexp::as_atom);
                        if kind == Some("reference") && reference.is_none() {
                            reference = val.map(str::to_owned);
                        } else if kind == Some("value") && value.is_none() {
                            value = val.map(str::to_owned);
                        }
                    }
                }
                if reference.is_some() || value.is_some() {
                    let r = reference.unwrap_or_default();
                    let v = value.unwrap_or_default();
                    components.push(match (r.is_empty(), v.is_empty()) {
                        (false, false) => format!("{r}: {v}"),
                        (false, true) => r,
                        _ => v,
                    });
                }
            }
            _ => {}
        }
    }

    let mut lines = vec!["Format: KiCad PCB".to_owned()];
    if !components.is_empty() {
        lines.push(format!("Components: {}", components.join("; ")));
    }
    if !nets.is_empty() {
        let joined: Vec<&str> = nets.iter().map(String::as_str).collect();
        lines.push(format!("Nets: {}", joined.join(", ")));
    }
    let mut out = Extracted::default();
    push_text(&mut out, &lines.join("\n"));
    Ok(out)
}

fn kicad_pro(text: &str) -> Result<Extracted, Failure> {
    let doc: Value =
        serde_json::from_str(text).map_err(|e| fail("corrupt", format!("invalid JSON: {e}")))?;
    let mut lines = vec!["Format: KiCad project".to_owned()];
    if let Some(meta) = doc.get("meta").and_then(Value::as_object) {
        if let Some(name) = meta.get("filename").and_then(Value::as_str) {
            lines.push(format!("Project file: {name}"));
        }
        if let Some(ver) = meta.get("version") {
            lines.push(format!("Version: {ver}"));
        }
    }
    if let Some(sheets) = doc.get("sheets").and_then(Value::as_array) {
        let names: Vec<String> = sheets
            .iter()
            .filter_map(|s| s.as_array().and_then(|a| a.get(1)).and_then(Value::as_str))
            .map(str::to_owned)
            .collect();
        if !names.is_empty() {
            lines.push(format!("Sheets: {}", names.join(", ")));
        }
    }
    let mut out = Extracted::default();
    push_text(&mut out, &lines.join("\n"));
    Ok(out)
}

fn is_spice_component(line: &str) -> bool {
    let Some(first) = line.bytes().next() else {
        return false;
    };
    matches!(
        first.to_ascii_uppercase(),
        b'R' | b'C'
            | b'L'
            | b'V'
            | b'I'
            | b'D'
            | b'Q'
            | b'M'
            | b'J'
            | b'K'
            | b'X'
            | b'E'
            | b'F'
            | b'G'
            | b'H'
            | b'S'
            | b'W'
            | b'T'
            | b'B'
    )
}

fn spice(text: &str) -> Extracted {
    let mut title = None;
    let mut components = Vec::new();
    let mut analyses = Vec::new();
    let mut models = Vec::new();

    for (idx, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if idx == 0 {
            title = Some(line.trim_start_matches('*').trim().to_owned());
            if line.starts_with('*') {
                continue;
            }
        }
        if line.starts_with('*') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('.') {
            let cmd = line
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();
            match cmd.as_str() {
                ".tran" | ".ac" | ".dc" | ".op" | ".noise" | ".tf" | ".pz" | ".sens" => {
                    analyses.push(line.to_owned());
                }
                ".model" | ".subckt" | ".param" | ".lib" | ".include" => {
                    models.push(line.to_owned());
                }
                _ => {}
            }
            continue;
        }
        if is_spice_component(line) {
            components.push(line.to_owned());
        }
    }

    let mut lines = vec!["Format: SPICE netlist".to_owned()];
    if let Some(t) = title.filter(|s| !s.is_empty()) {
        lines.push(format!("Title: {t}"));
    }
    if !components.is_empty() {
        lines.push("Components:".to_owned());
        lines.extend(components);
    }
    if !analyses.is_empty() {
        lines.push("Analyses:".to_owned());
        lines.extend(analyses);
    }
    if !models.is_empty() {
        lines.push("Directives:".to_owned());
        lines.extend(models);
    }
    let mut out = Extracted::default();
    push_text(&mut out, &lines.join("\n"));
    out
}

fn ltspice_asc(text: &str) -> Extracted {
    struct Pending {
        kind: String,
        inst: Option<String>,
        value: Option<String>,
    }

    let mut current: Option<Pending> = None;
    let mut components = Vec::new();
    let mut nets = BTreeSet::new();
    let mut analyses = Vec::new();

    let flush = |pending: Option<Pending>, components: &mut Vec<String>| {
        if let Some(p) = pending {
            let label = match (p.inst, p.value) {
                (Some(r), Some(v)) => format!("{r}: {v} ({})", p.kind),
                (Some(r), None) => format!("{r} ({})", p.kind),
                (None, Some(v)) => format!("{} ({})", v, p.kind),
                (None, None) => p.kind,
            };
            components.push(label);
        }
    };

    for raw in text.lines() {
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix("SYMBOL ") {
            flush(current.take(), &mut components);
            let kind = rest
                .split_whitespace()
                .next()
                .unwrap_or("symbol")
                .to_owned();
            current = Some(Pending {
                kind,
                inst: None,
                value: None,
            });
        } else if let Some(rest) = line.strip_prefix("SYMATTR ") {
            if let Some(p) = current.as_mut() {
                let mut parts = rest.splitn(2, char::is_whitespace);
                let attr = parts.next().unwrap_or("");
                let val = parts.next().unwrap_or("").trim().to_owned();
                if attr == "InstName" && !val.is_empty() {
                    p.inst = Some(val);
                } else if attr == "Value" && !val.is_empty() {
                    p.value = Some(val);
                }
            }
        } else {
            flush(current.take(), &mut components);
            if let Some(rest) = line.strip_prefix("FLAG ") {
                if let Some(net) = rest.split_whitespace().nth(2)
                    && net != "0"
                {
                    nets.insert(net.to_owned());
                }
            } else if let Some(rest) = line.strip_prefix("TEXT ")
                && let Some(idx) = rest.find('!')
            {
                let cmd = rest[idx + 1..].trim();
                if !cmd.is_empty() {
                    analyses.push(cmd.to_owned());
                }
            }
        }
    }
    flush(current.take(), &mut components);

    let mut lines = vec!["Format: LTspice schematic".to_owned()];
    if !components.is_empty() {
        lines.push(format!("Components: {}", components.join("; ")));
    }
    if !nets.is_empty() {
        let joined: Vec<&str> = nets.iter().map(String::as_str).collect();
        lines.push(format!("Nets: {}", joined.join(", ")));
    }
    if !analyses.is_empty() {
        lines.push(format!("Analyses: {}", analyses.join("; ")));
    }
    let mut out = Extracted::default();
    push_text(&mut out, &lines.join("\n"));
    out
}

fn gerber(text: &str) -> Extracted {
    let mut format_spec = None;
    let mut units = None;
    let mut file_function = None;
    let mut attributes = Vec::new();
    let mut apertures = 0usize;

    for raw in text.lines() {
        let line = raw.trim();
        let body = line
            .strip_prefix('%')
            .and_then(|s| s.strip_suffix("*%"))
            .unwrap_or("");
        if body.is_empty() {
            continue;
        }
        if body.starts_with("FS") {
            format_spec = Some(body.to_owned());
        } else if body == "MOMM" {
            units = Some("mm".to_owned());
        } else if body == "MOIN" {
            units = Some("in".to_owned());
        } else if body.starts_with("ADD") {
            apertures += 1;
        } else if let Some(rest) = body.strip_prefix("TF.") {
            if let Some(func) = rest.strip_prefix("FileFunction,") {
                file_function = Some(func.to_owned());
            } else {
                attributes.push(rest.to_owned());
            }
        }
    }

    let mut lines = vec!["Format: Gerber RS-274X".to_owned()];
    if let Some(f) = &file_function {
        lines.push(format!("Layer function: {f}"));
    }
    if let Some(fs_spec) = format_spec {
        lines.push(format!("Coordinate format: {fs_spec}"));
    }
    if let Some(u) = units {
        lines.push(format!("Units: {u}"));
    }
    lines.push(format!("Apertures: {apertures}"));
    if !attributes.is_empty() {
        lines.push(format!("Attributes: {}", attributes.join("; ")));
    }

    let locator = match file_function {
        Some(ref f) => format!("l:{}", f.split(',').next().unwrap_or("gerber")),
        None => "l:gerber".to_owned(),
    };
    let mut out = Extracted::default();
    out.chunks.push(Chunk {
        locator,
        text: lines.join("\n"),
    });
    out
}

/// Extracts an EDA format.
pub fn extract(text: &str, format: Format) -> Result<Extracted, Failure> {
    match format {
        Format::KicadSch => kicad_sch(text),
        Format::KicadPcb => kicad_pcb(text),
        Format::KicadPro => kicad_pro(text),
        Format::Spice => Ok(spice(text)),
        Format::LtspiceAsc => Ok(ltspice_asc(text)),
        Format::Gerber => Ok(gerber(text)),
        _ => Err(fail("unsupported", "not an EDA format")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_kicad_schematic_pcb_and_project() {
        let sch = r#"(kicad_sch (version 20231120)
  (lib_symbols (symbol "Device:R" (property "Reference" "R") (property "Value" "R")))
  (symbol (lib_id "Device:R") (property "Reference" "R1") (property "Value" "10k"))
  (label "VIN")
  (global_label "GND")
)"#;
        let out = extract(sch, Format::KicadSch).unwrap();
        assert!(out.chunks[0].text.contains("R1: 10k"));
        assert!(out.chunks[0].text.contains("VIN"));
        assert!(out.chunks[0].text.contains("GND"));

        let pcb = r#"(kicad_pcb (version 20240108)
  (net 1 "VOUT")
  (footprint "Resistor_SMD:R_0603" (property "Reference" "R1") (property "Value" "10k"))
)"#;
        let pcb_out = extract(pcb, Format::KicadPcb).unwrap();
        assert!(pcb_out.chunks[0].text.contains("R1: 10k"));
        assert!(pcb_out.chunks[0].text.contains("VOUT"));

        let pro = r#"{"meta":{"filename":"amp.kicad_pro","version":1},"sheets":[["root","Main"]]}"#;
        let pro_out = extract(pro, Format::KicadPro).unwrap();
        assert!(pro_out.chunks[0].text.contains("amp.kicad_pro"));
        assert!(pro_out.chunks[0].text.contains("Main"));
    }

    #[test]
    fn extracts_spice_ltspice_and_gerber() {
        let cir = "* RC low-pass filter\nR1 in out 1k\nC1 out 0 100n\n.tran 1m 10m\n.ac dec 10 1 100k\n.end\n";
        let cir_out = extract(cir, Format::Spice).unwrap();
        assert!(cir_out.chunks[0].text.contains("R1 in out 1k"));
        assert!(cir_out.chunks[0].text.contains(".tran 1m 10m"));
        assert!(cir_out.chunks[0].text.contains(".ac dec 10 1 100k"));

        let asc = "Version 4\nSHEET 1 880 680\nSYMBOL res 100 100 R0\nSYMATTR InstName R1\nSYMATTR Value 4.7k\nFLAG 100 200 VOUT\nTEXT 50 50 Left 2 !.tran 5m\n";
        let asc_out = extract(asc, Format::LtspiceAsc).unwrap();
        assert!(asc_out.chunks[0].text.contains("R1: 4.7k"));
        assert!(asc_out.chunks[0].text.contains("VOUT"));
        assert!(asc_out.chunks[0].text.contains(".tran 5m"));

        let gtl =
            "%FSLAX46Y46*%\n%MOMM*%\n%TF.FileFunction,Copper,L1,Top*%\n%ADD10C,0.254000*%\nM02*\n";
        let gtl_out = extract(gtl, Format::Gerber).unwrap();
        assert_eq!(gtl_out.chunks[0].locator, "l:Copper");
        assert!(gtl_out.chunks[0].text.contains("Copper,L1,Top"));
        assert!(gtl_out.chunks[0].text.contains("Apertures: 1"));
    }
}
