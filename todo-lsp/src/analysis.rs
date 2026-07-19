use std::sync::OnceLock;

use tower_lsp_server::ls_types::{
    Diagnostic, DiagnosticSeverity, DocumentSymbol, FoldingRange, FoldingRangeKind, Position,
    Range, SemanticToken, SemanticTokensLegend, SemanticTokenType, SymbolKind,
};
use tree_sitter::{Language, Node, Query, QueryCursor, StreamingIterator};

/// Build the outline: walk `source_file`'s named children, skipping zero-width
/// `indent`/`dedent` tokens, mapping `heading_block` -> `MODULE` and
/// `task_line` -> `STRING`, recursing through `task_block` for nesting.
pub fn document_symbols(root: Node, source: &[u8]) -> Vec<DocumentSymbol> {
    symbols_from_children(root, source)
}

fn symbols_from_children(node: Node, source: &[u8]) -> Vec<DocumentSymbol> {
    let mut out = Vec::new();
    for child in named_children_of(&node) {
        if let Some(sym) = symbol_from_node(&child, source) {
            out.push(sym);
        }
    }
    out
}

fn symbol_from_node(node: &Node, source: &[u8]) -> Option<DocumentSymbol> {
    match node.kind() {
        "heading_block" => Some(symbol_from_heading_block(node, source)),
        "task_line" => Some(symbol_from_task_line(node, source)),
        // indent, dedent, task_block (handled by its parent), comment (extra) -> skip
        _ => None,
    }
}

fn symbol_from_heading_block(node: &Node, source: &[u8]) -> DocumentSymbol {
    let mut heading_line = None;
    let mut task_block = None;
    for child in named_children_of(node) {
        match child.kind() {
            "heading_line" => heading_line = Some(child),
            "task_block" => task_block = Some(child),
            _ => {}
        }
    }

    let (name, selection_range) = match heading_line {
        Some(hl) => {
            let text = hl
                .child_by_field_name("text")
                .and_then(|t| t.utf8_text(source).ok())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "(untitled)".to_string());
            (text, range_from_node(&hl))
        }
        None => ("(untitled)".to_string(), range_from_node(node)),
    };

    let children = task_block
        .map(|tb| symbols_from_children(tb, source))
        .unwrap_or_default();
    let children = if children.is_empty() {
        None
    } else {
        Some(children)
    };

    make_symbol(
        name,
        SymbolKind::MODULE,
        range_from_node(node),
        selection_range,
        children,
    )
}

fn symbol_from_task_line(node: &Node, source: &[u8]) -> DocumentSymbol {
    let name = node
        .child_by_field_name("text")
        .and_then(|t| t.utf8_text(source).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            named_children_of(node)
                .into_iter()
                .find(|c| c.kind() == "tag")
                .and_then(|tag| tag.child_by_field_name("name"))
                .and_then(|n| n.utf8_text(source).ok())
                .map(|s| format!("@{}", s.trim()))
        })
        .unwrap_or_else(|| "(task)".to_string());

    let range = range_from_node(node);
    make_symbol(name, SymbolKind::STRING, range, range, None)
}

/// Construct a [`DocumentSymbol`]. `ls_types::DocumentSymbol` does not derive
/// `Default` and its `deprecated` field is itself `#[deprecated]`, so we
/// centralize construction here.
#[allow(deprecated)]
fn make_symbol(
    name: String,
    kind: SymbolKind,
    range: Range,
    selection_range: Range,
    children: Option<Vec<DocumentSymbol>>,
) -> DocumentSymbol {
    DocumentSymbol {
        name,
        detail: None,
        kind,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children,
    }
}

/// Build folding ranges: one `FoldingRange` (kind `REGION`) per `heading_block`
/// that owns a `task_block`, spanning from the `heading_line` start to the
/// `task_block` end (the `dedent` token is zero-width and would overshoot by a
/// line, so it is intentionally not used).
pub fn folding_ranges(root: Node, _source: &[u8]) -> Vec<FoldingRange> {
    let mut out = Vec::new();
    collect_folding_ranges(root, &mut out);
    out
}

