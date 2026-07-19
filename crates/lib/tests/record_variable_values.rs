//! Tests for `Options::record_variable_values` / `Visitor::variable_values`.
//!
//! These exercise the low-level `Visitor` API directly, because the recorded
//! values live on the `Visitor` and are meant to be inspected after
//! `visit_stylesheet` finishes — the use case being tooling (symbol collection,
//! language servers) rather than plain compilation.

use std::path::Path;
use std::sync::Arc;

use macros::TestFs;
use scss_rust::codemap::{CodeMap, Span};
use scss_rust::{Lexer, Options, ScssParser, StylesheetParser, Visitor};

#[macro_use]
mod macros;

fn opts() -> Options<'static> {
    Options::default().record_variable_values(true)
}

/// Compile `src` and hand the resulting `Visitor` (plus its empty span) to `f`.
fn with_visitor<T>(src: &str, options: &Options, f: impl FnOnce(&Visitor, Span) -> T) -> T {
    let path = Path::new("test.scss");
    let mut map = CodeMap::new();
    let file = map.add_file(
        Arc::new(path.to_string_lossy().into_owned()),
        Arc::new(src.to_owned()),
    );
    let empty_span = file.span.subspan(0, 0);
    let lexer = Lexer::new_from_file(&file);
    let sheet = ScssParser::new(lexer, options, empty_span, path)
        .parse()
        .expect("parse");

    let mut visitor = Visitor::new(path, options, &mut map, empty_span);
    visitor.visit_stylesheet(sheet).expect("visit");
    f(&visitor, empty_span)
}

/// A fully-materialized recorded entry, for exhaustive assertions on every
/// property of every recorded variable. `ancestry` is flattened to one
/// `"<kind> <name>"` string per caller frame (outermost first).
#[derive(Debug, PartialEq, Eq)]
struct Entry {
    name: String,
    value: String,
    selector: Option<String>,
    ancestry: Vec<String>,
}

/// All recorded entries, flattened across spans (span/source order) and repeated
/// evaluations (evaluation order within a span). Every property is captured.
fn entries(src: &str, options: &Options) -> Vec<Entry> {
    with_visitor(src, options, |visitor, span| {
        visitor
            .variable_values
            .values()
            .flatten()
            .map(|r| Entry {
                name: r.name.to_string(),
                value: r.value.inspect(span).unwrap(),
                selector: r.selector.clone(),
                ancestry: r
                    .ancestry
                    .iter()
                    .map(|f| format!("{} {}", f.kind.as_str(), f.name))
                    .collect(),
            })
            .collect()
    })
}

/// Build an expected [`Entry`].
fn e(name: &str, value: &str, selector: Option<&str>, ancestry: &[&str]) -> Entry {
    Entry {
        name: name.to_string(),
        value: value.to_string(),
        selector: selector.map(str::to_string),
        ancestry: ancestry.iter().map(|s| s.to_string()).collect(),
    }
}

// ---------------------------------------------------------------------------
// Basic behavior
// ---------------------------------------------------------------------------

#[test]
fn disabled_by_default() {
    assert!(entries(".name {\n  $cool: green;\n}\n", &Options::default()).is_empty());
}

#[test]
fn records_local_variable_scoped_to_nested_rule() {
    // `$cool` is local to `.name`; after evaluation it no longer exists in the
    // environment, but it was recorded at declaration time.
    assert_eq!(
        entries(
            ".name {\n  $bg: beige !global;\n  $cool: green;\n}\n",
            &opts()
        ),
        vec![
            e("bg", "beige", Some(".name"), &[]),
            e("cool", "green", Some(".name"), &[]),
        ]
    );
}

#[test]
fn records_value_computed_from_sibling_local() {
    // The recorded value is the *evaluated* result, captured in-scope, so a value
    // referencing a sibling local resolves correctly.
    assert_eq!(
        entries(".name {\n  $a: 2px;\n  $b: $a * 3;\n}\n", &opts()),
        vec![
            e("a", "2px", Some(".name"), &[]),
            e("b", "6px", Some(".name"), &[]),
        ]
    );
}

#[test]
fn records_top_level_and_global_have_no_ancestry() {
    assert_eq!(
        entries("$top: 1;\na {\n  $mid: 2 !global;\n}\n", &opts()),
        vec![e("top", "1", None, &[]), e("mid", "2", Some("a"), &[]),]
    );
}

// ---------------------------------------------------------------------------
// Complex expressions: maps, lists, function calls
// ---------------------------------------------------------------------------

