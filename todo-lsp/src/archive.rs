//! §アーカイブ — Archive (Alt+A) moves selected all-gray top-level blocks
//! under the root-level `Archive:` heading; Unarchive (Alt+Shift+A) moves
//! all-gray blocks under `Archive:` back to the document end.

use crate::format::{format_document, split_lines};
use crate::line;

/// One structure line of the document. `units` is the relative indent
/// measurement; `level` is the SPEC.md インデントレベル (`units / 4`) that
/// decides block membership.
struct Entry {
    line_idx: usize,
    units: usize,
    level: usize,
}

/// Structure entries (non-blank lines) in document order.
fn entries(lines: &[String]) -> Vec<Entry> {
    lines
        .iter()
        .enumerate()
        .filter_map(|(i, l)| {
            let p = line::parse_line(l);
            p.is_structure().then_some(Entry {
                line_idx: i,
                units: p.units,
                level: p.level,
            })
        })
        .collect()
}

/// Group entries into top-level blocks: a level-0 entry (インデントレベル 0,
/// including 1-3 leading spaces) plus every deeper entry that follows it.
/// Lines before the first level-0 entry belong to no top-level block.
fn top_level_blocks(lines: &[String]) -> Vec<Vec<usize>> {
    let mut blocks: Vec<Vec<usize>> = Vec::new();
    for e in entries(lines) {
        if e.level == 0 {
            blocks.push(Vec::new());
        }
        if let Some(block) = blocks.last_mut() {
            block.push(e.line_idx);
        }
    }
    blocks
}

/// Whether every member line of a block is a 灰色行 (has `@done`,
/// `@cancelled` or `@hide`).
fn block_is_all_gray(lines: &[String], block: &[usize]) -> bool {
    !block.is_empty()
        && block
            .iter()
            .all(|&i| line::parse_line(&lines[i]).gray().is_some())
}

/// Archive the selected all-gray top-level blocks under the root-level
/// `Archive:` heading (created at the document end when absent).
pub fn archive(text: &str, selection: &[usize]) -> String {
    let lines: Vec<String> = split_lines(text).iter().map(|s| s.to_string()).collect();
    if lines.is_empty() {
        return text.to_string();
    }
    let sel: Vec<usize> = selection.to_vec();
    let blocks = top_level_blocks(&lines);

    // Eligible: all-gray blocks containing a selected line. The `Archive:`
    // heading itself is never gray, so its own block can never qualify.
    let moved: Vec<usize> = blocks
        .iter()
        .filter(|b| block_is_all_gray(&lines, b) && b.iter().any(|i| sel.contains(i)))
        .flatten()
        .copied()
        .collect();
    if moved.is_empty() {
        return text.to_string();
    }

    // Insertion point: the end of the root-level `Archive:` heading's block,
    // or the document end when the heading must be created.
    let archive_line = root_archive_line(&lines);
    let (insert_after, create_heading) = match archive_line {
        Some(h) => (block_end(&lines, h), false),
        None => (lines.len() - 1, true),
    };

    // Rebuild: drop the moved lines, shift them one level deeper under
    // Archive, and reinsert them (normalized) after the insertion point.
    let shifted: Vec<String> = moved
        .iter()
        .map(|&i| {
            let raw = &lines[i];
            let parts = line::parse_line(raw);
            let depth = relative_depth(&lines, i);
            format!(
                "{}{}",
                line::indent_for_level(depth + 1),
                parts.normalize_body(raw)
            )
        })
        .collect();
    let mut out: Vec<String> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if moved.contains(&i) {
            continue;
        }
        out.push(l.clone());
        if !create_heading && i == insert_after {
            out.extend(shifted.iter().cloned());
        }
    }
    if create_heading {
        out.push("Archive:".to_string());
        out.extend(shifted);
    }
    // §フォーマット applies after Archive execution (the moved lines and
    // the document as a whole are left in the canonical form).
    format_document(&join(&out, text))
}

