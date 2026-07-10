//! Parsing and compiling SCSS **fragments** — bare property declarations that
//! are *not* wrapped in a selector block.
//!
//! Ordinarily a declaration such as `border: 1px solid black;` may only appear
//! inside a style rule; at the top level of a stylesheet the parser expects a
//! selector and a `{`, and the evaluator refuses a declaration that has no
//! enclosing rule. That is the correct behaviour for a whole stylesheet, but it
//! gets in the way when the thing you are handed is only the *inside* of a rule
//! — a snippet from an editor, a CSS-in-JS style object, a template partial, and
//! so on.
//!
//! Two opt-in entry points lift that restriction without changing any default:
//!
//!  - [`StylesheetParser::parse_allowing_declarations`] parses the document as
//!    though it were the body of a style rule, so bare declarations, style
//!    rules, and variable declarations are all accepted at the top level. Every
//!    span/line/column is identical to what you would get for the same content
//!    nested inside a selector.
//!  - [`Visitor::visit_stylesheet_allowing_declarations`] evaluates such a
//!    fragment, emitting the top-level declarations directly at the root of the
//!    output instead of raising "Declarations may only be used within style
//!    rules.".
//!
//! The default [`StylesheetParser::parse`] and [`Visitor::visit_stylesheet`]
//! are untouched and still reject top-level declarations.
//!
//! Run with:
//! ```bash
//! cargo run -p scss_rust --example declaration_fragment
//! ```

use std::path::{Path, PathBuf};
use std::sync::Arc;

use scss_rust::{
    codemap::{CodeMap, SpanLoc},
    sass_ast::AstStmt,
    serializer::StyleSerializer,
    Lexer, Options, OutputStyle, ScssParser, StyleSheet, StylesheetParser, Visitor,
};

/// The two inputs we care about: a conventional style rule, and just its body.
const WITH_SELECTOR: &str = "div {\n\tborder: 1px solid black;\n\tcolor: cyan;\n}\n";
const BARE_DECLARATIONS: &str = "border: 1px solid black;\ncolor: cyan;\n";

/// A fragment that also exercises variables, arithmetic, and a nested style
/// rule sitting next to bare declarations.
const RICH_FRAGMENT: &str = "\
$accent: #08c;
margin: 0;
padding: calc(4px * 2);
color: $accent;

a {
\tcolor: $accent;
\ttext-decoration: none;
}
";

/// Register `src` in a fresh `CodeMap` and parse it, allowing top-level
/// declarations. Returns the sheet plus the map its spans resolve against.
fn parse_fragment(src: &str, path: &Path) -> (StyleSheet, CodeMap) {
    let options = Options::default();
    let mut map = CodeMap::new();
    let file = map.add_file(
        Arc::new(path.to_string_lossy().into_owned()),
        Arc::new(src.to_owned()),
    );
    let empty_span = file.span.subspan(0, 0);
    let lexer = Lexer::new_from_file(&file);

    let sheet = ScssParser::new(lexer, &options, empty_span, path)
        .parse_allowing_declarations()
        .expect("fragment should parse");
    (sheet, map)
}

/// Print each top-level statement's kind and its 1-based line:column range,
/// recursing into style-rule and nested-declaration bodies.
fn print_positions(body: &[AstStmt], map: &CodeMap, depth: usize) {
    for stmt in body {
        let (kind, span, children): (&str, _, &[AstStmt]) = match stmt {
            AstStmt::Style(s) => ("declaration", Some(s.span), &s.body),
            AstStmt::RuleSet(r) => ("style rule ", Some(r.span), &r.body),
            AstStmt::VariableDecl(v) => ("variable   ", Some(v.span), &[]),
            _ => ("other      ", None, &[]),
        };

        let indent = "    ".repeat(depth + 1);
        if let Some(span) = span {
            let loc: SpanLoc = map.look_up_span(span);
            let snippet = loc.file.source_line(loc.begin.line);
            println!(
                "{indent}{kind}  {}:{:<2} -> {}:{:<2}  {:?}",
                loc.begin.line + 1,
                loc.begin.column + 1,
                loc.end.line + 1,
                loc.end.column + 1,
                snippet.trim_end(),
            );
        }
        print_positions(children, map, depth + 1);
    }
}