#[test]
fn map_literal() {
    assert_eq!(
        entries("$m: (a: 1, b: 2, c: 3);", &opts()),
        vec![e("m", "(a: 1, b: 2, c: 3)", None, &[])]
    );
}

#[test]
fn all_three_list_syntaxes() {
    assert_eq!(
        entries("$l: 1 2 3;\n$c: 1, 2, 3;\n$b: [1, 2, 3];", &opts()),
        vec![
            e("l", "1 2 3", None, &[]),
            e("c", "1, 2, 3", None, &[]),
            e("b", "[1, 2, 3]", None, &[]),
        ]
    );
}

#[test]
fn nested_maps_and_lists() {
    assert_eq!(
        entries(
            "$nested: (list: (1 2 3), map: (deep: (x: 1)));\n$mixed: 1px (a: 2) [3, 4];",
            &opts()
        ),
        vec![
            e("nested", "(list: 1 2 3, map: (deep: (x: 1)))", None, &[]),
            e("mixed", "1px (a: 2) [3, 4]", None, &[]),
        ]
    );
}

#[test]
fn deeply_nested_data_structure_read_back_with_functions() {
    assert_eq!(
        entries(
            "$data: (\n  sizes: (sm: 4px, lg: 16px),\n  names: [a, b, c]\n);\n\
             $lg: map-get(map-get($data, sizes), lg);\n\
             $first: nth(map-get($data, names), 1);",
            &opts()
        ),
        vec![
            e(
                "data",
                "(sizes: (sm: 4px, lg: 16px), names: [a, b, c])",
                None,
                &[]
            ),
            e("lg", "16px", None, &[]),
            e("first", "a", None, &[]),
        ]
    );
}

#[test]
fn list_functions() {
    assert_eq!(
        entries(
            "$len: length(1 2 3);\n\
             $nth: nth(4 5 6, 2);\n\
             $app: append(1 2, 3);\n\
             $join: join((1, 2), (3, 4));\n\
             $idx: index(a b c, b);\n\
             $setn: set-nth(1 2 3, 2, 9);\n\
             $sep: list-separator((1, 2));",
            &opts()
        ),
        vec![
            e("len", "3", None, &[]),
            e("nth", "5", None, &[]),
            e("app", "1 2 3", None, &[]),
            e("join", "1, 2, 3, 4", None, &[]),
            e("idx", "2", None, &[]),
            e("setn", "1 9 3", None, &[]),
            e("sep", "comma", None, &[]),
        ]
    );
}

#[test]
fn map_functions() {
    assert_eq!(
        entries(
            "$get: map-get((a: 1, b: 2), b);\n\
             $merge: map-merge((a: 1), (b: 2));\n\
             $keys: map-keys((p: 1, q: 2));\n\
             $vals: map-values((a: 1, b: 2));\n\
             $has: map-has-key((a: 1), a);",
            &opts()
        ),
        vec![
            e("get", "2", None, &[]),
            e("merge", "(a: 1, b: 2)", None, &[]),
            e("keys", "p, q", None, &[]),
            e("vals", "1, 2", None, &[]),
            e("has", "true", None, &[]),
        ]
    );
}

#[test]
fn string_functions() {
    assert_eq!(
        entries(
            "$up: to-upper-case(\"abc\");\n\
             $slen: str-length(\"hello\");\n\
             $sub: str-slice(\"hello\", 2, 4);",
            &opts()
        ),
        vec![
            e("up", "\"ABC\"", None, &[]),
            e("slen", "5", None, &[]),
            e("sub", "\"ell\"", None, &[]),
        ]
    );
}

#[test]
fn number_functions() {
    assert_eq!(
        entries(
            "$pct: percentage(0.5);\n\
             $abs: abs(-3);\n\
             $ceil: ceil(4.2);\n\
             $max: max(1, 2, 3);\n\
             $rnd: round(2.6);",
            &opts()
        ),
        vec![
            e("pct", "50%", None, &[]),
            e("abs", "3", None, &[]),
            e("ceil", "5", None, &[]),
            e("max", "3", None, &[]),
            e("rnd", "3", None, &[]),
        ]
    );
}

#[test]
fn color_function() {
    assert_eq!(
        entries("$col: rgb(1, 2, 3);", &opts()),
        vec![e("col", "rgb(1, 2, 3)", None, &[])]
    );
}

