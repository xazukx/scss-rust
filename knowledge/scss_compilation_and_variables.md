# SCSS → CSS Compilation and Variables

Paths are relative to repo root `d:/Projects/Oniz/scss-rust`.

---

## 1. Functions to Compile SCSS to CSS

### 1.1 Public API you normally call

**File:** `crates/lib/src/lib.rs`

- **`grass::from_string`**
  - Re-export of `grass_compiler::from_string`.
  - Use to compile a string of SCSS/Sass/CSS into CSS.

- **`grass::from_path`**
  - Re-export of `grass_compiler::from_path`.
  - Use to compile from a file path.

These two are the only functions you typically call directly from outside the crate.


### 1.2 Compiler crate entry points

**File:** `crates/compiler/src/lib.rs`

- **`from_string<S: Into<String>>(input: S, options: &Options) -> Result<String>`**
  - Path: lines ~239–252.
  - Calls internal `from_string_with_file_name(input.into(), "stdin", options)`.

- **`from_path<P: AsRef<Path>>(p: P, options: &Options) -> Result<String>`**
  - Path: lines ~223–237.
  - Reads file via `options.fs.read`, then calls `from_string_with_file_name`.

- **`parse_stylesheet<P: AsRef<Path>>(input: String, file_name: P, options: &Options) -> Result<StyleSheet>`**
  - Path: lines ~125–159.
  - Parses input into a `StyleSheet` AST without evaluating or serializing.

- **Internal `from_string_with_file_name`**
  - Path: lines ~161–221.
  - Full pipeline:
    1. Build `CodeMap` and `Lexer` from input.
    2. Pick parser based on `InputSyntax` and call:
       - `ScssParser::__parse()`
       - `SassParser::__parse()`
       - `CssParser::__parse()`.
    3. Evaluate AST:
       - `let mut visitor = Visitor::new(...);`
       - `visitor.visit_stylesheet(stylesheet)`
       - `let stmts = visitor.finish();`
    4. Serialize to CSS via `Serializer`:
       - Iterate `stmts` and call `serializer.visit_group(...)`.
       - `serializer.finish(...)` → final CSS string.


### 1.3 SCSS-specific parser

**File:** `crates/compiler/src/parse/scss.rs`

- **`ScssParser<'a>`**
  - Constructed with `ScssParser::new(lexer, options, empty_span, file_name)`.
  - Implements `StylesheetParser` and is used in `parse_stylesheet` and `from_string_with_file_name` when `InputSyntax::Scss`.

---

## 2. Where SCSS `$variables` Are Handled

There are three phases: parsing, storage, and evaluation.

### 2.1 Parsing variable declarations (`$foo: ...`)

**File:** `crates/compiler/src/parse/stylesheet.rs`

- **Detection of `$` at statement level**
  - `parse_children` and `parse_statements` (around lines 98–183):
    - On seeing a `$` token, they push `AstStmt::VariableDecl( self.parse_variable_declaration_without_namespace(None, None)? )`.

- **`parse_variable_declaration_without_namespace`**
  - Path: lines ~2664–2737.
  - Responsibilities:
    - Read variable name (`$name`) using `parse_variable_name`.
    - Disallow in plain CSS (`self.is_plain_css()` guard).
    - Parse `:` and the value expression via `parse_expression`.
    - Parse flags `!default` and `!global`.
    - Build `AstVariableDecl { namespace, name, value, is_guarded, is_global, span }`.

- **`parse_variable_declaration_with_namespace`**
  - Path: lines ~591–603.
  - Handles namespaced vars like `module.$var: ...` and delegates to
    `parse_variable_declaration_without_namespace`.

> All SCSS variable declarations eventually become `AstStmt::VariableDecl` via these functions.


### 2.2 Parsing variable *usage* (`$foo` in expressions)

**File:** `crates/compiler/src/parse/value.rs`

- **`ValueParser::parse_variable`**
  - Path: lines ~777–797.
  - Called whenever the expression parser sees `$`.
  - Produces `AstExpr::Variable { name: Spanned<Identifier>, namespace: None }`.
  - Disallows Sass variables in plain CSS.

- **Callers**
  - `ValueParser::parse_value` and `parse_single_expression` branch on `$` and call `parse_variable`.

> Any `$var` inside a value becomes `AstExpr::Variable` through this path.


### 2.3 Runtime storage and lookup (SCSS environment)

#### 2.3.1 Scope structure

**File:** `crates/compiler/src/evaluate/scope.rs`

- **`Scopes` struct**
  - Field of interest:
    - `variables: Arc<RefCell<Vec<Arc<RefCell<BTreeMap<Identifier, Value>>>>>>`
      - Stack of variable maps (outer → inner).

- **Key methods**
  - `insert_var(&mut self, idx, name, v)` — insert or update at given scope index.
  - `insert_var_last(&mut self, name, v)` — always in innermost scope.
  - `get_var(&mut self, name: Spanned<Identifier>) -> SassResult<Value>` — lookup from innermost to outermost.
  - `var_exists(&self, name) -> bool` — whether any scope has this identifier.
  - `global_variables(&self)` / `global_var_exists(&self, name)` — access/check global (outermost) scope.

