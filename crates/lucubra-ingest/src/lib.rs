// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! File watching, hashing, format routing, worker supervision, native
//! extractors, classification scoring and full-text indexing.

pub mod discover;
pub mod eda;
pub mod epub;
pub mod hash;
pub mod index;
pub mod limits;
pub mod native;
pub mod pipeline;
pub mod route;
pub mod scan;
pub mod spreadsheet;
pub mod worker;
pub mod zipguard;

pub use discover::{Discovered, Discovery, discover};
pub use hash::{Hashed, hash_file};
pub use route::{Extractor, Format, route, route_file};
pub use scan::{ScanReport, Scanned, scan};
