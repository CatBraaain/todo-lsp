//! §フォーマット — the document format applied by "Format Document"
//! (Shift+Alt+F) and after tag toggles / Repeat Tasks / Archive / Unarchive.

use crate::line::{self, LineParts};

/// Format a whole document per SPEC.md §フォーマット:
///
/// 1. each line's indent normalized to `(parent level + 1) × 4 spaces`;
/// 2. tokens within a line separated by single spaces, no stray whitespace;
/// 3. blank runs collapsed to one line, no leading blank, trailing newline;
/// 4. blank lines around top-level heading blocks.
pub fn format_document(text: &str) -> String {
    let lines = split_lines(text);
    if lines.iter().all(|l| line::parse_line(l).is_blank()) {
        return String::new();
    }

    // Rules 1-2: reindent and normalize each line. Parents are decided by
    // relative indent comparison (any deeper indent makes a child), so the
    // stack holds (indent units, new level).
    let mut stack: Vec<(usize, usize)> = Vec::new();
    let mut normalized: Vec<(String, usize, LineParts)> = Vec::new();
    for raw in lines {
        let parts = line::parse_line(raw);
        if parts.is_blank() {
            normalized.push((String::new(), 0, parts));
            continue;
        }
        while stack.last().is_some_and(|&(units, _)| units >= parts.units) {
            stack.pop();
        }
        let new_level = stack.last().map(|&(_, nl)| nl + 1).unwrap_or(0);
        stack.push((parts.units, new_level));
        let content = parts.normalize_body(raw);
        normalized.push((
            format!("{}{}", line::indent_for_level(new_level), content),
            new_level,
            parts,
        ));
    }

    // Rule 3: collapse blank runs, drop leading blanks, drop trailing blanks
    // (the trailing newline is added at the end).
    let mut collapsed: Vec<(String, usize, bool)> = Vec::new(); // (line, level, is_heading)
    for (text, level, parts) in normalized {
        if text.is_empty() {
            if collapsed.last().is_none_or(|t| t.0.is_empty()) {
                continue;
            }
            collapsed.push((text, level, false));
        } else {
            collapsed.push((text, level, parts.is_heading()));
        }
    }
    while collapsed.last().is_some_and(|t| t.0.is_empty()) {
        collapsed.pop();
    }

    // Rule 4: blank lines around top-level heading blocks. A block starts at
    // a level-0 heading and ends right before the next level-0 line.
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < collapsed.len() {
        let (text, level, is_heading) = &collapsed[i];
        if *level == 0 && *is_heading {
            if out.last().is_some_and(|l| !l.is_empty()) {
                out.push(String::new());
            }
            loop {
                out.push(collapsed[i].0.clone());
                let ends_block = collapsed.get(i + 1).is_none_or(|next| next.1 == 0);
                i += 1;
                if ends_block {
                    break;
                }
            }
            // A blank after the block only when content follows.
            if collapsed.get(i).is_some_and(|next| !next.0.is_empty()) {
                out.push(String::new());
            }
        } else {
            out.push(text.clone());
            i += 1;
        }
    }
    let mut result = out.join("\n");
    result.push('\n');
    result
}

/// Split into physical lines, dropping the empty tail element produced by a
/// trailing newline (it is re-added when joining).
pub(crate) fn split_lines(text: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(text: &str) -> String {
        format_document(text)
    }

    #[test]
    fn rule1_indent_normalized_to_parent_plus_one() {
        // A jumps two levels at once: children become level 1 (parent+1).
        let input = "A:\n        deep child\n";
        assert_eq!(fmt(input), "A:\n    deep child\n");
    }

    #[test]
    fn rule1_parent_chain() {
        let input = "A:\n  B:\n        deep\n  back\n";
        assert_eq!(fmt(input), "A:\n    B:\n        deep\n    back\n");
    }

    #[test]
    fn rule1_tab_input_written_as_spaces() {
        let input = "A:\n\ttask\n";
        assert_eq!(fmt(input), "A:\n    task\n");
    }

    #[test]
    fn rule1_dedent_creates_sibling() {
        // `top` dedents back to level 0; a following deeper line hangs off it.
        let input = "A:\n  a\ntop:\n      deep\n";
        assert_eq!(fmt(input), "A:\n    a\n\ntop:\n    deep\n");
    }

    #[test]
    fn rule2_single_spaces_and_trimmed_ends() {
        let input = "   buy   milk   @done(2024-01-01)   \n";
        assert_eq!(fmt(input), "buy milk @done(2024-01-01)\n");
    }

    #[test]
    fn rule2_heading_tokens() {
        let input = "  Foo  bar:  @a   @b\n";
        assert_eq!(fmt(input), "Foo bar: @a @b\n");
    }

    #[test]
    fn rule3_blank_runs_collapse_and_trailing_newline_added() {
        let input = "a\n\n\n\nb\n\n";
        assert_eq!(fmt(input), "a\n\nb\n");
    }

    #[test]
    fn rule3_leading_blanks_dropped() {
        assert_eq!(fmt("\n\na\n"), "a\n");
    }

    #[test]
    fn rule3_all_blank_document_stays_empty() {
        assert_eq!(fmt(""), "");
        assert_eq!(fmt("\n\n"), "");
    }

    #[test]
    fn rule4_blank_around_top_level_heading_blocks() {
        // The task between two heading blocks is separated on both sides:
        // a blank after block A and a blank before block B.
        let input = "A:\n  a\ntask\nB:\n  b\n";
        assert_eq!(fmt(input), "A:\n    a\n\ntask\n\nB:\n    b\n");
    }

    #[test]
    fn rule4_no_blank_before_document_start_or_after_end() {
        assert_eq!(fmt("A:\n  a\n"), "A:\n    a\n");
    }

    #[test]
    fn rule4_consecutive_heading_blocks_get_one_blank_between() {
        assert_eq!(fmt("A:\n  a\nB:\n  b\n"), "A:\n    a\n\nB:\n    b\n");
    }

    #[test]
    fn rule4_interior_blank_between_tasks_kept() {
        assert_eq!(fmt("a\n\nb\n"), "a\n\nb\n");
    }

    #[test]
    fn sample_is_formatted_with_blank_before_archive() {
        let sample = "\
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
        let expected = "\
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
        assert_eq!(fmt(sample), expected);
    }

    #[test]
    fn idempotent() {
        let once = fmt("A:\n\t x \n\n\nb @done\n");
        assert_eq!(fmt(&once), once);
    }
}
