//! Multi-file `CodeMap` + `@use`, with file-local error spans.
//!
//! This example builds a [`CodeMap`] that ends up holding *several* SCSS files and
//! drives a compilation where one file pulls in another with an `@use` rule. The
//! `@use`d file contains a mistake — its URL resolves to a default namespace that
//! is not a valid Sass identifier — so a parse error is raised while that second
//! file is being read.
//!
//! ## Why error spans are now file-local
//!
//! Earlier, a `CodeMap` stored every file in a single, shared coordinate space, as
//! if all the sources had been concatenated into one giant buffer: the first file
//! occupied bytes `[1, N]`, the next `[N + 1, M]`, and so on. A `Span` was just a
//! pair of offsets into that global buffer with no file identity, so turning one
//! back into a `file:line:column` meant binary-searching the map and subtracting
//! the file's base offset. That coupling was fragile — a span built against the
//! wrong base silently resolved to the wrong line, and an over-long span could
//! bleed past the end of its file into the next one.
//!
//! Now each file owns its **own** 0-based coordinate space, and every [`Span`]
//! carries the [`FileId`](scss_rust::codemap) of the file it points into. A span's
//! `low`/`high` are therefore the byte offsets a user would count in that one file
//! — no subtraction, no global layout to reason about. The `CodeMap` is reduced to
//! a registry that maps a file id back to its source for rendering; it never does
//! cross-file offset arithmetic, so a location can't drift between files no matter
//! what order files were added.
//!
//! Run with:
//! ```bash
//! cargo run -p scss_rust --example codemap_use_error
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use scss_rust::{
    codemap::{CodeMap, LineCol, SpanLoc},
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

    // Register every file with the in-memory FS, File 1 first.
    let mut fs = MemoryFs::new();
    fs.add("file1.scss", FILE_1);
    fs.add("file2.scss", FILE_2);
    fs.add("New Style Comp.scss", new_style_comp);

    let options = Options::default().fs(&fs);

    // Build a CodeMap and seed it with File 1 (the entry point). Loading File 2 via
    // `@use` will register it in this same map, giving us a multi-file CodeMap.
    let mut map = CodeMap::new();
    let entry_path = PathBuf::from("file1.scss");
    let entry_file = map.add_file(
        Arc::new(entry_path.to_string_lossy().into_owned()),
        Arc::new(FILE_1.to_owned()),
    );
    let empty_span = entry_file.span.subspan(0, 0);

    // Parse and then visit File 1. Visiting evaluates the `@use "file2"` rule,
    // which reads File 2 from the FS, registers it in the CodeMap, and parses it —
    // and it is *that* parse that fails on the invalid namespace.
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

    // Show the CodeMap's contents. Each file is its *own* 0-based coordinate space —
    // there is no global layout, so a file's id (its registry index) says nothing
    // about its byte offsets.
    println!("CodeMap registry ({} files):", map.files.len());
    for file in &map.files {
        println!(
            "  [id {}] {:<22} bytes 0..{}  ({} lines)",
            file.span.file().index(),
            file.name(),
            *file.span.high(),
            file.num_lines(),
        );
    }
    println!();

    // The error raised during visiting is a *raw* error: a message plus a span.
    // The span already names its file (via its FileId) and its offsets are
    // file-relative, so resolving it is a direct registry lookup — no subtraction.
    if !error.is_raw() {
        // Any non-raw error (e.g. I/O) just prints its message directly.
        print!("{}", error);
        return;
    }

    let (message, span) = error.raw();
    let loc: SpanLoc = map.look_up_span(span);

    println!("Resolved error location:");
    println!(
        "  message    : {}",
        message.lines().next().unwrap_or(&message)
    );
    println!(
        "  file        : {} (id {})",
        loc.file.name(),
        span.file().index()
    );
    // `span.low()/high()` ARE the file-relative byte offsets — nothing is subtracted.
    println!("  byte range  : {}..{}", *span.low(), *span.high());
    // `LineCol` is 0-indexed internally; +1 to match editor/dart-sass numbers.
    println!(
        "  start       : line {}, column {}",
        loc.begin.line + 1,
        loc.begin.column + 1
    );
    println!(
        "  end         : line {}, column {}",
        loc.end.line + 1,
        loc.end.column + 1
    );
    println!("  snippet     : {:?}", loc.file.source_line(loc.begin.line));
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
    println!();

    // ---- The error's span needs no knowledge of the CodeMap -------------------
    //
    // The rendered value is a `ParseError` carrying a resolved `loc`. Because the
    // span's offsets are already file-relative, "is this position inside the file?"
    // is just `0 <= low <= high <= len` — no rebasing against any global layout.
    match rendered.kind() {
        scss_rust::ErrorKind::ParseError { loc, .. } => {
            let source_len = loc.file.source().len();
            let start_pos = *span.low() as usize;
            let end_pos = *span.high() as usize;

            assert!(
                start_pos <= end_pos && end_pos <= source_len,
                "error span {}..{} escapes {} (len {})",
                start_pos,
                end_pos,
                loc.file.name(),
                source_len
            );
            assert!(
                loc.begin.line < loc.file.num_lines(),
                "begin line out of range"
            );
            assert!(loc.end.line < loc.file.num_lines(), "end line out of range");

            println!(
                "Bounds verified: {} is {} bytes; error span occupies file-relative bytes {}..{}.",
                loc.file.name(),
                source_len,
                start_pos,
                end_pos
            );
        }
        other => panic!("expected a ParseError, got {:?}", other),
    }

    // ---- Order independence is structural, not coincidental --------------------
    //
    // Build a fresh CodeMap that registers File 2 *before* File 1. File 2 now has a
    // different id, but its coordinate space is unchanged (still 0-based), so the
    // exact same file-relative span — the `@use` rule at bytes 13..34 — resolves to
    // the identical line and column.
    let mut reversed = CodeMap::new();
    let file2_first = reversed.add_file(
        Arc::new("file2.scss".to_owned()),
        Arc::new(FILE_2.to_owned()),
    );
    reversed.add_file(
        Arc::new("file1.scss".to_owned()),
        Arc::new(FILE_1.to_owned()),
    );

    let reordered_loc = reversed.look_up_span(file2_first.span.subspan(13, 34));
    assert_eq!(reordered_loc.file.name(), "file2.scss");
    assert_eq!(reordered_loc.begin, LineCol { line: 1, column: 0 });
    assert_eq!(
        reordered_loc.end,
        LineCol {
            line: 1,
            column: 21
        }
    );
    println!(
        "Reversed-order map: file2.scss @ bytes 13..34 still resolves to {}:{}–{}:{}.",
        reordered_loc.begin.line + 1,
        reordered_loc.begin.column + 1,
        reordered_loc.end.line + 1,
        reordered_loc.end.column + 1,
    );
}