/// Move all-gray blocks directly under the root-level `Archive:` heading
/// (containing a selected line) back to the document end, dedented to the
/// top level. Without an `Archive:` heading the document is unchanged.
pub fn unarchive(text: &str, selection: &[usize]) -> String {
    let lines: Vec<String> = split_lines(text).iter().map(|s| s.to_string()).collect();
    let sel: Vec<usize> = selection.to_vec();

    let archive_line = root_archive_line(&lines);
    let Some(archive_idx) = archive_line else {
        return text.to_string();
    };
    let end = block_end(&lines, archive_idx);

    // Direct-child sub-blocks of the root `Archive:` block: entries exactly
    // one level deeper (level 1), each with its own descendants. Deeper-only
    // content below the heading belongs to no direct-child block.
    let mut sub_blocks: Vec<Vec<usize>> = Vec::new();
    for e in entries(&lines) {
        if e.line_idx <= archive_idx || e.line_idx > end {
            continue;
        }
        if e.level == 1 {
            sub_blocks.push(Vec::new());
        }
        if let Some(block) = sub_blocks.last_mut() {
            block.push(e.line_idx);
        }
    }

    let moved: Vec<usize> = sub_blocks
        .iter()
        .filter(|b| block_is_all_gray(&lines, b) && b.iter().any(|i| sel.contains(i)))
        .flatten()
        .copied()
        .collect();
    if moved.is_empty() {
        return text.to_string();
    }

    // Dedent each moved line back to the top level (relative depth within
    // the moved sub-block kept) and append at the document end.
    let head_depth = relative_depth(&lines, moved[0]);
    let appended: Vec<String> = moved
        .iter()
        .map(|&i| {
            let raw = &lines[i];
            let parts = line::parse_line(raw);
            let depth = relative_depth(&lines, i) - head_depth;
            format!(
                "{}{}",
                line::indent_for_level(depth),
                parts.normalize_body(raw)
            )
        })
        .collect();
    let mut out: Vec<String> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if moved.contains(&i) {
            continue;
        }
        out.push(l.clone());
    }
    out.extend(appended);
    // §フォーマット applies after Unarchive execution.
    format_document(&join(&out, text))
}

fn root_archive_line(lines: &[String]) -> Option<usize> {
    lines.iter().position(|line_text| {
        let parts = line::parse_line(line_text);
        parts.units == 0 && parts.is_archive_heading(line_text)
    })
}

/// The last line index of the block headed by the line at `head_idx`:
/// the head plus following entries at a deeper インデントレベル.
fn block_end(lines: &[String], head_idx: usize) -> usize {
    let head_level = line::parse_line(&lines[head_idx]).level;
    let mut end = head_idx;
    for e in entries(lines) {
        if e.line_idx <= head_idx {
            continue;
        }
        if e.level > head_level {
            end = e.line_idx;
        } else {
            break;
        }
    }
    end
}

/// The 0-based depth of a line within its top-level block (0 for the block
/// head, 1 for its children, ...), derived from the level chain.
fn relative_depth(lines: &[String], idx: usize) -> usize {
    let level = line::parse_line(&lines[idx]).level;
    let mut depth = 0;
    let mut cur = level;
    for e in entries(lines).into_iter().rev() {
        if e.line_idx >= idx || e.level >= level {
            continue;
        }
        if e.level < cur {
            depth += 1;
            cur = e.level;
        }
    }
    depth
}

