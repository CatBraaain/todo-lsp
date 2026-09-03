use std::str::FromStr;

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use croner::Cron;
use tower_lsp_server::ls_types::{
    Diagnostic, DiagnosticSeverity, DocumentLink, DocumentSymbol, FoldingRange, FoldingRangeKind,
    Position, Range, SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokensLegend,
    SymbolKind,
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
        // indent, dedent, task_block (handled by its parent) -> skip
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
                .unwrap_or_default();
            (text, range_from_node(&hl))
        }
        None => (String::new(), range_from_node(node)),
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
        .unwrap_or_default();

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

/// Build heading and gray-block folding ranges. A heading with a leading gray
/// child run becomes a comment range; a `source_file` or `task_block`
/// contributes one comment range for its leading run of gray child blocks.
pub fn folding_ranges(root: Node, source: &[u8]) -> Vec<FoldingRange> {
    let tones = line_tones(source);
    let mut out = Vec::new();
    collect_folding_ranges(root, &tones, &mut out);
    out.sort_by_key(|range| (range.start_line, range.end_line));
    out.dedup_by(|left, right| {
        left.start_line == right.start_line && left.end_line == right.end_line
    });
    out
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LineTone {
    Blank,
    Gray,
    Plain,
}

fn line_tones(source: &[u8]) -> Vec<LineTone> {
    source
        .split(|&byte| byte == b'\n')
        .map(|line| {
            let line = if line.last() == Some(&b'\r') {
                &line[..line.len() - 1]
            } else {
                line
            };
            let Ok(text) = std::str::from_utf8(line) else {
                return LineTone::Plain;
            };
            let parts = crate::line::parse_line(text);
            if parts.is_blank() {
                LineTone::Blank
            } else if parts.gray().is_some() {
                LineTone::Gray
            } else {
                LineTone::Plain
            }
        })
        .collect()
}

fn collect_folding_ranges(node: Node, tones: &[LineTone], out: &mut Vec<FoldingRange>) {
    if matches!(node.kind(), "source_file" | "task_block") {
        if let Some(range) = leading_gray_children_range(&node, tones) {
            out.push(range);
        }
    }

    for child in named_children_of(&node) {
        match child.kind() {
            "heading_block" => {
                if let Some(range) = folding_range_for_heading(&child, tones) {
                    out.push(range);
                }
                collect_folding_ranges(child, tones, out);
            }
            "task_block" => collect_folding_ranges(child, tones, out),
            _ => {}
        }
    }
}

fn folding_range_for_heading(node: &Node, tones: &[LineTone]) -> Option<FoldingRange> {
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

    if let Some((_, end_line)) = leading_gray_children_bounds(&task_block, tones) {
        return Some(FoldingRange {
            start_line,
            end_line,
            kind: Some(FoldingRangeKind::Comment),
            ..Default::default()
        });
    }

    let end_line = last_line_of_block(node)? as u32;
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

fn leading_gray_children_range(node: &Node, tones: &[LineTone]) -> Option<FoldingRange> {
    let (start_line, end_line) = leading_gray_children_bounds(node, tones)?;
    if end_line <= start_line {
        return None;
    }
    Some(FoldingRange {
        start_line,
        end_line,
        kind: Some(FoldingRangeKind::Comment),
        ..Default::default()
    })
}

fn leading_gray_children_bounds(node: &Node, tones: &[LineTone]) -> Option<(u32, u32)> {
    let children = named_children_of(node);
    let first = children.first()?;
    if !is_gray_block(first, tones) {
        return None;
    }

    let mut last = first;
    for child in children.iter().skip(1) {
        if !is_gray_block(child, tones) {
            break;
        }
        last = child;
    }

    Some((
        first.start_position().row as u32,
        last_line_of_block(last)? as u32,
    ))
}

fn is_gray_block(node: &Node, tones: &[LineTone]) -> bool {
    let start_line = node.start_position().row;
    let Some(end_line) = last_line_of_block(node) else {
        return false;
    };
    (start_line..=end_line).all(|line| {
        matches!(
            tones.get(line),
            Some(LineTone::Blank | LineTone::Gray)
        )
    })
}

/// Return the last physical line of a `task_line` or `heading_block`. Lines
/// consume their trailing newline and any following blank lines. A heading's
/// zero-width `dedent` can point past its child block, so use `task_block.end`.
fn last_line_of_block(node: &Node) -> Option<usize> {
    let end_row = match node.kind() {
        "task_line" => node.end_position().row,
        "heading_block" => named_children_of(node)
            .into_iter()
            .find(|child| child.kind() == "task_block")
            .map(|task_block| task_block.end_position().row)
            .unwrap_or_else(|| node.end_position().row),
        _ => return None,
    };
    Some(end_row.saturating_sub(1))
}

/// Build diagnostics by walking the tree (severity ERROR, source "todo").
///
/// tree-sitter 0.26 does not expose MISSING nodes through `Node::child()` /
/// `child_count()` / `TreeCursor` — they appear only in `to_sexp()`. So a
/// `has_error()`-based walk is used instead: `has_error()` is set for both
/// ERROR and MISSING and aggregates up the subtree.
///
/// For each node that `has_error()` and is not itself an ERROR node, if none
/// of its traversable children `has_error()`, a non-traversable MISSING node
/// hides directly beneath it — its range is reported as "missing syntax
/// element". Direct ERROR nodes report "syntax error"; their children are
/// skipped to avoid noisy duplicate ranges.
pub fn diagnostics(root: Node) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    if !root.has_error() {
        return out;
    }
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if !node.has_error() {
            continue;
        }
        if node.is_error() {
            out.push(make_diagnostic(range_from_node(&node), "syntax error"));
            continue;
        }
        // `node.has_error()` && !`node.is_error()`: descend into traversable
        // children that still carry an error. If none do, a non-traversable
        // MISSING node sits directly under this node.
        let mut descended = false;
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i as u32) {
                if child.has_error() {
                    descended = true;
                    stack.push(child);
                }
            }
        }
        if !descended {
            out.push(make_diagnostic(
                range_from_node(&node),
                "missing syntax element",
            ));
        }
    }
    out
}

