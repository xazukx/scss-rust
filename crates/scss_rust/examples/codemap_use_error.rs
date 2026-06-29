//! Multi-file `CodeMap` + `@use` error alignment.
//!
//! This example builds a [`CodeMap`] that ends up holding *several* SCSS files and
//! drives a compilation where one file pulls in another with an `@use` rule. The
//! `@use`d file contains a mistake — its URL resolves to a default namespace that
//! is not a valid Sass identifier — so a parse error is raised while that second
//! file is being read.
//!
//! ## The subtle problem this demonstrates
//!
//! A `CodeMap` stores every file in a single, shared coordinate space, *as if all
//! of the sources had been concatenated into one giant buffer*. The first file
//! occupies bytes `[1, N]`, the next `[N + 1, M]`, and so on. A [`Span`] is just a
//! pair of offsets into that global space — it carries no explicit file identity.
//!
//! That design is compact and fast, but it is easy to construct a span relative to
//! the *wrong* base offset. If an error span is accidentally anchored to byte `0`
//! of a file (or to the start of the whole map) instead of to the construct that
//! actually failed, the reported line/column silently drifts: the map happily
//! resolves the bogus offset to *some* real location, just not the right one. When
//! several files are concatenated, an over-long span can even bleed past the end of
//! its own file into the next one.
//!
//! That is exactly the bug this example used to expose: the `@use` namespace error
//! was anchored to offset `0` of the file rather than to the `@use` rule, so it
//! pointed at line 1 (`/* File 2 */`) instead of line 2 (the actual `@use`).
//!
//! Two changes keep error locations honest:
//!   1. The parser now anchors the namespace error to the `@use` rule's own start
//!      cursor, so the highlighted span is the whole `@use "..."` statement.
//!   2. [`CodeMap::look_up_span`] clamps a span into the single file that contains
//!      its start, so a malformed span can never resolve into a neighbouring file
//!      or panic — errors are always aligned to the correct file's line numbering.
//!
//! Run with:
//! ```bash
//! cargo run -p scss_rust --example codemap_use_error
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use scss_rust::{
    codemap::{CodeMap, SpanLoc},
    Fs, Lexer, Options, ScssParser, StylesheetParser, Visitor,
};

/// A trivial in-memory file system so `@use` can resolve files without touching
/// the real disk.
#[derive(Debug)]
struct MemoryFs {
    files: BTreeMap<PathBuf, String>,
}

impl MemoryFs {
    fn new() -> Self {
        Self {
            files: BTreeMap::new(),
        }
    }

    fn add(&mut self, name: &str, contents: &str) {
        self.files.insert(PathBuf::from(name), contents.to_string());
    }
}

impl Fs for MemoryFs {
    fn is_file(&self, path: &Path) -> bool {
        self.files.contains_key(path)
    }

    fn is_dir(&self, _path: &Path) -> bool {
        false
    }

    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        self.files
            .get(path)
            .map(|c| c.as_bytes().to_vec())
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no such file"))
    }
}

// File 1 — the entry point. It is valid on its own; its only job is to pull in
// File 2 via `@use`, which is what forces File 2 into the same `CodeMap`.
const FILE_1: &str = r#"/* File 1 */
@use "file2";
"#;

// File 2 — the file with the mistake. `@use "New Style Comp"` derives the default
// namespace from the file name, but "New Style Comp" contains spaces and so is not
// a valid Sass identifier. The error must point at line 2, column 1.
const FILE_2: &str = r#"/* File 2 */
@use "New Style Comp"; // an error will occur

page {
  border: 1px solid black;
  color: cyan;
  background: grey;
  width: 100px;
  height: 100px;
  & > * {
    background: #cc0000aa;
    // height: 20px;
    // width: 50%;
    border-bottom: 2px dashed white;
  }
}
"#;

