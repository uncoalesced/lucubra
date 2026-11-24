// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Domain model, event log, replay, planner and storage for Lucubra.

pub mod appdata;
pub mod cache;
pub mod calendar;
pub mod error;
pub mod event;
pub mod hlc;
pub mod log;
pub mod organise;
pub mod paths;
pub mod planner;
pub mod projection;
pub mod replay;
pub mod state;
pub mod workspace;

pub use error::{Error, Result};
pub use workspace::Workspace;

/// Version of the Lucubra engine, taken from the workspace manifest.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// FTS5 tokenizer for the primary word index.
///
/// The default `unicode61` categories split Indic words at combining marks.
/// Adding the `Mn` and `Mc` categories keeps each word as one token.
/// See `docs/adr/0003-fts-tokenizer-strategy.md`.
pub const FTS_WORD_TOKENIZER: &str = "unicode61 categories 'L* N* Co Mn Mc'";

/// FTS5 tokenizer for the secondary substring index.
pub const FTS_SUBSTRING_TOKENIZER: &str = "trigram";

/// Format characters removed before text is indexed or queried: soft hyphen,
/// zero-width space, zero-width non-joiner, zero-width joiner, word joiner and
/// byte order mark. FTS5 treats them as separators, which splits Indic words
/// written with a joiner. See `docs/adr/0003-fts-tokenizer-strategy.md`.
pub const FTS_STRIPPED_CHARS: &[char] = &[
    '\u{00AD}', '\u{200B}', '\u{200C}', '\u{200D}', '\u{2060}', '\u{FEFF}',
];

/// Malayalam chillu letters and the consonant each is written with in the
/// older encoding, consonant + virama (U+0D4D) + zero-width joiner.
const CHILLU: &[(char, char)] = &[
    ('\u{0D23}', '\u{0D7A}'), // NNA
    ('\u{0D28}', '\u{0D7B}'), // NA
    ('\u{0D30}', '\u{0D7C}'), // RA
    ('\u{0D32}', '\u{0D7D}'), // LA
    ('\u{0D33}', '\u{0D7E}'), // LLA
    ('\u{0D15}', '\u{0D7F}'), // KA
];

/// Normalises text for full-text indexing and for search queries.
///
/// Steps: Unicode NFC; the joiner encoding of Malayalam chillu letters is
/// folded to the atomic letter; then `FTS_STRIPPED_CHARS` are removed. Both
/// sides of a search must use this function so that they match.
pub fn normalize_for_fts(text: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    let nfc: Vec<char> = text.nfc().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < nfc.len() {
        let c = nfc[i];
        if i + 2 < nfc.len()
            && nfc[i + 1] == '\u{0D4D}'
            && nfc[i + 2] == '\u{200D}'
            && let Some(&(_, chillu)) = CHILLU.iter().find(|(base, _)| *base == c)
        {
            out.push(chillu);
            i += 3;
            continue;
        }
        if !FTS_STRIPPED_CHARS.contains(&c) {
            out.push(c);
        }
        i += 1;
    }
    out
}

/// Formats a tokenizer specification as an FTS5 `tokenize` option.
pub fn fts_tokenize_option(spec: &str) -> String {
    format!("tokenize='{}'", spec.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_removes_only_format_characters() {
        assert_eq!(
            normalize_for_fts("a\u{00AD}b\u{200C}c\u{200D}d\u{FEFF}"),
            "abcd"
        );
        assert_eq!(normalize_for_fts("प्रकाश संश्लेषण"), "प्रकाश संश्लेषण");
    }

    #[test]
    fn normalize_applies_nfc_and_folds_chillu() {
        // NFD and NFC spellings of the same Malayalam word (o-sign U+0D4A).
        let nfd = "\u{0D15}\u{0D46}\u{0D3E}\u{0D32}\u{0D4D}";
        let nfc = "\u{0D15}\u{0D4A}\u{0D32}\u{0D4D}";
        assert_eq!(normalize_for_fts(nfd), normalize_for_fts(nfc));
        // Joiner chillu and atomic chillu: avan (he).
        let joiner = "\u{0D05}\u{0D35}\u{0D28}\u{0D4D}\u{200D}";
        let atomic = "\u{0D05}\u{0D35}\u{0D7B}";
        assert_eq!(normalize_for_fts(joiner), atomic);
        assert_eq!(normalize_for_fts(atomic), atomic);
        // A virama without a joiner is left alone.
        assert_eq!(normalize_for_fts("\u{0D28}\u{0D4D}"), "\u{0D28}\u{0D4D}");
        // Latin composed and decomposed forms match.
        assert_eq!(normalize_for_fts("e\u{0301}"), "\u{00E9}");
    }

    #[test]
    fn tokenize_option_escapes_quotes() {
        assert_eq!(
            fts_tokenize_option(FTS_WORD_TOKENIZER),
            "tokenize='unicode61 categories ''L* N* Co Mn Mc'''"
        );
        assert_eq!(
            fts_tokenize_option(FTS_SUBSTRING_TOKENIZER),
            "tokenize='trigram'"
        );
    }
}
