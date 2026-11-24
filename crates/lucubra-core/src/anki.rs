// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! One-way Anki export (SPEC section 14, ADR 0004): an `.apkg` package
//! holding a legacy `collection.anki2` database, plus a TSV fallback that Anki
//! imports as text. No Anki or genanki code is used; the import check in the
//! M6 gate ledger is the authority on compatibility.

use std::fs;
use std::io::Write;
use std::path::Path;

use rusqlite::{Connection, params};
use serde_json::json;

use crate::{Error, Result};

/// Stated in the tool response and in the deck description.
pub const NOTICE: &str = "Exported from Lucubra. Reviews you do in Anki do not flow back into \
Lucubra in v1; Lucubra keeps its own review schedule.";

/// One exported card.
#[derive(Debug, Clone, PartialEq)]
pub struct AnkiCard {
    /// Stable id (the Lucubra item or card id), used for the note guid.
    pub id: String,
    pub front: String,
    pub back: String,
    /// Where the card comes from, for example `notes/u1.pdf p:3`.
    pub source: String,
    pub tags: Vec<String>,
}

const SCHEMA: &str = "
CREATE TABLE col (id integer primary key, crt integer not null, mod integer not null,
  scm integer not null, ver integer not null, dty integer not null, usn integer not null,
  ls integer not null, conf text not null, models text not null, decks text not null,
  dconf text not null, tags text not null);
CREATE TABLE notes (id integer primary key, guid text not null, mid integer not null,
  mod integer not null, usn integer not null, tags text not null, flds text not null,
  sfld integer not null, csum integer not null, flags integer not null, data text not null);
CREATE TABLE cards (id integer primary key, nid integer not null, did integer not null,
  ord integer not null, mod integer not null, usn integer not null, type integer not null,
  queue integer not null, due integer not null, ivl integer not null, factor integer not null,
  reps integer not null, lapses integer not null, left integer not null, odue integer not null,
  odid integer not null, flags integer not null, data text not null);
CREATE TABLE revlog (id integer primary key, cid integer not null, usn integer not null,
  ease integer not null, ivl integer not null, lastIvl integer not null, factor integer not null,
  time integer not null, type integer not null);
CREATE TABLE graves (usn integer not null, oid integer not null, type integer not null);
CREATE INDEX ix_notes_usn on notes (usn);
CREATE INDEX ix_cards_usn on cards (usn);
CREATE INDEX ix_revlog_usn on revlog (usn);
CREATE INDEX ix_cards_nid on cards (nid);
CREATE INDEX ix_cards_sched on cards (did, queue, due);
CREATE INDEX ix_revlog_cid on revlog (cid);
CREATE INDEX ix_notes_csum on notes (csum);
";

/// A stable positive id from text, in the millisecond range Anki uses.
fn id_from(text: &str) -> i64 {
    let h = blake3::hash(text.as_bytes());
    let mut b = [0u8; 8];
    b.copy_from_slice(&h.as_bytes()[..8]);
    1_000_000_000_000 + (u64::from_le_bytes(b) % 1_000_000_000_000) as i64
}

fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\n', "<br>")
}

/// First 8 hex digits of the SHA-1 of the stripped first field, which Anki
/// stores in `notes.csum` for duplicate detection.
fn field_checksum(first: &str) -> i64 {
    let d = sha1(strip_html(first).as_bytes());
    i64::from(u32::from_be_bytes([d[0], d[1], d[2], d[3]]))
}

/// SHA-1 (FIPS 180-4), used only for the Anki duplicate checksum.
fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    let mut msg = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_be_bytes());
    for block in msg.chunks(64) {
        let mut w = [0u32; 80];
        for (i, word) in block.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let t = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            (e, d, c, b, a) = (d, c, b.rotate_left(30), a, t);
        }
        for (hv, v) in h.iter_mut().zip([a, b, c, d, e]) {
            *hv = hv.wrapping_add(v);
        }
    }
    let mut out = [0u8; 20];
    for (i, v) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

