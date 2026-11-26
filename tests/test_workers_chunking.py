# SPDX-License-Identifier: Apache-2.0
# Engineered by uncoalesced
"""Text chunking shared by the worker extractors."""

from lucubra_workers.chunking import CHUNK_CHARS, section_chunks, split_text


def test_short_paragraphs_join_into_one_piece():
    assert split_text("one\n\ntwo\n\n\n\nthree") == ["one\n\ntwo\n\nthree"]


def test_blank_text_gives_no_pieces():
    assert split_text("") == []
    assert split_text("\n\n  \n\n") == []


def test_pieces_never_exceed_the_limit():
    text = "\n\n".join(["word " * 30] * 20) + "\n\n" + "x" * 95
    pieces = split_text(text, limit=100)
    assert pieces
    assert all(len(p) <= 100 for p in pieces)
    assert "".join(p.replace("\n", "") for p in pieces).count("x") == 95


def test_one_long_line_is_cut_by_characters():
    pieces = split_text("a" * 250, limit=100)
    assert [len(p) for p in pieces] == [100, 100, 50]


def test_default_limit_matches_rust_extractors():
    assert CHUNK_CHARS == 2000
    assert all(len(p) <= CHUNK_CHARS for p in split_text("z" * 4500))


def test_section_chunks_number_sections_and_parts():
    chunks = section_chunks(["intro", "b" * 4500], prefix="s")
    refs = [c["ref"] for c in chunks]
    assert refs == ["s:0", "s:1", "s:1.1", "s:1.2"]
    assert chunks[0]["text"] == "intro"
