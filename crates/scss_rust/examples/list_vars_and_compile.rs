use std::{path::PathBuf, sync::Arc};

use scss_rust::{
    Lexer, Options, OutputStyle, ScssParser, StylesheetParser, Visitor,
    codemap::{CodeMap, Spanned},
    sass_ast::{AstImport, AstStmt, AstVariableDecl},
    sass_value::Value,
    serializer::{StyleSerializer, serialize_value},
};

#[derive(Debug, Default)]
struct TraversalResults {
    scss_vars: Vec<(AstVariableDecl, Value)>,
    css_custom_props: Vec<String>,
    imports: Vec<String>,
    mixins: Vec<String>,
    includes: Vec<String>,
    uses: Vec<String>,
}

fn collect_stylesheet_symbols(
    body: &[AstStmt],
    visitor: &mut Visitor<'_>,
    results: &mut TraversalResults,
) {
    for stmt in body {
        match stmt {
            AstStmt::VariableDecl(decl) => {
                let value = visitor.env.get_var(
                    Spanned {
                        node: decl.name,
                        span: decl.span,
                    },
                    decl.namespace.clone(),
                );

                if let Ok(value) = value {
                    results.scss_vars.push((decl.clone(), value));
                }
            }
            AstStmt::RuleSet(rule_set) => {
                collect_stylesheet_symbols(&rule_set.body, visitor, results);
            }
            AstStmt::Style(style) => {
                let name = style.name.initial_plain();
                if name.starts_with("--") {
                    results.css_custom_props.push(name.to_string());
                }

                collect_stylesheet_symbols(&style.body, visitor, results);
            }
            AstStmt::If(if_stmt) => {
                for clause in &if_stmt.if_clauses {
                    collect_stylesheet_symbols(&clause.body, visitor, results);
                }

                if let Some(else_body) = &if_stmt.else_clause {
                    collect_stylesheet_symbols(else_body, visitor, results);
                }
            }
            AstStmt::For(for_stmt) => {
                collect_stylesheet_symbols(&for_stmt.body, visitor, results);
            }
            AstStmt::Each(each_stmt) => {
                collect_stylesheet_symbols(&each_stmt.body, visitor, results);
            }
            AstStmt::Media(media_rule) => {
                collect_stylesheet_symbols(&media_rule.body, visitor, results);
            }
            AstStmt::While(while_rule) => {
                collect_stylesheet_symbols(&while_rule.body, visitor, results);
            }
            AstStmt::FunctionDecl(func) => {
                collect_stylesheet_symbols(&func.body, visitor, results);
            }
            AstStmt::Mixin(mixin) => {
                results.mixins.push(mixin.name.to_string());
                collect_stylesheet_symbols(&mixin.body, visitor, results);
            }
            AstStmt::AtRootRule(at_root) => {
                collect_stylesheet_symbols(&at_root.body, visitor, results);
            }
            AstStmt::Supports(supports_rule) => {
                collect_stylesheet_symbols(&supports_rule.body, visitor, results);
            }
            AstStmt::ImportRule(import_rule) => {
                for import in &import_rule.imports {
                    match import {
                        AstImport::Plain(plain) => {
                            let url = plain.url.as_plain().unwrap_or("<dynamic>").to_string();
                            results.imports.push(url);
                        }
                        AstImport::Sass(sass_import) => {
                            results.imports.push(sass_import.url.clone());
                        }
                    }
                }
            }
            AstStmt::Include(include_stmt) => {
                let mut full_name = String::new();
                if let Some(ns) = &include_stmt.namespace {
                    full_name.push_str(&ns.node.to_string());
                    full_name.push('.');
                }
                full_name.push_str(&include_stmt.name.node.to_string());
                results.includes.push(full_name);

                if let Some(content) = &include_stmt.content {
                    collect_stylesheet_symbols(&content.body, visitor, results);
                }
            }
            AstStmt::Use(use_rule) => {
                let url = use_rule.url.to_string_lossy().into_owned();
                let entry = match &use_rule.namespace {
                    Some(ns) => format!("{} as {}", url, ns),
                    None => url,
                };
                results.uses.push(entry);
            }
            // The remaining statement kinds (return, comments, error/warn, forward, etc.)
            // either do not introduce new nested statement bodies or their nested behavior
            // is handled through other nodes we already traverse, so they are intentionally
            // ignored for symbol discovery.
            _ => {}
        }
    }
}

