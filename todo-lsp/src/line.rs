//! Line-level lexical model of the Todo language, shared by highlighting,
//! commands, formatting, archiving and repeat generation.
//!
//! This module mirrors SPEC.md's 用語 definitions verbatim:
//! 見出し行 / タスク行 / タグ列 / 灰色行 / インデントレベル. All consumers
//! classify a physical line through [`parse_line`]; nothing here talks to the
//! syntax tree.

/// A `@name` / `@name(arg)` token inside a line's leading or trailing tag
/// column.
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
    /// 見出し行: non-empty body, then `:`, then an optional trailing tag
    /// column. Leading tag-like tokens are body text (SPEC 行頭タグ列
    /// applies to task lines only).
    /// The value is the byte index of the `:`.
    Heading { colon: usize },
    /// タスク行: an optional leading tag column, a body that may be empty
    /// (a tag-only line), and an optional trailing tag column.
    Task,
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
    /// The leading tag column (行頭タグ列), in line order. Empty when the
    /// line starts with body text.
    pub leading_tags: Vec<Tag>,
    /// Byte range of the trimmed body text — before the `:` for headings,
    /// before the trailing tag column for tasks. Empty range for blank
    /// lines and tag-only lines.
    pub text_range: (usize, usize),
    /// The trailing tag column (行末タグ列), in line order.
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

    /// The trimmed body text (heading text excludes the `:`). Empty for
    /// tag-only lines.
    pub fn text<'a>(&self, line: &'a str) -> &'a str {
        &line[self.text_range.0..self.text_range.1]
    }

    /// タスクテキスト per SPEC.md: the line minus tags and whitespace, with
    /// runs of whitespace collapsed to single spaces. Heading lines keep
    /// their trailing `:`. Empty for tag-only lines.
    pub fn task_text(&self, line: &str) -> String {
        let end = match self.kind {
            Kind::Heading { colon } => colon + 1,
            _ => self.text_range.1,
        };
        let raw = &line[self.text_range.0..end];
        raw.split_ascii_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// All tags on the line: the leading tag column then the trailing tag
    /// column, in line order.
    pub fn all_tags(&self) -> impl Iterator<Item = &Tag> {
        self.leading_tags.iter().chain(self.tags.iter())
    }

    /// Whether the line has a `@name` tag in either tag column.
    pub fn has_tag(&self, name: &str) -> bool {
        self.all_tags().any(|t| t.name == name)
    }

    /// The first `@name` argument on the line, if any (leading column first).
    pub fn tag_arg(&self, name: &str) -> Option<&str> {
        self.all_tags()
            .find(|t| t.name == name)
            .and_then(|t| t.arg.as_deref())
    }

    /// The 灰色行 rule for this line: `@done` / `@cancelled` / `@hide` in
    /// either tag column, first match in 適用規則 order.
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

    /// Rule 2 of §フォーマット for this line alone: every token (leading
    /// tags, text, `:`, trailing tags) separated by single spaces, no
    /// leading/trailing whitespace. Returns an empty string for blank
    /// lines. The caller prepends the normalized indent.
    pub fn normalize_body(&self, line: &str) -> String {
        render(self, line, &self.leading_tags, &self.tags)
    }
}

