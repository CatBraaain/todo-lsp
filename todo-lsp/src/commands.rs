//! §コマンド — tag toggles, queue numbering, and indent / dedent.
//!
//! All operations are pure `text × selection -> text` functions; the LSP
//! layer applies the result as line-limited workspace edits. Selections are
//! 0-based line numbers (the union of the editor's selection ranges).

use chrono::NaiveDate;

use crate::format::split_lines;
use crate::line::{self, Tag};

/// The ten tag-toggle commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toggle {
    Done,
    Cancelled,
    Start,
    Due,
    Queue,
    QueueUnshift,
    Waiting,
    Pending,
    Hide,
    Repeat,
}

impl Toggle {
    fn name(&self) -> &'static str {
        match self {
            Toggle::Done => "done",
            Toggle::Cancelled => "cancelled",
            Toggle::Start => "start",
            Toggle::Due => "due",
            Toggle::Queue | Toggle::QueueUnshift => "queue",
            Toggle::Waiting => "waiting",
            Toggle::Pending => "pending",
            Toggle::Hide => "hide",
            Toggle::Repeat => "repeat",
        }
    }

    /// The tag text added by this toggle. `queue_number` is only used for
    /// [`Toggle::Queue`]; `today` (実行日) for the dated toggles.
    fn tag_text(&self, today: NaiveDate, queue_number: usize) -> String {
        match self {
            Toggle::Done | Toggle::Cancelled | Toggle::Start | Toggle::Due => {
                format!("@{}({})", self.name(), today.format("%Y-%m-%d"))
            }
            Toggle::Queue => format!("@queue({queue_number})"),
            Toggle::QueueUnshift => "@queue(0)".to_string(),
            Toggle::Waiting | Toggle::Pending | Toggle::Hide => {
                format!("@{}", self.name())
            }
            Toggle::Repeat => "@repeat(0 0 * * *)".to_string(),
        }
    }

    /// 共通規則 4: tags removed from the target lines when @done / @cancelled
    /// is added.
    fn removes_on_add(&self) -> &'static [&'static str] {
        match self {
            Toggle::Done | Toggle::Cancelled => {
                &["done", "cancelled", "queue", "waiting", "pending"]
            }
            _ => &[],
        }
    }
}

/// Apply a tag toggle to the selected lines (タグトグルの共通規則).
pub fn toggle(text: &str, selection: &[usize], action: Toggle, today: NaiveDate) -> String {
    let mut lines: Vec<String> = split_lines(text).iter().map(|s| s.to_string()).collect();
    let targets = structure_lines(&lines, selection);
    if targets.is_empty() {
        return text.to_string();
    }

    let name = action.name();
    let all_have = targets
        .iter()
        .all(|&i| line::parse_line(&lines[i]).has_tag(name));
    let queue_number = next_queue_number(&lines);

    if all_have {
        // 共通規則 3: every selected line has the tag -> remove it from all.
        for &i in &targets {
            lines[i] = retag(&lines[i], |tags| tags.retain(|t| t.name != name));
        }
    } else {
        // 共通規則 2: add the tag to the lines that lack it (the others stay).
        for &i in &targets {
            let parts = line::parse_line(&lines[i]);
            if parts.has_tag(name) {
                continue;
            }
            let removes = action.removes_on_add();
            let tag = Tag::from_text(&action.tag_text(today, queue_number));
            lines[i] = retag(&lines[i], |tags| {
                tags.retain(|t| !removes.contains(&t.name.as_str()));
                tags.push(tag.clone());
            });
        }
    }

    // 再採番 happens on every @queue toggle execution.
    if matches!(action, Toggle::Queue | Toggle::QueueUnshift) {
        renumber_queues(&mut lines);
    }
    join_lines(&lines, text)
}

/// Indent Lines (+1) / Dedent Lines (−1): the selected structure lines move
/// by one level, clamped at 0, rewritten as 4 spaces per level.
pub fn reindent(text: &str, selection: &[usize], delta: i64) -> String {
    let mut lines: Vec<String> = split_lines(text).iter().map(|s| s.to_string()).collect();
    for i in structure_lines(&lines, selection) {
        let raw = lines[i].clone();
        let parts = line::parse_line(&raw);
        let new_level = (parts.level as i64 + delta).max(0) as usize;
        lines[i] = format!(
            "{}{}",
            line::indent_for_level(new_level),
            &raw[parts.indent_len..]
        );
    }
    join_lines(&lines, text)
}

/// キュー番号: the number for a new `@queue(n)` — the count of non-gray
/// `@queue` lines in the document, plus 1.
fn next_queue_number(lines: &[String]) -> usize {
    lines
        .iter()
        .filter(|l| {
            let p = line::parse_line(l);
            p.has_tag("queue") && p.gray().is_none()
        })
        .count()
        + 1
}