fn main() {
    // The component referenced by File 2. It is a perfectly valid file; the error
    // is purely about the *namespace* derived from its quoted URL, so this content
    // is never even reached. It lives in the FS to make the scenario realistic.
    let new_style_comp = "/* New Style Comp */\n.button { color: hotpink; }\n";

    let mut fs = MemoryFs::new();
    fs.add("file2.scss", FILE_2);
    fs.add("New Style Comp.scss", new_style_comp);

    let options = Options::default().fs(&fs);

    // Build a CodeMap and seed it with File 1 (the entry point). Loading File 2 via
    // `@use` will append it to this same map, giving us a multi-file CodeMap.
    let mut map = CodeMap::new();
    let entry_path = PathBuf::from("file1.scss");
    let entry_file = map.add_file(
        Arc::new(entry_path.to_string_lossy().into_owned()),
        Arc::new(FILE_1.to_owned()),
    );
    let empty_span = entry_file.span.subspan(0, 0);

    // Parse and then visit File 1. Visiting evaluates the `@use "file2"` rule,
    // which reads File 2 from the FS, adds it to the CodeMap, and parses it — and
    // it is *that* parse that fails on the invalid namespace.
    let lexer = Lexer::new_from_file(&entry_file);
    let stylesheet = match ScssParser::new(lexer, &options, empty_span, &entry_path).parse() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("unexpected error parsing the entry file: {:?}", e);
            return;
        }
    };

    let mut visitor = Visitor::new(&entry_path, &options, &mut map, empty_span);
    let result = visitor.visit_stylesheet(stylesheet);
    drop(visitor);

    let error = match result {
        Ok(_) => {
            println!("Expected a compilation error, but compilation succeeded.");
            return;
        }
        Err(e) => e,
    };

    // Show how the CodeMap laid the files out: a single, contiguous global
    // coordinate space. File 2 does *not* start at offset 0 — that is the whole
    // reason the start/end of an error span has to be computed carefully.
    println!("CodeMap layout ({} files):", map.files.len());
    for file in &map.files {
        println!(
            "  {:<22} global bytes [{:>4}, {:>4}]  ({} lines)",
            file.name(),
            *file.span.low(),
            *file.span.high(),
            file.num_lines(),
        );
    }
    println!();

    // The error raised during visiting is a *raw* error: a message plus a span in
    // the CodeMap's global coordinate space. A raw error has no location attached
    // yet, so we resolve its span against the map to get a file-relative line and
    // column. This resolution step is where alignment matters.
    if !error.is_raw() {
        // Any non-raw error (e.g. I/O) just prints its message directly.
        print!("{}", error);
        return;
    }

    let (message, span) = error.raw();
    let loc: SpanLoc = map.look_up_span(span);

    println!("Resolved error location:");
    println!("  message : {}", message.lines().next().unwrap_or(&message));
    println!("  file    : {}", loc.file.name());
    // `LineCol` is 0-indexed internally; +1 to match editor/dart-sass numbers.
    println!(
        "  start   : line {}, column {}",
        loc.begin.line + 1,
        loc.begin.column + 1
    );
    println!(
        "  end     : line {}, column {}",
        loc.end.line + 1,
        loc.end.column + 1
    );
    println!("  snippet : {:?}", loc.file.source_line(loc.begin.line));
    println!();

    // Sanity-check the alignment: the offending `@use` sits on line 2 of File 2.
    assert_eq!(
        loc.file.name(),
        "file2.scss",
        "error attributed to wrong file"
    );
    assert_eq!(loc.begin.line + 1, 2, "error attributed to wrong line");
    assert_eq!(loc.begin.column + 1, 1, "error attributed to wrong column");
    println!("Alignment verified: error points at file2.scss:2:1 (the `@use` rule).\n");

    // Finally, the human-facing rendering. A raw error only knows its message, so we
    // promote it to a located error using the span we just resolved — the same step
    // the library performs internally before showing an error to the user. Its caret
    // underlines the entire `@use "New Style Comp"` statement on line 2, not line 1.
    let unicode = true;
    let rendered = scss_rust::Error::from_loc(message, loc, unicode);
    println!("Rendered diagnostic:\n");
    print!("{}", rendered);
}