/// Render a line's content from its parsed parts and (possibly edited) tag
/// columns: leading tags, whitespace-normalized text, `:` for headings,
/// trailing tags — each joined with single spaces. Blank lines render as
/// empty strings.
pub fn render(parts: &LineParts, line: &str, leading: &[Tag], trailing: &[Tag]) -> String {
    let text = parts
        .text(line)
        .split_ascii_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let mut out = String::new();
    for tag in leading {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&tag.text());
    }
    if !text.is_empty() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&text);
    }
    if parts.is_heading() {
        out.push(':');
    }
    for tag in trailing {
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
        leading_tags: Vec::new(),
        text_range: (indent_len, indent_len),
        tags: Vec::new(),
    };
    if indent_len == line.len() {
        return blank;
    }

    // The leading tag column (行頭タグ列): tags from the line start up to the
    // first non-tag token. A line the leading column fills entirely is a
    // tag-only task line with no body and no trailing column.
    let (leading_tags, content_start) = scan_leading_tags_forward(line, indent_len);
    if content_start == line.len() {
        return LineParts {
            kind: Kind::Task,
            indent_len,
            units,
            level,
            leading_tags,
            text_range: (content_start, content_start),
            tags: Vec::new(),
        };
    }

    // The trailing tag column is the line-end suffix of tags (SPEC 行末タグ列).
    // Tags are parsed backward from the line end so that arguments containing
    // spaces (`@repeat(0 0 * * *)`) stay intact.
    let tags = scan_tag_column_backward(line, content_start);
    let body_end = tags
        .first()
        .map(|t| t.start)
        .unwrap_or_else(|| line.trim_end_matches([' ', '\t']).len());
    let body = &line[content_start..body_end];

    // 見出し行: the body's rightmost `:` with only whitespace after it (up to
    // the tag column). Any earlier `:` sits inside the body text, so only the
    // rightmost colon can qualify — this mirrors the external scanner, where
    // the last valid colon wins. The text before the `:` may be empty (SPEC
    // 見出し: `:` で終わる行は本文が空でも見出し).
    let colon = body
        .bytes()
        .rposition(|b| b == b':')
        .filter(|&i| body[i + 1..].bytes().all(|b| b == b' ' || b == b'\t'))
        .map(|i| content_start + i);
    let kind = match colon {
        Some(c) => Kind::Heading { colon: c },
        None => Kind::Task,
    };
    // 見出し行は行頭タグ列を持たず、行頭のタグ字面は本文に含める（SPEC 用語
    // 行頭タグ列）。タスク行だけが行頭タグ列を持つ。Heading text excludes
    // the `:`; task text excludes trailing whitespace.
    let (leading_tags, text_start) = match colon {
        Some(_) => (Vec::new(), indent_len),
        None => (leading_tags, content_start),
    };
    let text_end = match colon {
        Some(c) => c,
        None => text_start + body.trim_end_matches([' ', '\t']).len(),
    };
    LineParts {
        kind,
        indent_len,
        units,
        level,
        leading_tags,
        text_range: (text_start, text_end),
        tags,
    }
}

/// The leading tag column: tags parsed left-to-right from `start`, each
/// followed by whitespace (or the line end for a tag-only line). Returns the
/// tags and the byte offset where the body starts.
fn scan_leading_tags_forward(line: &str, start: usize) -> (Vec<Tag>, usize) {
    let bytes = line.as_bytes();
    let mut tags = Vec::new();
    let mut pos = start;
    loop {
        let Some((tag, end)) = parse_tag_starting_at(line, pos) else {
            break;
        };
        // A leading tag must be followed by whitespace or the line end;
        // `@x(a)y` is not part of the leading column.
        if end < line.len() && bytes[end] != b' ' && bytes[end] != b'\t' {
            break;
        }
        tags.push(tag);
        pos = end;
        while pos < line.len() && (bytes[pos] == b' ' || bytes[pos] == b'\t') {
            pos += 1;
        }
        if pos == line.len() {
            break;
        }
    }
    (tags, pos)
}

