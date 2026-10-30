//! Finds the comments in a source file. A single forward pass over the text
//! with the states Code, LineComment, Block(depth) and Str(delim), driven by
//! the language table in `lang`.

use crate::lang::{Lang, StrDelim};
use std::borrow::Cow;
use std::collections::HashSet;

/// One comment: a block, or a run of consecutive line comments.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // read by the tag parser in a later task
pub struct Comment {
    /// Line of the first character of the comment marker, 1-based.
    pub start_line: usize,
    pub lines: Vec<CommentLine>,
}

/// One source line of a comment.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // read by the tag parser in a later task
pub struct CommentLine {
    /// 1-based line number.
    pub line: usize,
    /// 1-based column, in characters, of the first character of `text`.
    pub col: usize,
    /// The text after the comment marker (and after the leading `*` of a
    /// block line). Never contains `\n` or `\r`.
    pub text: String,
}

/// The comments of `src`, in source order.
///
/// `\r\n` is one line end (ruling R1): the text is lexed as its LF twin, so
/// lines and columns match and no `\r` reaches a comment.
#[allow(dead_code)] // called by the scanner in a later task
pub fn comments(lang: &Lang, src: &str) -> Vec<Comment> {
    let src = if src.contains("\r\n") {
        Cow::Owned(src.replace("\r\n", "\n"))
    } else {
        Cow::Borrowed(src)
    };
    Lexer::new(lang, &src).run()
}

/// A here-document whose body starts on the next line.
struct Heredoc {
    term: String,
    /// The terminator line may be indented.
    indent: bool,
    /// PHP: the terminator may be followed by `;`, `)` and so on.
    php: bool,
}

/// A run of standalone line comments on adjacent lines.
struct LineRun {
    marker: &'static str,
    last_line: usize,
    comment: Comment,
}

struct Lexer<'a> {
    lang: &'a Lang,
    src: &'a str,
    pos: usize,
    line: usize,
    col: usize,
    /// Byte offset where the current line starts.
    line_start: usize,
    out: Vec<Comment>,
    run: Option<LineRun>,
    pending: Vec<Heredoc>,
    /// Terminators already searched for and not found, so a file full of
    /// `<<x` shift operators is searched once, not once per operator.
    missing: HashSet<String>,
}

impl<'a> Lexer<'a> {
    fn new(lang: &'a Lang, src: &'a str) -> Self {
        Lexer {
            lang,
            src,
            pos: 0,
            line: 1,
            col: 1,
            line_start: 0,
            out: Vec::new(),
            run: None,
            pending: Vec::new(),
            missing: HashSet::new(),
        }
    }

    fn run(mut self) -> Vec<Comment> {
        while self.pos < self.src.len() {
            self.step();
        }
        self.flush_run();
        self.out
    }