/// 再採番: rewrite every `@queue(n)` number to its 1-based rank among the
/// distinct numbers in ascending order; equal numbers share a rank.
fn renumber_queues(lines: &mut [String]) {
    let mut numbers: Vec<u64> = Vec::new();
    for l in lines.iter() {
        for tag in &line::parse_line(l).tags {
            if tag.name == "queue" {
                if let Some(n) = tag.arg.as_deref().and_then(|a| a.parse().ok()) {
                    numbers.push(n);
                }
            }
        }
    }
    numbers.sort_unstable();
    numbers.dedup();
    for i in 0..lines.len() {
        let raw = lines[i].clone();
        let parts = line::parse_line(&raw);
        if !parts.tags.iter().any(|t| t.name == "queue") {
            continue;
        }
        let mut tags = parts.tags.clone();
        for tag in &mut tags {
            if tag.name == "queue" {
                if let Some(n) = tag.arg.as_deref().and_then(|a| a.parse::<u64>().ok()) {
                    let rank = numbers.iter().position(|&m| m == n).unwrap() + 1;
                    tag.arg = Some(rank.to_string());
                }
            }
        }
        lines[i] = line::indent_for_level(parts.level) + &line::render(&parts, &raw, &tags);
    }
}

/// Selection lines that are structure lines (non-blank), sorted and unique.
/// Blank lines are not part of the document structure and never take tags.
fn structure_lines(lines: &[String], selection: &[usize]) -> Vec<usize> {
    let mut out: Vec<usize> = selection
        .iter()
        .copied()
        .filter(|&i| i < lines.len() && !line::parse_line(&lines[i]).is_blank())
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Rebuild a line with an edited tag column, normalized per §フォーマット
/// rules 1-2 (indent from the line's current level, single-spaced tokens).
fn retag(raw: &str, edit_tags: impl FnOnce(&mut Vec<Tag>)) -> String {
    let parts = line::parse_line(raw);
    let mut tags = parts.tags.clone();
    edit_tags(&mut tags);
    format!(
        "{}{}",
        line::indent_for_level(parts.level),
        line::render(&parts, raw, &tags)
    )
}

fn join_lines(lines: &[String], original: &str) -> String {
    let mut out = lines.join("\n");
    if original.ends_with('\n') {
        out.push('\n');
    }
    out
}

impl Tag {
    /// Parse a rendered tag back into a [`Tag`] (positions are meaningless;
    /// the value is only re-rendered).
    fn from_text(text: &str) -> Tag {
        let (name, arg) = match (text.find('('), text.ends_with(')')) {
            (Some(open), true) => (
                text[1..open].to_string(),
                Some(text[open + 1..text.len() - 1].to_string()),
            ),
            _ => (text[1..].to_string(), None),
        };
        Tag {
            name,
            arg,
            start: 0,
            end: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2024, 6, 15).unwrap()
    }

    // ----- 共通規則 2 / 3 -----

    #[test]
    fn toggle_adds_tag_to_lacking_lines() {
        let out = toggle("task a\ntask b @done\n", &[0], Toggle::Done, today());
        assert_eq!(out, "task a @done(2024-06-15)\ntask b @done\n");
    }

    #[test]
    fn toggle_removes_when_all_selected_have_it() {
        let out = toggle(
            "a @done(2024-01-01)\nb @done\n",
            &[0, 1],
            Toggle::Done,
            today(),
        );
        assert_eq!(out, "a\nb\n");
    }

    #[test]
    fn toggle_mixed_selection_only_adds() {
        // One line has the tag, one lacks it: only the lacking one changes.
        let out = toggle("a @waiting\nb\n", &[0, 1], Toggle::Waiting, today());
        assert_eq!(out, "a @waiting\nb @waiting\n");
    }

    #[test]
    fn toggle_appends_after_existing_tags() {
        let out = toggle("task @priority(high)\n", &[0], Toggle::Pending, today());
        assert_eq!(out, "task @priority(high) @pending\n");
    }

    // ----- 共通規則 4 -----

    #[test]
    fn done_removes_conflicting_tags_before_add() {
        let input = "task @cancelled(2024-01-01) @queue(1) @waiting @pending @due(2024-07-01)\n";
        let out = toggle(input, &[0], Toggle::Done, today());
        assert_eq!(out, "task @due(2024-07-01) @done(2024-06-15)\n");
    }

    #[test]
    fn cancelled_removes_conflicting_tags_before_add() {
        let input = "task @done @queue(2)\n";
        let out = toggle(input, &[0], Toggle::Cancelled, today());
        assert_eq!(out, "task @cancelled(2024-06-15)\n");
    }

    #[test]
    fn hide_keeps_other_state_tags() {
        let input = "task @queue(1)\n";
        let out = toggle(input, &[0], Toggle::Hide, today());
        assert_eq!(out, "task @queue(1) @hide\n");
    }

    // ----- 共通規則 5 (formatting of target lines) -----

    #[test]
    fn toggled_lines_are_normalized() {
        let input = "    buy   milk\n";
        let out = toggle(input, &[0], Toggle::Done, today());
        assert_eq!(out, "    buy milk @done(2024-06-15)\n");
    }

    #[test]
    fn toggle_on_heading_appends_to_tag_column() {
        let input = "List: @collapsed\n";
        let out = toggle(input, &[0], Toggle::Done, today());
        assert_eq!(out, "List: @collapsed @done(2024-06-15)\n");
    }

    #[test]
    fn toggle_on_crlf_document_behaves_like_lf() {
        // Regression: the trailing CR used to hide the tag column, so
        // toggling @done on `task @done\r` duplicated the tag instead of
        // removing it. CRLF lines toggle exactly like LF ones.
        let out = toggle("task @done\r\nplain\r\n", &[0], Toggle::Done, today());
        assert_eq!(out, "task\nplain\n");
        let out = toggle("plain\r\n", &[0], Toggle::Done, today());
        assert_eq!(out, "plain @done(2024-06-15)\n");
    }

    #[test]
    fn toggle_skips_blank_lines() {
        let out = toggle("a\n\nb\n", &[0, 1, 2], Toggle::Done, today());
        assert_eq!(out, "a @done(2024-06-15)\n\nb @done(2024-06-15)\n");
    }

    #[test]
    fn toggle_without_selection_or_on_unknown_lines_is_noop() {
        let text = "a\n";
        assert_eq!(toggle(text, &[], Toggle::Done, today()), text);
        assert_eq!(toggle(text, &[9], Toggle::Done, today()), text);
    }

    // ----- dated / value toggles -----

    #[test]
    fn start_due_use_run_date() {
        let out = toggle("task\n", &[0], Toggle::Start, today());
        assert_eq!(out, "task @start(2024-06-15)\n");
        let out = toggle("task\n", &[0], Toggle::Due, today());
        assert_eq!(out, "task @due(2024-06-15)\n");
    }

    #[test]
    fn repeat_toggles_default_cron() {
        let out = toggle("task\n", &[0], Toggle::Repeat, today());
        assert_eq!(out, "task @repeat(0 0 * * *)\n");
        let out = toggle("task @repeat(0 0 * * *)\n", &[0], Toggle::Repeat, today());
        assert_eq!(out, "task\n");
    }

    // ----- キュー番号 -----

    #[test]
    fn queue_number_counts_non_gray_queue_lines_plus_one() {
        let input = "a @queue(1)\nb @queue(2) @done\nc\n";
        // `b` is gray, so only `a` counts -> the new queue is 2.
        let out = toggle(input, &[2], Toggle::Queue, today());
        assert_eq!(out, "a @queue(1)\nb @queue(2) @done\nc @queue(2)\n");
    }

    #[test]
    fn queue_renumber_after_toggle() {
        // Adding a queue renumbers everything by ascending rank.
        let input = "a @queue(2)\nb\n";
        let out = toggle(input, &[1], Toggle::Queue, today());
        // New queue number = 1 non-gray queue line + 1 = 2; after renumber
        // the sorted numbers {2, 2} share rank... distinct {2} -> both 1? No:
        // `a` has 2, the new line gets 2 as well -> distinct {2} -> rank 1.
        assert_eq!(out, "a @queue(1)\nb @queue(1)\n");
    }

    #[test]
    fn queue_renumber_spec_example() {
        let input = "a @queue(0)\nb @queue(2)\nc @queue(5)\nd @queue(5)\n";
        let out = toggle(input, &[], Toggle::Queue, today());
        // Empty selection is a no-op (no renumber side effect).
        assert_eq!(out, input);
        // Toggling a queue OFF triggers the renumber.
        let out = toggle(input, &[0], Toggle::Queue, today());
        assert_eq!(out, "a\nb @queue(1)\nc @queue(2)\nd @queue(2)\n");
    }

    #[test]
    fn queue_unshift_adds_zero_then_renumbers_to_one() {
        let input = "first\na @queue(1)\nnew\n";
        // Unshift adds @queue(0); renumber ranks {0, 1} -> 1, 2, so the new
        // line lands at the head.
        let out = toggle(input, &[2], Toggle::QueueUnshift, today());
        assert_eq!(out, "first\na @queue(2)\nnew @queue(1)\n");
    }

    // ----- インデント -----

    #[test]
    fn indent_increases_one_level_as_four_spaces() {
        let out = reindent("a\n  b\n", &[0], 1);
        assert_eq!(out, "    a\n  b\n");
    }

    #[test]
    fn indent_tab_input_becomes_spaces() {
        let out = reindent("a\n\tb\n", &[1], 1);
        assert_eq!(out, "a\n        b\n");
    }

    #[test]
    fn dedent_clamps_at_zero() {
        let out = reindent("a\n  b\n", &[0, 1], -1);
        assert_eq!(out, "a\nb\n");
        let out = reindent("a\n", &[0], -5);
        assert_eq!(out, "a\n");
    }

    #[test]
    fn indent_skips_blank_lines() {
        let out = reindent("a\n\n", &[0, 1], 1);
        assert_eq!(out, "    a\n\n");
    }
}