fn make_diagnostic(range: Range, message: &str) -> Diagnostic {
    Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some("todo".to_string()),
        message: message.to_string(),
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

/// Build document links for closed `<https://…>`, `<http://…>` and
/// `<ftp://…>` spans. Link ranges obey the same display precedence as inline
/// styling: gray / Archive / heading lines do not expose individual links.
pub fn document_links(source: &[u8]) -> Vec<DocumentLink> {
    let mut out = Vec::new();
    let mut line_idx = 0u32;
    let mut pos = 0usize;
    while pos < source.len() {
        let line_start = pos;
        let line_end = source[line_start..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|offset| line_start + offset)
            .unwrap_or(source.len());
        let content_end = if line_end > line_start && source[line_end - 1] == b'\r' {
            line_end - 1
        } else {
            line_end
        };
        let line = &source[line_start..content_end];
        if let Ok(text) = std::str::from_utf8(line) {
            let parts = crate::line::parse_line(text);
            if !parts.is_blank()
                && !parts.is_archive_heading(text)
                && parts.gray().is_none()
                && !parts.is_heading()
            {
                let (start, end) = parts.text_range;
                collect_document_links(&line[start..end], start, line_idx, &mut out);
            }
        }
        pos = if line_end < source.len() {
            line_end + 1
        } else {
            source.len()
        };
        line_idx += 1;
    }
    out
}

fn collect_document_links(line: &[u8], offset: usize, line_idx: u32, out: &mut Vec<DocumentLink>) {
    let mut i = 0;
    while i < line.len() {
        if line[i] != b'<' {
            i += 1;
            continue;
        }
        let rest = &line[i + 1..];
        let has_scheme = rest.starts_with(b"https://")
            || rest.starts_with(b"http://")
            || rest.starts_with(b"ftp://");
        if !has_scheme {
            i += 1;
            continue;
        }
        let Some(gt) = rest.iter().position(|&b| b == b'>') else {
            i += 1; // unclosed URL: no link
            continue;
        };
        let end = i + gt + 2;
        if let Ok(url) = std::str::from_utf8(&line[i + 1..end - 1]) {
            if let Ok(target) = url.parse::<tower_lsp_server::ls_types::Uri>() {
                out.push(DocumentLink {
                    range: Range {
                        start: Position {
                            line: line_idx,
                            character: (offset + i) as u32,
                        },
                        end: Position {
                            line: line_idx,
                            character: (offset + end) as u32,
                        },
                    },
                    target: Some(target),
                    tooltip: None,
                    data: None,
                });
            }
        }
        i = end;
    }
}

// === Semantic tokens ===
//
// Highlighting is a line-based scanner over the raw document bytes. Each
// line is classified via `crate::line` (SPEC.md's 用語 model) by the 適用規則
// precedence (archive -> done -> cancelled -> hide -> heading -> plain) and
// emits the corresponding token types. `@start`/`@due`/`@repeat` tags
// additionally carry past/future/valid/invalid modifiers computed from the
// argument (date parse / cron validation vs `now`). Columns are UTF-8 byte
// offsets — the server advertises `offsetEncoding: utf-8`.

/// Token type indices. MUST stay in sync with `semantic_tokens_legend`.
mod tt {
    pub const TODO_LINE: u32 = 0;
    pub const TODO_HEADING_CONTENT: u32 = 1;
    pub const TODO_HEADING_SYMBOL: u32 = 2;
    pub const TODO_TAG: u32 = 3;
    pub const START_TAG: u32 = 4;
    pub const DUE_TAG: u32 = 5;
    pub const REPEAT_TAG: u32 = 6;
    pub const TODO_BOLD: u32 = 7;
    pub const TODO_ITALIC: u32 = 8;
    pub const TODO_CODE: u32 = 9;
    pub const TODO_URL: u32 = 10;
}

/// Token modifier bits. MUST stay in sync with `semantic_tokens_legend`.
mod tm {
    pub const ITALIC: u32 = 1 << 0;
    pub const QUEUE1: u32 = 1 << 1;
    pub const PAST: u32 = 1 << 2;
    pub const FUTURE: u32 = 1 << 3;
    pub const INVALID: u32 = 1 << 4;
    pub const VALID: u32 = 1 << 5;
}

/// The semantic-token legend advertised to clients. The index of each type /
/// modifier MUST stay in sync with the `tt::*` / `tm::*` constants.
pub fn semantic_tokens_legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: vec![
            SemanticTokenType::new("todo-line"),
            SemanticTokenType::new("todo-heading-content"),
            SemanticTokenType::new("todo-heading-symbol"),
            SemanticTokenType::new("todo-tag"),
            SemanticTokenType::new("start-tag"),
            SemanticTokenType::new("due-tag"),
            SemanticTokenType::new("repeat-tag"),
            SemanticTokenType::new("todo-bold"),
            SemanticTokenType::new("todo-italic"),
            SemanticTokenType::new("todo-code"),
            SemanticTokenType::new("todo-url"),
        ],
        token_modifiers: vec![
            SemanticTokenModifier::new("italic"),
            SemanticTokenModifier::new("queue1"),
            SemanticTokenModifier::new("past"),
            SemanticTokenModifier::new("future"),
            SemanticTokenModifier::new("invalid"),
            SemanticTokenModifier::new("valid"),
        ],
    }
}

