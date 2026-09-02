//! §リピート — Repeat Tasks: materialize `@repeat(cron)` definitions into
//! concrete task lines dated with the cron's most recent past occurrence.

use std::str::FromStr;

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Timelike, Utc};
use croner::Cron;

use crate::format::{format_document, split_lines};
use crate::line;

/// One structure line with its indent units and parent link (index into the
/// node list). `parent: None` means the line hangs off the document root.
struct Node {
    line_idx: usize,
    units: usize,
    parent: Option<usize>,
}

/// Build the structure nodes (document order). A line's parent is the
/// nearest preceding structure line with a smaller indent.
fn build_nodes(lines: &[String]) -> Vec<Node> {
    let mut nodes: Vec<Node> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let p = line::parse_line(l);
        if !p.is_structure() {
            continue;
        }
        let parent = nodes.iter().rposition(|n| n.units < p.units);
        nodes.push(Node {
            line_idx: i,
            units: p.units,
            parent,
        });
    }
    nodes
}

/// Run Repeat Tasks on the whole document and return the formatted result
/// (手順 1-7).
pub fn repeat_tasks(text: &str, now: DateTime<Utc>) -> String {
    let mut lines: Vec<String> = split_lines(text).iter().map(|s| s.to_string()).collect();
    if lines.is_empty() {
        return text.to_string();
    }

    // 手順 1: definitions are the valid-cron @repeat lines, in document
    // order. They are carried by content because insertions shift indices.
    let definitions: Vec<String> = lines
        .iter()
        .filter(|l| {
            line::parse_line(l)
                .tag_arg("repeat")
                .is_some_and(|arg| Cron::from_str(arg.trim()).is_ok())
        })
        .cloned()
        .collect();

    for def in &definitions {
        process_definition(&mut lines, def, now);
    }

    let mut joined = lines.join("\n");
    joined.push('\n');
    format_document(&joined)
}

fn process_definition(lines: &mut Vec<String>, def: &str, now: DateTime<Utc>) {
    let parts = line::parse_line(def);
    let Some(cron_arg) = parts.tag_arg("repeat") else {
        return;
    };
    let Ok(cron) = Cron::from_str(cron_arg.trim()) else {
        return;
    };
    // 手順 2: cron式の現在より前の直近の日時.
    let Ok(prev) = cron.find_previous_occurrence(&now, false) else {
        return;
    };

    // 手順 3: a definition whose @start is later than prev has nothing to
    // generate yet.
    if let Some(start) = parts.tag_arg("start").and_then(parse_date) {
        if start > prev {
            return;
        }
    }

    // 手順 4: resolve the placement path `親見出し/…/タスク名`.
    let task_text = parts.task_text(def);
    let mut path: Vec<&str> = task_text.split('/').collect();
    let name = path.pop().unwrap_or_default().trim().to_string();
    if name.is_empty() {
        return;
    }

    let mut container: Option<usize> = None;
    for parent_name in path.iter().map(|p| p.trim()).filter(|p| !p.is_empty()) {
        match find_child(lines, container, parent_name) {
            Some(child) => container = Some(child),
            None => {
                // ないときは親名の行をその階層（配置先ブロックの末尾）に作る.
                let insert_at = block_end(lines, container) + 1;
                let rendered = format!("{}{}", indent_for_container(lines, container), parent_name);
                lines.insert(insert_at, rendered);
                container = build_nodes(lines)
                    .iter()
                    .position(|n| n.line_idx == insert_at);
            }
        }
    }

    // 手順 6: skip when the destination already has the same task name with
    // the same @start.
    let already_there = children(lines, container).any(|idx| {
        let text = &lines[idx];
        let p = line::parse_line(text);
        p.task_text(text) == name && p.tag_arg("start").and_then(parse_date) == Some(prev)
    });
    if already_there {
        return;
    }

    // 手順 5: append `タスク名 @start(prev)` at the container's block end.
    let insert_at = block_end(lines, container) + 1;
    lines.insert(
        insert_at,
        format!(
            "{}{} @start({})",
            indent_for_container(lines, container),
            name,
            render_prev(prev)
        ),
    );
}

/// The canonical indent for a new child of the container: one level (4
/// spaces) deeper than the container's own indent; root children get none.
fn indent_for_container(lines: &[String], container: Option<usize>) -> String {
    match container {
        None => String::new(),
        Some(c) => " ".repeat(build_nodes(lines)[c].units + 4),
    }
}

/// Direct children of a container, as line indices; root children when the
/// container is `None`.
fn children(lines: &[String], container: Option<usize>) -> impl Iterator<Item = usize> + '_ {
    build_nodes(lines)
        .into_iter()
        .filter(move |n| n.parent == container)
        .map(|n| n.line_idx)
}

/// Find a container child whose タスクテキスト contains `name` (手順 4).
fn find_child(lines: &[String], container: Option<usize>, name: &str) -> Option<usize> {
    children(lines, container).find(|&idx| {
        let text = &lines[idx];
        line::parse_line(text).task_text(text).contains(name)
    })
}

