// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Domain model, event log, replay, planner and storage for Lucubra.

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

/// Normalises text for full-text indexing and for search queries.
///
/// Both sides of a search must use this function so that they match.
pub fn normalize_for_fts(text: &str) -> String {
    text.chars()
        .filter(|c| !FTS_STRIPPED_CHARS.contains(c))
        .collect()
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
