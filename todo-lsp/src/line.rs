//! Line-level lexical model of the Todo language, shared by highlighting,
//! commands, formatting, archiving and repeat generation.
//!
//! This module mirrors SPEC.md's 用語 definitions verbatim:
//! 見出し行 / タスク行 / タグ列 / 灰色行 / インデントレベル. All consumers
//! classify a physical line through [`parse_line`]; nothing here talks to the
//! syntax tree.

/// A `@name` / `@name(arg)` token inside a line's trailing tag column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub name: String,
    pub arg: Option<String>,
    /// Byte offset of the leading `@` within the line.
    pub start: usize,
    /// Byte offset one past the token's last byte.
    pub end: usize,
}

impl Tag {
    /// The token as it appears in a line: `@name` or `@name(arg)`.
    pub fn text(&self) -> String {
        match &self.arg {
            Some(arg) => format!("@{}({})", self.name, arg),
            None => format!("@{}", self.name),
        }
    }
}

/// How a physical line classifies per SPEC.md's line grammar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Blank or whitespace-only line: not part of the document structure.
    Blank,
    /// 見出し行: non-empty body, then `:`, then an optional tag column.
    /// The value is the byte index of the `:`.
    Heading { colon: usize },
    /// タスク行: body text, then an optional tag column.
    Task,
    /// Non-blank line whose tag column exists but whose body is empty
    /// (`@done` alone). This is a syntax error per the grammar.
    TagOnly,
}

/// The whole-line gray rule that applies to a line, in SPEC.md 適用規則 order
/// (done before cancelled before hide; `Archive:` headings are handled
/// separately via [`LineParts::is_archive_heading`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gray {
    Done,
    Cancelled,
    Hide,
}

/// The parsed structure of one physical line (without its newline).
#[derive(Debug, Clone)]
pub struct LineParts {
    pub kind: Kind,
    /// Byte length of the leading whitespace (spaces and tabs).
    pub indent_len: usize,
    /// Indent measurement (4 spaces per level; tab = to the next multiple
    /// of 4). Structure is decided by comparing these relatively.
    pub units: usize,
    /// The level per SPEC.md's formula (`units / 4`), used when writing
    /// indents (indent / dedent commands).
    pub level: usize,
    /// Byte range of the trimmed body text — before the `:` for headings,
    /// before the tag column for tasks. Empty range for blank / tag-only
    /// lines.
    pub text_range: (usize, usize),
    /// The trailing tag column, in line order.
    pub tags: Vec<Tag>,
}

impl LineParts {
    pub fn is_blank(&self) -> bool {
        matches!(self.kind, Kind::Blank)
    }

    pub fn is_heading(&self) -> bool {
        matches!(self.kind, Kind::Heading { .. })
    }

    /// Byte index of the heading `:`, if this line is a 見出し行.
    pub fn colon(&self) -> Option<usize> {
        match self.kind {
            Kind::Heading { colon } => Some(colon),
            _ => None,
        }
    }

    pub fn is_structure(&self) -> bool {
        !matches!(self.kind, Kind::Blank)
    }