fn collect_folding_ranges(node: Node, out: &mut Vec<FoldingRange>) {
    for child in named_children_of(&node) {
        match child.kind() {
            "heading_block" => {
                if let Some(r) = folding_range_for_heading(&child) {
                    out.push(r);
                }
                collect_folding_ranges(child, out);
            }
            "task_block" => {
                // Nested heading_blocks live inside a task_block.
                collect_folding_ranges(child, out);
            }
            _ => {}
        }
    }
}

fn folding_range_for_heading(node: &Node) -> Option<FoldingRange> {
    let mut heading_line = None;
    let mut task_block = None;
    for child in named_children_of(node) {
        match child.kind() {
            "heading_line" => heading_line = Some(child),
            "task_block" => task_block = Some(child),
            _ => {}
        }
    }
    let heading_line = heading_line?;
    let task_block = task_block?;
    let start_line = heading_line.start_position().row as u32;
    // task_block.end sits at the start of the line *after* its last child (each
    // task_line/heading_line consumes its trailing newline), so subtract one to
    // fold through the last content line.
    let end_line = (task_block.end_position().row as u32).saturating_sub(1);
    if end_line <= start_line {
        return None;
    }
    Some(FoldingRange {
        start_line,
        end_line,
        kind: Some(FoldingRangeKind::Region),
        ..Default::default()
    })
}

/// Build diagnostics: walk the tree and turn `is_error()` / `is_missing()`
/// nodes into `Diagnostic`s (severity ERROR, source "todo"). Children of an
/// error node are skipped to avoid noisy duplicate ranges.
pub fn diagnostics(root: Node) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.is_error() || node.is_missing() {
            out.push(diagnostic_for_node(&node));
            continue;
        }
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                stack.push(child);
            }
        }
    }
    out
}

fn diagnostic_for_node(node: &Node) -> Diagnostic {
    Diagnostic {
        range: range_from_node(node),
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some("todo".to_string()),
        message: if node.is_missing() {
            "missing syntax element".to_string()
        } else {
            "syntax error".to_string()
        },
        ..Default::default()
    }
}

fn named_children_of<'a>(node: &Node<'a>) -> Vec<Node<'a>> {
    (0..node.named_child_count())
        .filter_map(|i| node.named_child(i as u32))
        .collect()
}

/// Map a node's byte offsets to an LSP `Range`. Columns are UTF-8 byte offsets;
/// the server advertises `offsetEncoding: utf-8` so clients interpret them
/// correctly even for non-ASCII task text.
fn range_from_node(node: &Node) -> Range {
    let start = node.start_position();
    let end = node.end_position();
    Range {
        start: Position {
            line: start.row as u32,
            character: start.column as u32,
        },
        end: Position {
            line: end.row as u32,
            character: end.column as u32,
        },
    }
}

// Tree-sitter highlight query (consumed by `semantic_tokens`). Cross-crate
// path: todo-lsp/src/ -> ../../ -> workspace root -> tree-sitter-todo/queries.
const HIGHLIGHTS_QUERY_SRC: &str =
    include_str!("../../tree-sitter-todo/queries/highlights.scm");

/// Compile the highlight query once and reuse it across requests. The query
/// depends only on the (immutable) language, not on parser state, so caching
/// is safe — unlike `parse`, which rebuilds a fresh parser every call to
/// reset the external scanner's indentation stack.
fn highlights_query() -> &'static Query {
    static Q: OnceLock<Query> = OnceLock::new();
    Q.get_or_init(|| {
        let language: Language = tree_sitter_todo::LANGUAGE.into();
        Query::new(&language, HIGHLIGHTS_QUERY_SRC).expect("failed to compile highlights.scm")
    })
}