#[test]
fn combined_maps_lists_and_functions_in_nested_rule() {
    // Every declaration is inside `.grid` and reached directly (no ancestry).
    assert_eq!(
        entries(
            ".grid {\n  $cols: 3;\n  $gaps: 4px 8px;\n  \
             $config: (cols: $cols, gap: nth($gaps, 1));\n  \
             $picked: map-get($config, gap);\n}",
            &opts()
        ),
        vec![
            e("cols", "3", Some(".grid"), &[]),
            e("gaps", "4px 8px", Some(".grid"), &[]),
            e("config", "(cols: 3, gap: 4px)", Some(".grid"), &[]),
            e("picked", "4px", Some(".grid"), &[]),
        ]
    );
}

#[test]
fn complex_expression_carries_nested_selector() {
    assert_eq!(
        entries(
            ".card {\n  $pad: 4px * 2;\n  &:hover {\n    $shadow: 0 0 $pad rgba(0, 0, 0, 0.5);\n  }\n}",
            &opts()
        ),
        vec![
            e("pad", "8px", Some(".card"), &[]),
            e("shadow", "0 0 8px rgba(0, 0, 0, 0.5)", Some(".card:hover"), &[]),
        ]
    );
}

// ---------------------------------------------------------------------------
// Caller ancestry
// ---------------------------------------------------------------------------

#[test]
fn each_over_map_has_no_call_ancestry() {
    // `@each` is control flow, not a call, so iteration variables carry no
    // ancestry — but the single `$pair` span is still recorded once per iteration.
    assert_eq!(
        entries(
            "a {\n  @each $k, $v in (a: 1, b: 2) {\n    $pair: $k $v;\n    x: $pair;\n  }\n}",
            &opts()
        ),
        vec![
            e("pair", "a 1", Some("a"), &[]),
            e("pair", "b 2", Some("a"), &[]),
        ]
    );
}

#[test]
fn function_internal_variable_has_function_ancestry() {
    // `$r` lives in the function body (ancestry = the call); `$d` is at the root.
    assert_eq!(
        entries(
            "@function scale($n) {\n  $r: $n * 2;\n  @return $r;\n}\n$d: scale(21);",
            &opts()
        ),
        vec![
            e("r", "42", None, &["function scale"]),
            e("d", "42", None, &[]),
        ]
    );
}

#[test]
fn mixin_included_twice_records_per_include_with_caller_selector() {
    // `$area` (one span inside the mixin) is evaluated once per `@include`, each
    // carrying `mixin box` ancestry and the call site's selector.
    assert_eq!(
        entries(
            "@mixin box($n) {\n  $area: $n * $n;\n  width: $area;\n}\n\
             .a { @include box(2px); }\n\
             .b { @include box(3px); }",
            &opts()
        ),
        vec![
            e("area", "4px*px", Some(".a"), &["mixin box"]),
            e("area", "9px*px", Some(".b"), &["mixin box"]),
        ]
    );
}

#[test]
fn nested_calls_stack_ancestry_outermost_first() {
    // `.c` includes `outer`, which includes `inner`, which calls `triple`,
    // whose body declares `$t`. The ancestry lists every frame, outermost first.
    assert_eq!(
        entries(
            "@function triple($n) { $t: $n * 3; @return $t; }\n\
             @mixin inner($n) { $i: triple($n); width: $i; }\n\
             @mixin outer($n) { @include inner($n); }\n\
             .c { @include outer(2); }",
            &opts()
        ),
        vec![
            // `$t` is inside `triple`, reached via outer -> inner -> triple.
            e(
                "t",
                "6",
                Some(".c"),
                &["mixin outer", "mixin inner", "function triple"]
            ),
            // `$i` is inside `inner`, reached via outer -> inner.
            e("i", "6", Some(".c"), &["mixin outer", "mixin inner"]),
        ]
    );
}

#[test]
fn ancestry_pops_between_sibling_includes() {
    // After one `@include` returns, its frame must be gone for the next.
    assert_eq!(
        entries(
            "@mixin m($n) { $x: $n; width: $x; }\n\
             .a { @include m(1); }\n\
             $root: 2;\n\
             .b { @include m(3); }",
            &opts()
        ),
        vec![
            e("x", "1", Some(".a"), &["mixin m"]),
            e("x", "3", Some(".b"), &["mixin m"]),
            e("root", "2", None, &[]),
        ]
    );
}

// ---------------------------------------------------------------------------
// A function and mixin defined in one file, used from multiple other files
// ---------------------------------------------------------------------------

