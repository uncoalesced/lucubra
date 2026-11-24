// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Fuzz target for the handoff block parser (BUILD_PLAN M7): any input, raw
//! or wrapped in a block, must parse or fail without panicking.

#![no_main]

use libfuzzer_sys::fuzz_target;
use lucubra_pack::handoff::parse_all;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let _ = parse_all(&text);
    let _ = parse_all(&format!("```lucubra-handoff v1\n{text}\nend: lucubra-handoff\n```"));
});