/// The semantic-token legend advertised to clients. The index of each token
/// type here MUST stay in sync with the capture -> type-index mapping in
/// `semantic_tokens` (heading = 0, tag = 1).
pub fn semantic_tokens_legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: vec![SemanticTokenType::TYPE, SemanticTokenType::DECORATOR],
        token_modifiers: vec![],
    }
}

/// Build semantic tokens by running the highlight query and delta-encoding
/// the captures. `@comment` is intentionally absent from the legend (the
/// grammar's `comment` rule is dormant — see parse.rs test
/// `hash_line_is_treated_as_task_not_comment`), so any comment capture is
/// skipped via the catch-all arm. Keeping `@comment` out of the legend is
/// deliberate: it forces an explicit legend update when the comment bug is
/// eventually fixed, making the intent visible at that commit.
pub fn semantic_tokens(root: Node, source: &[u8]) -> Vec<SemanticToken> {
    let query = highlights_query();
    let heading_cap = query
        .capture_index_for_name("heading")
        .expect("highlights.scm must define @heading");
    let tag_cap = query
        .capture_index_for_name("tag")
        .expect("highlights.scm must define @tag");

    let mut cursor = QueryCursor::new();
    let mut captures = cursor.captures(query, root, source);

    // (row, col, length, type_idx) — unsorted, a row may carry several.
    let mut raw: Vec<(u32, u32, u32, u32)> = Vec::new();
    while let Some((m, idx)) = captures.next() {
        let cap = m.captures[*idx];
        let type_idx = match cap.index {
            i if i == heading_cap => 0,
            i if i == tag_cap => 1,
            _ => continue, // @comment and any capture not in the legend
        };
        let start = cap.node.start_position();
        let start_byte = cap.node.start_byte();
        let mut end_byte = cap.node.end_byte();
        // heading_line ends with $._newline, so the node spans into the next
        // row (see the comment in `folding_range_for_heading`). LSP semantic
        // tokens cannot span rows, so strip the trailing newline/CR bytes.
        while end_byte > start_byte
            && matches!(source.get(end_byte - 1), Some(b'\n') | Some(b'\r'))
        {
            end_byte -= 1;
        }
        let length = (end_byte - start_byte) as u32;
        if length == 0 {
            continue;
        }
        raw.push((start.row as u32, start.column as u32, length, type_idx));
    }

    // Stable sort by (row, col): ties keep insertion order.
    raw.sort_by_key(|&(r, c, _, _)| (r, c));

    // Delta-encode per LSP spec: delta_start is relative to the previous
    // token's start on the same row, absolute on a new row.
    let mut tokens = Vec::with_capacity(raw.len());
    let mut prev_line = 0u32;
    let mut prev_start = 0u32;
    for (line, start, length, type_idx) in raw {
        let delta_line = line - prev_line;
        let delta_start = if delta_line == 0 { start - prev_start } else { start };
        tokens.push(SemanticToken {
            delta_line,
            delta_start,
            length,
            token_type: type_idx,
            token_modifiers_bitset: 0,
        });
        prev_line = line;
        prev_start = start;
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;

    const SAMPLE: &str = "\
Inbox:
  buy milk
  call mom @done(2024-01-01)
  Project:
    draft spec @priority(high)
    review @done
  wrap up
Archive:
  old task
";

    fn analyze(text: &str) -> (Vec<DocumentSymbol>, Vec<FoldingRange>, Vec<Diagnostic>) {
        let tree = parse(text);
        let root = tree.root_node();
        let source = text.as_bytes();
        (
            document_symbols(root, source),
            folding_ranges(root, source),
            diagnostics(root),
        )
    }

    /// Top-level symbol names in document order.
    fn top_names(symbols: &[DocumentSymbol]) -> Vec<String> {
        symbols.iter().map(|s| s.name.clone()).collect()
    }

    /// Immediate children's names of one symbol (empty when it has none).
    fn child_names(sym: &DocumentSymbol) -> Vec<String> {
        sym.children
            .as_ref()
            .map(|c| c.iter().map(|s| s.name.clone()).collect())
            .unwrap_or_default()
    }

    /// Immediate children's kinds (e.g. to assert MODULE vs STRING nesting).
    fn child_kinds(sym: &DocumentSymbol) -> Vec<SymbolKind> {
        sym.children
            .as_ref()
            .map(|c| c.iter().map(|s| s.kind).collect())
            .unwrap_or_default()
    }

    fn symbols_of(text: &str) -> Vec<DocumentSymbol> {
        let tree = parse(text);
        document_symbols(tree.root_node(), text.as_bytes())
    }

    fn folds_of(text: &str) -> Vec<FoldingRange> {
        let tree = parse(text);
        folding_ranges(tree.root_node(), text.as_bytes())
    }

    fn diags_of(text: &str) -> Vec<Diagnostic> {
        diagnostics(parse(text).root_node())
    }

    fn semantic_tokens_of(text: &str) -> Vec<SemanticToken> {
        let tree = parse(text);
        semantic_tokens(tree.root_node(), text.as_bytes())
    }

    /// Assert that a (valid) input produces no diagnostics.
    fn assert_clean(text: &str) {
        let diags = diags_of(text);
        assert!(
            diags.is_empty(),
            "unexpected diagnostics for {text:?}: {diags:?}"
        );
    }

    #[test]
    fn sample_symbols() {
        let (symbols, _, _) = analyze(SAMPLE);
        assert_eq!(symbols.len(), 2, "top-level: Inbox, Archive");

        let inbox = &symbols[0];
        assert_eq!(inbox.name, "Inbox");
        assert_eq!(inbox.kind, SymbolKind::MODULE);

        let inbox_children = inbox.children.as_ref().expect("Inbox has children");
        assert_eq!(inbox_children.len(), 4);
        assert_eq!(inbox_children[0].name, "buy milk");
        assert_eq!(inbox_children[1].name, "call mom");
        assert_eq!(inbox_children[2].name, "Project");
        assert_eq!(inbox_children[2].kind, SymbolKind::MODULE);
        assert_eq!(inbox_children[3].name, "wrap up");

        let project_children = inbox_children[2]
            .children
            .as_ref()
            .expect("Project has children");
        assert_eq!(project_children.len(), 2);
        assert_eq!(project_children[0].name, "draft spec");
        assert_eq!(project_children[1].name, "review");

        let archive = &symbols[1];
        assert_eq!(archive.name, "Archive");
        let archive_children = archive.children.as_ref().expect("Archive has children");
        assert_eq!(archive_children.len(), 1);
        assert_eq!(archive_children[0].name, "old task");
    }

    #[test]
    fn sample_folding() {
        let (_, ranges, _) = analyze(SAMPLE);
        assert_eq!(ranges.len(), 3, "folds: Inbox, Project, Archive");
        assert!(ranges
            .iter()
            .all(|r| r.kind == Some(FoldingRangeKind::Region)));
        // 0-based rows: Inbox 0..6, Project 3..5, Archive 7..8.
        assert_eq!(ranges[0].start_line, 0);
        assert_eq!(ranges[0].end_line, 6);
        assert_eq!(ranges[1].start_line, 3);
        assert_eq!(ranges[1].end_line, 5);
        assert_eq!(ranges[2].start_line, 7);
        assert_eq!(ranges[2].end_line, 8);
    }

    #[test]
    fn sample_no_diagnostics() {
        let (_, _, diags) = analyze(SAMPLE);
        assert!(diags.is_empty(), "expected no diagnostics, got {diags:?}");
    }

    #[test]
    fn broken_input_has_diagnostics() {
        // `@done(` — a tag whose argument is never closed. With no preceding
        // text the grammar cannot recover, so tree-sitter yields an ERROR node.
        let (_, _, diags) = analyze("@done(");
        assert!(!diags.is_empty(), "expected at least one diagnostic");
        assert!(diags
            .iter()
            .all(|d| d.severity == Some(DiagnosticSeverity::ERROR)));
    }

    // ----- document_symbols (corpus behaviors mirrored at the symbol level) -----

    #[test]
    fn symbols_simple_task_line() {
        let s = symbols_of("buy milk\n");
        assert_eq!(top_names(&s), ["buy milk"]);
        assert_eq!(s[0].kind, SymbolKind::STRING);
        assert!(s[0].children.is_none());
    }

    #[test]
    fn symbols_tab_indentation() {
        let s = symbols_of("A:\n\ttask\n");
        assert_eq!(top_names(&s), ["A"]);
        assert_eq!(s[0].kind, SymbolKind::MODULE);
        assert_eq!(child_names(&s[0]), ["task"]);
        assert_eq!(child_kinds(&s[0]), [SymbolKind::STRING]);
    }

    #[test]
    fn symbols_blank_lines_ignored() {
        let s = symbols_of("task a\n\ntask b\n");
        assert_eq!(top_names(&s), ["task a", "task b"]);
        assert_eq!(s[0].kind, SymbolKind::STRING);
        assert_eq!(s[1].kind, SymbolKind::STRING);
    }

    #[test]
    fn symbols_empty_text_task_falls_back_to_tag() {
        // No `text` field: the first tag's name becomes the label.
        let s = symbols_of("@done\n");
        assert_eq!(top_names(&s), ["@done"]);
        assert_eq!(s[0].kind, SymbolKind::STRING);
    }

    #[test]
    fn symbols_heading_empty_text_with_tag_is_untitled() {
        // Heading with empty text and a tag falls back to "(untitled)".
        let s = symbols_of(": @done\n");
        assert_eq!(top_names(&s), ["(untitled)"]);
        assert_eq!(s[0].kind, SymbolKind::MODULE);
        assert!(s[0].children.is_none());
    }

    #[test]
    fn symbols_colon_in_body_is_task_not_heading() {
        let s = symbols_of("time is 12:30\n");
        assert_eq!(top_names(&s), ["time is 12:30"]);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].kind, SymbolKind::STRING);
    }

    #[test]
    fn symbols_url_in_body_is_single_task() {
        let s = symbols_of("see http://example.com for details\n");
        assert_eq!(top_names(&s), ["see http://example.com for details"]);
        assert_eq!(s[0].kind, SymbolKind::STRING);
    }

    #[test]
    fn symbols_email_at_in_body_not_in_name() {
        // Non-empty text wins over the tag fallback, so `@done` is dropped from
        // the label even though it is a real tag.
        let s = symbols_of("email me at user@example.com @done\n");
        assert_eq!(top_names(&s), ["email me at user@example.com"]);
        assert_eq!(s[0].kind, SymbolKind::STRING);
    }

    #[test]
    fn symbols_nested_headings() {
        let s = symbols_of("Project:\n  Phase 1:\n    design spec\n    prototype\n  kickoff meeting\n");
        assert_eq!(top_names(&s), ["Project"]);
        assert_eq!(s[0].kind, SymbolKind::MODULE);
        assert_eq!(child_names(&s[0]), ["Phase 1", "kickoff meeting"]);
        assert_eq!(
            child_kinds(&s[0]),
            [SymbolKind::MODULE, SymbolKind::STRING]
        );
        let phase1 = &s[0].children.as_ref().unwrap()[0];
        assert_eq!(child_names(phase1), ["design spec", "prototype"]);
    }

    #[test]
    fn symbols_sibling_headings() {
        let s = symbols_of("List A:\n  task 1\nList B:\n  task 2\n");
        assert_eq!(top_names(&s), ["List A", "List B"]);
        assert_eq!(s[0].kind, SymbolKind::MODULE);
        assert_eq!(s[1].kind, SymbolKind::MODULE);
        assert_eq!(child_names(&s[0]), ["task 1"]);
        assert_eq!(child_names(&s[1]), ["task 2"]);
    }

    #[test]
    fn symbols_header_without_body_has_no_children() {
        let s = symbols_of("Inbox:\n");
        assert_eq!(top_names(&s), ["Inbox"]);
        assert_eq!(s[0].kind, SymbolKind::MODULE);
        assert!(s[0].children.is_none());
    }

    #[test]
    fn symbols_tag_arguments_stripped_from_name() {
        for input in [
            "task @done\n",
            "task @done(2024-01-01)\n",
            "task @done(2024-01-01) @folding @priority(high)\n",
        ] {
            let s = symbols_of(input);
            assert_eq!(top_names(&s), ["task"], "for {input:?}");
            assert_eq!(s[0].kind, SymbolKind::STRING, "for {input:?}");
        }
    }

    #[test]
    fn symbols_tag_edge_arguments() {
        for input in [
            "task @flag()\n",
            "task @link(http://example.com/path?q=1)\n",
            "task @note(remember to follow up tomorrow)\n",
            "task @ref(@other)\n",
        ] {
            let s = symbols_of(input);
            assert_eq!(top_names(&s), ["task"], "for {input:?}");
            assert_eq!(s[0].kind, SymbolKind::STRING, "for {input:?}");
        }
        // A tag on a heading line does not change the heading name.
        let s = symbols_of("List: @collapsed\n  item one\n");
        assert_eq!(top_names(&s), ["List"]);
        assert_eq!(s[0].kind, SymbolKind::MODULE);
        assert_eq!(child_names(&s[0]), ["item one"]);
    }

    // ----- folding_ranges (exact 0-based rows, all kind == Region) -----

    #[test]
    fn fold_simple_heading_with_block() {
        let r = folds_of(
            "Archive:\n  Alt + A to move selected grayed blocks to archive @done(2024-01-01) @folding\n",
        );
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].start_line, 0);
        assert_eq!(r[0].end_line, 1);
        assert_eq!(r[0].kind, Some(FoldingRangeKind::Region));
    }

    #[test]
    fn fold_tab_indent() {
        let r = folds_of("A:\n\ttask\n");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].start_line, 0);
        assert_eq!(r[0].end_line, 1);
    }

    #[test]
    fn fold_multiple_task_lines() {
        let r = folds_of("Shopping:\n  apples\n  oranges\n  bananas\n");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].start_line, 0);
        assert_eq!(r[0].end_line, 3);
    }

    #[test]
    fn fold_nested_boundaries() {
        // Project's task_block ends after `kickoff meeting\n` (row 5 -> end 4);
        // Phase 1's ends after `prototype\n` (row 4 -> end 3).
        let r = folds_of("Project:\n  Phase 1:\n    design spec\n    prototype\n  kickoff meeting\n");
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].start_line, 0);
        assert_eq!(r[0].end_line, 4);
        assert_eq!(r[1].start_line, 1);
        assert_eq!(r[1].end_line, 3);
    }

    #[test]
    fn fold_sibling_headings() {
        let r = folds_of("List A:\n  task 1\nList B:\n  task 2\n");
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].start_line, 0);
        assert_eq!(r[0].end_line, 1);
        assert_eq!(r[1].start_line, 2);
        assert_eq!(r[1].end_line, 3);
    }

    #[test]
    fn fold_two_levels() {
        let r = folds_of("A:\n  B:\n    task\n");
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].start_line, 0);
        assert_eq!(r[0].end_line, 2);
        assert_eq!(r[1].start_line, 1);
        assert_eq!(r[1].end_line, 2);
    }

    #[test]
    fn fold_dedent_to_top_level() {
        // `back to top` is a top-level task_line and produces no fold.
        let r = folds_of("A:\n  B:\n    task\nback to top\n");
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].start_line, 0);
        assert_eq!(r[0].end_line, 2);
        assert_eq!(r[1].start_line, 1);
        assert_eq!(r[1].end_line, 2);
    }

    #[test]
    fn fold_dedent_to_intermediate() {
        // `sibling of B` rejoins A's task_block, extending its end to row 3.
        let r = folds_of("A:\n  B:\n    task\n  sibling of B\n");
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].start_line, 0);
        assert_eq!(r[0].end_line, 3);
        assert_eq!(r[1].start_line, 1);
        assert_eq!(r[1].end_line, 2);
    }

    #[test]
    fn fold_header_without_body_is_empty() {
        let r = folds_of("Inbox:\n");
        assert!(r.is_empty());
    }

    #[test]
    fn fold_top_level_task_lines_is_empty() {
        let r = folds_of("buy milk\ncall mom\n");
        assert!(r.is_empty());
    }

    // ----- diagnostics -----

    #[test]
    fn diagnostics_valid_corpus_inputs_are_clean() {
        let valid = [
            "buy milk\n",
            "task a\n\ntask b\n",
            "@done\n",
            ": @done\n",
            "time is 12:30\n",
            "see http://example.com for details\n",
            "email me at user@example.com @done\n",
            "Project:\n  Phase 1:\n    design spec\n    prototype\n  kickoff meeting\n",
            "List A:\n  task 1\nList B:\n  task 2\n",
            "Inbox:\n",
            "A:\n\ttask\n",
            "A:\n  B:\n    task\n",
            "A:\n  B:\n    task\nback to top\n",
            "A:\n  B:\n    task\n  sibling of B\n",
            "task @done(2024-01-01) @folding @priority(high)\n",
            "task @flag()\n",
            "task @link(http://example.com/path?q=1)\n",
            "task @note(remember to follow up tomorrow)\n",
            "task @ref(@other)\n",
            "List: @collapsed\n  item one\n",
        ];
        for input in valid {
            assert_clean(input);
        }
    }

    #[test]
    fn diagnostics_broken_unclosed_tag() {
        let diags = diags_of("@done(");
        assert!(!diags.is_empty(), "expected at least one diagnostic");
        assert!(diags
            .iter()
            .all(|d| d.severity == Some(DiagnosticSeverity::ERROR)));
        assert!(diags.iter().all(|d| d.source.as_deref() == Some("todo")));
    }

    #[test]
    fn diagnostics_broken_indented_tag() {
        // Complements `broken_input_has_diagnostics`, which only covers the
        // no-text case. Here a broken tag follows a top-level task line on the
        // next indented line; tree-sitter yields an ERROR node.
        let diags = diags_of("task\n  @done(");
        assert!(!diags.is_empty(), "expected at least one diagnostic");
        assert!(diags
            .iter()
            .all(|d| d.severity == Some(DiagnosticSeverity::ERROR)));
        assert!(diags.iter().all(|d| d.source.as_deref() == Some("todo")));
    }

    #[test]
    fn diagnostics_messages_in_allowed_set() {
        // Note: `diagnostics()` currently only surfaces ERROR nodes — tree-sitter
        // MISSING nodes are not reachable through `Node::child()`, so the
        // "missing syntax element" branch is effectively dormant. Both inputs
        // here produce ERROR nodes. We still assert the allowed message set so
        // the test stays correct if MISSING detection is ever wired up.
        for input in ["@done(", "task\n  @done("] {
            let diags = diags_of(input);
            assert!(!diags.is_empty(), "expected diagnostics for {input:?}");
            for d in &diags {
                assert!(
                    d.message == "syntax error" || d.message == "missing syntax element",
                    "unexpected message {:?} for {input:?}",
                    d.message
                );
            }
        }
    }

    #[test]
    fn semantic_tokens_legend_matches_token_indices() {
        // The legend is the contract clients use to decode token_type. Pin the
        // order: index 0 = "type" (heading), index 1 = "decorator" (tag).
        let legend = semantic_tokens_legend();
        assert_eq!(legend.token_types.len(), 2);
        assert_eq!(legend.token_types[0].as_str(), "type");
        assert_eq!(legend.token_types[1].as_str(), "decorator");
        assert!(legend.token_modifiers.is_empty());
    }

    #[test]
    fn semantic_tokens_empty_document() {
        assert!(semantic_tokens_of("").is_empty());
    }

    #[test]
    fn sample_semantic_tokens() {
        let tokens = semantic_tokens_of(SAMPLE);
        assert_eq!(tokens.len(), 6, "3 headings + 3 tags");

        // (delta_line, delta_start, length, token_type, modifier_bitset)
        let expected: [(u32, u32, u32, u32, u32); 6] = [
            (0, 0, 6, 0, 0),   // L0 `Inbox:`
            (2, 11, 17, 1, 0), // L2 `@done(2024-01-01)`
            (1, 2, 8, 0, 0),   // L3 `Project:`
            (1, 15, 15, 1, 0), // L4 `@priority(high)`
            (1, 11, 5, 1, 0),  // L5 `@done`
            (2, 0, 8, 0, 0),   // L7 `Archive:`
        ];
        for (i, want) in expected.iter().enumerate() {
            let got = &tokens[i];
            assert_eq!(
                (
                    got.delta_line,
                    got.delta_start,
                    got.length,
                    got.token_type,
                    got.token_modifiers_bitset
                ),
                *want,
                "mismatch at token {i}",
            );
        }
    }

    #[test]
    fn semantic_tokens_heading_alone_strips_trailing_newline() {
        // heading_line ends with $._newline, so the node spans into the next
        // row. Token length must be the line content only (6 for "Inbox:"),
        // not 7 — this verifies the trailing-newline stripping in
        // `semantic_tokens`.
        let tokens = semantic_tokens_of("Inbox:\n");
        assert_eq!(tokens.len(), 1);
        let t = &tokens[0];
        assert_eq!(
            (t.delta_line, t.delta_start, t.length, t.token_type),
            (0, 0, 6, 0)
        );
    }

    #[test]
    fn semantic_tokens_tag_arg_included_in_range() {
        // Pins the grammar contract: a tag with an arg spans @name(arg) fully.
        // "task @done(2024-01-01)\n" -> tag at col 5, length 17.
        let tokens = semantic_tokens_of("task @done(2024-01-01)\n");
        assert_eq!(tokens.len(), 1);
        let t = &tokens[0];
        assert_eq!(
            (t.delta_line, t.delta_start, t.length, t.token_type),
            (0, 5, 17, 1)
        );
    }

    #[test]
    fn semantic_tokens_tag_without_arg() {
        // "review @done\n" -> tag at col 7, length 5 ("@done").
        let tokens = semantic_tokens_of("review @done\n");
        assert_eq!(tokens.len(), 1);
        let t = &tokens[0];
        assert_eq!(
            (t.delta_line, t.delta_start, t.length, t.token_type),
            (0, 7, 5, 1)
        );
    }

    #[test]
    fn semantic_tokens_multiple_tags_same_line_delta_encoded() {
        // Two tags on one line: the second token's delta_start is relative to
        // the first token's start (same row), not absolute.
        // "task @a @b\n" -> @a at col 5 len 2, @b at col 8 len 2.
        let tokens = semantic_tokens_of("task @a @b\n");
        assert_eq!(tokens.len(), 2);
        let first = &tokens[0];
        let second = &tokens[1];
        assert_eq!(
            (first.delta_line, first.delta_start, first.length, first.token_type),
            (0, 5, 2, 1)
        );
        // delta_start = col2 - col1 = 8 - 5 = 3
        assert_eq!(
            (second.delta_line, second.delta_start, second.length, second.token_type),
            (0, 3, 2, 1)
        );
    }

    #[test]
    fn semantic_tokens_hash_line_does_not_produce_comment() {
        // Pins the known grammar bug: `# foo` parses as a task_line, not a
        // comment node. highlights.scm has `(comment) @comment`, but since no
        // comment node is produced it never matches, and @comment is absent
        // from the legend anyway. Result: zero tokens.
        let tokens = semantic_tokens_of("# just a note\n");
        assert!(tokens.is_empty(), "comment capture is dormant; got {tokens:?}");
    }
}
