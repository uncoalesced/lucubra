// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! File watching, hashing, format routing, worker supervision, native
//! extractors, classification scoring and full-text indexing.

pub mod discover;
pub mod hash;
pub mod route;

pub use discover::{Discovered, Discovery, discover};
pub use hash::{Hashed, hash_file};
pub use route::{Extractor, Format, route, route_file};
