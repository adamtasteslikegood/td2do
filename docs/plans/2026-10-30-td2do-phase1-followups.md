# td2do phase 1: rulings and follow-ups

Recorded from the phase 1 build (subagent-driven, 2026-10-30). Plan: [2026-10-30-td2do-phase1.md](2026-10-30-td2do-phase1.md).

## Rulings made during the build

- Ruling: controller installed Rust before Task 1 instead of Task 1 step 1 — subagents share one toolchain — costs nothing if wrong.
- Ruling R1: lexer (T2) treats \r\n as one line end and never puts \r in CommentLine.text — spec Review Focus 1 — costs a T3 fix round if wrong.
- Ruling R2: tag must be followed by `(`, `:`, whitespace or end of line (prose wins over the sample regex) — spec section 3 sense; `TODO-x` is not a TODO — costs a missed odd TODO if wrong.
- Ruling R3: T4 adds `pub fn scan_with_threads(repo: &Repo, threads: usize) -> anyhow::Result<ScanResult>`; `scan` calls it with available_parallelism — makes the plan's proptest possible — costs one extra pub fn.
- Ruling R4: T6 runs `git -c core.quotePath=false diff -U0 -M --no-color [--cached] <mb>` and parses `+++ b/<path>`, unquoting C-style quoted paths; no `-z` — `-z` only affects raw/numstat — Review Focus 5 — costs a fix round if wrong.
- Ruling R5: T6 default (working-tree) mode also counts every line of untracked, not-ignored files as added; `--staged` does not — a brand-new file must not escape check — costs a false failure on scratch files the user hasn't added (they can gitignore them).
- Ruling R6: single-line quote kinds ("…", '…' in languages where they can't span lines unescaped) end at an unescaped newline; triple-quoted, raw, backtick, verbatim strings and heredocs still run to EOF when unterminated — spec goal is no missed TODOs and no false ones; one stray apostrophe (JSX `Don't`) must not hide a file — costs a misread for languages that allow raw newlines in ordinary quotes (shell, Ruby, PHP, SQL, YAML keep multi-line; see fix).
- Ruling R7: reviewer's ❌ on `#[allow(dead_code)]` for scan_source is wrong — it has no caller until Task 6, so the allow stays until then — costs nothing.
- Ruling R8: `scan_source` applies the same path skip rules (vendor dirs, lockfiles, *.min.*, config ignore) and returns nothing for skipped paths — check must never count TODOs list hides (load-bearing for T6) — costs one extra check per file.
- Ruling R9: Fixture isolates git config (GIT_CONFIG_GLOBAL=/dev/null, GIT_CONFIG_NOSYSTEM=1) and a committed-file test is added — T5–T7 build on the fixture; removing --cached must fail a test — costs nothing.
- Ruling R10: `print!` broken-pipe panic (`td2do list | head`) is fixed in Task 6 with a shared stdout writer that ignores BrokenPipe — piped output is the main use; T6 also prints — costs nothing.
- Ruling R11: untracked-file line counting must not follow symlinks or read files over 2 MB / special files (an untracked symlink to /dev/zero hangs check) — a hang in CI is worse than a missed file; scan skips them anyway — costs nothing.
- Ruling R12: index reads use `:0:<path>` — `1:x.rs` is otherwise read as a stage — costs nothing.
- Ruling R13: unborn HEAD diffs against the empty tree (whole index/working tree is new); default_base tries origin/HEAD, origin/main, origin/master, main, master; init writes the base it found unchanged (keeps origin/) only if it resolves, else leaves base commented out — first commit after init must not be blocked — costs a differently-written config line.
- Ruling R14: diff passes --text — the scan's NUL probe already skips real binaries — costs nothing.
- Ruling R15: --tag compares with eq_ignore_ascii_case, folded into the fix wave — one line — costs nothing.

## Follow-ups (minor findings, none blocks merge)

Closed as resolved or moot by the final review: BOM on line 1, empty trailing comment line, core.hooksPath (rev-parse --git-path honours it), invalid glob in scan_source (unreachable), missing RED evidence in Task 1.

- **Task 1:** ci.yml `rustup show` may not install the pinned toolchain on rustup ≥1.28 — use `rustup toolchain install`.
- **Task 1:** checkout pinned to v4 (latest release is v7.0.1); comment names a moving tag.
- **Task 1:** `-C <missing dir>` silently uses defaults and exits 0.
- **Task 1:** `td2do --json` (no subcommand) is a usage error.
- **Task 2:** language rules keyed on name strings (lang.rs:875-924, lexer.rs:1311,1318).
- **Task 2:** TOML ''' strings treated as escaping.
- **Task 2:** shell here-string <<< misread; CSS url(//cdn) read as comment; Dockerfile.dev / lowercase makefile not mapped; Lua level≥3 brackets, Rust r####.
- **Task 2:** lone \r dropped from text; unterminated block emits trailing empty CommentLine (T3 must ignore empty lines).
- **Task 3:** `TODO (x)` with a space before `(` is taken as TAG(inner) — prose in parens leaves text/fingerprint; `TODO (note: slow)` → Unknown ref.
- **Task 3:** nested parens `TODO(foo (bar)): x` close at first `)`.
- **Task 3:** gh#0 accepted.
- **Task 3:** normalize lacks `>` so `-->` may survive into fingerprint (check in Task 4).
- **Task 4:** scan_file swallows PermissionDenied silently; has_tag error → no tag.
- **Task 4:** PathBuf component order differs from git's bytewise order.
- **Task 4:** config ignore globs: `*` crosses `/`; bare `docs` doesn't match nested.
- **Task 4:** same_result_any_threads runs 256 cases over 7 values (slow).
- **Task 4:** outside_git_walks fails if TMPDIR is inside a git work tree.
- **Task 4:** mod support declared via #[path] inside scan.rs.
- **Task 4:** redundant git::root call in scan_with_threads; empty-tags early return reports skipped_unknown_lang 0; duplicate config load in main (T5 fixes).
- **Task 4:** production git:: calls in scan tests still read user git config.
- **Task 5:** --path glob compiled after full scan; --ref exact string match undocumented; weak assertions in list_untracked_filter/list_ref_filter/list_json_schema; --path `*` crosses `/` undocumented.
- **Task 6:** unborn HEAD gives the misleading fetch-depth message.
- **Task 6:** --staged DuplicateText `other` may come from an unstaged/untracked file.
- **Task 6:** --all --staged needs a resolvable base; --all duplicate pairs reported both ways.
- **Task 6:** untracked_symlink_is_not_read_through can't fail on old code (guard only).
- **Task 7:** core.hooksPath ignored → hook may never run; init runs after scan::open so bad config blocks init; base_ref interpolated in run: (use env:); generated workflow lacks token wiring and cargo cache; no test for declined→skipped JSON or --json prompt purity or linked worktree.
- **Final review:** a plain `mv` rename (not `git mv`) makes every TODO in the file new in default mode (document next to R5); a merge in the hook ignores MERGE_HEAD; `check --json` names a base under `--all` and before the first commit; error paths print no JSON; `--verbose` (count of unknown-language files) not built; no scanner-level fuzz test or CLI CRLF test for `check`.
