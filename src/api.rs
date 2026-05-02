//! Library-side public API.
//!
//! Holds the `diff_file_content` and `check_only_text` functions that
//! were previously private to `main.rs`. Both `main.rs` (the binary)
//! and `lib.rs` (the library, added by difftui's T010) include this
//! module so the same code path serves both.

use std::env;
use std::path::Path;

use humansize::{format_size, FormatSizeOptions, BINARY};
use typed_arena::Arena;

use crate::diff::changes::ChangeMap;
use crate::diff::dijkstra::{mark_syntax, ExceededGraphLimit};
use crate::diff::sliders::fix_all_sliders;
use crate::diff::unchanged;
use crate::display::context::opposite_positions;
use crate::display::hunks::{matched_pos_to_hunks, merge_adjacent};
use crate::lines::MaxLine;
use crate::line_parser;
use crate::options::{DiffOptions, DisplayOptions, FileArgument};
use crate::parse::guess_language::{guess, language_name, LanguageOverride};
use crate::parse::syntax::{self, init_next_prev};
use crate::parse::tree_sitter_parser as tsp;
use crate::summary::{DiffResult, FileContent, FileFormat};

/// Build the `DiffResult` produced for a file when we already know we
/// are running in `--check-only` mode and tree-sitter parsing failed.
/// Skips Dijkstra and yields a structurally-empty result that still
/// reports byte- and syntactic-change flags.
pub fn check_only_text(
    file_format: &FileFormat,
    display_path: &str,
    extra_info: Option<String>,
    lhs_src: &str,
    rhs_src: &str,
) -> DiffResult {
    let has_byte_changes = if lhs_src == rhs_src {
        None
    } else {
        Some((lhs_src.as_bytes().len(), rhs_src.as_bytes().len()))
    };

    DiffResult {
        display_path: display_path.to_owned(),
        extra_info,
        file_format: file_format.clone(),
        lhs_src: FileContent::Text(lhs_src.into()),
        rhs_src: FileContent::Text(rhs_src.into()),
        lhs_positions: vec![],
        rhs_positions: vec![],
        hunks: vec![],
        has_byte_changes,
        has_syntactic_changes: lhs_src != rhs_src,
    }
}