// Line classification is delegated to `crate::line::parse_line` (SPEC.md's
// 用語 definitions: 見出し行 / タスク行 / タグ列 / 灰色行).

/// Result of interpreting a `@start`/`@due` argument as a date against `now`.
enum DateMod {
    Past,
    Future,
    Invalid,
}

/// Parse a `@start`/`@due` argument and classify it relative to `now`.
///
/// Accepted formats (first match wins, all parsed as UTC): `%Y-%m-%d`,
/// `%Y-%m-%d %H:%M`. `<= now` is `Past`, `> now` is `Future`, and anything
/// that fails to parse is `Invalid`. `now` is a parameter so tests can inject
/// a fixed instant.
fn classify_date(arg: &str, now: DateTime<Utc>) -> DateMod {
    let arg = arg.trim();
    let parsed: Result<chrono::DateTime<chrono::Utc>, _> =
        NaiveDateTime::parse_from_str(arg, "%Y-%m-%d %H:%M")
            .map(|dt| dt.and_utc())
            .or_else(|_| {
                NaiveDate::parse_from_str(arg, "%Y-%m-%d")
                    .map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc())
            });
    match parsed {
        Ok(dt) if dt <= now => DateMod::Past,
        Ok(_) => DateMod::Future,
        Err(_) => DateMod::Invalid,
    }
}

/// Whether `arg` is a valid cron expression. Uses `croner` (POSIX/Vixie 5-field
/// + `L`/`#`/`W` extensions) to match SPEC.md's `@repeat` grammar.
fn is_valid_cron(arg: &str) -> bool {
    Cron::from_str(arg.trim()).is_ok()
}

/// Build the document's semantic tokens by scanning `source` line by line
/// and delta-encoding the result per the LSP spec.
pub fn semantic_tokens(source: &[u8]) -> Vec<SemanticToken> {
    semantic_tokens_at(source, Utc::now())
}

/// Same as [`semantic_tokens`] but with an injectable `now`, so the
/// past/future boundary for `@start`/`@due` is deterministic in tests.
fn semantic_tokens_at(source: &[u8], now: DateTime<Utc>) -> Vec<SemanticToken> {
    // (line, byte_col, byte_len, type_idx, modifier_bitset) — a row may carry
    // several; sorted and delta-encoded below.
    let mut raw: Vec<(u32, u32, u32, u32, u32)> = Vec::new();

    let mut line_idx: u32 = 0;
    let mut pos = 0;
    while pos < source.len() {
        let line_start = pos;
        let line_end = source[line_start..]
            .iter()
            .position(|&b| b == b'\n')
            .map(|offset| line_start + offset)
            .unwrap_or(source.len());
        // Exclude a trailing CR so token spans stay within the line content.
        let content_end = if line_end > line_start && source[line_end - 1] == b'\r' {
            line_end - 1
        } else {
            line_end
        };
        classify_line(&source[line_start..content_end], line_idx, now, &mut raw);

        pos = if line_end < source.len() {
            line_end + 1
        } else {
            source.len()
        };
        line_idx += 1;
    }

    // Stable sort by (line, col): ties keep insertion order.
    raw.sort_by_key(|&(l, c, _, _, _)| (l, c));

    // Delta-encode per LSP spec: delta_start is relative to the previous
    // token's start on the same row, absolute on a new row.
    let mut tokens = Vec::with_capacity(raw.len());
    let mut prev_line = 0u32;
    let mut prev_start = 0u32;
    for (line, start, length, type_idx, mods) in raw {
        let delta_line = line - prev_line;
        let delta_start = if delta_line == 0 {
            start - prev_start
        } else {
            start
        };
        tokens.push(SemanticToken {
            delta_line,
            delta_start,
            length,
            token_type: type_idx,
            token_modifiers_bitset: mods,
        });
        prev_line = line;
        prev_start = start;
    }
    tokens
}

/// Classify a single line (no trailing newline) and push its tokens. Columns
/// are byte offsets from the line start.
fn classify_line(
    line: &[u8],
    line_idx: u32,
    now: DateTime<Utc>,
    raw: &mut Vec<(u32, u32, u32, u32, u32)>,
) {
    let Ok(text) = std::str::from_utf8(line) else {
        return;
    };
    let parts = crate::line::parse_line(text);
    if parts.is_blank() {
        return;
    }
    // 適用規則 1: `Archive:` 見出し行 — whole line gray.
    if parts.is_archive_heading(text) {
        raw.push((line_idx, 0, line.len() as u32, tt::TODO_LINE, 0));
        return;
    }
    // 適用規則 2-4: done / cancelled / hide — the whole line is one grayed
    // token; inner tags and stylings are suppressed. cancelled is italic.
    match parts.gray() {
        Some(crate::line::Gray::Done) | Some(crate::line::Gray::Hide) => {
            raw.push((line_idx, 0, line.len() as u32, tt::TODO_LINE, 0));
            return;
        }
        Some(crate::line::Gray::Cancelled) => {
            raw.push((line_idx, 0, line.len() as u32, tt::TODO_LINE, tm::ITALIC));
            return;
        }
        None => {}
    }
    // 適用規則 5: 見出し行 — content + symbol + the tag column; no stylings.
    if let Some(colon) = parts.colon() {
        let (text_start, text_end) = parts.text_range;
        if text_end > text_start {
            raw.push((
                line_idx,
                text_start as u32,
                (text_end - text_start) as u32,
                tt::TODO_HEADING_CONTENT,
                0,
            ));
            raw.push((line_idx, colon as u32, 1, tt::TODO_HEADING_SYMBOL, 0));
            for tag in &parts.tags {
                push_tag_token(tag, line_idx, now, raw);
            }
        }
        return;
    }
    // 適用規則 6: 通常行 — the tag column, plus stylings over the body text.
    for tag in &parts.tags {
        push_tag_token(tag, line_idx, now, raw);
    }
    let (text_start, text_end) = parts.text_range;
    scan_styles(&line[text_start..text_end], text_start, line_idx, raw);
}

