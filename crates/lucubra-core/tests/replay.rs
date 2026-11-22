// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Gate M1-1: replay gives identical state for any device interleaving,
//! duplicate delivery and partial sync.

use lucubra_core::cache::Cache;
use lucubra_core::event::Event;
use lucubra_core::hlc::Hlc;
use lucubra_core::replay::replay;
use proptest::prelude::*;
use serde_json::{Value, json};

const DEVICES: &[&str] = &["d-0000000a", "d-0000000b", "d-0000000c", "d-0000000d"];

/// One generated operation: (device index, millisecond step, op code, key, value).
type Op = (usize, u64, u8, u8, u8);

fn payload(op: u8, key: u8, value: u8) -> (&'static str, Value) {
    let subject = format!("s{}", key % 3);
    let unit = format!("u{}", key % 4);
    match op % 8 {
        0 => (
            "setting.changed",
            json!({"key": format!("k{key}"), "value": value}),
        ),
        1 => (
            "unit.created",
            json!({"subject": subject, "unit": unit, "title": format!("T{value}")}),
        ),
        2 => (
            "unit.updated",
            json!({"subject": subject, "unit": unit, "summary": format!("S{value}"), "mastery": value}),
        ),
        3 => (
            "exam.set",
            json!({"id": format!("e{}", key % 3), "subject": subject, "date": format!("2026-12-{:02}", value % 28 + 1)}),
        ),
        4 => ("exam.removed", json!({"id": format!("e{}", key % 3)})),
        5 => (
            "profile.set",
            json!({"level": format!("L{value}"), "programme": null}),
        ),
        6 => ("item.answered", json!({"item": key})),
        _ => ("future.type", json!({"x": value})),
    }
}

/// Builds per-device logs. Each device's clock is strictly increasing; clocks
/// on different devices collide often, so ties break on the device id.
fn build(ops: &[Op]) -> Vec<Event> {
    let mut clocks: Vec<Option<Hlc>> = vec![None; DEVICES.len()];
    let mut seqs = vec![0u64; DEVICES.len()];
    let mut ms = vec![1_762_872_000_000u64; DEVICES.len()];
    ops.iter()
        .enumerate()
        .map(|(i, &(d, step, op, key, value))| {
            ms[d] += step;
            seqs[d] += 1;
            let hlc = Hlc::tick(clocks[d].as_ref(), ms[d], DEVICES[d]);
            clocks[d] = Some(hlc.clone());
            let (kind, data) = payload(op, key, value);
            Event {
                v: if op % 13 == 12 { 2 } else { 1 },
                id: format!("01J{i:023}"),
                dev: DEVICES[d].to_owned(),
                seq: seqs[d],
                hlc: hlc.to_string(),
                ts: String::new(),
                kind: kind.to_owned(),
                data,
            }
        })
        .collect()
}

fn op() -> impl Strategy<Value = Op> {
    (
        0..DEVICES.len(),
        0u64..3,
        any::<u8>(),
        any::<u8>(),
        any::<u8>(),
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn replay_is_independent_of_interleaving(
        (ops, perm) in prop::collection::vec(op(), 1..80)
            .prop_flat_map(|ops| {
                let n = ops.len();
                (Just(ops), Just((0..n).collect::<Vec<_>>()).prop_shuffle())
            }),
        dupes in prop::collection::vec(any::<prop::sample::Index>(), 0..10),
    ) {
        let events = build(&ops);
        let reference = replay(events.clone()).state.hash();

        let mut shuffled: Vec<Event> = perm.iter().map(|&i| events[i].clone()).collect();
        prop_assert_eq!(&replay(shuffled.clone()).state.hash(), &reference);

        for d in &dupes {
            shuffled.push(events[d.index(events.len())].clone());
        }
        shuffled.reverse();
        prop_assert_eq!(&replay(shuffled).state.hash(), &reference);
    }

    #[test]
    fn replay_through_partial_sync_matches_full_replay(
        (ops, perm, cuts) in prop::collection::vec(op(), 1..60)
            .prop_flat_map(|ops| {
                let n = ops.len();
                (
                    Just(ops),
                    Just((0..n).collect::<Vec<_>>()).prop_shuffle(),
                    prop::collection::vec(0..=n, 0..4),
                )
            }),
    ) {
        let events = build(&ops);
        let reference = replay(events.clone()).state.hash();

        // A device receives the events in arrival order `perm`, in batches,
        // refreshing its cache after each batch.
        let dir = tempfile::tempdir().unwrap();
        let mut cache = Cache::open(&dir.path().join("cache.sqlite")).unwrap();
        let mut cuts = cuts;
        cuts.sort_unstable();
        for cut in cuts.into_iter().chain([events.len()]) {
            let arrived: Vec<Event> = perm[..cut].iter().map(|&i| events[i].clone()).collect();
            cache.refresh(arrived).unwrap();
        }
        prop_assert_eq!(&cache.state_hash().unwrap(), &reference);

        // Deleting the cache and rebuilding gives the same hash.
        drop(cache);
        std::fs::remove_file(dir.path().join("cache.sqlite")).unwrap();
        let mut rebuilt = Cache::open(&dir.path().join("cache.sqlite")).unwrap();
        rebuilt.refresh(events).unwrap();
        prop_assert_eq!(&rebuilt.state_hash().unwrap(), &reference);
    }
}

#[test]
fn replay_hash_changes_with_content() {
    let a = build(&[(0, 1, 0, 1, 1)]);
    let b = build(&[(0, 1, 0, 1, 2)]);
    assert_ne!(replay(a).state.hash(), replay(b).state.hash());
}
