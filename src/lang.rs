//! The language table: for each supported language, how comments and strings
//! are written. `lexer` reads these tables; nothing else guesses at syntax.

use std::path::Path;

/// How one language writes comments and strings.
#[derive(Debug)]
pub struct Lang {
    pub name: &'static str,
    /// Line-comment markers.
    pub line: &'static [&'static str],
    /// Block-comment `(open, close)` pairs.
    pub block: &'static [(&'static str, &'static str)],
    /// Whether block comments nest.
    pub nested: bool,
    pub strings: &'static [StrDelim],
}

/// A kind of string literal, which hides comment markers inside it.
#[derive(Debug)]
pub enum StrDelim {
    /// Opens and closes with the same text; a backslash escapes the next
    /// character unless [`Lang::backslash_escapes`] says otherwise.
    Plain(&'static str),
    /// Runs from `open` to the first `close`, with no escapes.
    Raw {
        open: &'static str,
        close: &'static str,
    },
    /// The language has here-documents (`<<EOF`); the lexer finds them itself.
    Heredoc,
}

use StrDelim::{Heredoc, Plain, Raw};

const DQ: StrDelim = Plain("\"");
const SQ: StrDelim = Plain("'");
const BACKTICK: StrDelim = Plain("`");
const TRIPLE_DQ: StrDelim = Plain("\"\"\"");
const TRIPLE_SQ: StrDelim = Plain("'''");

const C_BLOCK: &[(&str, &str)] = &[("/*", "*/")];

static RUST: Lang = Lang {
    name: "Rust",
    line: &["//"],
    block: C_BLOCK,
    nested: true,
    strings: &[
        DQ,
        Raw {
            open: "r\"",
            close: "\"",
        },
        Raw {
            open: "r#\"",
            close: "\"#",
        },
        Raw {
            open: "r##\"",
            close: "\"##",
        },
        Raw {
            open: "r###\"",
            close: "\"###",
        },
    ],
};

static C: Lang = Lang {
    name: "C",
    line: &["//"],
    block: C_BLOCK,
    nested: false,
    strings: &[DQ],
};

static CPP: Lang = Lang {
    name: "C++",
    line: &["//"],
    block: C_BLOCK,
    nested: false,
    strings: &[
        DQ,
        Raw {
            open: "R\"(",
            close: ")\"",
        },
    ],
};

static CSHARP: Lang = Lang {
    name: "C#",
    line: &["//"],
    block: C_BLOCK,
    nested: false,
    // A doubled quote in a verbatim string closes it and opens the next one,
    // so the quotes still pair up.
    strings: &[
        DQ,
        Raw {
            open: "@\"",
            close: "\"",
        },
    ],
};

static GO: Lang = Lang {
    name: "Go",
    line: &["//"],
    block: C_BLOCK,
    nested: false,
    strings: &[DQ, BACKTICK],
};

static JAVA: Lang = Lang {
    name: "Java",
    line: &["//"],
    block: C_BLOCK,
    nested: false,
    strings: &[DQ, TRIPLE_DQ],
};

static KOTLIN: Lang = Lang {
    name: "Kotlin",
    line: &["//"],
    block: C_BLOCK,
    nested: true,
    strings: &[DQ, TRIPLE_DQ],
};

static SWIFT: Lang = Lang {
    name: "Swift",
    line: &["//"],
    block: C_BLOCK,
    nested: true,
    strings: &[
        DQ,
        TRIPLE_DQ,
        Raw {
            open: "#\"",
            close: "\"#",
        },
    ],
};

static JAVASCRIPT: Lang = Lang {
    name: "JavaScript",
    line: &["//"],
    block: C_BLOCK,
    nested: false,
    strings: &[DQ, SQ, BACKTICK],
};

static TYPESCRIPT: Lang = Lang {
    name: "TypeScript",
    line: &["//"],
    block: C_BLOCK,
    nested: false,
    strings: &[DQ, SQ, BACKTICK],
};

static PYTHON: Lang = Lang {
    name: "Python",
    line: &["#"],
    block: &[],
    nested: false,
    strings: &[TRIPLE_DQ, TRIPLE_SQ, DQ, SQ],
};

static RUBY: Lang = Lang {
    name: "Ruby",
    line: &["#"],
    block: &[("=begin", "=end")],
    nested: false,
    strings: &[DQ, SQ, Heredoc],
};

static PHP: Lang = Lang {
    name: "PHP",
    line: &["//", "#"],
    block: C_BLOCK,
    nested: false,
    strings: &[DQ, SQ, Heredoc],
};

static LUA: Lang = Lang {
    name: "Lua",
    line: &["--"],
    block: &[("--[[", "]]"), ("--[=[", "]=]"), ("--[==[", "]==]")],
    nested: false,
    strings: &[
        DQ,
        SQ,
        Raw {
            open: "[[",
            close: "]]",
        },
        Raw {
            open: "[=[",
            close: "]=]",
        },
        Raw {
            open: "[==[",
            close: "]==]",
        },
    ],
};

static SHELL: Lang = Lang {
    name: "Shell",
    line: &["#"],
    block: &[],
    nested: false,
    strings: &[DQ, SQ, Heredoc],
};

static SQL: Lang = Lang {
    name: "SQL",
    line: &["--"],
    block: C_BLOCK,
    nested: false,
    strings: &[SQ, DQ, BACKTICK],
};

static YAML: Lang = Lang {
    name: "YAML",
    line: &["#"],
    block: &[],
    nested: false,
    strings: &[DQ, SQ],
};

static TOML: Lang = Lang {
    name: "TOML",
    line: &["#"],
    block: &[],
    nested: false,
    strings: &[TRIPLE_DQ, TRIPLE_SQ, DQ, SQ],
};

static HTML: Lang = Lang {
    name: "HTML",
    line: &[],
    block: &[("<!--", "-->")],
    nested: false,
    strings: &[],
};

static CSS: Lang = Lang {
    name: "CSS/SCSS",
    line: &["//"],
    block: C_BLOCK,
    nested: false,
    strings: &[DQ, SQ],
};

static MARKDOWN: Lang = Lang {
    name: "Markdown",
    line: &[],
    block: &[("<!--", "-->")],
    nested: false,
    strings: &[],
};

static DOCKER_MAKE: Lang = Lang {
    name: "Dockerfile/Makefile",
    line: &["#"],
    block: &[],
    nested: false,
    strings: &[DQ],
};

/// Every language in the table.
#[allow(dead_code)] // used by the tests now, and by the scanner in a later task
pub static ALL: [&Lang; 22] = [
    &RUST,
    &C,
    &CPP,
    &CSHARP,
    &GO,
    &JAVA,
    &KOTLIN,
    &SWIFT,
    &JAVASCRIPT,
    &TYPESCRIPT,
    &PYTHON,
    &RUBY,
    &PHP,
    &LUA,
    &SHELL,
    &SQL,
    &YAML,
    &TOML,
    &HTML,
    &CSS,
    &MARKDOWN,
    &DOCKER_MAKE,
];

/// File extensions, lower case, without the dot.
static BY_EXTENSION: &[(&str, &Lang)] = &[
    ("rs", &RUST),
    ("c", &C),
    ("h", &C),
    ("cc", &CPP),
    ("cpp", &CPP),
    ("cxx", &CPP),
    ("hh", &CPP),
    ("hpp", &CPP),
    ("hxx", &CPP),
    ("cs", &CSHARP),
    ("go", &GO),
    ("java", &JAVA),
    ("kt", &KOTLIN),
    ("kts", &KOTLIN),
    ("swift", &SWIFT),
    ("js", &JAVASCRIPT),
    ("jsx", &JAVASCRIPT),
    ("mjs", &JAVASCRIPT),
    ("cjs", &JAVASCRIPT),
    ("ts", &TYPESCRIPT),
    ("tsx", &TYPESCRIPT),
    ("mts", &TYPESCRIPT),
    ("cts", &TYPESCRIPT),
    ("py", &PYTHON),
    ("pyi", &PYTHON),
    ("rb", &RUBY),
    ("php", &PHP),
    ("lua", &LUA),
    ("sh", &SHELL),
    ("bash", &SHELL),
    ("zsh", &SHELL),
    ("sql", &SQL),
    ("yml", &YAML),
    ("yaml", &YAML),
    ("toml", &TOML),
    ("html", &HTML),
    ("htm", &HTML),
    ("css", &CSS),
    ("scss", &CSS),
    ("md", &MARKDOWN),
    ("markdown", &MARKDOWN),
    ("dockerfile", &DOCKER_MAKE),
    ("mk", &DOCKER_MAKE),
];

/// Files known by their whole name.
static BY_NAME: &[(&str, &Lang)] = &[
    ("Dockerfile", &DOCKER_MAKE),
    ("Makefile", &DOCKER_MAKE),
    ("GNUmakefile", &DOCKER_MAKE),
];

/// The language of a file, by extension and then by exact file name. `None`
/// means the file is skipped, never guessed at.
#[allow(dead_code)] // used by the tests now, and by the scanner in a later task
pub fn for_path(path: &Path) -> Option<&'static Lang> {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        let ext = ext.to_ascii_lowercase();
        if let Some((_, lang)) = BY_EXTENSION.iter().find(|(e, _)| *e == ext) {
            return Some(lang);
        }
    }
    let name = path.file_name()?.to_str()?;
    BY_NAME
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, lang)| *lang)
}