fn join(lines: &[String], original: &str) -> String {
    let mut out = lines.join("\n");
    if original.ends_with('\n') || !out.is_empty() {
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const DONE_DOC: &str = "\
Inbox:
    a @done
    b
Archive:
    old @done
";

    #[test]
    fn archive_moves_selected_gray_top_level_block() {
        // Only top-level blocks can be archived: `a @done` is level 0.
        let input = "Inbox:\n    b\na @done\n";
        let out = archive(input, &[2]);
        assert_eq!(out, "Inbox:\n    b\n\nArchive:\n    a @done\n");
    }

    #[test]
    fn archive_into_existing_archive_block_end() {
        let input = "Inbox:\n    b\nArchive:\n    old @done\nc @done\n";
        let out = archive(input, &[4]);
        assert_eq!(out, "Inbox:\n    b\n\nArchive:\n    old @done\n    c @done\n");
    }

    #[test]
    fn archive_requires_all_lines_gray() {
        // The Inbox block contains non-gray lines (`b`), so nothing moves
        // even though a selected line is gray.
        let out = archive(DONE_DOC, &[1, 2]);
        assert_eq!(out, DONE_DOC);
    }

    #[test]
    fn archive_requires_selection() {
        assert_eq!(archive(DONE_DOC, &[2]), DONE_DOC);
    }

    #[test]
    fn archive_creates_archive_heading_when_absent() {
        let out = archive("t\ndone task @cancelled\n", &[1]);
        assert_eq!(out, "t\n\nArchive:\n    done task @cancelled\n");
    }

    #[test]
    fn archive_moves_whole_block_with_descendants() {
        let input = "x @done\n    deep @hide\ny\n";
        let out = archive(input, &[0]);
        assert_eq!(out, "y\n\nArchive:\n    x @done\n        deep @hide\n");
    }

    #[test]
    fn archive_moves_multiple_blocks_in_document_order() {
        let input = "L:\n    keep\none @done\ntwo @cancelled\n";
        let out = archive(input, &[2, 3]);
        assert_eq!(
            out,
            "L:\n    keep\n\nArchive:\n    one @done\n    two @cancelled\n"
        );
    }

    #[test]
    fn archive_block_under_archive_is_not_top_level() {
        // A gray block already inside Archive cannot be re-archived.
        let out = archive(DONE_DOC, &[4]);
        assert_eq!(out, DONE_DOC);
    }

    #[test]
    fn archive_ignores_indented_archive_heading_as_destination() {
        let input = "    Archive:\n        old @done\ndone @done\n";
        let out = archive(input, &[2]);
        // The archive decision ignored the indented `Archive:` (a new root
        // heading was created); §フォーマット then normalizes the indented
        // heading to the top level.
        assert_eq!(
            out,
            "Archive:\n    old @done\n\nArchive:\n    done @done\n"
        );
    }

    #[test]
    fn unarchive_requires_root_level_archive_heading() {
        let input = "    Archive:\n        old @done\n";
        assert_eq!(unarchive(input, &[1]), input);
    }

    #[test]
    fn unarchive_moves_block_to_document_end() {
        let input = "\
Inbox:
    a
Archive:
    old @done
        note @hide
    other @done
";
        let out = unarchive(input, &[3]);
        assert_eq!(
            out,
            "Inbox:\n    a\n\nArchive:\n    other @done\n\nold @done\n    note @hide\n"
        );
    }

    #[test]
    fn unarchive_without_archive_heading_is_noop() {
        let input = "a @done\n";
        assert_eq!(unarchive(input, &[0]), input);
    }

    #[test]
    fn unarchive_requires_selection() {
        let input = "Archive:\n    old @done\n";
        assert_eq!(unarchive(input, &[]), input);
    }

    #[test]
    fn unarchive_requires_gray_block() {
        let input = "Archive:\n    active\n";
        assert_eq!(unarchive(input, &[1]), input);
    }

    #[test]
    fn partial_indent_line_is_its_own_top_level_block() {
        // 1-3 leading spaces are still インデントレベル 0, so `   b @done` is
        // a separate top-level block from `a @done` and archives alone.
        let out = archive("a @done\n   b @done\n", &[1]);
        assert_eq!(out, "a @done\n\nArchive:\n    b @done\n");
    }

    #[test]
    fn leading_indented_line_is_not_a_top_level_block() {
        // A document starting with an indented line has no top-level block
        // there, so the selection cannot be archived.
        let input = "    a @done\nplain\n";
        assert_eq!(archive(input, &[0]), input);
    }

    #[test]
    fn blank_line_separates_top_level_blocks() {
        let out = archive("a @done\n\nb @done\n", &[2]);
        assert_eq!(out, "a @done\n\nArchive:\n    b @done\n");
    }

    #[test]
    fn archive_formats_the_whole_document() {
        // §フォーマット applies after Archive: the untouched ` t` line is
        // normalized too.
        let out = archive("  x @done\n t\n", &[0]);
        assert_eq!(out, "t\n\nArchive:\n    x @done\n");
    }

    #[test]
    fn unarchive_ignores_deeper_only_content_under_archive() {
        // Only direct children (level 1) of the root `Archive:` heading are
        // unarchive targets; a deeper line with no level-1 block does not move.
        let input = "Archive:\n        deep @done\n";
        assert_eq!(unarchive(input, &[1]), input);
    }
}
