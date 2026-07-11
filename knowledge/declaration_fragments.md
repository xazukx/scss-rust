# Parsing & Compiling Declaration Fragments

Paths are relative to repo root `d:/Projects/Oniz/scss-rust`.

Normally the top level of a stylesheet must contain selectors — a bare
declaration like `border: 1px solid black;` is a parse error (`expected "{".`)
and, even if parsed, an evaluation error (`Declarations may only be used within
style rules.`). Two **opt-in** entry points lift that restriction so you can
process just the *inside* of a rule (an editor snippet, a template partial, a
style object). Defaults are unchanged.

---

## 1. Entry points

- **`StylesheetParser::parse_allowing_declarations()`**
  - File: `crates/scss_rust/src/parse/stylesheet.rs`.
  - Same as `parse()`, but parses the document as the body of an implicit style
    rule. Accepts bare declarations, style rules, and `$variable` declarations
    at the top level. Spans/line/column are identical to the equivalent nested
    content.

- **`Visitor::visit_stylesheet_allowing_declarations(sheet)`**
  - File: `crates/scss_rust/src/evaluate/visitor.rs`.
  - Same as `visit_stylesheet()`, but emits top-level declarations directly at
    the root of the output instead of erroring.

Both are additive: `parse()` and `visit_stylesheet()` still reject top-level
declarations exactly as before.

---

## 2. Minimal usage

Add `scss_rust` as a dependency, then:

```rust
use std::{path::PathBuf, sync::Arc};
use scss_rust::{
    codemap::CodeMap, serializer::StyleSerializer,
    Lexer, Options, OutputStyle, ScssParser, StylesheetParser, Visitor,
};

fn compile_fragment(src: &str) -> String {
    let options = Options::default().style(OutputStyle::Expanded);
    let mut map = CodeMap::new();
    let path = PathBuf::from("fragment.scss");
    let file = map.add_file(
        Arc::new(path.to_string_lossy().into_owned()),
        Arc::new(src.to_owned()),
    );
    let empty_span = file.span.subspan(0, 0);
    let lexer = Lexer::new_from_file(&file);

    // 1. Parse (declarations allowed at the top level).
    let sheet = ScssParser::new(lexer, &options, empty_span, &path)
        .parse_allowing_declarations()
        .expect("parse");

    // 2. Evaluate (declarations allowed at the root).
    let mut visitor = Visitor::new(&path, &options, &mut map, empty_span);
    visitor.visit_stylesheet_allowing_declarations(sheet).expect("visit");
    let stmts = visitor.finish();
    drop(visitor);

    // 3. Serialize.
    let mut serializer = StyleSerializer::new(&options, &map, false, empty_span);
    let (mut group_end, mut semi) = (false, false);
    for sm in stmts {
        if sm.is_invisible() { continue; }
        let (is_end, needs_semi) = (sm.is_group_end(), StyleSerializer::requires_semicolon(&sm));
        serializer.visit_group(sm.stmt, group_end, semi).expect("serialize");
        group_end = is_end;
        semi = needs_semi;
    }
    serializer.finish(semi)
}

// compile_fragment("$c: black;\nborder: 1px solid $c;\ncolor: red;\n")
//   => "border: 1px solid black;\ncolor: red;\n"
```

To only parse (e.g. for tooling that reads positions from the AST), stop after
step 1 and walk `sheet.body`; each `AstStmt::Style { span, .. }` resolves via
`map.look_up_span(span)`.

---

## 3. Behaviour notes

- **Positions preserved.** A bare declaration on line 1 reports `1:1`; the same
  declaration nested one tab deep reports `2:2`. Line/column bookkeeping is
  identical to the selector form.
- **Full evaluation works.** Variables, `calc()`/arithmetic, custom properties
  (`--foo`), and nested declarations (`font: { family: … }` → `font-family`) all
  evaluate at the root.
- **Mixed content is fine.** Bare declarations may sit next to normal style
  rules; each is emitted in source order.
- **Top-level `&` resolves to `:scope`.** Because a fragment has no real
  enclosing rule, an explicit parent selector at the top level refers to the
  implicit `:scope` root: `& .yolo` → `:scope .yolo`, `&:hover` → `:scope:hover`.
  A top-level selector *without* `&` is left untouched (`.foo` stays `.foo`).
  Nested `&` still resolves against its real enclosing selector. Outside
  fragment mode a top-level `&` remains an error.
- **Imports.** The visitor relaxation lasts for the whole call, including files
  pulled in via `@use`/`@import`/`@forward`.
- **Compressed mode** keeps a trailing `;` (e.g. `border:1px solid #000;`) — the
  root-level flush has no closing `}` to elide it.

---

## 4. Reference

- Example: `crates/scss_rust/examples/declaration_fragment.rs`
  (`cargo run -p scss_rust --example declaration_fragment`).
- Tests: `crates/scss_rust/tests/declaration_fragment.rs`
  (`cargo test -p scss_rust --test declaration_fragment`).