// Rules that data alone cannot say. They are keyed on the language name and
// kept here, next to the table, so the lexer stays free of language names.
impl Lang {
    /// `'x'` is a character literal here, and a lone `'` (a Rust lifetime, a
    /// C++ digit separator) is not a string.
    pub fn has_char_literals(&self) -> bool {
        matches!(
            self.name,
            "Rust" | "C" | "C++" | "C#" | "Go" | "Java" | "Kotlin"
        )
    }

    /// Whether a line-comment `marker` starts a comment here. `prev` is the
    /// character before it and `after` the text after it.
    pub fn line_marker_ok(&self, marker: &str, prev: Option<char>, after: &str) -> bool {
        let blank_before = prev.is_none_or(char::is_whitespace);
        match self.name {
            // `$#`, `${#x}` and `a#b` are not comments.
            "Shell" => blank_before || matches!(prev, Some(';' | '&' | '|' | '(' | ')')),
            "YAML" => blank_before,
            // `url(http://x)`
            "CSS/SCSS" => prev != Some(':'),
            // `#[Attribute]`
            "PHP" if marker == "#" => !after.starts_with('['),
            _ => true,
        }
    }

    /// Whether a string may open after `prev`. YAML quotes open only at the
    /// start of a value, so the apostrophe in `don't` is not a string.
    pub fn string_open_ok(&self, prev: Option<char>) -> bool {
        match self.name {
            "YAML" => prev.is_none_or(|c| c.is_whitespace() || matches!(c, '[' | '{' | ',' | '\'')),
            _ => true,
        }
    }