    /// The trimmed body text (heading text excludes the `:`).
    pub fn text<'a>(&self, line: &'a str) -> &'a str {
        &line[self.text_range.0..self.text_range.1]
    }

    /// タスクテキスト per SPEC.md: the line minus tags and whitespace, with
    /// runs of whitespace collapsed to single spaces. Heading lines keep
    /// their trailing `:`.
    pub fn task_text(&self, line: &str) -> String {
        let end = match self.kind {
            Kind::Heading { colon } => colon + 1,
            _ => self.text_range.1,
        };
        let raw = &line[self.text_range.0..end];
        raw.split_ascii_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// Whether the tag column contains `@name`.
    pub fn has_tag(&self, name: &str) -> bool {
        self.tags.iter().any(|t| t.name == name)
    }

    /// The tag column's first `@name` argument, if any.
    pub fn tag_arg(&self, name: &str) -> Option<&str> {
        self.tags
            .iter()
            .find(|t| t.name == name)
            .and_then(|t| t.arg.as_deref())
    }

    /// The 灰色行 rule for this line: `@done` / `@cancelled` / `@hide` in the
    /// tag column, first match in 適用規則 order.
    pub fn gray(&self) -> Option<Gray> {
        if self.has_tag("done") {
            Some(Gray::Done)
        } else if self.has_tag("cancelled") {
            Some(Gray::Cancelled)
        } else if self.has_tag("hide") {
            Some(Gray::Hide)
        } else {
            None
        }
    }

    /// Whether this is an `Archive:` 見出し行 (the display §行単位の色付け and
    /// §アーカイブ target). Indented archives qualify; the heading text must
    /// be exactly `Archive`.
    pub fn is_archive_heading(&self, line: &str) -> bool {
        self.is_heading() && self.text(line) == "Archive"
    }

    /// Rule 2 of §フォーマット for this line alone: every token (text, `:`,
    /// tags) separated by single spaces, no leading/trailing whitespace.
    /// Returns an empty string for blank lines. The caller prepends the
    /// normalized indent.
    pub fn normalize_body(&self, line: &str) -> String {
        render(self, line, &self.tags)
    }
}

/// Render a line's content from its parsed parts and an (edited) tag column:
/// whitespace-normalized text, `:` for headings, tags joined with single
/// spaces. Blank lines render as empty strings.
pub fn render(parts: &LineParts, line: &str, tags: &[Tag]) -> String {
    let text = parts
        .text(line)
        .split_ascii_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let mut out = text;
    if parts.is_heading() {
        out.push(':');
    }
    for tag in tags {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&tag.text());
    }
    out
}

/// SPEC.md インデントレベルの測定単位: 4 spaces per level; a tab advances
/// to the next multiple of 4 (so up to 3 preceding spaces merge with it).
/// Structure (parent / child / sibling) is decided by comparing these units
/// relatively — any deeper indent makes a child.
pub fn indent_units(indent: &str) -> usize {
    let mut units = 0usize;
    for b in indent.bytes() {
        match b {
            b'\t' => units = (units / 4 + 1) * 4,
            _ => units += 1,
        }
    }
    units
}

/// The level per SPEC.md's formula: `units / 4` (the canonical level used
/// when *writing* indents — 4 spaces per level).
pub fn indent_level(indent: &str) -> usize {
    indent_units(indent) / 4
}

/// The canonical indentation for a level: 4 spaces per level (§インデント).
pub fn indent_for_level(level: usize) -> String {
    " ".repeat(4 * level)
}

/// Parse one physical line (newline already stripped) into [`LineParts`].
pub fn parse_line(line: &str) -> LineParts {
    let indent_len = line
        .bytes()
        .take_while(|&b| b == b' ' || b == b'\t')
        .count();
    let units = indent_units(&line[..indent_len]);
    let level = indent_level(&line[..indent_len]);
    let blank = LineParts {
        kind: Kind::Blank,
        indent_len,
        units,
        level,
        text_range: (indent_len, indent_len),
        tags: Vec::new(),
    };
    if indent_len == line.len() {
        return blank;
    }

    // The tag column is the line-end suffix of tags (SPEC タグ列). Tags are
    // parsed backward from the line end so that arguments containing spaces
    // (`@repeat(0 0 * * *)`) stay intact.
    let tags = scan_tag_column_backward(line, indent_len);
    let body_end = tags
        .first()
        .map(|t| t.start)
        .unwrap_or_else(|| line.trim_end_matches([' ', '\t']).len());
    let body = &line[indent_len..body_end];
    if body.trim().is_empty() {
        // Tag-only line (tags but no body): a syntax error per the grammar.
        return LineParts {
            kind: Kind::TagOnly,
            indent_len,
            units,
            level,
            text_range: (indent_len, indent_len),
            tags,
        };
    }

    // 見出し行: the body's rightmost `:` with only whitespace after it (up to
    // the tag column) and non-empty text before it. Any earlier `:` sits
    // inside the body text, so only the rightmost colon can qualify — this
    // mirrors the external scanner, where the last valid colon wins.
    let colon = body
        .bytes()
        .rposition(|b| b == b':')
        .filter(|&i| body[i + 1..].bytes().all(|b| b == b' ' || b == b'\t'))
        .filter(|&i| !body[..i].trim().is_empty())
        .map(|i| indent_len + i);
    let kind = match colon {
        Some(c) => Kind::Heading { colon: c },
        None => Kind::Task,
    };
    // Heading text excludes the `:`; task text excludes trailing whitespace.
    let text_end = match colon {
        Some(c) => c - indent_len,
        None => body.trim_end_matches([' ', '\t']).len(),
    };
    LineParts {
        kind,
        indent_len,
        units,
        level,
        text_range: (indent_len, indent_len + text_end),
        tags,
    }
}