/// Parse one tag starting exactly at byte `start`: `@name` or `@name(arg)`.
/// `name` is 1+ chars without whitespace or `(`; the argument may contain
/// whitespace but no `)` and must close before the line ends. Returns the tag
/// and the byte offset just past it.
fn parse_tag_starting_at(line: &str, start: usize) -> Option<(Tag, usize)> {
    let bytes = line.as_bytes();
    if bytes.get(start) != Some(&b'@') {
        return None;
    }
    let mut i = start + 1;
    while i < line.len() && bytes[i] != b' ' && bytes[i] != b'\t' && bytes[i] != b'(' {
        i += 1;
    }
    if i == start + 1 {
        return None; // empty name
    }
    let name = line[start + 1..i].to_string();
    if bytes.get(i) == Some(&b'(') {
        let close = bytes[i + 1..].iter().position(|&b| b == b')')?;
        let arg_end = i + 1 + close;
        let tag = Tag {
            name,
            arg: Some(line[i + 1..arg_end].to_string()),
            start,
            end: arg_end + 1,
        };
        Some((tag, arg_end + 1))
    } else {
        let tag = Tag {
            name,
            arg: None,
            start,
            end: i,
        };
        Some((tag, i))
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
/// whitespace — the trailing column begins after a space, so a tag glued to
/// the heading `:` (`Foo:@tag`) is body text.
fn parse_tag_ending_at(line: &str, end: usize, content_start: usize) -> Option<Tag> {
    let bytes = line.as_bytes();
    if end <= content_start {
        return None;
    }
    let boundary_ok = |start: usize| {
        start == content_start || bytes[start - 1] == b' ' || bytes[start - 1] == b'\t'
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
    fn tag_only_line_is_a_task_with_no_body() {
        let p = parts("@done");
        assert_eq!(p.kind, Kind::Task);
        assert!(p.leading_tags.iter().map(|t| t.name.clone()).eq(["done".to_string()]));
        assert!(p.tags.is_empty());
        assert_eq!(p.text("@done"), "");
        assert_eq!(p.task_text("@done"), "");
        assert_eq!(p.gray(), Some(Gray::Done));
    }

    #[test]
    fn multiple_tag_only_line() {
        let p = parts("@done @waiting");
        assert_eq!(p.kind, Kind::Task);
        assert_eq!(p.leading_tags.len(), 2);
        assert!(p.tags.is_empty());
        assert_eq!(p.text("@done @waiting"), "");
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
    fn tag_glued_to_heading_colon_is_body_text() {
        // SPEC 行末タグ列: the trailing column starts after at least one
        // space. `Foo:@tag` has no space, so it is a plain task line — the
        // same rule as `time is 12:30`.
        let line = "Foo:@tag";
        let p = parts(line);
        assert_eq!(p.kind, Kind::Task);
        assert!(p.tags.is_empty());
        assert_eq!(p.text(line), "Foo:@tag");
        // With a space it is a heading plus its trailing tag column.
        let line = "Foo: @tag";
        let p = parts(line);
        assert_eq!(p.kind, Kind::Heading { colon: 3 });
        assert_eq!(p.text(line), "Foo");
        assert!(p.tags.iter().map(|t| t.name.clone()).eq(["tag".to_string()]));
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
    fn colon_without_text_is_a_heading() {
        // SPEC 見出し: `:` または `:` と行末タグ列で終わる行は、本文が空でも
        // 見出し。本文テキストは空になる。
        let p = parts(":");
        assert_eq!(p.kind, Kind::Heading { colon: 0 });
        assert_eq!(p.text_range, (0, 0));
        let p = parts(": @done");
        assert_eq!(p.kind, Kind::Heading { colon: 0 });
        assert_eq!(p.text_range, (0, 0));
        assert_eq!(tag_names(": @done"), ["done"]);
    }

    #[test]
    fn colon_initial_body_classification() {
        // `:foo` — body continues after the colon, so not a heading.
        let p = parts(":foo");
        assert_eq!(p.kind, Kind::Task);
        assert_eq!(p.text(":foo"), ":foo");
        // `:foo:` — the rightmost colon ends the body, so it is a heading.
        let p = parts(":foo:");
        assert_eq!(p.kind, Kind::Heading { colon: 4 });
        assert_eq!(p.text(":foo:"), ":foo");
    }

    #[test]
    fn tag_name_may_contain_colon() {
        // SPEC タグ: `name` excludes only whitespace and `(` — `@done:` is a
        // leading tag named `done:`, so the line is not a heading and its
        // body is the text after the tag column.
        let line = "@done: 名前はコロンを含めるため見出しにならない";
        let p = parts(line);
        assert_eq!(p.kind, Kind::Task);
        assert_eq!(p.leading_tags.len(), 1);
        assert_eq!(p.leading_tags[0].name, "done:");
        assert_eq!(p.text(line), "名前はコロンを含めるため見出しにならない");
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
    fn normalize_body_heading_leading_tag_tokens_stay_body() {
        // Heading leading tag-like tokens are body text: normalization keeps
        // them in place before the `:`, and the `@done` token does not act as
        // a tag (so it is not moved or dropped).
        let line = "  @done   Foo:  ";
        let p = parts(line);
        assert_eq!(p.normalize_body(line), "@done Foo:");
    }

    #[test]
    fn normalize_body_blank_and_tag_only() {
        assert_eq!(parts("").normalize_body(""), "");
        assert_eq!(parts("  ").normalize_body("  "), "");
        assert_eq!(parts("  @done").normalize_body("  @done"), "@done");
        assert_eq!(parts("@done  @waiting").normalize_body("@done  @waiting"), "@done @waiting");
    }

    // ----- leading tag column -----

    #[test]
    fn leading_tag_column_before_body() {
        let line = "@done buy milk";
        let p = parts(line);
        assert_eq!(p.kind, Kind::Task);
        assert_eq!(p.leading_tags.len(), 1);
        assert_eq!(p.leading_tags[0].name, "done");
        assert_eq!(p.text(line), "buy milk");
        assert!(p.tags.is_empty());
        assert_eq!(p.gray(), Some(Gray::Done));
    }

    #[test]
    fn leading_and_trailing_tag_columns() {
        let line = "@done @waiting buy milk @queue(1)";
        let p = parts(line);
        assert_eq!(p.kind, Kind::Task);
        assert_eq!(p.leading_tags.len(), 2);
        assert!(p.tags.iter().map(|t| t.name.clone()).eq(["queue".to_string()]));
        assert_eq!(p.text(line), "buy milk");
        assert_eq!(p.gray(), Some(Gray::Done));
        assert_eq!(p.tag_arg("queue"), Some("1"));
    }

    #[test]
    fn leading_tag_column_on_heading() {
        // SPEC 行頭タグ列: a leading tag column exists on task lines only.
        // On a heading the tag-like tokens are body text: no gray rule, and
        // the text range covers the whole body including the tokens.
        let line = "@done Project: @queue(1)";
        let p = parts(line);
        assert_eq!(p.kind, Kind::Heading { colon: 13 });
        assert!(p.leading_tags.is_empty());
        assert_eq!(p.text(line), "@done Project");
        assert!(p.tags.iter().map(|t| t.name.clone()).eq(["queue".to_string()]));
        assert_eq!(p.gray(), None);
        assert_eq!(p.task_text(line), "@done Project:");
    }

    #[test]
    fn leading_tag_with_unclosed_argument_is_body() {
        // SPEC タグの構文: an unclosed argument makes it body text.
        let line = "@done( buy milk";
        let p = parts(line);
        assert_eq!(p.kind, Kind::Task);
        assert!(p.leading_tags.is_empty());
        assert!(p.tags.is_empty());
        assert_eq!(p.text(line), "@done( buy milk");
    }

    #[test]
    fn leading_tag_followed_by_non_space_is_body() {
        // `@x(a)y` — the tag-like token is not whitespace-separated, so the
        // whole prefix is body text.
        let line = "@x(a)y buy";
        let p = parts(line);
        assert_eq!(p.kind, Kind::Task);
        assert!(p.leading_tags.is_empty());
        assert_eq!(p.text(line), "@x(a)y buy");
    }

    #[test]
    fn leading_tag_render_keeps_position() {
        let line = "@done   buy  milk   @queue(1)";
        let p = parts(line);
        assert_eq!(p.normalize_body(line), "@done buy milk @queue(1)");
    }
}