/// Push one token for a tag-column [`Tag`], with the date/cron modifiers for
/// `@start` / `@due` / `@repeat` and the yellow tier for `@queue(1)`.
fn push_tag_token(
    tag: &crate::line::Tag,
    line_idx: u32,
    now: DateTime<Utc>,
    raw: &mut Vec<(u32, u32, u32, u32, u32)>,
) {
    let arg = tag.arg.as_deref().unwrap_or("");
    let (type_idx, mods) = match tag.name.as_str() {
        "start" => {
            let m = match classify_date(arg, now) {
                DateMod::Past => tm::PAST,
                DateMod::Future => tm::FUTURE,
                DateMod::Invalid => tm::INVALID,
            };
            (tt::START_TAG, m)
        }
        "due" => {
            let m = match classify_date(arg, now) {
                DateMod::Past => tm::PAST,
                DateMod::Future => tm::FUTURE,
                DateMod::Invalid => tm::INVALID,
            };
            (tt::DUE_TAG, m)
        }
        "repeat" => {
            let m = if is_valid_cron(arg) {
                tm::VALID
            } else {
                tm::INVALID
            };
            (tt::REPEAT_TAG, m)
        }
        "queue" => {
            let m = if arg == "1" { tm::QUEUE1 } else { 0 };
            (tt::TODO_TAG, m)
        }
        _ => (tt::TODO_TAG, 0),
    };
    raw.push((
        line_idx,
        tag.start as u32,
        (tag.end - tag.start) as u32,
        type_idx,
        mods,
    ));
}

