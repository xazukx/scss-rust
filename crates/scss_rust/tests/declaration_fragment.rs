//! Tests for parsing and compiling *stylesheet fragments* — SCSS that consists
//! of bare property declarations without an enclosing selector block.
//!
//! The parser opts in via
//! [`StylesheetParser::parse_allowing_declarations`] and the visitor via
//! [`Visitor::visit_stylesheet_allowing_declarations`]. Both are strictly
//! additive: the default [`StylesheetParser::parse`] /
//! [`Visitor::visit_stylesheet`] entry points reject top-level declarations
//! exactly as before.

use std::path::PathBuf;
use std::sync::Arc;

use scss_rust::{
    codemap::{CodeMap, LineCol},
    sass_ast::AstStmt,
    serializer::StyleSerializer,
    Lexer, Options, OutputStyle, ScssParser, StyleSheet, StylesheetParser, Visitor,
};

/// Parse `src` as a fragment (declarations allowed at the top level), returning
/// the parsed sheet together with the `CodeMap` its spans point into so callers
/// can resolve positions.
fn parse_fragment(src: &str) -> (StyleSheet, CodeMap) {
    let options = Options::default();
    let mut map = CodeMap::new();
    let path = PathBuf::from("frag.scss");
    let file = map.add_file(
        Arc::new(path.to_string_lossy().into_owned()),
        Arc::new(src.to_owned()),
    );
    let empty_span = file.span.subspan(0, 0);
    let lexer = Lexer::new_from_file(&file);

    let sheet = ScssParser::new(lexer, &options, empty_span, &path)
        .parse_allowing_declarations()
        .expect("fragment should parse");
    (sheet, map)
}

/// Compile `src` as a fragment with the given output style, returning the CSS.
fn compile_fragment_with(src: &str, style: OutputStyle) -> Result<String, String> {
    let options = Options::default().style(style);

    let mut map = CodeMap::new();
    let path = PathBuf::from("frag.scss");
    let file = map.add_file(
        Arc::new(path.to_string_lossy().into_owned()),
        Arc::new(src.to_owned()),
    );
    let empty_span = file.span.subspan(0, 0);
    let lexer = Lexer::new_from_file(&file);

    let sheet = ScssParser::new(lexer, &options, empty_span, &path)
        .parse_allowing_declarations()
        .map_err(|e| format!("parse: {:?}", e))?;

    let mut visitor = Visitor::new(&path, &options, &mut map, empty_span);
    visitor
        .visit_stylesheet_allowing_declarations(sheet)
        .map_err(|e| format!("visit: {:?}", e))?;
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
            .map_err(|e| format!("serialize: {:?}", e))?;
        prev_was_group_end = is_group_end;
        prev_requires_semicolon = requires_semicolon;
    }
    Ok(serializer.finish(prev_requires_semicolon))
}

fn compile_fragment(src: &str) -> String {
    compile_fragment_with(src, OutputStyle::Expanded).expect("fragment should compile")
}

/// 0-indexed (line, column) of a statement's start, resolved against `map`.
fn begin_of(stmt: &AstStmt, map: &CodeMap) -> LineCol {
    let span = match stmt {
        AstStmt::Style(s) => s.span,
        AstStmt::RuleSet(r) => r.span,
        AstStmt::VariableDecl(v) => v.span,
        other => panic!("unexpected statement kind: {other:?}"),
    };
    map.look_up_span(span).begin
}

// ---------------------------------------------------------------------------
// Compilation
// ---------------------------------------------------------------------------

#[test]
fn compiles_plain_bare_declarations() {
    assert_eq!(
        compile_fragment("border: 1px solid black;\ncolor: red;\n"),
        "border: 1px solid black;\ncolor: red;\n"
    );
}

#[test]
fn resolves_variables_in_bare_declarations() {
    assert_eq!(
        compile_fragment("$c: black;\nborder: 1px solid $c;\ncolor: red;\n"),
        "border: 1px solid black;\ncolor: red;\n"
    );
}

#[test]
fn custom_property_at_root() {
    assert_eq!(
        compile_fragment("--foo: bar;\ncolor: red;\n"),
        "--foo: bar;\ncolor: red;\n"
    );
}

#[test]
fn nested_declaration_at_root() {
    // `font: { family: ...; weight: ...; }` expands to hyphenated properties,
    // even though there is no enclosing style rule.
    assert_eq!(
        compile_fragment("font: {\n  family: serif;\n  weight: bold;\n}\n"),
        "font-family: serif;\nfont-weight: bold;\n"
    );
}

#[test]
fn mixed_declarations_and_style_rule() {
    assert_eq!(
        compile_fragment("margin: 0;\ndiv {\n  color: red;\n}\npadding: 4px;\n"),
        "margin: 0;\ndiv {\n  color: red;\n}\n\npadding: 4px;\n"
    );
}

