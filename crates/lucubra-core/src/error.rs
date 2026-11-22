// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Error type for the core crate.

use std::fmt;
use std::io;

/// Errors raised by the event core, cache and projections.
#[derive(Debug)]
pub enum Error {
    /// File system error, with the path or action that failed.
    Io(String, io::Error),
    /// SQLite cache error.
    Sqlite(rusqlite::Error),
    /// JSON encoding or decoding error.
    Json(serde_json::Error),
    /// The workspace or its data is not in the expected shape.
    Invalid(String),
}

/// Result alias for the core crate.
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// Wraps an I/O error with context.
    pub fn io(context: impl Into<String>, err: io::Error) -> Error {
        Error::Io(context.into(), err)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(context, err) => write!(f, "{context}: {err}"),
            Error::Sqlite(err) => write!(f, "state cache: {err}"),
            Error::Json(err) => write!(f, "json: {err}"),
            Error::Invalid(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(_, err) => Some(err),
            Error::Sqlite(err) => Some(err),
            Error::Json(err) => Some(err),
            Error::Invalid(_) => None,
        }
    }
}

impl From<rusqlite::Error> for Error {
    fn from(err: rusqlite::Error) -> Self {
        Error::Sqlite(err)
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::Json(err)
    }
}