#### 2.3.2 Evaluation environment

**File:** `crates/compiler/src/evaluate/env.rs`

- **`Environment` struct**
  - Field: `pub scopes: Scopes`.

- **Variable-related methods**
  - `var_exists(&self, name, namespace) -> SassResult<bool>`
    - Namespaced: check module.
    - Otherwise: `self.scopes.var_exists(name)`.
  - `get_var(&mut self, name, namespace) -> SassResult<Value>`
    - Namespaced: fetch from module.
    - Otherwise: `self.scopes.get_var(name)` or from global modules.
  - `insert_var(&mut self, name, namespace, value, is_global, in_semi_global_scope)`
    - Handles writes to modules, global scope, or the appropriate local scope.
  - `global_vars(&self) -> Arc<RefCell<BTreeMap<Identifier, Value>>>`
    - Direct access to global variable map.

> If you need to *inspect* or manipulate all SCSS variables, work with `Environment` and its `scopes`.


### 2.4 Evaluating variable declarations and reads

**File:** `crates/compiler/src/evaluate/visitor.rs`

- **`Visitor::visit_stylesheet`**
  - Path: lines ~176–193.
  - Walks all `AstStmt`s and calls `visit_stmt`.

- **`Visitor::visit_stmt`**
  - Path: lines ~212–257.
  - For variables:
    - `AstStmt::VariableDecl(decl) => self.visit_variable_decl(decl)`.

- **`Visitor::visit_variable_decl`**
  - Path: lines ~1966–2013.
  - Steps:
    1. Wrap `decl.name` into a `Spanned<Identifier>`.
    2. Apply `!default` logic (may skip assignment).
    3. Evaluate `decl.value` via `self.visit_expr`.
    4. Insert the resulting `Value` via `self.env.insert_var(...)`.

- **`Visitor::visit_expr`**
  - Path: lines ~2537–2574.
  - For `AstExpr::Variable { name, namespace }`:
    - Returns `self.env.get_var(name, namespace)?`.

> `visit_variable_decl` is the write side; the `AstExpr::Variable` arm in `visit_expr` is the read side.

---

## 3. CSS Custom Properties and `var()`

### 3.1 Parsing identifiers that can be `--foo`

**File:** `crates/compiler/src/parse/stylesheet.rs`

- **`parse_interpolated_identifier`**
  - Path: lines ~1960–1994.
  - Allows identifiers starting with `-` and `--` before continuing the name.
  - Used by higher-level parsing to treat `--custom-property` as a valid identifier (possibly interpolated).

> Custom properties are *not* stored as SCSS variables; they are normal identifiers in declarations.


### 3.2 Recognizing custom properties in `@supports`

**File:** `crates/compiler/src/evaluate/visitor.rs`

- **`Visitor::visit_supports_condition`**
  - Path: lines ~422–473.
  - For `AstSupportsCondition::Declaration { name, value }`:
    - Detects custom property names if `name` is an unquoted string starting with `"--"`:
      - `text.initial_plain().starts_with("--")`.
    - When serializing, omits the space after the colon for custom properties.


### 3.3 `var()` inside calculations

**File:** `crates/compiler/src/evaluate/visitor.rs`

- **`Visitor::visit_calculation_value`**
  - Path: lines ~2577–2644.
  - Special-cases `AstExpr::FunctionCall` whose name lowercases to `"var"` when converting expressions into `CalculationArg`.
  - This is where CSS `var()` references (often pointing at `--custom-properties`) are handled inside `calc()`-style contexts.

---

## 4. Quick “Where to Look” Summary

- **To compile SCSS → CSS (callable API):**
  - `crates/lib/src/lib.rs` → `from_string`, `from_path`.
  - `crates/compiler/src/lib.rs` → `from_string`, `from_path`, `parse_stylesheet`.

- **To see how SCSS variables are declared and stored:**
  - Parsing declarations:
    - `crates/compiler/src/parse/stylesheet.rs`
      - `parse_variable_declaration_without_namespace`
      - `parse_variable_declaration_with_namespace`
  - Parsing `$var` usage:
    - `crates/compiler/src/parse/value.rs`
      - `ValueParser::parse_variable`.
  - Runtime storage:
    - `crates/compiler/src/evaluate/scope.rs`
      - `Scopes` + `insert_var`, `get_var`, `var_exists`.
    - `crates/compiler/src/evaluate/env.rs`
      - `Environment::insert_var`, `get_var`, `global_vars`, `var_exists`.
  - Evaluation:
    - `crates/compiler/src/evaluate/visitor.rs`
      - `Visitor::visit_variable_decl` (writes)
      - `Visitor::visit_expr` `AstExpr::Variable` arm (reads).

- **To see how CSS custom properties / `var()` are recognized:**
  - Parsing identifiers:
    - `crates/compiler/src/parse/stylesheet.rs` → `parse_interpolated_identifier`.
  - `@supports` handling:
    - `crates/compiler/src/evaluate/visitor.rs` → `visit_supports_condition` (custom-property name check).
  - Calculations with `var()`:
    - `crates/compiler/src/evaluate/visitor.rs` → `visit_calculation_value` (special-case `var()` in calc).
