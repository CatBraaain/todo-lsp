//! Minimal-edit computation for §コマンド edits and semantic-token deltas.
//!
//! `line_edits` turns a `text -> text` command result into line-limited
//! `TextEdit`s (SPEC.md §コマンド: 編集は編集前後で内容が変わる行に限定される).
//! `token_delta_edits` turns two semantic-token arrays into
//! `SemanticTokensEdit`s (SPEC.md §表示 色付けの更新: 既知の応答識別子には
//! 差分を返す).

use tower_lsp_server::ls_types::{
    Position, Range, SemanticToken, SemanticTokensEdit, TextEdit,
};

/// Whole lines of `text`, each slice keeping its trailing `\n` when present.
fn lines_with_newline(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

/// A line slice without its line ending (`\r\n` or `\n`) — the EOL is not
/// part of the replaced content; the surrounding lines keep their own.
fn line_content(line: &str) -> &str {
    line.strip_suffix("\r\n")
        .or_else(|| line.strip_suffix('\n'))
        .unwrap_or(line)
}

/// Re-line-end `text` (built from LF lines) to `model`'s EOL style: when
/// `model` contains any CRLF, every LF in `text` becomes CRLF. Mixed-EOL
/// documents are unified to CRLF; CR-only line endings are unsupported
/// (VSCode documents use LF or CRLF). `text` must not already contain CR.
pub(crate) fn match_eol(model: &str, text: String) -> String {
    if model.contains("\r\n") {
        text.replace('\n', "\r\n")
    } else {
        text
    }
}

/// §コマンド: line-limited edits transforming `old` into `new`.
///
/// Line-level LCS: every maximal run of changed lines becomes one edit whose
/// range never covers an unchanged line. Returns `[]` when the texts are
/// equal. Positions use UTF-8 byte columns (the server's offsetEncoding).
pub fn line_edits(old: &str, new: &str) -> Vec<TextEdit> {
    // `new` is built from LF lines; emit it in the document's own EOL so
    // unchanged lines stay byte-identical (CRLF documents keep CRLF).
    let new = match_eol(old, new.to_string());
    let old_lines = lines_with_newline(old);
    let new_lines = lines_with_newline(&new);
    let n = old_lines.len();
    let m = new_lines.len();

    // Trim the common prefix/suffix; the LCS table only covers the middle.
    let prefix = (0..n.min(m))
        .take_while(|&i| old_lines[i] == new_lines[i])
        .count();
    let suffix = (0..(n - prefix).min(m - prefix))
        .take_while(|&i| old_lines[n - 1 - i] == new_lines[m - 1 - i])
        .count();
    let old_mid = &old_lines[prefix..n - suffix];
    let new_mid = &new_lines[prefix..m - suffix];

    // Longest common subsequence of the middle. Documents here are small, so
    // the O(n*m) table is fine for a one-shot command execution.
    let on = old_mid.len();
    let mn = new_mid.len();
    let w = mn + 1;
    let mut dp = vec![0u32; (on + 1) * w];
    for i in (0..on).rev() {
        for j in (0..mn).rev() {
            dp[i * w + j] = if old_mid[i] == new_mid[j] {
                dp[(i + 1) * w + j + 1] + 1
            } else {
                dp[(i + 1) * w + j].max(dp[i * w + j + 1])
            };
        }
    }

    // Matches as (old_line, new_line) pairs, ascending. Prefix and suffix
    // lines match themselves; the LCS path fills the middle.
    let mut matches: Vec<(usize, usize)> = (0..prefix).map(|i| (i, i)).collect();
    let (mut i, mut j) = (0, 0);
    while i < on && j < mn {
        if old_mid[i] == new_mid[j] {
            matches.push((prefix + i, prefix + j));
            i += 1;
            j += 1;
        } else if dp[(i + 1) * w + j] >= dp[i * w + j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    matches.extend((0..suffix).map(|k| (n - suffix + k, m - suffix + k)));

    // Each gap between consecutive matches (including before the first and
    // after the last) is one changed-line run.
    let mut edits = Vec::new();
    let mut cursor = (0usize, 0usize);
    for &(om, nm) in matches.iter().chain(std::iter::once(&(n, m))) {
        if om > cursor.0 || nm > cursor.1 {
            edits.push(run_edit(
                &old_lines,
                cursor.0,
                om,
                &new_lines,
                cursor.1,
                nm,
            ));
        }
        cursor = (om + 1, nm + 1);
    }
    edits
}

/// One edit for the changed-line run: old lines `a0..a1` become new lines
/// `c0..c1` (exactly one side is non-empty, or both for a replacement).
fn run_edit(
    old_lines: &[&str],
    a0: usize,
    a1: usize,
    new_lines: &[&str],
    c0: usize,
    c1: usize,
) -> TextEdit {
    let old_ends_with_newline = old_ends_with_newline(old_lines);

    if c0 == c1 {
        // Deletion: cover the removed lines including the last one's newline
        // so no blank line remains. Line `a1` always exists here unless the
        // run reaches the end of a text without a trailing newline.
        let end = if a1 < old_lines.len() || old_ends_with_newline {
            Position::new(a1 as u32, 0)
        } else {
            Position::new((a1 - 1) as u32, line_content(old_lines[a1 - 1]).len() as u32)
        };
        return TextEdit {
            range: Range::new(Position::new(a0 as u32, 0), end),
            new_text: String::new(),
        };
    }

    if a0 == a1 {
        // Insertion between lines `a0 - 1` and `a0`. Inside the document the
        // insertion point is the start of line `a0`; at the end of the text
        // it is the line boundary after the last line.
        let (range, new_text) = if a1 < old_lines.len() {
            (
                Range::new(
                    Position::new(a1 as u32, 0),
                    Position::new(a1 as u32, 0),
                ),
                new_lines[c0..c1].concat(),
            )
        } else if old_ends_with_newline {
            let line = old_lines.len() as u32;
            (
                Range::new(Position::new(line, 0), Position::new(line, 0)),
                new_lines[c0..c1].concat(),
            )
        } else {
            let last = old_lines.len() - 1;
            (
                Range::new(
                    Position::new(last as u32, line_content(old_lines[last]).len() as u32),
                    Position::new(last as u32, line_content(old_lines[last]).len() as u32),
                ),
                format!("\n{}", new_lines[c0..c1].concat()),
            )
        };
        return TextEdit { range, new_text };
    }

    // Replacement: the range covers whole old lines but leaves the last
    // line's newline in place; the new text supplies the rest.
    TextEdit {
        range: Range::new(
            Position::new(a0 as u32, 0),
            Position::new(
                (a1 - 1) as u32,
                line_content(old_lines[a1 - 1]).len() as u32,
            ),
        ),
        new_text: format!(
            "{}{}",
            new_lines[c0..c1 - 1].concat(),
            line_content(new_lines[c1 - 1]),
        ),
    }
}

fn old_ends_with_newline(old_lines: &[&str]) -> bool {
    old_lines.last().is_some_and(|l| l.ends_with('\n'))
}

/// §表示 色付けの更新: edits transforming `old` token data into `new`.
///
/// One prefix/suffix edit at token granularity. `start` / `deleteCount` are
/// indices into the flattened data array (5 numbers per token).
pub fn token_delta_edits(
    old: &[SemanticToken],
    new: &[SemanticToken],
) -> Vec<SemanticTokensEdit> {
    let prefix = old
        .iter()
        .zip(new.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let delete_count = old.len() - prefix - suffix;
    let inserted: Vec<SemanticToken> = new[prefix..new.len() - suffix].to_vec();
    if delete_count == 0 && inserted.is_empty() {
        return Vec::new();
    }
    vec![SemanticTokensEdit {
        start: (prefix * 5) as u32,
        delete_count: (delete_count * 5) as u32,
        data: (!inserted.is_empty()).then_some(inserted),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Apply `edits` (byte-column positions) to `text` — the reconstruction
    /// oracle: `apply(text, line_edits(text, new)) == new`.
    fn apply(text: &str, edits: &[TextEdit]) -> String {
        let mut out = text.to_string();
        for edit in edits.iter().rev() {
            let start = offset_of(text, edit.range.start);
            let end = offset_of(text, edit.range.end);
            out.replace_range(start..end, &edit.new_text);
        }
        out
    }

    fn offset_of(text: &str, pos: Position) -> usize {
        let mut offset = 0usize;
        for (i, line) in lines_with_newline(text).iter().enumerate() {
            if i as u32 == pos.line {
                return offset + pos.character as usize;
            }
            offset += line.len();
        }
        offset
    }

    fn assert_roundtrip(old: &str, new: &str) -> Vec<TextEdit> {
        let edits = line_edits(old, new);
        let expected = match_eol(old, new.to_string());
        assert_eq!(apply(old, &edits), expected, "old={old:?} new={new:?}");
        edits
    }

    #[test]
    fn equal_texts_produce_no_edits() {
        assert!(line_edits("a\nb\n", "a\nb\n").is_empty());
        assert!(line_edits("", "").is_empty());
    }

    #[test]
    fn single_line_change_touches_only_that_line() {
        let edits = assert_roundtrip("task a\ntask b\nthird\n", "task a\ntask b @done\nthird\n");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].range.start, Position::new(1, 0));
        assert_eq!(edits[0].range.end, Position::new(1, 6));
        assert_eq!(edits[0].new_text, "task b @done");
    }

    #[test]
    fn scattered_changes_become_separate_edits() {
        // Queue renumber shape: lines 0 and 2 change, line 1 does not.
        let edits = assert_roundtrip(
            "a @queue(2)\nb\nc @queue(5)\n",
            "a @queue(1)\nb\nc @queue(2)\n",
        );
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[0].range.start.line, 0);
        assert_eq!(edits[0].range.end.line, 0);
        assert_eq!(edits[1].range.start.line, 2);
        assert_eq!(edits[1].range.end.line, 2);
    }

    #[test]
    fn insertion_mid_document() {
        let edits = assert_roundtrip("A:\n  x\n", "A:\n  x\n  y\n");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].range.start, Position::new(2, 0));
        assert_eq!(edits[0].new_text, "  y\n");
    }

    #[test]
    fn insertion_at_end_with_trailing_newline() {
        let edits = assert_roundtrip("a\n", "a\nb\n");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].range.start, Position::new(1, 0));
        assert_eq!(edits[0].new_text, "b\n");
    }

    #[test]
    fn insertion_at_end_without_trailing_newline() {
        // The old last line gains a newline ("a" -> "a\n"), so the diff is a
        // replacement of that one line — still limited to changed lines.
        let edits = assert_roundtrip("a", "a\nb");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].range.start, Position::new(0, 0));
        assert_eq!(edits[0].range.end, Position::new(0, 1));
        assert_eq!(edits[0].new_text, "a\nb");
    }

    #[test]
    fn deletion_mid_document() {
        let edits = assert_roundtrip("a\nB\nc\n", "a\nc\n");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].range.start, Position::new(1, 0));
        assert_eq!(edits[0].range.end, Position::new(2, 0));
        assert_eq!(edits[0].new_text, "");
    }

    #[test]
    fn deletion_of_last_line_without_trailing_newline() {
        let edits = assert_roundtrip("a\nB", "a\n");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].range.end, Position::new(1, 1));
        assert_eq!(edits[0].new_text, "");
    }

    #[test]
    fn replacement_growing_line_count() {
        // Archive shape: one line becomes two.
        let edits = assert_roundtrip(
            "keep\nold @done\n",
            "keep\nArchive:\n    old @done\n",
        );
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].range.start, Position::new(1, 0));
        assert_eq!(edits[0].range.end, Position::new(1, 9));
        assert_eq!(edits[0].new_text, "Archive:\n    old @done");
    }

    #[test]
    fn replacement_shrinking_line_count() {
        let edits = assert_roundtrip("x\nA\nB\ny\n", "x\nN\ny\n");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].range.start, Position::new(1, 0));
        assert_eq!(edits[0].range.end, Position::new(2, 1));
        assert_eq!(edits[0].new_text, "N");
    }

    #[test]
    fn whole_document_rewrite_is_one_edit() {
        let edits = assert_roundtrip("a\nb\n", "x\ny\nz\n");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "x\ny\nz");
    }

    #[test]
    fn multibyte_content_uses_byte_columns() {
        let edits = assert_roundtrip("タスク\nb\n", "タスク @done\nb\n");
        assert_eq!(edits[0].range.end, Position::new(0, 9)); // 3 chars × 3 bytes
    }

    #[test]
    fn crlf_document_limits_edits_to_changed_lines() {
        // One changed line: the untouched CRLF line stays byte-identical,
        // the edit covers only line 0's content (CR is left outside the
        // range, so the document keeps its EOL).
        let edits = assert_roundtrip("task @done\r\nplain\r\n", "task\nplain\n");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].range.start, Position::new(0, 0));
        assert_eq!(edits[0].range.end, Position::new(0, 10)); // "task @done"
        assert_eq!(edits[0].new_text, "task");
    }

    #[test]
    fn crlf_multiline_new_text_uses_crlf() {
        // A replacement that grows the line count must join the new lines
        // with CRLF (archive shape), or the document would get mixed EOLs.
        let edits = assert_roundtrip(
            "keep\r\nold @done\r\n",
            "keep\nArchive:\n    old @done\n",
        );
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].range.start, Position::new(1, 0));
        assert_eq!(edits[0].range.end, Position::new(1, 9)); // "old @done"
        assert_eq!(edits[0].new_text, "Archive:\r\n    old @done");
    }

    // ----- token_delta_edits -----

    fn token(delta_line: u32, delta_start: u32, length: u32) -> SemanticToken {
        SemanticToken {
            delta_line,
            delta_start,
            length,
            token_type: 0,
            token_modifiers_bitset: 0,
        }
    }

    #[test]
    fn identical_tokens_have_no_edits() {
        let data = [token(0, 0, 5), token(1, 0, 3)];
        assert!(token_delta_edits(&data, &data).is_empty());
    }

    #[test]
    fn appended_token_is_a_single_insert() {
        let old = [token(0, 0, 5)];
        let new = [token(0, 0, 5), token(1, 0, 3)];
        let edits = token_delta_edits(&old, &new);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].start, 5);
        assert_eq!(edits[0].delete_count, 0);
        assert_eq!(edits[0].data.as_deref(), Some(&new[1..]));
    }

    #[test]
    fn middle_change_deletes_and_inserts() {
        let old = [token(0, 0, 5), token(1, 0, 3), token(1, 4, 2)];
        let new = [token(0, 0, 5), token(1, 0, 7), token(1, 4, 2)];
        let edits = token_delta_edits(&old, &new);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].start, 5);
        assert_eq!(edits[0].delete_count, 5);
        assert_eq!(edits[0].data.as_deref(), Some(&new[1..2]));
    }

    #[test]
    fn removed_trailing_tokens_delete_without_data() {
        let old = [token(0, 0, 5), token(1, 0, 3)];
        let new = [token(0, 0, 5)];
        let edits = token_delta_edits(&old, &new);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].delete_count, 5);
        assert_eq!(edits[0].data, None);
    }
}