/// The last line index of a container's block (its subtree); for the root,
/// the last structure line of the document.
fn block_end(lines: &[String], container: Option<usize>) -> usize {
    let nodes = build_nodes(lines);
    match container {
        None => nodes.last().map(|n| n.line_idx).unwrap_or(0),
        Some(c) => {
            let units = nodes[c].units;
            let start = nodes[c].line_idx;
            let mut end = start;
            for n in &nodes {
                if n.line_idx <= start {
                    continue;
                }
                if n.units > units {
                    end = n.line_idx;
                } else {
                    break;
                }
            }
            end
        }
    }
}

/// Parse `YYYY-MM-DD` / `YYYY-MM-DD HH:mm` (UTC) into a `DateTime<Utc>`.
pub(crate) fn parse_date(arg: &str) -> Option<DateTime<Utc>> {
    let arg = arg.trim();
    NaiveDateTime::parse_from_str(arg, "%Y-%m-%d %H:%M")
        .or_else(|_| {
            NaiveDate::parse_from_str(arg, "%Y-%m-%d")
                .map(|d| d.and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap()))
        })
        .ok()
        .map(|dt| dt.and_utc())
}

/// Render prev per SPEC: the date alone when the time is 00:00, otherwise
/// `YYYY-MM-DD HH:mm`.
fn render_prev(prev: DateTime<Utc>) -> String {
    let time = prev.time();
    if time.hour() == 0 && time.minute() == 0 {
        prev.format("%Y-%m-%d").to_string()
    } else {
        prev.format("%Y-%m-%d %H:%M").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        NaiveDate::from_ymd_opt(2024, 6, 15)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_utc()
    }

    #[test]
    fn daily_repeat_appends_task_at_root_end() {
        // prev of `0 0 * * *` before 2024-06-15 12:00 is 2024-06-15 00:00,
        // rendered as a plain date (00:00).
        let out = repeat_tasks("sweep @repeat(0 0 * * *)\n", now());
        assert_eq!(out, "sweep @repeat(0 0 * * *)\nsweep @start(2024-06-15)\n");
    }

    #[test]
    fn non_midnight_prev_keeps_time() {
        // prev of `0 9 * * *` is 2024-06-15 09:00.
        let out = repeat_tasks("check @repeat(0 9 * * *)\n", now());
        assert_eq!(
            out,
            "check @repeat(0 9 * * *)\ncheck @start(2024-06-15 09:00)\n"
        );
    }

    #[test]
    fn path_places_task_under_existing_parent() {
        let input = "Home:\n    stuff\nHome/mop floor @repeat(0 0 * * *)\n";
        let out = repeat_tasks(input, now());
        assert_eq!(
            out,
            "Home:\n    stuff\n    mop floor @start(2024-06-15)\n\nHome/mop floor @repeat(0 0 * * *)\n"
        );
    }

    #[test]
    fn parent_matching_the_definition_line_itself() {
        // The definition line is itself a root child whose タスクテキスト
        // contains the parent name, so it becomes the container.
        let out = repeat_tasks("Chores/mop floor @repeat(0 0 * * *)\n", now());
        assert_eq!(
            out,
            "Chores/mop floor @repeat(0 0 * * *)\n    mop floor @start(2024-06-15)\n"
        );
    }

    #[test]
    fn missing_parent_is_created_under_its_container() {
        // The def line matches `Home`, then `Kitchen` is created under it.
        let out = repeat_tasks("Home/Kitchen/mop floor @repeat(0 0 * * *)\n", now());
        assert_eq!(
            out,
            "Home/Kitchen/mop floor @repeat(0 0 * * *)\n    Kitchen\n        mop floor @start(2024-06-15)\n"
        );
    }

    #[test]
    fn idempotent_when_same_name_and_start_exist() {
        let input = "Inbox:\n    buy milk @start(2024-06-15)\nInbox/buy milk @repeat(0 0 * * *)\n";
        let out = repeat_tasks(input, now());
        assert_eq!(
            out,
            "Inbox:\n    buy milk @start(2024-06-15)\n\nInbox/buy milk @repeat(0 0 * * *)\n"
        );
    }

    #[test]
    fn future_start_skips_definition() {
        // prev is 2024-06-15 00:00; @start(2024-06-16) is later -> skip.
        let input = "task @repeat(0 0 * * *) @start(2024-06-16)\n";
        assert_eq!(repeat_tasks(input, now()), input);
    }

    #[test]
    fn invalid_cron_is_ignored() {
        let input = "task @repeat(nonsense)\n";
        assert_eq!(repeat_tasks(input, now()), input);
    }

    #[test]
    fn multiple_definitions_process_in_document_order() {
        let input = "a @repeat(0 0 * * *)\nb @repeat(0 12 * * *)\n";
        // b's prev is 2024-06-14 12:00 (before now 06-15 12:00? no — 0 12 * *
        // * fires at 12:00; strictly before 12:00 the latest is 06-14 12:00).
        let out = repeat_tasks(input, now());
        assert_eq!(
            out,
            "a @repeat(0 0 * * *)\nb @repeat(0 12 * * *)\na @start(2024-06-15)\nb @start(2024-06-14 12:00)\n"
        );
    }

    #[test]
    fn result_is_fully_formatted() {
        let input = "  A:\n      deep\nflat @repeat(0 0 * * *)\n";
        let out = repeat_tasks(input, now());
        assert_eq!(
            out,
            "A:\n    deep\n\nflat @repeat(0 0 * * *)\nflat @start(2024-06-15)\n"
        );
    }
}
