//! Library surface of difftastic, added to support callers like
//! difftui that consume the structural-diff machinery as a library
//! rather than via the `difft` binary.
//!
//! The binary (`src/main.rs`) remains the primary entry point and
//! continues to declare its own copies of these modules. The library
//! and the binary are compiled as separate crates in the same package;
//! this is wasteful in build time but lets the library land without
//! disturbing any existing `difft` behaviour.

// Mirror the lints from `main.rs` that affect compilation. These are
// the only ones that are required for the library to compile cleanly
// with the rest of the source as-is.
#![allow(renamed_and_removed_lints)]
#![allow(clippy::type_complexity)]
#![allow(clippy::comparison_to_empty)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::if_same_then_else)]
#![allow(clippy::mutable_key_type)]
#![allow(unknown_lints)]
#![allow(clippy::manual_unwrap_or_default)]
#![allow(clippy::implicit_saturating_sub)]
#![allow(clippy::needless_as_bytes)]

#[macro_use]
extern crate log;

// All modules currently declared in `main.rs`. Mirror their privacy:
// modules are kept `pub` here so the curated re-exports below resolve.
pub mod conflicts;
pub mod constants;
pub mod diff;
pub mod display;
pub mod exit_codes;
pub mod files;
pub mod gitattributes;
pub mod hash;
pub mod line_parser;
pub mod lines;
pub mod options;
pub mod parse;
pub mod summary;
pub mod version;
pub mod words;

// New library-only module containing the `Result`-shaped (well, total —
// difftastic falls back instead of erroring) `diff_file_content` entry
// point and its `check_only_text` helper.
pub mod api;

// ---- Curated public surface ----------------------------------------
//
// difftui's `Differ` impl reaches for these. Re-exports give it a flat
// `use difftastic::*` path without coupling to internal module layout.

pub use api::{check_only_text, diff_file_content};
pub use display::hunks::Hunk;
pub use options::{DiffOptions, DisplayOptions, FileArgument};
pub use parse::guess_language::{guess, language_name, Language, LanguageOverride};
pub use line_numbers::SingleLineSpan;
pub use parse::syntax::{AtomKind, MatchKind, MatchedPos, StringKind, Syntax, TokenKind};
pub use summary::{DiffResult, FileContent, FileFormat};