    fn rest(&self) -> &'a str {
        &self.src[self.pos..]
    }

    fn prev(&self) -> Option<char> {
        self.src[..self.pos].chars().next_back()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.rest().chars().next()?;
        self.pos += c.len_utf8();
        if c == '\n' {
            self.line += 1;
            self.col = 1;
            self.line_start = self.pos;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn bump_bytes(&mut self, n: usize) {
        let end = self.pos + n;
        while self.pos < end && self.bump().is_some() {}
    }

    /// One token of code: a comment, a string, a heredoc marker or a character.
    fn step(&mut self) {
        let rest = self.rest();
        let prev = self.prev();

        if let Some(&(open, close)) = self
            .lang
            .block
            .iter()
            .filter(|(open, _)| self.block_opens_here(rest, open))
            .max_by_key(|(open, _)| open.len())
        {
            self.flush_run();
            let comment = self.block(open, close);
            self.out.push(comment);
            return;
        }
        if let Some(marker) = self
            .lang
            .line
            .iter()
            .copied()
            .filter(|m| rest.starts_with(m))
            .max_by_key(|m| m.len())
            && self
                .lang
                .line_marker_ok(marker, prev, &rest[marker.len()..])
        {
            self.line_comment(marker);
            return;
        }
        if self.lang.string_open_ok(prev)
            && let Some(delim) = self.string_here(rest)
        {
            self.skip_string(delim);
            return;
        }
        if self
            .lang
            .strings
            .iter()
            .any(|d| matches!(d, StrDelim::Heredoc))
            && rest.starts_with("<<")
            && let Some((len, heredoc)) = self.heredoc_at(rest)
        {
            self.bump_bytes(len);
            self.pending.push(heredoc);
            return;
        }
        if rest.starts_with('\'') && self.lang.has_char_literals() {
            match char_literal_len(rest) {
                Some(len) => self.bump_bytes(len),
                None => {
                    self.bump();
                }
            }
            return;
        }
        match self.bump() {
            Some('\\') if self.lang.code_backslash_escapes() => {
                self.bump();
            }
            Some('\n') if !self.pending.is_empty() => self.skip_heredoc_bodies(),
            _ => {}
        }
    }

    /// Ruby's `=begin` opens a comment only at the start of a line, on its own.
    fn block_opens_here(&self, rest: &str, open: &str) -> bool {
        rest.starts_with(open)
            && (!open.starts_with('=')
                || (self.col == 1
                    && rest[open.len()..]
                        .chars()
                        .next()
                        .is_none_or(|c| !c.is_alphanumeric() && c != '_')))
    }

    fn flush_run(&mut self) {
        if let Some(run) = self.run.take() {
            self.out.push(run.comment);
        }
    }

    /// A line comment, up to but not including the line end.
    fn line_comment(&mut self, marker: &'static str) {
        let line = self.line;
        let standalone = self.src[self.line_start..self.pos].trim().is_empty();
        self.bump_bytes(marker.len());
        // `///`, `//!`, `####`: the text starts after the whole decoration.
        let last = marker.chars().next_back().unwrap_or('/');
        while self.rest().starts_with(last) {
            self.bump();
        }
        if marker.starts_with('/') && self.rest().starts_with('!') {
            self.bump();
        }
        let col = self.col;
        let mut text = String::new();
        while let Some(c) = self.rest().chars().next().filter(|&c| c != '\n') {
            if c != '\r' {
                text.push(c);
            }
            self.bump();
        }
        let this = CommentLine { line, col, text };

        if !standalone {
            self.flush_run();
            self.out.push(Comment {
                start_line: line,
                lines: vec![this],
            });
            return;
        }
        let extends = self
            .run
            .as_ref()
            .is_some_and(|r| r.marker == marker && r.last_line + 1 == line);
        if extends && let Some(run) = self.run.as_mut() {
            run.last_line = line;
            run.comment.lines.push(this);
        } else {
            self.flush_run();
            self.run = Some(LineRun {
                marker,
                last_line: line,
                comment: Comment {
                    start_line: line,
                    lines: vec![this],
                },
            });
        }
    }

    /// A block comment from its opener to its closer, or to the end of the
    /// file when it is never closed. Nested openers count when the language
    /// nests.
    fn block(&mut self, open: &'static str, close: &'static str) -> Comment {
        let star_lines = open == "/*";
        let start_line = self.line;
        self.bump_bytes(open.len());
        if star_lines {
            // `/** doc` and `/*** doc`, but not the `*/` of `/**/`.
            while self.rest().starts_with('*') && !self.rest().starts_with("*/") {
                self.bump();
            }
        }
        let mut depth = 1;
        let mut lines = Vec::new();
        let mut text = String::new();
        let (mut text_line, mut text_col) = (self.line, self.col);
        loop {
            let rest = self.rest();
            if rest.is_empty() {
                lines.push(CommentLine {
                    line: text_line,
                    col: text_col,
                    text,
                });
                break;
            }
            if self.lang.nested && rest.starts_with(open) {
                depth += 1;
                text.push_str(open);
                self.bump_bytes(open.len());
            } else if rest.starts_with(close) && (!close.starts_with('=') || self.col == 1) {
                self.bump_bytes(close.len());
                depth -= 1;
                if depth == 0 {
                    lines.push(CommentLine {
                        line: text_line,
                        col: text_col,
                        text,
                    });
                    break;
                }
                text.push_str(close);
            } else if let Some(c) = self.bump() {
                if c == '\n' {
                    lines.push(CommentLine {
                        line: text_line,
                        col: text_col,
                        text: std::mem::take(&mut text),
                    });
                    text_line = self.line;
                    if star_lines {
                        self.skip_star_prefix();
                    }
                    text_col = self.col;
                } else if c != '\r' {
                    text.push(c);
                }
            }
        }
        Comment { start_line, lines }
    }

    /// Skips the indentation and the `*` that start a line of a block comment.
    /// A line that starts with the closer loses only its indentation.
    fn skip_star_prefix(&mut self) {
        let rest = self.rest();
        let indent = rest.len() - rest.trim_start_matches([' ', '\t']).len();
        let after = &rest[indent..];
        if after.starts_with("*/") {
            self.bump_bytes(indent);
        } else if after.starts_with('*') {
            self.bump_bytes(indent + 1);
        }
    }

    /// The longest string opener at the start of `rest`.
    fn string_here(&self, rest: &str) -> Option<&'static StrDelim> {
        self.lang
            .strings
            .iter()
            .filter(|d| opener(d).is_some_and(|o| rest.starts_with(o)))
            .max_by_key(|d| opener(d).map_or(0, str::len))
    }

    fn skip_string(&mut self, delim: &StrDelim) {
        match delim {
            StrDelim::Plain(quote) => {
                let escapes = self.lang.backslash_escapes(quote);
                let one_line = self.lang.string_ends_at_newline(quote);
                self.bump_bytes(quote.len());
                while !self.rest().is_empty() {
                    if one_line && self.rest().starts_with('\n') {
                        return;
                    }
                    if escapes && self.rest().starts_with('\\') {
                        self.bump();
                        self.bump();
                    } else if self.rest().starts_with(quote) {
                        self.bump_bytes(quote.len());
                        return;
                    } else {
                        self.bump();
                    }
                }
            }
            StrDelim::Raw { open, close } => {
                self.bump_bytes(open.len());
                while !self.rest().is_empty() {
                    if self.rest().starts_with(close) {
                        self.bump_bytes(close.len());
                        return;
                    }
                    self.bump();
                }
            }
            StrDelim::Heredoc => {}
        }
    }

    /// Reads `<<EOF`, `<<-EOF`, `<<~EOF`, `<< 'EOF'` (shell) or `<<<EOT` (PHP)
    /// at the start of `rest`: how many bytes it takes and its terminator.
    fn heredoc_at(&self, rest: &str) -> Option<(usize, Heredoc)> {
        let php = self.lang.name == "PHP";
        let mut s = rest.strip_prefix(if php { "<<<" } else { "<<" })?;
        let mut indent = php;
        if !php && let Some(after) = s.strip_prefix('-').or_else(|| s.strip_prefix('~')) {
            s = after;
            indent = true;
        }
        if self.lang.name == "Shell" {
            s = s.trim_start_matches([' ', '\t']);
        }
        let quote = s.chars().next().filter(|c| matches!(c, '\'' | '"'));
        if quote.is_some() {
            s = &s[1..];
        }
        let end = s
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(s.len());
        let term = &s[..end];
        if term.is_empty() || term.starts_with(|c: char| c.is_ascii_digit()) {
            return None;
        }
        s = &s[end..];
        if let Some(q) = quote {
            s = s.strip_prefix(q)?;
        }
        Some((
            rest.len() - s.len(),
            Heredoc {
                term: term.to_string(),
                indent,
                php,
            },
        ))
    }

    /// Called just after the line end that follows a heredoc marker: skips the
    /// bodies, up to and including each terminator line. A marker with no
    /// terminator line was not a heredoc (`$((1<<n))`) and is left as code.
    fn skip_heredoc_bodies(&mut self) {
        for heredoc in std::mem::take(&mut self.pending) {
            let key = format!(
                "{}{}{}",
                u8::from(heredoc.indent),
                u8::from(heredoc.php),
                heredoc.term
            );
            if self.missing.contains(&key) {
                continue;
            }
            match self.terminator_end(&heredoc) {
                Some(len) => self.bump_bytes(len),
                None => {
                    self.missing.insert(key);
                }
            }
        }
    }

    /// Bytes from here to the end of the terminator line, if there is one.
    fn terminator_end(&self, heredoc: &Heredoc) -> Option<usize> {
        let mut offset = 0;
        for line in self.rest().split_inclusive('\n') {
            offset += line.len();
            let body = line.strip_suffix('\n').unwrap_or(line);
            let body = if heredoc.indent {
                body.trim_start_matches([' ', '\t'])
            } else {
                body
            };
            let is_terminator = if heredoc.php {
                body.strip_prefix(heredoc.term.as_str())
                    .is_some_and(|after| {
                        !after.starts_with(|c: char| c.is_alphanumeric() || c == '_')
                    })
            } else {
                body == heredoc.term
            };
            if is_terminator {
                return Some(offset);
            }
        }
        None
    }
}

