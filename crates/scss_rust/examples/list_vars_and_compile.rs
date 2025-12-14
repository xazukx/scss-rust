use std::path::PathBuf;

use scss_rust::{
	Options,
	OutputStyle,
	parse_stylesheet,
	sass_ast::{AstImport, AstStmt, AstVariableDecl},
};

#[derive(Debug, Default)]
struct TraversalResults {
	scss_vars: Vec<AstVariableDecl>,
	css_custom_props: Vec<String>,
	imports: Vec<String>,
	mixins: Vec<String>,
	includes: Vec<String>,
	uses: Vec<String>,
}

fn collect_stylesheet_symbols(body: &[AstStmt], results: &mut TraversalResults) {
	for stmt in body {
		match stmt {
			AstStmt::VariableDecl(decl) => {
				results.scss_vars.push(decl.clone());
			}
			AstStmt::RuleSet(rule_set) => {
				collect_stylesheet_symbols(&rule_set.body, results);
			}
			AstStmt::Style(style) => {
				let name = style.name.initial_plain();
				if name.starts_with("--") {
					results.css_custom_props.push(name.to_string());
				}

				collect_stylesheet_symbols(&style.body, results);
			}
			AstStmt::If(if_stmt) => {
				for clause in &if_stmt.if_clauses {
					collect_stylesheet_symbols(&clause.body, results);
				}

				if let Some(else_body) = &if_stmt.else_clause {
					collect_stylesheet_symbols(else_body, results);
				}
			}
			AstStmt::For(for_stmt) => {
				collect_stylesheet_symbols(&for_stmt.body, results);
			}
			AstStmt::Each(each_stmt) => {
				collect_stylesheet_symbols(&each_stmt.body, results);
			}
			AstStmt::Media(media_rule) => {
				collect_stylesheet_symbols(&media_rule.body, results);
			}
			AstStmt::While(while_rule) => {
				collect_stylesheet_symbols(&while_rule.body, results);
			}
			AstStmt::FunctionDecl(func) => {
				collect_stylesheet_symbols(&func.body, results);
			}
			AstStmt::Mixin(mixin) => {
				results.mixins.push(mixin.name.to_string());
				collect_stylesheet_symbols(&mixin.body, results);
			}
			AstStmt::AtRootRule(at_root) => {
				collect_stylesheet_symbols(&at_root.body, results);
			}
			AstStmt::Supports(supports_rule) => {
				collect_stylesheet_symbols(&supports_rule.body, results);
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
					collect_stylesheet_symbols(&content.body, results);
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

	let stylesheet = parse_stylesheet(
		scss.to_owned(),
		PathBuf::from("input.scss"),
		&options,
	)?;
	// println!("{stylesheet:#?}");

	let mut results = TraversalResults::default();
	collect_stylesheet_symbols(&stylesheet.body, &mut results);

	println!("Found {} SCSS variable declarations:\n", results.scss_vars.len());

	for (index, decl) in results.scss_vars.iter().enumerate() {
		let namespace = match &decl.namespace {
			Some(ns) => ns.node.to_string(),
			None => String::new(),
		};

		let full_name = if namespace.is_empty() {
			format!("${}", decl.name)
		} else {
			format!("{}.${}", namespace, decl.name)
		};

		println!("{}: {}", index + 1, full_name);
	}

	if !results.css_custom_props.is_empty() {
		println!("\nFound {} CSS custom properties:\n", results.css_custom_props.len());
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

	let css = scss_rust::from_string(scss, &options)?;

	println!("\nCompiled CSS:\n\n{}", css);

	Ok(())
}