#[test]
fn function_and_mixin_used_from_multiple_files() {
    let mut fs = TestFs::new();
    fs.add_file(
        "lib.scss",
        "@mixin box($n) {\n  $area: $n * $n;\n  width: $area;\n}\n\
         @function scale($n) {\n  $r: $n * 2;\n  @return $r;\n}\n",
    );
    fs.add_file(
        "a.scss",
        "@use \"lib\";\n.from-a {\n  @include lib.box(2px);\n  height: lib.scale(3);\n}\n",
    );
    fs.add_file(
        "b.scss",
        "@use \"lib\";\n.from-b {\n  @include lib.box(4px);\n  height: lib.scale(5);\n}\n",
    );

    let options = Options::default().fs(&fs).record_variable_values(true);

    let path = Path::new("entry.scss");
    let mut map = CodeMap::new();
    let file = map.add_file(
        Arc::new(path.to_string_lossy().into_owned()),
        Arc::new("@use \"a\";\n@use \"b\";\n".to_owned()),
    );
    let empty_span = file.span.subspan(0, 0);
    let lexer = Lexer::new_from_file(&file);
    let sheet = ScssParser::new(lexer, &options, empty_span, path)
        .parse()
        .expect("parse");
    let mut visitor = Visitor::new(path, &options, &mut map, empty_span);
    visitor.visit_stylesheet(sheet).expect("visit");

    // Materialize what we need, then drop the visitor so `map` is free to resolve
    // spans to file names.
    struct Rec {
        decl_span: Span,
        name: String,
        value: String,
        selector: Option<String>,
        // (kind name, call-site span) per frame
        ancestry: Vec<(String, Span)>,
    }
    let recs: Vec<Rec> = visitor
        .variable_values
        .iter()
        .flat_map(|(k, v)| {
            v.iter().map(move |r| Rec {
                decl_span: *k,
                name: r.name.to_string(),
                value: r.value.inspect(empty_span).unwrap(),
                selector: r.selector.clone(),
                ancestry: r
                    .ancestry
                    .iter()
                    .map(|f| (format!("{} {}", f.kind.as_str(), f.name), f.call_site))
                    .collect(),
            })
        })
        .collect();
    drop(visitor);

    let fname = |s: Span| map.look_up_span(s).file.name().to_string();

    // Exactly four recorded entries: $area x2 (mixin) and $r x2 (function).
    assert_eq!(recs.len(), 4);

    // --- `$area`, declared in lib.scss, evaluated once per include ------------
    let mut area: Vec<&Rec> = recs.iter().filter(|r| r.name == "area").collect();
    area.sort_by_key(|r| r.value.clone());
    assert_eq!(area.len(), 2);

    // box(2px) from a.scss under `.from-a`.
    assert_eq!(area[0].value, "16px*px"); // 4px * 4px, sorts before "4px*px"
    assert_eq!(fname(area[0].decl_span), "lib.scss");
    assert_eq!(area[0].selector.as_deref(), Some(".from-b"));
    assert_eq!(area[0].ancestry.len(), 1);
    assert_eq!(area[0].ancestry[0].0, "mixin box");
    assert_eq!(fname(area[0].ancestry[0].1), "b.scss");

    assert_eq!(area[1].value, "4px*px"); // 2px * 2px
    assert_eq!(fname(area[1].decl_span), "lib.scss");
    assert_eq!(area[1].selector.as_deref(), Some(".from-a"));
    assert_eq!(area[1].ancestry.len(), 1);
    assert_eq!(area[1].ancestry[0].0, "mixin box");
    assert_eq!(fname(area[1].ancestry[0].1), "a.scss");

    // --- `$r`, declared in lib.scss, evaluated once per function call ---------
    let mut r: Vec<&Rec> = recs.iter().filter(|r| r.name == "r").collect();
    r.sort_by_key(|r| r.value.parse::<u32>().unwrap_or(0));
    assert_eq!(r.len(), 2);

    // scale(3) from a.scss.
    assert_eq!(r[0].value, "6");
    assert_eq!(fname(r[0].decl_span), "lib.scss");
    assert_eq!(r[0].selector.as_deref(), Some(".from-a"));
    assert_eq!(r[0].ancestry.len(), 1);
    assert_eq!(r[0].ancestry[0].0, "function scale");
    assert_eq!(fname(r[0].ancestry[0].1), "a.scss");

    // scale(5) from b.scss.
    assert_eq!(r[1].value, "10");
    assert_eq!(fname(r[1].decl_span), "lib.scss");
    assert_eq!(r[1].selector.as_deref(), Some(".from-b"));
    assert_eq!(r[1].ancestry.len(), 1);
    assert_eq!(r[1].ancestry[0].0, "function scale");
    assert_eq!(fname(r[1].ancestry[0].1), "b.scss");

    // The declaration file (lib.scss) is never the caller file — the whole point.
    for rec in &recs {
        for (_, call_site) in &rec.ancestry {
            assert_ne!(fname(*call_site), fname(rec.decl_span));
        }
    }
}