    /// Whether a backslash escapes the next character inside a `Plain` string
    /// opened by `delim`.
    pub fn backslash_escapes(&self, delim: &str) -> bool {
        !matches!(
            (self.name, delim),
            ("Shell" | "YAML" | "TOML", "'") | ("SQL", _) | ("Go", "`")
        )
    }

    /// Whether a `Plain` string opened by `delim` ends at an unescaped line
    /// end, so a stray quote (the apostrophe in JSX text, a regex literal)
    /// cannot swallow the rest of the file. Only `"` and `'` qualify; triple
    /// quotes, backticks and raw strings may span lines. Rust `"` strings and
    /// those of Shell, Ruby, PHP, SQL and YAML are multi-line by design.
    pub fn string_ends_at_newline(&self, delim: &str) -> bool {
        matches!(delim, "\"" | "'")
            && matches!(
                self.name,
                "JavaScript"
                    | "TypeScript"
                    | "Python"
                    | "C"
                    | "C++"
                    | "C#"
                    | "Java"
                    | "Kotlin"
                    | "Swift"
                    | "Go"
                    | "CSS/SCSS"
                    | "Lua"
                    | "TOML"
                    | "Dockerfile/Makefile"
            )
    }

    /// Whether a backslash outside strings escapes the next character
    /// (`\#`, `\'` in shell).
    pub fn code_backslash_escapes(&self) -> bool {
        self.name == "Shell"
    }
}
