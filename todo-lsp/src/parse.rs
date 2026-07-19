use std::collections::HashMap;

use tower_lsp_server::ls_types::Uri;
use tree_sitter::Tree;

/// A parsed document: its version, full text, and the syntax tree.
pub struct Document {
    /// LSP document version, retained for future version-aware diagnostics.
    #[allow(dead_code)]
    pub version: i32,
    pub text: String,
    pub tree: Tree,
}

pub type DocumentStore = HashMap<Uri, Document>;

/// Full re-parse of the given text.
///
/// A fresh [`Parser`](tree_sitter::Parser) is constructed on every call. This is
/// deliberate: the grammar's external scanner keeps an indentation stack as
/// state, and full re-parse (`old_tree = None`) cleanly re-initializes it,
/// avoiding subtle desync that incremental `Tree::edit` could introduce when an
/// edit changes indentation. The parser is also dropped before returning, so no
/// parser state crosses an `.await`.
pub fn parse(text: &str) -> Tree {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_todo::LANGUAGE.into())
        .expect("failed to load todo grammar");
    parser.parse(text, None).expect("failed to parse")
}

#[cfg(test)]
mod tests {
    use super::*;

    // `Node` borrows the `Tree` it came from, so these helpers own the freshly
    // parsed tree for the duration of the call and return owned values rather
    // than handing back a borrowing `Node`.

    fn root_kind(text: &str) -> String {
        parse(text).root_node().kind().to_string()
    }

    fn named_child_count(text: &str) -> usize {
        parse(text).root_node().named_child_count()
    }

    fn first_named_child_kind(text: &str) -> Option<String> {
        let tree = parse(text);
        tree.root_node()
            .named_child(0)
            .map(|c| c.kind().to_string())
    }

    fn has_error(text: &str) -> bool {
        parse(text).root_node().has_error()
    }

    #[test]
    fn grammar_loads_for_simple_input() {
        assert_eq!(root_kind("buy milk\n"), "source_file");
        assert_eq!(named_child_count("buy milk\n"), 1);
        assert_eq!(
            first_named_child_kind("buy milk\n").as_deref(),
            Some("task_line")
        );
    }

    #[test]
    fn empty_input_is_clean() {
        assert_eq!(root_kind(""), "source_file");
        assert_eq!(named_child_count(""), 0);
        assert!(!has_error(""));
    }

    #[test]
    fn hash_line_is_treated_as_task_not_comment() {
        // NOTE: the grammar declares a `comment` extra (`# ...`), but the
        // external scanner that produces `text` consumes the whole line,
        // including the leading `#`. So a `#`-prefixed line parses as a
        // `task_line`, not as a comment. This pins the current behavior.
        let input = "# just a note\n";
        assert!(!has_error(input));
        assert_eq!(named_child_count(input), 1);
        assert_eq!(first_named_child_kind(input).as_deref(), Some("task_line"));
    }

    #[test]
    fn leading_blank_lines_are_consumed() {
        assert!(!has_error("\n\ntask\n"));
        assert_eq!(named_child_count("\n\ntask\n"), 1);
        assert_eq!(
            first_named_child_kind("\n\ntask\n").as_deref(),
            Some("task_line")
        );
    }

    #[test]
    fn valid_nested_input_has_no_error() {
        let nested = "Project:\n  Phase 1:\n    design spec\n    prototype\n  kickoff meeting\n";
        assert!(!has_error(nested));
    }

    #[test]
    fn broken_input_sets_has_error() {
        // `@done(` with no preceding text: the grammar's `repeat1(tag)` path
        // forces a tag, then `(` (with no closing `)`) is unexpected at EOF, so
        // tree-sitter yields an ERROR node.
        assert!(has_error("@done("));
    }

    #[test]
    fn incomplete_tag_after_text_is_absorbed_into_text() {
        // `task @done(` (text present): the scanner never recognizes `@done(`
        // as a tag because its `(` is unclosed, so the whole line is consumed
        // as a single `text` token and no error is reported. This pins the
        // current behavior (the scanner avoids false-positive errors here).
        assert!(!has_error("task @done("));
        assert_eq!(first_named_child_kind("task @done(").as_deref(), Some("task_line"));
    }

    #[test]
    fn parse_is_deterministic() {
        // The fresh-parser-per-call contract must yield identical trees for the
        // same input across calls.
        let input = "A:\n  task\n  B:\n    deeper\n";
        let first = parse(input).root_node().to_sexp();
        let second = parse(input).root_node().to_sexp();
        assert_eq!(first, second);
    }
}