fn build_collection(conn: &Connection, deck: &str, cards: &[AnkiCard], now_ms: i64) -> Result<()> {
    let now_s = now_ms / 1000;
    let did = id_from(&format!("deck:{deck}"));
    let mid = id_from("model:lucubra-basic-v1");
    conn.execute_batch(SCHEMA)?;
    let field = |name: &str, ord: u32| {
        json!({"name": name, "ord": ord, "sticky": false, "rtl": false,
               "font": "Arial", "size": 20, "media": []})
    };
    let model = json!({
        mid.to_string(): {
            "id": mid, "name": "Lucubra Basic", "type": 0, "mod": now_s, "usn": -1,
            "sortf": 0, "did": did, "tags": [], "vers": [],
            "flds": [field("Front", 0), field("Back", 1), field("Source", 2)],
            "tmpls": [{
                "name": "Card 1", "ord": 0, "did": null, "bqfmt": "", "bafmt": "",
                "qfmt": "{{Front}}",
                "afmt": "{{FrontSide}}<hr id=answer>{{Back}}<br><small>{{Source}}</small>"
            }],
            "css": ".card { font-family: sans-serif; font-size: 20px; text-align: left; }",
            "latexPre": "\\documentclass[12pt]{article}\n\\special{papersize=3in,5in}\n\\usepackage{amssymb,amsmath}\n\\pagestyle{empty}\n\\begin{document}\n",
            "latexPost": "\\end{document}",
            "latexsvg": false,
            "req": [[0, "any", [0]]]
        }
    });
    let deck_json = |id: i64, name: &str, desc: &str| {
        json!({"id": id, "name": name, "desc": desc, "mod": now_s, "usn": -1,
               "lrnToday": [0, 0], "revToday": [0, 0], "newToday": [0, 0], "timeToday": [0, 0],
               "collapsed": false, "browserCollapsed": false, "conf": 1, "dyn": 0,
               "extendNew": 0, "extendRev": 0})
    };
    let decks = json!({
        "1": deck_json(1, "Default", ""),
        did.to_string(): deck_json(did, deck, NOTICE),
    });
    let dconf = json!({"1": {
        "id": 1, "name": "Default", "mod": 0, "usn": 0, "maxTaken": 60, "autoplay": true,
        "timer": 0, "replayq": true, "dyn": false,
        "new": {"bury": false, "delays": [1.0, 10.0], "initialFactor": 2500,
                "ints": [1, 4, 0], "order": 1, "perDay": 20},
        "lapse": {"delays": [10.0], "leechAction": 1, "leechFails": 8, "minInt": 1, "mult": 0.0},
        "rev": {"bury": false, "ease4": 1.3, "ivlFct": 1.0, "maxIvl": 36500, "perDay": 200,
                "hardFactor": 1.2}
    }});
    let conf = json!({"activeDecks": [1], "curDeck": 1, "newSpread": 0, "collapseTime": 1200,
                      "timeLim": 0, "estTimes": true, "dueCounts": true, "curModel": mid,
                      "nextPos": cards.len() + 1, "sortType": "noteFld", "sortBackwards": false,
                      "addToCur": true});
    conn.execute(
        "INSERT INTO col VALUES (1, ?1, ?2, ?3, 11, 0, 0, 0, ?4, ?5, ?6, ?7, '{}')",
        params![
            now_s,
            now_ms,
            now_ms,
            conf.to_string(),
            model.to_string(),
            decks.to_string(),
            dconf.to_string()
        ],
    )?;
    for (pos, card) in cards.iter().enumerate() {
        let nid = id_from(&format!("note:{}", card.id));
        let cid = id_from(&format!("card:{}", card.id));
        let front = escape_html(&card.front);
        let flds = [
            front.clone(),
            escape_html(&card.back),
            escape_html(&card.source),
        ]
        .join("\x1f");
        let tags = if card.tags.is_empty() {
            String::new()
        } else {
            format!(" {} ", card.tags.join(" "))
        };
        let guid = blake3::hash(card.id.as_bytes()).to_hex()[..10].to_owned();
        conn.execute(
            "INSERT INTO notes VALUES (?1, ?2, ?3, ?4, -1, ?5, ?6, ?7, ?8, 0, '')",
            params![
                nid,
                guid,
                mid,
                now_s,
                tags,
                flds,
                strip_html(&card.front),
                field_checksum(&front)
            ],
        )?;
        conn.execute(
            "INSERT INTO cards VALUES (?1, ?2, ?3, 0, ?4, -1, 0, 0, ?5, 0, 0, 0, 0, 0, 0, 0, 0, '')",
            params![cid, nid, did, now_s, pos as i64 + 1],
        )?;
    }
    Ok(())
}

