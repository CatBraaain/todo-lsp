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