#[test]
fn evaluates_calc_and_arithmetic() {
    assert_eq!(
        compile_fragment("$w: 8px;\nwidth: calc($w * 2);\n"),
        "width: 16px;\n"
    );
}

#[test]
fn compressed_output() {
    // Compressed mode still minifies values (`black` -> `#000`); the trailing
    // `;` is the root-level flush (there is no closing `}` to elide it).
    assert_eq!(
        compile_fragment_with(
            "border: 1px solid black;\ncolor: red;\n",
            OutputStyle::Compressed
        )
        .unwrap(),
        "border:1px solid #000;color:red;"
    );
}

#[test]
fn selector_form_still_compiles_under_fragment_mode() {
    // A normal style rule parses and compiles identically whether or not
    // declarations are permitted at the top level.
    assert_eq!(
        compile_fragment("div {\n\tborder: 1px solid black;\n}\n"),
        "div {\n  border: 1px solid black;\n}\n"
    );
}

// ---------------------------------------------------------------------------
// Positions / line numbers are preserved
// ---------------------------------------------------------------------------

#[test]
fn positions_preserved_for_bare_declarations() {
    let src = "border: 1px solid black;\ncolor: red;\n";
    let (sheet, map) = parse_fragment(src);
    assert_eq!(sheet.body.len(), 2);

    // First declaration starts at line 1, column 1 (0-indexed 0, 0).
    assert_eq!(
        begin_of(&sheet.body[0], &map),
        LineCol { line: 0, column: 0 }
    );
    // Second declaration starts at line 2, column 1.
    assert_eq!(
        begin_of(&sheet.body[1], &map),
        LineCol { line: 1, column: 0 }
    );
}

#[test]
fn inner_declaration_position_matches_selector_form() {
    // A bare declaration on line 1 sits at column 1; the same declaration nested
    // one tab deep in a selector sits on line 2 at column 2. Line/column
    // bookkeeping is identical in both cases, differing only by the tab indent.
    let bare = "border: 1px solid black;\n";
    let (bare_sheet, bare_map) = parse_fragment(bare);
    assert_eq!(
        begin_of(&bare_sheet.body[0], &bare_map),
        LineCol { line: 0, column: 0 }
    );

    let nested = "div {\n\tborder: 1px solid black;\n}\n";
    let (nested_sheet, nested_map) = parse_fragment(nested);
    let AstStmt::RuleSet(rule) = &nested_sheet.body[0] else {
        panic!("expected a style rule");
    };
    // Inner declaration: line 2, after one tab -> column index 1.
    assert_eq!(
        begin_of(&rule.body[0], &nested_map),
        LineCol { line: 1, column: 1 }
    );
}

// ---------------------------------------------------------------------------
// Regressions: the default entry points still reject bare declarations
// ---------------------------------------------------------------------------

#[test]
fn default_parse_rejects_bare_declaration() {
    let options = Options::default();
    let mut map = CodeMap::new();
    let path = PathBuf::from("frag.scss");
    let src = "border: 1px solid black;\n";
    let file = map.add_file(
        Arc::new(path.to_string_lossy().into_owned()),
        Arc::new(src.to_owned()),
    );
    let empty_span = file.span.subspan(0, 0);
    let lexer = Lexer::new_from_file(&file);

    let err = ScssParser::new(lexer, &options, empty_span, &path)
        .parse()
        .unwrap_err();
    assert!(
        format!("{:?}", err).contains("expected \\\"{\\\"")
            || format!("{:?}", err).contains("expected \"{\""),
        "unexpected error: {:?}",
        err
    );
}

#[test]
fn default_visit_rejects_bare_declaration() {
    // Even when parsed as a fragment, the *default* visitor still enforces the
    // "declarations must be inside a style rule" rule.
    let options = Options::default();
    let mut map = CodeMap::new();
    let path = PathBuf::from("frag.scss");
    let src = "border: 1px solid black;\n";
    let file = map.add_file(
        Arc::new(path.to_string_lossy().into_owned()),
        Arc::new(src.to_owned()),
    );
    let empty_span = file.span.subspan(0, 0);
    let lexer = Lexer::new_from_file(&file);
    let sheet = ScssParser::new(lexer, &options, empty_span, &path)
        .parse_allowing_declarations()
        .unwrap();

    let mut visitor = Visitor::new(&path, &options, &mut map, empty_span);
    let err = visitor.visit_stylesheet(sheet).unwrap_err();
    assert!(
        format!("{:?}", err).contains("Declarations may only be used within style rules"),
        "unexpected error: {:?}",
        err
    );
}
