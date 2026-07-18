use tower_lsp_server::ls_types::{
    Diagnostic, DiagnosticSeverity, DocumentSymbol, FoldingRange, FoldingRangeKind, Position,
    Range, SymbolKind,
};
use tree_sitter::Node;

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
}