/// The text that opens a string of this kind.
fn opener(delim: &StrDelim) -> Option<&'static str> {
    match delim {
        StrDelim::Plain(quote) => Some(quote),
        StrDelim::Raw { open, .. } => Some(open),
        StrDelim::Heredoc => None,
    }
}

/// Length in bytes of the character literal at the start of `rest` (`'a'`,
/// `'\n'`, `'\u{1F600}'`), or `None` when the `'` is something else, such as
/// a Rust lifetime or a C++ digit separator.
fn char_literal_len(rest: &str) -> Option<usize> {
    let mut chars = rest.char_indices().skip(1);
    match chars.next()?.1 {
        '\\' => {
            chars.next()?;
            for (i, c) in chars.take(10) {
                match c {
                    '\'' => return Some(i + 1),
                    '\n' => return None,
                    _ => {}
                }
            }
            None
        }
        '\'' | '\n' => None,
        _ => {
            let (i, c) = chars.next()?;
            (c == '\'').then_some(i + 1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::{ALL, for_path};
    use proptest::prelude::*;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn lang_for(file: &str) -> &'static Lang {
        for_path(Path::new(file)).unwrap_or_else(|| panic!("no language for {file}"))
    }

    fn corpus_files() -> Vec<PathBuf> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus");
        let mut files: Vec<PathBuf> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.is_file()
                    && p.extension().is_none_or(|e| e != "expected")
                    && p.file_name().unwrap() != ".gitattributes"
            })
            .collect();
        files.sort();
        files
    }

    /// One comment per line: `start_line|text|text...`.
    fn render(found: &[Comment]) -> String {
        let mut out = String::new();
        for c in found {
            let texts: Vec<&str> = c.lines.iter().map(|l| l.text.as_str()).collect();
            out.push_str(&format!("{}|{}\n", c.start_line, texts.join("|")));
        }
        out
    }

    #[test]
    fn golden() {
        let mut failures = Vec::new();
        for path in corpus_files() {
            let lang =
                for_path(&path).unwrap_or_else(|| panic!("no language for {}", path.display()));
            let src = fs::read_to_string(&path).unwrap();
            let expected = fs::read_to_string(format!("{}.expected", path.display())).unwrap();
            let got = render(&comments(lang, &src));
            if got != expected {
                failures.push(format!(
                    "{}\n--- expected\n{expected}--- got\n{got}",
                    path.display()
                ));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn corpus_has_a_file_for_every_language() {
        let mut seen: Vec<&str> = corpus_files()
            .iter()
            .map(|p| for_path(p).unwrap().name)
            .collect();
        seen.sort_unstable();
        seen.dedup();
        let mut all: Vec<&str> = ALL.iter().map(|l| l.name).collect();
        all.sort_unstable();
        assert_eq!(seen, all);
    }

    /// Ruling R1: a CRLF file reports the same lines and columns as its LF twin.
    #[test]
    fn crlf_twin_matches_lf_for_every_corpus_file() {
        for path in corpus_files() {
            let lang = for_path(&path).unwrap();
            let lf = fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
            let crlf = lf.replace('\n', "\r\n");
            assert_eq!(
                comments(lang, &crlf),
                comments(lang, &lf),
                "{}",
                path.display()
            );
        }
    }

    #[test]
    fn crlf_never_leaks_a_carriage_return() {
        let src = "// TODO a\r\n// b\r\n/* c\r\n * d */\r\n";
        for c in comments(lang_for("x.rs"), src) {
            for l in c.lines {
                assert!(!l.text.contains('\r') && !l.text.contains('\n'), "{l:?}");
            }
        }
    }

    fn line(line: usize, col: usize, text: &str) -> CommentLine {
        CommentLine {
            line,
            col,
            text: text.to_string(),
        }
    }

    #[test]
    fn columns_count_characters_from_the_text() {
        let found = comments(lang_for("x.js"), "  // TODO x\né // y\n");
        assert_eq!(found[0].lines, vec![line(1, 5, " TODO x")]);
        assert_eq!(found[1].lines, vec![line(2, 5, " y")]);
    }

    #[test]
    fn block_lines_drop_the_leading_star() {
        let found = comments(lang_for("x.c"), "/**\n * a\n   * b\n */ int x;");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].start_line, 1);
        assert_eq!(
            found[0].lines,
            vec![
                line(1, 4, ""),
                line(2, 3, " a"),
                line(3, 5, " b"),
                line(4, 2, "")
            ]
        );
    }

    #[test]
    fn empty_block_is_one_empty_line() {
        let found = comments(lang_for("x.c"), "/**/ /***/");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].lines, vec![line(1, 3, "")]);
        assert_eq!(found[1].lines, vec![line(1, 9, "")]);
    }

    #[test]
    fn consecutive_line_comments_are_one_comment_only_when_standalone() {
        let found = comments(
            lang_for("x.rs"),
            "// a\n// b\n\n// c\nlet x = 1; // d\n// e\n",
        );
        let shape: Vec<(usize, usize)> = found
            .iter()
            .map(|c| (c.start_line, c.lines.len()))
            .collect();
        assert_eq!(shape, vec![(1, 2), (4, 1), (5, 1), (6, 1)]);
    }

    #[test]
    fn unterminated_block_runs_to_end_of_file() {
        let found = comments(lang_for("x.c"), "int x; /* TODO\nmore");
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].lines,
            vec![line(1, 10, " TODO"), line(2, 1, "more")]
        );
    }

    #[test]
    fn unterminated_string_runs_to_end_of_file() {
        assert!(comments(lang_for("x.js"), "let s = `abc\n// TODO\n").is_empty());
        assert!(comments(lang_for("x.sh"), "echo \"abc\n# TODO\n").is_empty());
        assert!(comments(lang_for("x.rs"), "let s = \"abc\n// TODO\n").is_empty());
        assert!(comments(lang_for("x.py"), "s = '''abc\n# TODO\n").is_empty());
    }

    #[test]
    fn single_line_quotes_end_at_the_line_end() {
        let found = comments(lang_for("x.js"), "let s = \"abc\n// TODO: y\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].start_line, 2);
        // a backslash-newline continues the string
        assert!(comments(lang_for("x.js"), "let s = \"abc\\\n// TODO\"\n").is_empty());
        // a stray apostrophe does not flip the pairing after it
        let found = comments(
            lang_for("x.ts"),
            "<p>Don't</p>\n// a\nlet t = 'x // TODO';\n",
        );
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn unterminated_heredoc_is_code() {
        let found = comments(lang_for("x.sh"), "echo $((1<<n))\n# TODO\n");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].start_line, 2);
    }

    #[test]
    fn for_path_uses_extension_then_file_name() {
        assert_eq!(lang_for("a/b.RS").name, "Rust");
        assert_eq!(lang_for("a/b.tsx").name, "TypeScript");
        assert_eq!(lang_for("a/Dockerfile").name, "Dockerfile/Makefile");
        assert_eq!(lang_for("Makefile").name, "Dockerfile/Makefile");
        assert!(for_path(Path::new("a/b.unknown")).is_none());
        assert!(for_path(Path::new("a/LICENSE")).is_none());
    }

    #[test]
    fn table_has_22_distinct_languages() {
        let mut names: Vec<&str> = ALL.iter().map(|l| l.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 22);
    }

    const TOKENS: &[&str] = &[
        "/*", "*/", "//", "#", "--", "\"", "'", "`", "\\", "\n", "\r\n", "<<EOF\n", "EOF\n", "<<-",
        "<<<", "EOT;\n", "=begin\n", "=end\n", "<!--", "-->", "--[[", "]]", "[[", "r#\"", "\"#",
        "R\"(", ")\"", "@\"", "\"\"\"", "'''", "TODO", " ", "\u{e9}", "\u{65e5}", "*", "$", "{",
        "}",
    ];

    fn token_soup() -> impl Strategy<Value = String> {
        prop::collection::vec(prop::sample::select(TOKENS), 0..60).prop_map(|v| v.concat())
    }

    /// Every comment line is a real slice of the source at its line and column.
    fn check_well_formed(lang: &Lang, src: &str) {
        let src_lines: Vec<&str> = src.split('\n').collect();
        let mut last = 0;
        for c in comments(lang, src) {
            assert!(!c.lines.is_empty(), "{}: empty comment", lang.name);
            assert_eq!(c.start_line, c.lines[0].line, "{}", lang.name);
            for l in &c.lines {
                assert!(l.line >= last.max(1) && l.col >= 1, "{}: {l:?}", lang.name);
                last = l.line;
                assert!(!l.text.contains('\n') && !l.text.contains('\r'));
                let there: String = src_lines[l.line - 1]
                    .chars()
                    .skip(l.col - 1)
                    .take(l.text.chars().count())
                    .collect();
                assert_eq!(there, l.text, "{}: {l:?} in {src:?}", lang.name);
            }
        }
    }

    proptest! {
        #[test]
        fn never_panics(arbitrary in any::<String>(), soup in token_soup()) {
            for lang in ALL {
                comments(lang, &arbitrary);
                comments(lang, &soup);
            }
        }

        #[test]
        fn comment_lines_are_slices_of_the_source(soup in token_soup()) {
            for lang in ALL {
                check_well_formed(lang, &soup.replace("\r\n", "\n"));
            }
        }

        #[test]
        fn crlf_twin_matches_lf(soup in token_soup()) {
            let lf = soup.replace("\r\n", "\n");
            let crlf = lf.replace('\n', "\r\n");
            for lang in ALL {
                prop_assert_eq!(comments(lang, &crlf), comments(lang, &lf));
            }
        }
    }
}
