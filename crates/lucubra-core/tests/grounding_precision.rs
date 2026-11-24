// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! M6 grounding precision gate (D53): every anchor window cut from the
//! passages below is checked clean (must pass) and perturbed (must fail).

use lucubra_core::grounding::{tokens, verify_text};

const PASSAGES: &[&str] = &[
    "A Gaussian beam keeps its profile as it propagates, but its radius grows \
     with distance. At the Rayleigh range the area of the beam has doubled, \
     and the wavefront curvature reaches its maximum. Lenses cannot remove \
     diffraction; they only move the waist. A beam with a 2 mm waist at 633 nm \
     has a Rayleigh range of about 19.8 metres, which is never reached inside \
     a typical bench setup.",
    "Under the Companies Act a private company must keep a register of members \
     at its registered office. The register records each member's name, \
     address and the date of entry, and it is not open to inspection without \
     notice. A company that fails to keep the register commits an offence, and \
     every officer in default is liable to a fine. Entries older than 10 years \
     may be removed after the member has left.",
    "Reinforced concrete beams fail in flexure when the steel yields before the \
     concrete crushes. Designers keep the neutral axis depth below 0.45 of the \
     effective depth so that failure is ductile. Shear links are spaced at no \
     more than 300 mm, and the cover to the main bars is at least 25 mm for \
     internal exposure. Deflection is checked with the span to depth ratio, \
     except for cantilevers longer than 7 metres.",
    "In the Calvin cycle, rubisco fixes carbon dioxide onto ribulose \
     bisphosphate, producing two molecules of glycerate phosphate. Each \
     molecule is reduced using ATP and NADPH from the light reactions. Five of \
     every six triose phosphates regenerate the acceptor, so only 1 in 6 \
     leaves the cycle as product. Rubisco does not distinguish perfectly \
     between carbon dioxide and oxygen, which causes photorespiration.",
];

fn windows(passage: &str, len: usize) -> Vec<Vec<String>> {
    let raw: Vec<String> = passage.split_whitespace().map(str::to_owned).collect();
    raw.windows(len)
        .step_by(3)
        .map(<[String]>::to_vec)
        .collect()
}

fn clean_variants(words: &[String]) -> Vec<String> {
    let plain = words.join(" ");
    let mut out = vec![plain.clone(), plain.to_uppercase(), words.join("  \n ")];
    if let Some((i, w)) = words
        .iter()
        .enumerate()
        .find(|(_, w)| w.len() >= 8 && w.chars().all(char::is_alphabetic))
    {
        let mut v = words.to_vec();
        let mid = w.len() / 2;
        v[i] = format!("{}-\n{}", &w[..mid], &w[mid..]);
        out.push(v.join(" "));
    }
    if plain.contains("fi") {
        out.push(plain.replacen("fi", "\u{FB01}", 1));
    }
    out
}

fn perturbed(words: &[String], other: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    // Changed number.
    if let Some(i) = words
        .iter()
        .position(|w| w.starts_with(|c: char| c.is_ascii_digit()))
    {
        let mut v = words.to_vec();
        let digits: String = v[i].chars().take_while(char::is_ascii_digit).collect();
        let n: u64 = digits.parse().unwrap_or(0);
        v[i] = format!("{}", n + 1);
        out.push(v.join(" "));
    }
    // Inserted negation in the middle.
    let mut v = words.to_vec();
    v.insert(words.len() / 2, "not".to_owned());
    out.push(v.join(" "));
    // Dropped negation (interior only: an edge drop is still a true sub-quote).
    if let Some(i) = words
        .iter()
        .enumerate()
        .skip(1)
        .take(words.len() - 2)
        .position(|(_, w)| {
            let w = w.trim_matches(|c: char| !c.is_alphabetic()).to_lowercase();
            matches!(
                w.as_str(),
                "not" | "no" | "never" | "without" | "except" | "cannot"
            )
        })
    {
        let mut v = words.to_vec();
        v.remove(i + 1);
        out.push(v.join(" "));
    }
    // Swapped entity: a long word replaced by one from another passage.
    if let Some(i) = words.iter().position(|w| w.len() >= 6) {
        let source_tokens = tokens(&words.join(" "));
        if let Some(swap) = other
            .iter()
            .flat_map(|p| p.split_whitespace())
            .find(|w| w.len() >= 7 && !source_tokens.contains(&w.to_lowercase()))
        {
            let mut v = words.to_vec();
            v[i] = swap.to_owned();
            out.push(v.join(" "));
        }
    }
    // Shuffled order.
    let mut v = words.to_vec();
    v.reverse();
    out.push(v.join(" "));
    out
}

#[test]
fn grounding_precision_gate() {
    let (mut clean_total, mut clean_ok, mut bad_total, mut false_accepts) = (0, 0, 0, 0);
    for (pi, passage) in PASSAGES.iter().enumerate() {
        let others: Vec<&str> = PASSAGES
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != pi)
            .map(|(_, p)| *p)
            .collect();
        for len in [6, 12, 20] {
            for w in windows(passage, len) {
                for a in clean_variants(&w) {
                    clean_total += 1;
                    if verify_text(&a, &[passage]).is_ok() {
                        clean_ok += 1;
                    } else {
                        eprintln!("clean rejected: {a}");
                    }
                }
                for a in perturbed(&w, &others) {
                    bad_total += 1;
                    if verify_text(&a, &[passage]).is_ok() {
                        false_accepts += 1;
                        eprintln!("false accept: {a}");
                    }
                }
            }
        }
        // Anchors cut from another file never pass against this one.
        for other in &others {
            for w in windows(other, 12) {
                bad_total += 1;
                if verify_text(&w.join(" "), &[passage]).is_ok() {
                    false_accepts += 1;
                    eprintln!("false accept (other file): {}", w.join(" "));
                }
            }
        }
    }
    // Adjacent chunk: a quote spanning a split passes when both are given.
    let words: Vec<&str> = PASSAGES[0].split_whitespace().collect();
    let mid = words.len() / 2;
    let (head, tail) = (words[..mid].join(" "), words[mid..].join(" "));
    let across = words[mid - 4..mid + 4].join(" ");
    clean_total += 1;
    if verify_text(&across, &[&head, &tail]).is_ok() {
        clean_ok += 1;
    }
    println!(
        "grounding gate: clean {clean_ok}/{clean_total} accepted, perturbed {false_accepts}/{bad_total} falsely accepted"
    );
    assert_eq!(false_accepts, 0);
    assert_eq!(clean_ok, clean_total);
    assert!(bad_total > 100 && clean_total > 100);
}