/// The trailing tag column: tags parsed right-to-left from the line end,
/// separated by whitespace, returned left-to-right.
fn scan_tag_column_backward(line: &str, content_start: usize) -> Vec<Tag> {
    let bytes = line.as_bytes();
    let mut pos = line.len();
    let mut tags = Vec::new();
    loop {
        while pos > content_start && (bytes[pos - 1] == b' ' || bytes[pos - 1] == b'\t') {
            pos -= 1;
        }
        let Some(tag) = parse_tag_ending_at(line, pos, content_start) else {
            break;
        };
        pos = tag.start;
        tags.push(tag);
        if pos == content_start {
            break;
        }
    }
    tags.reverse();
    tags
}

/// Parse one tag that ends exactly at byte `end`. A tag is `@name` or
/// `@name(arg)`: `name` is 1+ chars without whitespace or `(` (a `)` or `@`
/// inside the name is fine), the argument may be empty and contain
/// whitespace but no `)`. The tag must start at `content_start` or after
/// whitespace / the heading `:`.
fn parse_tag_ending_at(line: &str, end: usize, content_start: usize) -> Option<Tag> {
    let bytes = line.as_bytes();
    if end <= content_start {
        return None;
    }
    let boundary_ok = |start: usize| {
        start == content_start
            || bytes[start - 1] == b' '
            || bytes[start - 1] == b'\t'
            || bytes[start - 1] == b':' // `heading:@tag` needs no space
    };
    if bytes[end - 1] == b')' {
        // Arg-carrying tag. `(` candidates are tried right-to-left because an
        // argument may itself contain `(`; the correct opener is the one with
        // a `@name` directly before it and no `)` inside the argument.
        let mut search_end = end - 1;
        while let Some(open) = bytes[content_start..search_end]
            .iter()
            .rposition(|&b| b == b'(')
            .map(|i| content_start + i)
        {
            let arg = &line[open + 1..end - 1];
            if !arg.contains(')') {
                let mut j = open;
                while j > content_start
                    && bytes[j - 1] != b' '
                    && bytes[j - 1] != b'\t'
                    && bytes[j - 1] != b'('
                {
                    j -= 1;
                }
                if j < open && bytes[j] == b'@' && boundary_ok(j) {
                    return Some(Tag {
                        name: line[j + 1..open].to_string(),
                        arg: Some(arg.to_string()),
                        start: j,
                        end,
                    });
                }
            }
            search_end = open;
        }
        None
    } else {
        // No-argument tag `@name`.
        let mut j = end;
        while j > content_start
            && bytes[j - 1] != b' '
            && bytes[j - 1] != b'\t'
            && bytes[j - 1] != b'('
        {
            j -= 1;
        }
        if j + 1 < end && bytes[j] == b'@' && boundary_ok(j) {
            return Some(Tag {
                name: line[j + 1..end].to_string(),
                arg: None,
                start: j,
                end,
            });
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(line: &str) -> LineParts {
        parse_line(line)
    }

    fn tag_names(line: &str) -> Vec<String> {
        parts(line).tags.iter().map(|t| t.name.clone()).collect()
    }

    // ----- indent levels -----

    #[test]
    fn level_spaces_and_tabs() {
        assert_eq!(indent_level(""), 0);
        assert_eq!(indent_level("    "), 1);
        assert_eq!(indent_level("        "), 2);
        assert_eq!(indent_level("\t"), 1);
        assert_eq!(indent_level("\t\t"), 2);
        // A tab merges with up to 3 preceding spaces into one level.
        assert_eq!(indent_level("   \t"), 1);
        assert_eq!(indent_level(" \t"), 1);
        // 4 spaces + tab = the tab opens a new level.
        assert_eq!(indent_level("    \t"), 2);
        assert_eq!(indent_level("     \t"), 2);
    }

    // ----- basic classification -----

    #[test]
    fn blank_lines() {
        for line in ["", " ", "  \t "] {
            let p = parts(line);
            assert!(p.is_blank(), "{line:?}");
            assert!(!p.is_structure());
            assert!(p.tags.is_empty());
        }
    }

    #[test]
    fn plain_task() {
        let p = parts("buy milk");
        assert_eq!(p.kind, Kind::Task);
        assert_eq!(p.text_range, (0, 8));
        assert!(p.tags.is_empty());
        assert_eq!(p.task_text("buy milk"), "buy milk");
    }

    #[test]
    fn task_with_tag_column() {
        let line = "call mom @done(2024-01-01)";
        let p = parts(line);
        assert_eq!(p.kind, Kind::Task);
        assert_eq!(p.text_range, (0, 8));
        assert_eq!(p.tags.len(), 1);
        assert_eq!(p.tags[0].name, "done");
        assert_eq!(p.tags[0].arg.as_deref(), Some("2024-01-01"));
        assert_eq!(p.tags[0].start, 9);
        assert_eq!(p.tags[0].end, 26);
        assert_eq!(p.gray(), Some(Gray::Done));
    }

    #[test]
    fn tag_only_line_is_tag_only() {
        let p = parts("@done");
        assert_eq!(p.kind, Kind::TagOnly);
        assert_eq!(tag_names("@done"), ["done"]);
        assert_eq!(p.gray(), Some(Gray::Done));
    }

    #[test]
    fn empty_argument_tag() {
        let p = parts("task @flag()");
        assert_eq!(p.kind, Kind::Task);
        assert_eq!(p.tags.len(), 1);
        assert_eq!(p.tags[0].arg.as_deref(), Some(""));
    }

    // ----- tag column boundaries -----

    #[test]
    fn at_in_body_is_not_tagged() {
        // SPEC 文書構造: `@` not in a line-end tag column is body text.
        let line = "send to a@b";
        let p = parts(line);
        assert_eq!(p.kind, Kind::Task);
        assert_eq!(p.text_range, (0, line.len()));
        assert!(p.tags.is_empty());
    }

    #[test]
    fn email_local_part_is_body_text() {
        let line = "email user@example.com @done";
        let p = parts(line);
        assert_eq!(p.kind, Kind::Task);
        assert_eq!(p.text(line), "email user@example.com");
        assert_eq!(tag_names(line), ["done"]);
    }

    #[test]
    fn tag_not_at_end_is_body_text() {
        // `@a` is not line-end (a word follows), so the whole line is text.
        let line = "task @a extra";
        let p = parts(line);
        assert_eq!(p.kind, Kind::Task);
        assert!(p.tags.is_empty());
        assert_eq!(p.text(line), "task @a extra");
    }

    #[test]
    fn unclosed_tag_is_body_text() {
        let line = "task @done(";
        let p = parts(line);
        assert!(p.tags.is_empty());
        assert_eq!(p.text(line), "task @done(");
    }

    #[test]
    fn tag_with_trailing_chars_after_arg_is_body_text() {
        let p = parts("task @x(a)y");
        assert!(p.tags.is_empty());
    }

    #[test]
    fn bare_at_is_not_a_tag() {
        let p = parts("literal @ here");
        assert!(p.tags.is_empty());
        assert_eq!(p.text_range.1, "literal @ here".len());
    }

    // ----- headings -----

    #[test]
    fn heading_without_tags() {
        let line = "Inbox:";
        let p = parts(line);
        assert_eq!(p.kind, Kind::Heading { colon: 5 });
        assert_eq!(p.text(line), "Inbox");
        assert!(p.tags.is_empty());
    }

    #[test]
    fn indented_heading_with_tags() {
        let line = "  Project: @collapsed";
        let p = parts(line);
        assert_eq!(p.kind, Kind::Heading { colon: 9 });
        assert_eq!(p.text(line), "Project");
        assert_eq!(tag_names(line), ["collapsed"]);
        assert_eq!(p.level, 0); // 2 spaces < 4: level 0
    }

    #[test]
    fn colon_in_body_is_not_heading() {
        for line in ["time is 12:30", "http://x y", "a:b:c"] {
            assert_eq!(parts(line).kind, Kind::Task, "{line:?}");
        }
    }

    #[test]
    fn heading_requires_tag_column_after_colon() {
        // `Foo: @a b` — after the colon is not a tag column, so not a heading.
        assert_eq!(parts("Foo: @a b").kind, Kind::Task);
        // Same with plain text after the colon.
        assert_eq!(parts("Foo: bar").kind, Kind::Task);
    }

    #[test]
    fn heading_text_may_contain_colons() {
        // The rightmost colon with a valid tag-column suffix wins.
        let line = "a:b: @x";
        let p = parts(line);
        assert_eq!(p.kind, Kind::Heading { colon: 3 });
        assert_eq!(p.text(line), "a:b");
        assert_eq!(tag_names(line), ["x"]);
    }

    #[test]
    fn colon_without_text_is_not_heading() {
        assert_eq!(parts(":").kind, Kind::Task); // error line; body is ":"
        assert_eq!(parts(": @done").kind, Kind::Task);
    }

    #[test]
    fn archive_heading_detection() {
        assert!(parts("Archive:").is_archive_heading("Archive:"));
        assert!(parts("  Archive: @done").is_archive_heading("  Archive: @done"));
        assert!(!parts("Archive: old stuff").is_archive_heading("Archive: old stuff"));
        assert!(!parts("Inbox:").is_archive_heading("Inbox:"));
    }

    // ----- gray precedence -----

    #[test]
    fn gray_precedence_done_cancelled_hide() {
        assert_eq!(parts("t @cancelled @done").gray(), Some(Gray::Done));
        assert_eq!(parts("t @hide @cancelled").gray(), Some(Gray::Cancelled));
        assert_eq!(parts("t @hide").gray(), Some(Gray::Hide));
        assert_eq!(parts("t @done @due(2000-01-01)").gray(), Some(Gray::Done));
        assert_eq!(parts("t @queue(1)").gray(), None);
        // A tag with trailing text after it is body text, not a tag.
        assert_eq!(parts("t @done x").gray(), None);
    }

    #[test]
    fn tag_argument_may_contain_spaces() {
        let line = "task @repeat(0 0 * * *)";
        let p = parts(line);
        assert_eq!(p.kind, Kind::Task);
        assert_eq!(p.tags.len(), 1);
        assert_eq!(p.tags[0].name, "repeat");
        assert_eq!(p.tags[0].arg.as_deref(), Some("0 0 * * *"));
        assert_eq!(p.text(line), "task");
    }

    // ----- task_text / normalize_body -----

    #[test]
    fn task_text_normalizes_whitespace() {
        assert_eq!(parts("buy   milk").task_text("buy   milk"), "buy milk");
        assert_eq!(parts("  a\tb  @done").task_text("  a\tb  @done"), "a b");
    }

    #[test]
    fn heading_task_text_keeps_colon() {
        assert_eq!(parts("Inbox:").task_text("Inbox:"), "Inbox:");
    }

    #[test]
    fn normalize_body_single_spaces() {
        let line = "  buy   milk  @done(2024-01-01)  ";
        let p = parts(line);
        assert_eq!(p.normalize_body(line), "buy milk @done(2024-01-01)");
    }

    #[test]
    fn normalize_body_heading() {
        let line = "  Foo  bar:  @a @b";
        let p = parts(line);
        assert_eq!(p.normalize_body(line), "Foo bar: @a @b");
    }

    #[test]
    fn normalize_body_blank_and_tag_only() {
        assert_eq!(parts("").normalize_body(""), "");
        assert_eq!(parts("  ").normalize_body("  "), "");
        assert_eq!(parts("  @done").normalize_body("  @done"), "@done");
    }
}