/// Compile a fragment to CSS via the declaration-allowing visitor.
fn compile_fragment(src: &str, path: &Path) -> String {
    let options = Options::default().style(OutputStyle::Expanded);
    let mut map = CodeMap::new();
    let file = map.add_file(
        Arc::new(path.to_string_lossy().into_owned()),
        Arc::new(src.to_owned()),
    );
    let empty_span = file.span.subspan(0, 0);
    let lexer = Lexer::new_from_file(&file);

    let sheet = ScssParser::new(lexer, &options, empty_span, path)
        .parse_allowing_declarations()
        .expect("parse");

    let mut visitor = Visitor::new(path, &options, &mut map, empty_span);
    visitor
        .visit_stylesheet_allowing_declarations(sheet)
        .expect("visit");
    let stmts = visitor.finish();
    drop(visitor);

    let mut serializer = StyleSerializer::new(&options, &map, false, empty_span);
    let mut prev_was_group_end = false;
    let mut prev_requires_semicolon = false;
    for sm in stmts {
        if sm.is_invisible() {
            continue;
        }
        let is_group_end = sm.is_group_end();
        let requires_semicolon = StyleSerializer::requires_semicolon(&sm);
        serializer
            .visit_group(sm.stmt, prev_was_group_end, prev_requires_semicolon)
            .expect("serialize");
        prev_was_group_end = is_group_end;
        prev_requires_semicolon = requires_semicolon;
    }
    serializer.finish(prev_requires_semicolon)
}

fn main() {
    let path = PathBuf::from("fragment.scss");

    // ---- 1. The same declarations, with and without a selector ------------
    //
    // Both inputs parse. The declarations land at the same columns; the only
    // difference is that the nested ones are indented one tab (line 2, col 2)
    // and sit under a `style rule`, while the bare ones start at column 1.
    println!("=== with selector ===");
    println!("{WITH_SELECTOR}");
    let (with_sel, map) = parse_fragment(WITH_SELECTOR, &path);
    print_positions(&with_sel.body, &map, 0);

    println!("\n=== bare declarations (no selector) ===");
    println!("{BARE_DECLARATIONS}");
    let (bare, map) = parse_fragment(BARE_DECLARATIONS, &path);
    print_positions(&bare.body, &map, 0);

    // The `border` declaration is on line 1 in the bare form and line 2 in the
    // selector form; both preserve exact positions.
    let bare_border = map.look_up_span(match &bare.body[0] {
        AstStmt::Style(s) => s.span,
        _ => unreachable!(),
    });
    assert_eq!(bare_border.begin.line + 1, 1);
    assert_eq!(bare_border.begin.column + 1, 1);

    // ---- 2. The default parser still rejects a bare declaration -----------
    {
        let options = Options::default();
        let mut map = CodeMap::new();
        let file = map.add_file(
            Arc::new(path.to_string_lossy().into_owned()),
            Arc::new(BARE_DECLARATIONS.to_owned()),
        );
        let empty_span = file.span.subspan(0, 0);
        let lexer = Lexer::new_from_file(&file);
        match ScssParser::new(lexer, &options, empty_span, &path).parse() {
            Ok(_) => panic!("default parse unexpectedly accepted a bare declaration"),
            Err(e) => {
                let (msg, _) = e.raw();
                println!("\n=== default parse() still rejects the bare form ===");
                println!("  error: {}", msg.lines().next().unwrap_or(&msg));
            }
        }
    }

    // ---- 3. Compiling a richer fragment to CSS ----------------------------
    println!("\n=== compiled fragment ===");
    println!("--- input ---\n{RICH_FRAGMENT}");
    let css = compile_fragment(RICH_FRAGMENT, &path);
    println!("--- output ---\n{css}");
}