/// Writes `cards` as an `.apkg` package at `path` with deck name `deck`.
pub fn write_apkg(path: &Path, deck: &str, cards: &[AnkiCard], now_ms: i64) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| Error::io("creating the export folder", e))?;
    }
    let db_path = path.with_extension("anki2.tmp");
    let _ = fs::remove_file(&db_path);
    let built = (|| {
        let conn = Connection::open(&db_path)?;
        build_collection(&conn, deck, cards, now_ms)?;
        conn.close().map_err(|(_, e)| Error::from(e))?;
        fs::read(&db_path).map_err(|e| Error::io("reading the Anki collection", e))
    })();
    let _ = fs::remove_file(&db_path);
    let db = built?;
    let file = fs::File::create(path).map_err(|e| Error::io("creating the Anki package", e))?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let zerr = |e: zip::result::ZipError| Error::Invalid(format!("writing the Anki package: {e}"));
    let werr = |e| Error::io("writing the Anki package", e);
    zip.start_file("collection.anki2", opts).map_err(zerr)?;
    zip.write_all(&db).map_err(werr)?;
    zip.start_file("media", opts).map_err(zerr)?;
    zip.write_all(b"{}").map_err(werr)?;
    zip.finish().map_err(zerr)?;
    Ok(())
}

/// TSV fallback: `front<TAB>back<TAB>source<TAB>tags`, one line per card,
/// with tabs and line breaks inside fields flattened.
pub fn tsv(cards: &[AnkiCard]) -> String {
    let clean = |s: &str| escape_html(&s.replace(['\t', '\r'], " "));
    let mut out = String::from("#separator:tab\n#html:true\n#tags column:4\n");
    for c in cards {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            clean(&c.front),
            clean(&c.back),
            clean(&c.source),
            c.tags.join(" ")
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cards() -> Vec<AnkiCard> {
        vec![
            AnkiCard {
                id: "i1".into(),
                front: "What does rubisco fix?".into(),
                back: "Carbon dioxide <and> oxygen".into(),
                source: "bio/u1.pdf p:3".into(),
                tags: vec!["bio".into(), "u1".into()],
            },
            AnkiCard {
                id: "i2".into(),
                front: "Krebs cycle site".into(),
                back: "Mitochondrial\tmatrix".into(),
                source: "bio/u2.pdf p:1".into(),
                tags: vec![],
            },
        ]
    }

    #[test]
    fn anki_sha1_known_vectors() {
        let hex = |d: [u8; 20]| d.iter().map(|b| format!("{b:02x}")).collect::<String>();
        assert_eq!(
            hex(sha1(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(hex(sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    }

    #[test]
    fn anki_package_holds_notes_cards_and_media() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("exports/anki/bio.apkg");
        write_apkg(&path, "Lucubra::Biology", &cards(), 1_764_000_000_000).unwrap();
        assert!(!path.with_extension("anki2.tmp").exists());

        let mut zip = zip::ZipArchive::new(fs::File::open(&path).unwrap()).unwrap();
        let mut media = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("media").unwrap(), &mut media).unwrap();
        assert_eq!(media, "{}");
        let db_path = dir.path().join("col.anki2");
        let mut db = Vec::new();
        std::io::Read::read_to_end(&mut zip.by_name("collection.anki2").unwrap(), &mut db).unwrap();
        fs::write(&db_path, db).unwrap();

        let conn = Connection::open(&db_path).unwrap();
        let count = |t: &str| -> i64 {
            conn.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0))
                .unwrap()
        };
        assert_eq!((count("notes"), count("cards"), count("col")), (2, 2, 1));
        let (decks, ver): (String, i64) = conn
            .query_row("SELECT decks, ver FROM col", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(ver, 11);
        assert!(decks.contains("do not flow back"));
        let flds: String = conn
            .query_row("SELECT flds FROM notes LIMIT 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(flds.split('\x1f').count(), 3);
        let orphans: i64 = conn
            .query_row(
                "SELECT count(*) FROM cards WHERE nid NOT IN (SELECT id FROM notes)",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(orphans, 0);
    }

    #[test]
    fn anki_tsv_one_line_per_card() {
        let text = tsv(&cards());
        let rows: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.split('\t').count() == 4));
    }
}