/// Scan `line` for inline stylings (`**bold**`, `*italic*`, `` `code` ``,
/// `<url>`) and push one token per span. Bold is tried before italic at each
/// position to mirror the grammar's `#stylings` include order.
fn scan_styles(
    line: &[u8],
    offset: usize,
    line_idx: u32,
    raw: &mut Vec<(u32, u32, u32, u32, u32)>,
) {
    let mut i = 0;
    while i < line.len() {
        match line[i] {
            b'*' if i + 1 < line.len() && line[i + 1] == b'*' => {
                // bold: content is [^*]* then a closing `**`.
                let mut j = i + 2;
                while j < line.len() && line[j] != b'*' {
                    j += 1;
                }
                if j + 1 < line.len() && line[j + 1] == b'*' {
                    let end = j + 2;
                    raw.push((
                        line_idx,
                        (offset + i) as u32,
                        (end - i) as u32,
                        tt::TODO_BOLD,
                        0,
                    ));
                    i = end;
                } else {
                    i += 2;
                }
            }
            b'*' => {
                // italic: content is [^*]* then a closing `*`.
                let mut j = i + 1;
                while j < line.len() && line[j] != b'*' {
                    j += 1;
                }
                if j < line.len() {
                    let end = j + 1;
                    raw.push((
                        line_idx,
                        (offset + i) as u32,
                        (end - i) as u32,
                        tt::TODO_ITALIC,
                        0,
                    ));
                    i = end;
                } else {
                    i += 1;
                }
            }
            b'`' => {
                // code: a run of n backticks, content, then a run of exactly n.
                let mut n = 0;
                while i + n < line.len() && line[i + n] == b'`' {
                    n += 1;
                }
                let mut k = i + n;
                let mut close: Option<usize> = None;
                while k + n <= line.len() {
                    if (k..k + n).all(|p| line[p] == b'`')
                        && (k == 0 || line[k - 1] != b'`')
                        && (k + n >= line.len() || line[k + n] != b'`')
                    {
                        close = Some(k);
                        break;
                    }
                    k += 1;
                }
                match close {
                    Some(c) => {
                        let end = c + n;
                        raw.push((
                            line_idx,
                            (offset + i) as u32,
                            (end - i) as u32,
                            tt::TODO_CODE,
                            0,
                        ));
                        i = end;
                    }
                    None => i += n,
                }
            }
            b'<' => {
                // url: <scheme://...> up to the next `>`.
                let rest = &line[i + 1..];
                let has_scheme = rest.starts_with(b"https://")
                    || rest.starts_with(b"http://")
                    || rest.starts_with(b"ftp://");
                if has_scheme {
                    if let Some(gt) = rest.iter().position(|&b| b == b'>') {
                        let end = i + 1 + gt + 1;
                        raw.push((
                            line_idx,
                            (offset + i) as u32,
                            (end - i) as u32,
                            tt::TODO_URL,
                            0,
                        ));
                        i = end;
                        continue;
                    }
                }
                i += 1;
            }
            _ => i += 1,
        }
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
        semantic_tokens(text.as_bytes())
    }

    /// Like `semantic_tokens_of` but with a fixed `now` (UTC), so date-tag
    /// past/future boundary tests are deterministic.
    fn tokens_at(text: &str, now: DateTime<Utc>) -> Vec<SemanticToken> {
        semantic_tokens_at(text.as_bytes(), now)
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
    fn symbols_tag_only_line_yields_no_symbol() {
        // `text` is required: a tag-only line is an ERROR node, not a task.
        assert!(symbols_of("@done\n").is_empty());
    }

    #[test]
    fn symbols_heading_without_text_yields_no_symbol() {
        // `text` is required: a heading with no body is an ERROR node.
        assert!(symbols_of(": @done\n").is_empty());
        assert!(symbols_of(":\n").is_empty());
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
        let s =
            symbols_of("Project:\n  Phase 1:\n    design spec\n    prototype\n  kickoff meeting\n");
        assert_eq!(top_names(&s), ["Project"]);
        assert_eq!(s[0].kind, SymbolKind::MODULE);
        assert_eq!(child_names(&s[0]), ["Phase 1", "kickoff meeting"]);
        assert_eq!(child_kinds(&s[0]), [SymbolKind::MODULE, SymbolKind::STRING]);
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

    // ----- folding_ranges (exact 0-based rows) -----

    #[test]
    fn fold_heading_with_only_gray_child_as_comment() {
        let r = folds_of(
            "Archive:\n  Alt + A to move selected grayed blocks to archive @done(2024-01-01) @folding\n",
        );
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].start_line, 0);
        assert_eq!(r[0].end_line, 1);
        assert_eq!(r[0].kind, Some(FoldingRangeKind::Comment));
    }

    #[test]
    fn fold_leading_gray_children_replaces_heading_region_with_comment() {
        let r = folds_of("Project:\n  a @done\n  b @cancelled\n  c\n");
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].start_line, 0);
        assert_eq!(r[0].end_line, 2);
        assert_eq!(r[0].kind, Some(FoldingRangeKind::Comment));
        assert_eq!(r[1].start_line, 1);
        assert_eq!(r[1].end_line, 2);
        assert_eq!(r[1].kind, Some(FoldingRangeKind::Comment));
    }

    #[test]
    fn fold_ignores_gray_children_after_a_plain_first_child() {
        let r = folds_of("Project:\n  a\n  b @done\n  c @hide\n");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].start_line, 0);
        assert_eq!(r[0].end_line, 3);
        assert_eq!(r[0].kind, Some(FoldingRangeKind::Region));
    }

    #[test]
    fn fold_leading_gray_children_across_blank_lines() {
        let r = folds_of("a @done\n\nb @hide\nc\n");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].start_line, 0);
        assert_eq!(r[0].end_line, 2);
        assert_eq!(r[0].kind, Some(FoldingRangeKind::Comment));
    }

    #[test]
    fn fold_all_gray_heading_and_children_as_nested_comments() {
        let r = folds_of("Archive:\n  old @done\n  old2 @hide\n");
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].start_line, 0);
        assert_eq!(r[0].end_line, 2);
        assert_eq!(r[0].kind, Some(FoldingRangeKind::Comment));
        assert_eq!(r[1].start_line, 1);
        assert_eq!(r[1].end_line, 2);
        assert_eq!(r[1].kind, Some(FoldingRangeKind::Comment));
    }

    #[test]
    fn fold_nested_comment_ranges_do_not_partially_overlap() {
        let r = folds_of("Outer:\n  a\n  Inner:\n    x @done\n    y @done\n  z\n");
        assert_eq!(r.len(), 3);
        assert_eq!(r[0].start_line, 0);
        assert_eq!(r[0].end_line, 5);
        assert_eq!(r[0].kind, Some(FoldingRangeKind::Region));
        assert_eq!(r[1].start_line, 2);
        assert_eq!(r[1].end_line, 4);
        assert_eq!(r[1].kind, Some(FoldingRangeKind::Comment));
        assert_eq!(r[2].start_line, 3);
        assert_eq!(r[2].end_line, 4);
        assert_eq!(r[2].kind, Some(FoldingRangeKind::Comment));
    }

    #[test]
    fn fold_omits_single_line_gray_runs_and_duplicate_ranges() {
        assert!(folds_of("a @done\nb\n").is_empty());

        let r = folds_of("P: @done\n  a @done\nz\n");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].start_line, 0);
        assert_eq!(r[0].end_line, 1);
        assert_eq!(r[0].kind, Some(FoldingRangeKind::Comment));
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
        let r =
            folds_of("Project:\n  Phase 1:\n    design spec\n    prototype\n  kickoff meeting\n");
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
        // The walk surfaces both ERROR nodes ("syntax error") and
        // non-traversable MISSING descendants ("missing syntax element").
        // `@done(` and the indented variant yield ERROR nodes; the
        // heading-indented form yields a MISSING _newline. A tag-only line
        // and a textless heading are plain ERROR nodes.
        for input in [
            "@done(",
            "task\n  @done(",
            "Project:\n  @done(",
            "@done",
            ":",
        ] {
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
    fn diagnostics_indented_tag_only_line_is_error_on_its_row() {
        // `@done(` indented under a heading: with `text` required, the
        // tag-only line is a plain ERROR node ("syntax error") sitting on
        // the indented row (row 1), not on the heading.
        let diags = diags_of("Project:\n  @done(");
        assert!(!diags.is_empty());
        for d in &diags {
            assert_eq!(d.message, "syntax error", "got {diags:?}");
            assert_eq!(d.severity, Some(DiagnosticSeverity::ERROR));
            assert_eq!(d.source.as_deref(), Some("todo"));
            assert_eq!(
                d.range.start.line, 1,
                "diagnostic must sit on the indented line, got {diags:?}"
            );
        }
    }

    /// Decode delta-encoded tokens back to absolute (line, col, len, type,
    /// mods) positions — easier to reason about than raw deltas.
    fn abs_positions(tokens: &[SemanticToken]) -> Vec<(u32, u32, u32, u32, u32)> {
        let mut out = Vec::with_capacity(tokens.len());
        let mut line = 0u32;
        let mut col = 0u32;
        for t in tokens {
            line += t.delta_line;
            col = if t.delta_line == 0 {
                col + t.delta_start
            } else {
                t.delta_start
            };
            out.push((line, col, t.length, t.token_type, t.token_modifiers_bitset));
        }
        out
    }

    fn token_tuple(t: &SemanticToken) -> (u32, u32, u32, u32, u32) {
        (
            t.delta_line,
            t.delta_start,
            t.length,
            t.token_type,
            t.token_modifiers_bitset,
        )
    }

    /// A fixed "now" (UTC) for deterministic @start/@due classification.
    fn fixed_now() -> DateTime<Utc> {
        NaiveDate::from_ymd_opt(2024, 6, 15)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_utc()
    }

    #[test]
    fn semantic_tokens_legend_matches_token_indices() {
        // The legend is the contract clients use to decode token_type and
        // token_modifiers_bitset. Pin the order so indices match `tt::*` /
        // `tm::*`.
        let legend = semantic_tokens_legend();
        let type_names: Vec<&str> = legend.token_types.iter().map(|t| t.as_str()).collect();
        assert_eq!(
            type_names,
            [
                "todo-line",
                "todo-heading-content",
                "todo-heading-symbol",
                "todo-tag",
                "start-tag",
                "due-tag",
                "repeat-tag",
                "todo-bold",
                "todo-italic",
                "todo-code",
                "todo-url",
            ]
        );
        let mod_names: Vec<&str> = legend.token_modifiers.iter().map(|m| m.as_str()).collect();
        assert_eq!(
            mod_names,
            ["italic", "queue1", "past", "future", "invalid", "valid"]
        );
    }

    #[test]
    fn semantic_tokens_empty_document() {
        assert!(semantic_tokens_of("").is_empty());
    }

    #[test]
    fn sample_semantic_tokens() {
        let abs = abs_positions(&semantic_tokens_of(SAMPLE));
        assert_eq!(
            abs.len(),
            8,
            "2 headings x2 tokens + 1 tag + 2 gray lines + archive"
        );
        assert_eq!(abs[0], (0, 0, 5, tt::TODO_HEADING_CONTENT, 0)); // L0 "Inbox"
        assert_eq!(abs[1], (0, 5, 1, tt::TODO_HEADING_SYMBOL, 0)); // L0 ":"
                                                                   // L2 has @done in its tag column -> whole line gray.
        assert_eq!(abs[2], (2, 0, 28, tt::TODO_LINE, 0));
        assert_eq!(abs[3], (3, 2, 7, tt::TODO_HEADING_CONTENT, 0)); // L3 "Project"
        assert_eq!(abs[4], (3, 9, 1, tt::TODO_HEADING_SYMBOL, 0)); // L3 ":"
        assert_eq!(abs[5], (4, 15, 15, tt::TODO_TAG, 0)); // L4 "@priority(high)"
        assert_eq!(abs[6], (5, 0, 16, tt::TODO_LINE, 0)); // L5 gray (@done)
        assert_eq!(abs[7], (7, 0, 8, tt::TODO_LINE, 0)); // L7 "Archive:"
    }

    #[test]
    fn semantic_tokens_heading_content_and_symbol() {
        let abs = abs_positions(&semantic_tokens_of("Inbox:\n"));
        assert_eq!(abs.len(), 2);
        assert_eq!(abs[0], (0, 0, 5, tt::TODO_HEADING_CONTENT, 0));
        assert_eq!(abs[1], (0, 5, 1, tt::TODO_HEADING_SYMBOL, 0));
    }

    #[test]
    fn semantic_tokens_heading_without_text_is_empty() {
        // ":\n" is a syntax error (text is required), so no tokens are emitted.
        assert!(semantic_tokens_of(":\n").is_empty());
    }

    #[test]
    fn semantic_tokens_heading_shows_tags_not_stylings() {
        // A heading's tag column is tokenized, but its body shows no stylings.
        let abs = abs_positions(&semantic_tokens_of("List: @priority(high)\n"));
        assert_eq!(abs.len(), 3);
        assert_eq!(abs[0], (0, 0, 4, tt::TODO_HEADING_CONTENT, 0)); // "List"
        assert_eq!(abs[1], (0, 4, 1, tt::TODO_HEADING_SYMBOL, 0)); // ":"
        assert_eq!(abs[2], (0, 6, 15, tt::TODO_TAG, 0)); // "@priority(high)"
    }

    #[test]
    fn semantic_tokens_non_tag_suffix_is_task_line() {
        // SPEC 見出し行: after the colon must come nothing or a tag column.
        // `List: **bold**` is therefore a task line and its styling shows.
        let abs = abs_positions(&semantic_tokens_of("List: **bold**\n"));
        assert_eq!(abs.len(), 1);
        assert_eq!(abs[0], (0, 6, 8, tt::TODO_BOLD, 0));
        // Same for `Foo: @a b` — the tag is mid-line, so no heading, no tag.
        let tokens = semantic_tokens_of("Foo: @a b\n");
        assert!(tokens.is_empty(), "got {tokens:?}");
    }

    #[test]
    fn semantic_tokens_done_with_trailing_text_is_plain() {
        // SPEC タグ列: tags live at the line end. `@done` followed by more
        // text is body text, so the line is not grayed and emits nothing.
        let tokens = semantic_tokens_of("task @done trailing\n");
        assert!(tokens.is_empty(), "got {tokens:?}");
    }

    #[test]
    fn semantic_tokens_done_at_eol_is_grayed() {
        // SPEC 灰色行: a line whose tag column has @done is grayed, whether
        // or not text follows the tag.
        let abs = abs_positions(&semantic_tokens_of("task @done\n"));
        assert_eq!(abs.len(), 1);
        assert_eq!(abs[0], (0, 0, 10, tt::TODO_LINE, 0));
    }

    #[test]
    fn semantic_tokens_cancelled_grayed_italic() {
        let abs = abs_positions(&semantic_tokens_of("call mom @cancelled(2024-01-01)\n"));
        assert_eq!(abs.len(), 1);
        assert_eq!(abs[0], (0, 0, 31, tt::TODO_LINE, tm::ITALIC));
    }

    #[test]
    fn semantic_tokens_cancelled_at_eol_is_grayed_italic() {
        let abs = abs_positions(&semantic_tokens_of("task @cancelled\n"));
        assert_eq!(abs.len(), 1);
        assert_eq!(abs[0], (0, 0, 15, tt::TODO_LINE, tm::ITALIC));
    }

    #[test]
    fn semantic_tokens_hide_grayed() {
        let abs = abs_positions(&semantic_tokens_of("task @hide\n"));
        assert_eq!(abs.len(), 1);
        assert_eq!(abs[0], (0, 0, 10, tt::TODO_LINE, 0));
    }

    #[test]
    fn semantic_tokens_precedence_done_beats_cancelled() {
        // First-listed wins; done has no italic even though @cancelled exists.
        let abs = abs_positions(&semantic_tokens_of("task @cancelled @done\n"));
        assert_eq!(abs.len(), 1);
        assert_eq!(abs[0], (0, 0, 21, tt::TODO_LINE, 0));
    }

    #[test]
    fn semantic_tokens_archive() {
        let abs = abs_positions(&semantic_tokens_of("Archive:\n"));
        assert_eq!(abs.len(), 1);
        assert_eq!(abs[0], (0, 0, 8, tt::TODO_LINE, 0));
        // An indented Archive: heading is still recognized...
        let abs = abs_positions(&semantic_tokens_of("  Archive: @done\n"));
        assert_eq!(abs.len(), 1);
        // ...and the Archive rule outranks @done (no italic).
        assert_eq!(abs[0], (0, 0, 16, tt::TODO_LINE, 0));
        // A suffix that is not a tag column is not an Archive heading.
        let tokens = semantic_tokens_of("  Archive: old stuff\n");
        assert!(tokens.is_empty(), "got {tokens:?}");
    }

    #[test]
    fn semantic_tokens_queue_tiers() {
        // Only @queue(1) gets the yellow tier.
        let abs = abs_positions(&semantic_tokens_of("@queue(1)\n"));
        assert_eq!(abs[0], (0, 0, 9, tt::TODO_TAG, tm::QUEUE1));
        for input in ["@queue(2)\n", "@queue(3)\n", "@queue(9)\n"] {
            let abs = abs_positions(&semantic_tokens_of(input));
            assert_eq!(abs[0], (0, 0, 9, tt::TODO_TAG, 0), "for {input:?}");
        }
    }

    #[test]
    fn semantic_tokens_generic_tag() {
        // A generic tag's range spans the entire @name(arg).
        let abs = abs_positions(&semantic_tokens_of("task @priority(high)\n"));
        assert_eq!(abs.len(), 1);
        assert_eq!(abs[0], (0, 5, 15, tt::TODO_TAG, 0));
    }

    #[test]
    fn semantic_tokens_at_in_text_is_not_tagged() {
        // SPEC 文書構造: an `@` outside the line-end tag column is body text
        // and gets no tag token.
        let tokens = semantic_tokens_of("email user@example.com\n");
        assert!(tokens.is_empty(), "got {tokens:?}");
        // Only the tag column token is highlighted; `a@b` stays body text.
        let abs = abs_positions(&semantic_tokens_of("send to a@b @priority(high)\n"));
        assert_eq!(abs.len(), 1);
        assert_eq!(abs[0], (0, 12, 15, tt::TODO_TAG, 0));
    }

    #[test]
    fn semantic_tokens_multiple_tags_same_line_delta_encoded() {
        // Two tags on one line: the second token's delta_start is relative to
        // the first token's start (same row), not absolute.
        let tokens = semantic_tokens_of("task @a @b\n");
        assert_eq!(tokens.len(), 2);
        assert_eq!(token_tuple(&tokens[0]), (0, 5, 2, tt::TODO_TAG, 0));
        // delta_start = col2 - col1 = 8 - 5 = 3
        assert_eq!(token_tuple(&tokens[1]), (0, 3, 2, tt::TODO_TAG, 0));
    }

    #[test]
    fn semantic_tokens_bold_italic_code_url() {
        let abs = abs_positions(&semantic_tokens_of("**bold**\n"));
        assert_eq!(abs[0], (0, 0, 8, tt::TODO_BOLD, 0));
        let abs = abs_positions(&semantic_tokens_of("*italic*\n"));
        assert_eq!(abs[0], (0, 0, 8, tt::TODO_ITALIC, 0));
        let abs = abs_positions(&semantic_tokens_of("`code`\n"));
        assert_eq!(abs[0], (0, 0, 6, tt::TODO_CODE, 0));
        let abs = abs_positions(&semantic_tokens_of("<https://example.com>\n"));
        assert_eq!(abs[0], (0, 0, 21, tt::TODO_URL, 0));
    }

    #[test]
    fn semantic_tokens_bold_then_italic_ordering() {
        // Bold is tried before italic, so `**b**` is bold then `*i*` is italic.
        let abs = abs_positions(&semantic_tokens_of("**b** *i*\n"));
        assert_eq!(abs.len(), 2);
        assert_eq!(abs[0], (0, 0, 5, tt::TODO_BOLD, 0));
        assert_eq!(abs[1], (0, 6, 3, tt::TODO_ITALIC, 0));
    }

    #[test]
    fn semantic_tokens_code_double_backtick() {
        // A double-backtick fence allows a single backtick inside its content.
        let abs = abs_positions(&semantic_tokens_of("``a`b``\n"));
        assert_eq!(abs.len(), 1);
        assert_eq!(abs[0], (0, 0, 7, tt::TODO_CODE, 0));
    }

    #[test]
    fn semantic_tokens_unclosed_stylings_and_urls_have_no_tokens() {
        // 適用規則: 閉じていないインライン装飾やURLは個別表示しない。
        assert!(semantic_tokens_of("*unclosed\n").is_empty());
        assert!(semantic_tokens_of("see **not bold\n").is_empty());
        assert!(semantic_tokens_of("`unclosed\n").is_empty());
        assert!(semantic_tokens_of("see <https://example.com\n").is_empty());
    }

    #[test]
    fn semantic_tokens_start_tag_modifiers() {
        let now = fixed_now();
        let abs = abs_positions(&tokens_at("@start(2000-01-01)\n", now));
        assert_eq!(abs[0], (0, 0, 18, tt::START_TAG, tm::PAST));
        let abs = abs_positions(&tokens_at("@start(2100-01-01)\n", now));
        assert_eq!(abs[0], (0, 0, 18, tt::START_TAG, tm::FUTURE));
        let abs = abs_positions(&tokens_at("@start(foo)\n", now));
        assert_eq!(abs[0], (0, 0, 11, tt::START_TAG, tm::INVALID));
    }

    #[test]
    fn semantic_tokens_due_tag_modifiers() {
        let now = fixed_now();
        let abs = abs_positions(&tokens_at("@due(2000-01-01)\n", now));
        assert_eq!(abs[0], (0, 0, 16, tt::DUE_TAG, tm::PAST));
        let abs = abs_positions(&tokens_at("@due(2100-01-01)\n", now));
        assert_eq!(abs[0], (0, 0, 16, tt::DUE_TAG, tm::FUTURE));
        let abs = abs_positions(&tokens_at("@due(foo)\n", now));
        assert_eq!(abs[0], (0, 0, 9, tt::DUE_TAG, tm::INVALID));
    }

    #[test]
    fn semantic_tokens_now_boundary_is_past() {
        // parsed == now counts as Past (inclusive boundary).
        let now = fixed_now();
        let abs = abs_positions(&tokens_at("@start(2024-06-15 12:00)\n", now));
        assert_eq!(abs[0], (0, 0, 24, tt::START_TAG, tm::PAST));
    }

    #[test]
    fn semantic_tokens_seconds_format_is_invalid() {
        // SPEC 期限タグ: dates are `YYYY-MM-DD` or `YYYY-MM-DD HH:mm` — a
        // seconds field makes the date unparseable (red + underline).
        let now = fixed_now();
        let abs = abs_positions(&tokens_at("@start(2024-06-15 12:00:00)\n", now));
        assert_eq!(abs[0], (0, 0, 27, tt::START_TAG, tm::INVALID));
    }

    #[test]
    fn semantic_tokens_grayed_suppresses_date_tag() {
        // A grayed line emits only the todo-line; the @due tag is suppressed.
        let abs = abs_positions(&semantic_tokens_of("task @done @due(2100-01-01)\n"));
        assert_eq!(abs.len(), 1);
        assert_eq!(abs[0], (0, 0, 27, tt::TODO_LINE, 0));
    }

    #[test]
    fn semantic_tokens_repeat_valid_invalid() {
        let abs = abs_positions(&semantic_tokens_of("@repeat(0 0 * * *)\n"));
        assert_eq!(abs[0], (0, 0, 18, tt::REPEAT_TAG, tm::VALID));
        let abs = abs_positions(&semantic_tokens_of("@repeat(notacron)\n"));
        assert_eq!(abs[0], (0, 0, 17, tt::REPEAT_TAG, tm::INVALID));
    }

    #[test]
    fn semantic_tokens_non_ascii_byte_columns() {
        // Columns are UTF-8 byte offsets (offsetEncoding utf-8).
        let abs = abs_positions(&semantic_tokens_of("タスク @a\n"));
        assert_eq!(abs.len(), 1);
        assert_eq!(abs[0], (0, 10, 2, tt::TODO_TAG, 0));
    }

    #[test]
    fn semantic_tokens_hash_line_emits_no_token() {
        // A `#`-prefixed line is plain task text with no tags/stylings.
        let tokens = semantic_tokens_of("# just a note\n");
        assert!(tokens.is_empty(), "got {tokens:?}");
    }

    #[test]
    fn document_links_closed_urls_and_display_precedence() {
        let links = document_links(b"see <https://example.com> <ftp://example.org/path>\n");
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].range.start, Position::new(0, 4));
        assert_eq!(links[0].range.end, Position::new(0, 25));
        assert_eq!(
            links[0].target.as_ref().unwrap().to_string(),
            "https://example.com"
        );
        assert_eq!(
            links[1].target.as_ref().unwrap().to_string(),
            "ftp://example.org/path"
        );
        // Unclosed URLs and URLs in gray lines do not expose individual links.
        assert!(document_links(b"see <http://example.com\n").is_empty());
        assert!(document_links(b"see <http://example.com> @done\n").is_empty());
    }

    #[test]
    fn semantic_tokens_cron_l_w_hash_extensions_are_valid() {
        // SPEC 繰り返しタグ: cron式は5フィールドで `L` `#` `W` 拡張を受理する.
        for input in [
            "@repeat(0 0 * * 5L)\n",
            "@repeat(0 0 1W * *)\n",
            "@repeat(0 0 * * 1#1)\n",
        ] {
            let abs = abs_positions(&semantic_tokens_of(input));
            assert_eq!(abs.len(), 1, "for {input:?}");
            assert_eq!(abs[0].3, tt::REPEAT_TAG, "for {input:?}");
            assert_eq!(abs[0].4, tm::VALID, "for {input:?}");
        }
    }
}
