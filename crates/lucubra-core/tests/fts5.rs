// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Verifies that the bundled SQLite provides FTS5 and that the chosen
//! tokenizers behave as recorded in `docs/adr/0003-fts-tokenizer-strategy.md`.

use lucubra_core::{
    FTS_SUBSTRING_TOKENIZER, FTS_WORD_TOKENIZER, fts_tokenize_option, normalize_for_fts,
};
use rusqlite::Connection;

const DOCS: &[(&str, &str)] = &[
    ("english", "Photosynthesis occurs in plants"),
    ("hindi", "प्रकाश संश्लेषण पौधों में होता है"),
    ("malayalam", "പ്രകാശസംശ്ലേഷണം സസ്യങ്ങളിൽ നടക്കുന്നു"),
    ("tamil", "ஒளிச்சேர்க்கை தாவரங்களில் நடைபெறுகிறது"),
    ("bengali", "সালোকসংশ্লেষণ উদ্ভিদে ঘটে"),
];

/// Script, whole word, prefix, inner substring.
const QUERIES: &[(&str, &str, &str, &str)] = &[
    ("english", "plants", "photo", "synth"),
    ("hindi", "संश्लेषण", "संश्ले", "श्लेष"),
    ("malayalam", "സസ്യങ്ങളിൽ", "സസ്യ", "സംശ്ലേഷണം"),
    ("tamil", "தாவரங்களில்", "தாவர", "சேர்க்கை"),
    ("bengali", "উদ্ভিদে", "উদ্ভি", "সংশ্লেষণ"),
];

fn index(conn: &Connection, table: &str, tokenizer: &str) {
    conn.execute_batch(&format!(
        "CREATE VIRTUAL TABLE {table} USING fts5(script UNINDEXED, body, {});
         CREATE VIRTUAL TABLE {table}_vocab USING fts5vocab({table}, 'row');",
        fts_tokenize_option(tokenizer)
    ))
    .unwrap();
    for (script, body) in DOCS {
        conn.execute(
            &format!("INSERT INTO {table}(script, body) VALUES (?1, ?2)"),
            [script, body],
        )
        .unwrap();
    }
}

fn hits(conn: &Connection, table: &str, expr: &str) -> Vec<String> {
    let sql = format!("SELECT script FROM {table} WHERE {table} MATCH ?1 ORDER BY script");
    let mut stmt = conn.prepare(&sql).unwrap();
    stmt.query_map([expr], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn vocab(conn: &Connection, table: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare(&format!("SELECT term FROM {table}_vocab"))
        .unwrap();
    stmt.query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn bundled_sqlite_has_fts5() {
    let conn = Connection::open_in_memory().unwrap();
    let enabled: i64 = conn
        .query_row(
            "SELECT sqlite_compileoption_used('ENABLE_FTS5')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(enabled, 1);
}

#[test]
fn word_tokenizer_keeps_indic_words_whole() {
    let conn = Connection::open_in_memory().unwrap();
    index(&conn, "words", FTS_WORD_TOKENIZER);
    let terms = vocab(&conn, "words");
    for (script, word, prefix, _) in QUERIES {
        assert!(
            terms.contains(&word.to_lowercase()),
            "{script}: {word} is not one token"
        );
        assert_eq!(
            hits(&conn, "words", &format!("\"{word}\"")),
            [*script],
            "{script} word"
        );
        assert_eq!(
            hits(&conn, "words", &format!("\"{prefix}\"*")),
            [*script],
            "{script} prefix"
        );
    }
}

#[test]
fn default_unicode61_splits_indic_words() {
    let conn = Connection::open_in_memory().unwrap();
    index(&conn, "plain", "unicode61");
    let terms = vocab(&conn, "plain");
    assert!(terms.contains(&"plants".to_string()));
    for (script, word, _, _) in QUERIES.iter().filter(|q| q.0 != "english") {
        assert!(
            !terms.contains(&word.to_string()),
            "{script}: {word} was expected to be split"
        );
    }
}

#[test]
fn substring_tokenizer_matches_inside_words() {
    let conn = Connection::open_in_memory().unwrap();
    index(&conn, "grams", FTS_SUBSTRING_TOKENIZER);
    for (script, word, _, inner) in QUERIES {
        assert_eq!(
            hits(&conn, "grams", &format!("\"{word}\"")),
            [*script],
            "{script} word"
        );
        assert_eq!(
            hits(&conn, "grams", &format!("\"{inner}\"")),
            [*script],
            "{script} inner"
        );
    }
}

#[test]
fn substring_tokenizer_needs_three_characters() {
    let conn = Connection::open_in_memory().unwrap();
    index(&conn, "grams", FTS_SUBSTRING_TOKENIZER);
    assert!(hits(&conn, "grams", "\"pl\"").is_empty());
    assert_eq!(hits(&conn, "grams", "\"pla\""), ["english"]);
}

/// Words written with a zero-width joiner (older Malayalam chillu encoding)
/// or a zero-width non-joiner (Hindi half forms). Both characters are in
/// Unicode category Cf.
const JOINER_WORDS: &[&str] = &["അവന\u{0D4D}\u{200D}", "क\u{094D}\u{200C}ष"];

fn joiner_table(conn: &Connection, normalize: bool) -> Vec<String> {
    conn.execute_batch(&format!(
        "DROP TABLE IF EXISTS j; DROP TABLE IF EXISTS j_vocab;
         CREATE VIRTUAL TABLE j USING fts5(body, {});
         CREATE VIRTUAL TABLE j_vocab USING fts5vocab(j, 'row');",
        fts_tokenize_option(FTS_WORD_TOKENIZER)
    ))
    .unwrap();
    for word in JOINER_WORDS {
        let body = if normalize {
            normalize_for_fts(word)
        } else {
            word.to_string()
        };
        conn.execute("INSERT INTO j(body) VALUES (?1)", [body])
            .unwrap();
    }
    vocab(conn, "j")
}

#[test]
fn raw_zero_width_non_joiner_splits_a_word() {
    let conn = Connection::open_in_memory().unwrap();
    let terms = joiner_table(&conn, false);
    assert!(terms.contains(&"क\u{094D}".to_string()));
    assert!(terms.contains(&"ष".to_string()));
}

#[test]
fn normalized_joiner_words_stay_whole_and_match() {
    let conn = Connection::open_in_memory().unwrap();
    let terms = joiner_table(&conn, true);
    assert_eq!(terms.len(), JOINER_WORDS.len(), "{terms:?}");
    for word in JOINER_WORDS {
        let normalized = normalize_for_fts(word);
        assert!(
            terms.contains(&normalized),
            "{} split",
            word.escape_unicode()
        );
        let query = format!("\"{}\"", normalize_for_fts(word));
        let found: i64 = conn
            .query_row("SELECT count(*) FROM j WHERE j MATCH ?1", [query], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(found, 1, "{}", word.escape_unicode());
    }
}