/// The library-callable entry point: diff two already-loaded source
/// strings. Returns a `DiffResult` (never errors — falls back to
/// line-oriented diff for non-tree-sitter content, exceeded limits,
/// etc.).
pub fn diff_file_content(
    display_path: &str,
    extra_info: Option<String>,
    _lhs_path: &FileArgument,
    rhs_path: &FileArgument,
    lhs_src: &str,
    rhs_src: &str,
    display_options: &DisplayOptions,
    diff_options: &DiffOptions,
    overrides: &[(LanguageOverride, Vec<glob::Pattern>)],
) -> DiffResult {
    let guess_src = match rhs_path {
        FileArgument::DevNull => &lhs_src,
        _ => &rhs_src,
    };

    let language = guess(Path::new(display_path), guess_src, overrides);
    let lang_config = language.map(|lang| (lang, tsp::from_language(lang)));

    if lhs_src == rhs_src {
        let file_format = match language {
            Some(language) => FileFormat::SupportedLanguage(language),
            None => FileFormat::PlainText,
        };

        return DiffResult {
            extra_info,
            display_path: display_path.to_owned(),
            file_format,
            lhs_src: FileContent::Text("".into()),
            rhs_src: FileContent::Text("".into()),
            lhs_positions: vec![],
            rhs_positions: vec![],
            hunks: vec![],
            has_byte_changes: None,
            has_syntactic_changes: false,
        };
    }

    let (file_format, lhs_positions, rhs_positions) = match lang_config {
        None => {
            let file_format = FileFormat::PlainText;
            if diff_options.check_only {
                return check_only_text(&file_format, display_path, extra_info, lhs_src, rhs_src);
            }

            let lhs_positions = line_parser::change_positions(lhs_src, rhs_src);
            let rhs_positions = line_parser::change_positions(rhs_src, lhs_src);
            (file_format, lhs_positions, rhs_positions)
        }
        Some((language, lang_config)) => {
            let arena = Arena::new();
            match tsp::to_tree_with_limit(diff_options, &lang_config, lhs_src, rhs_src) {
                Ok((lhs_tree, rhs_tree)) => {
                    match tsp::to_syntax_with_limit(
                        lhs_src,
                        rhs_src,
                        &lhs_tree,
                        &rhs_tree,
                        &arena,
                        &lang_config,
                        diff_options,
                    ) {
                        Ok((lhs, rhs)) => {
                            if diff_options.check_only {
                                let has_syntactic_changes = lhs != rhs;
                                let has_byte_changes = if lhs_src == rhs_src {
                                    None
                                } else {
                                    Some((lhs_src.as_bytes().len(), rhs_src.as_bytes().len()))
                                };
                                return DiffResult {
                                    extra_info,
                                    display_path: display_path.to_owned(),
                                    file_format: FileFormat::SupportedLanguage(language),
                                    lhs_src: FileContent::Text(lhs_src.to_owned()),
                                    rhs_src: FileContent::Text(rhs_src.to_owned()),
                                    lhs_positions: vec![],
                                    rhs_positions: vec![],
                                    hunks: vec![],
                                    has_byte_changes,
                                    has_syntactic_changes,
                                };
                            }

                            let mut change_map = ChangeMap::default();
                            let possibly_changed = if env::var("DFT_DBG_KEEP_UNCHANGED").is_ok() {
                                vec![(lhs.clone(), rhs.clone())]
                            } else {
                                unchanged::mark_unchanged(&lhs, &rhs, &mut change_map)
                            };

                            let mut exceeded_graph_limit = false;
                            for (lhs_section_nodes, rhs_section_nodes) in possibly_changed {
                                init_next_prev(&lhs_section_nodes);
                                init_next_prev(&rhs_section_nodes);

                                match mark_syntax(
                                    lhs_section_nodes.first().copied(),
                                    rhs_section_nodes.first().copied(),
                                    &mut change_map,
                                    diff_options.graph_limit,
                                ) {
                                    Ok(()) => {}
                                    Err(ExceededGraphLimit {}) => {
                                        exceeded_graph_limit = true;
                                        break;
                                    }
                                }
                            }

                            if exceeded_graph_limit {
                                let lhs_positions =
                                    line_parser::change_positions(lhs_src, rhs_src);
                                let rhs_positions =
                                    line_parser::change_positions(rhs_src, lhs_src);
                                (
                                    FileFormat::TextFallback {
                                        reason: "exceeded DFT_GRAPH_LIMIT".into(),
                                    },
                                    lhs_positions,
                                    rhs_positions,
                                )
                            } else {
                                fix_all_sliders(language, &lhs, &mut change_map);
                                fix_all_sliders(language, &rhs, &mut change_map);

                                let mut lhs_positions =
                                    syntax::change_positions(&lhs, &change_map);
                                let mut rhs_positions =
                                    syntax::change_positions(&rhs, &change_map);

                                if diff_options.ignore_comments {
                                    let lhs_comments = tsp::comment_positions(
                                        &lhs_tree, lhs_src, &lang_config,
                                    );
                                    lhs_positions.extend(lhs_comments);
                                    let rhs_comments = tsp::comment_positions(
                                        &rhs_tree, rhs_src, &lang_config,
                                    );
                                    rhs_positions.extend(rhs_comments);
                                }

                                (
                                    FileFormat::SupportedLanguage(language),
                                    lhs_positions,
                                    rhs_positions,
                                )
                            }
                        }
                        Err(tsp::ExceededParseErrorLimit(error_count)) => {
                            let file_format = FileFormat::TextFallback {
                                reason: format!(
                                    "{} {} parse error{}, exceeded DFT_PARSE_ERROR_LIMIT",
                                    error_count,
                                    language_name(language),
                                    if error_count == 1 { "" } else { "s" }
                                ),
                            };
                            if diff_options.check_only {
                                return check_only_text(
                                    &file_format,
                                    display_path,
                                    extra_info,
                                    lhs_src,
                                    rhs_src,
                                );
                            }
                            let lhs_positions =
                                line_parser::change_positions(lhs_src, rhs_src);
                            let rhs_positions =
                                line_parser::change_positions(rhs_src, lhs_src);
                            (file_format, lhs_positions, rhs_positions)
                        }
                    }
                }
                Err(tsp::ExceededByteLimit(num_bytes)) => {
                    let format_options = FormatSizeOptions::from(BINARY).decimal_places(1);
                    let file_format = FileFormat::TextFallback {
                        reason: format!(
                            "{} exceeded DFT_BYTE_LIMIT",
                            &format_size(num_bytes, format_options)
                        ),
                    };
                    if diff_options.check_only {
                        return check_only_text(
                            &file_format,
                            display_path,
                            extra_info,
                            lhs_src,
                            rhs_src,
                        );
                    }
                    let lhs_positions = line_parser::change_positions(lhs_src, rhs_src);
                    let rhs_positions = line_parser::change_positions(rhs_src, lhs_src);
                    (file_format, lhs_positions, rhs_positions)
                }
            }
        }
    };

    let opposite_to_lhs = opposite_positions(&lhs_positions);
    let opposite_to_rhs = opposite_positions(&rhs_positions);

    let hunks = matched_pos_to_hunks(&lhs_positions, &rhs_positions);
    let hunks = merge_adjacent(
        &hunks,
        &opposite_to_lhs,
        &opposite_to_rhs,
        lhs_src.max_line(),
        rhs_src.max_line(),
        display_options.num_context_lines as usize,
    );
    let has_syntactic_changes = !hunks.is_empty();

    let has_byte_changes = if lhs_src == rhs_src {
        None
    } else {
        Some((lhs_src.as_bytes().len(), rhs_src.as_bytes().len()))
    };

    DiffResult {
        extra_info,
        display_path: display_path.to_owned(),
        file_format,
        lhs_src: FileContent::Text(lhs_src.to_owned()),
        rhs_src: FileContent::Text(rhs_src.to_owned()),
        lhs_positions,
        rhs_positions,
        hunks,
        has_byte_changes,
        has_syntactic_changes,
    }
}