fn main() -> Result<(), Box<scss_rust::Error>> {
    let scss = r#"
$primary-color: #333;
$spacing-unit: 8px;

@mixin button-base($bg, $fg) {
	padding: $spacing-unit * 2;
	background-color: $bg;
	color: $fg;
}

.button {
	$local-var: 10px;
	margin: $local-var;
	@include button-base($primary-color, white);

	@if $local-var == 10px {
		$conditional-var: red;
		color: $conditional-var;
	}
    
    .nested {
        --normal-css-property: 1px;
        $apple: blue;
        background-color: $apple;
        border-width: var(--normal-css-property);
        padding: calc($spacing-unit * 2);
    }
}
"#;

    let mut options = Options::default();
    options = options.style(OutputStyle::Expanded);

    let mut map = CodeMap::new();
    let path = PathBuf::from("input.scss");
    let file = map.add_file(
        Arc::new(path.to_string_lossy().into_owned()),
        Arc::new(scss.to_owned()),
    );
    let empty_span = file.span.subspan(0, 0);
    let lexer = Lexer::new_from_file(&file);

    let stylesheet = ScssParser::new(lexer, &options, empty_span, &path).parse()?;

    let mut visitor = Visitor::new(&path, &options, &mut map, empty_span);
    let stylesheet = visitor.visit_stylesheet(stylesheet)?;
    let stmts = visitor.finish();

    let mut results = TraversalResults::default();
    collect_stylesheet_symbols(&stylesheet.body, &mut visitor, &mut results);

    drop(visitor);

    let mut serializer = StyleSerializer::new(&options, &map, false, empty_span);
    let mut prev_was_group_end = false;
    let mut prev_requires_semicolon = false;
    for stmt_and_module in stmts {
        if stmt_and_module.is_invisible() {
            continue;
        }

        let is_group_end = stmt_and_module.is_group_end();
        let requires_semicolon = StyleSerializer::requires_semicolon(&stmt_and_module);

        serializer.visit_group(
            stmt_and_module.stmt,
            prev_was_group_end,
            prev_requires_semicolon,
        )?;

        prev_was_group_end = is_group_end;
        prev_requires_semicolon = requires_semicolon;
    }
    let css = serializer.finish(prev_requires_semicolon);

    println!(
        "Found {} SCSS variable declarations:\n",
        results.scss_vars.len()
    );

    for (index, (decl, value)) in results.scss_vars.iter().enumerate() {
        let namespace = match &decl.namespace {
            Some(ns) => ns.node.to_string(),
            None => String::new(),
        };

        let full_name = if namespace.is_empty() {
            format!("${}", decl.name)
        } else {
            format!("{}.${}", namespace, decl.name)
        };
        let serialized_value = serialize_value(&value, &options, empty_span, true)
            .unwrap_or_else(|_| "error".to_string());
        println!("{}: {} = {}", index + 1, full_name, serialized_value);
    }

    if !results.css_custom_props.is_empty() {
        println!(
            "\nFound {} CSS custom properties:\n",
            results.css_custom_props.len()
        );
        for (index, name) in results.css_custom_props.iter().enumerate() {
            println!("{}: {}", index + 1, name);
        }
    }

    if !results.imports.is_empty() {
        println!("\nFound {} imports:\n", results.imports.len());
        for (index, name) in results.imports.iter().enumerate() {
            println!("{}: {}", index + 1, name);
        }
    }

    if !results.mixins.is_empty() {
        println!("\nFound {} mixin declarations:\n", results.mixins.len());
        for (index, name) in results.mixins.iter().enumerate() {
            println!("{}: {}", index + 1, name);
        }
    }

    if !results.includes.is_empty() {
        println!("\nFound {} includes:\n", results.includes.len());
        for (index, name) in results.includes.iter().enumerate() {
            println!("{}: {}", index + 1, name);
        }
    }

    if !results.uses.is_empty() {
        println!("\nFound {} uses:\n", results.uses.len());
        for (index, name) in results.uses.iter().enumerate() {
            println!("{}: {}", index + 1, name);
        }
    }

    println!("\nCompiled CSS:\n\n{}", css);

    Ok(())
}
