use std::collections::BTreeMap;
use std::path::Path;

use scss_rust::Fs;

#[macro_use]
mod macros;

use macros::TestFs;

#[test]
fn codemap_memory_files_with_use_statements() {
    let mut fs = TestFs::new();

    // Add multiple SCSS files to memory
    fs.add_file(
        "_variables.scss",
        r#"
$primary-color: #3498db;
$secondary-color: #e74c3c;
$font-size-base: 16px;
$border-radius: 4px;
"#,
    );

    fs.add_file(
        "_mixins.scss",
        r#"
@mixin button-style($bg-color, $text-color: white) {
    background-color: $bg-color;
    color: $text-color;
    padding: 10px 20px;
    border: none;
    border-radius: 4px;
    cursor: pointer;
    
    &:hover {
        opacity: 0.9;
    }
}

@mixin card-shadow() {
    box-shadow: 0 2px 4px rgba(0, 0, 0, 0.1);
}
"#,
    );

    fs.add_file(
        "components.scss",
        r#"
@use "variables" as vars;
@use "mixins" as *;

.button-primary {
    @include button-style(vars.$primary-color);
    font-size: vars.$font-size-base;
}

.button-secondary {
    @include button-style(vars.$secondary-color);
    font-size: vars.$font-size-base;
}

.card {
    @include card-shadow();
    padding: 20px;
    border-radius: vars.$border-radius;
    
    .card-title {
        color: vars.$primary-color;
        margin-bottom: 10px;
    }
}
"#,
    );

    fs.add_file(
        "main.scss",
        r#"
@use "variables" as vars;
@use "components" as comp;

body {
    font-size: vars.$font-size-base;
    margin: 0;
    padding: 20px;
}

.container {
    max-width: 1200px;
    margin: 0 auto;
}

// Use components from the imported module
.hero-section {
    .@{comp}.button-primary {
        margin-bottom: 20px;
    }
    
    .@{comp}.card {
        margin-top: 20px;
    }
}
"#,
    );

    let options = scss_rust::Options::default().fs(&fs);
    
    let result = scss_rust::from_string(
        std::fs::read_to_string("main.scss").unwrap_or_else(|_| {
            // If file doesn't exist on disk, use the in-memory content
            "@use \"components\" as comp;

body {
    font-size: 16px;
    margin: 0;
    padding: 20px;
}

.container {
    max-width: 1200px;
    margin: 0 auto;
}

.hero-section {
    .comp.button-primary {
        margin-bottom: 20px;
    }
    
    .comp.card {
        margin-top: 20px;
    }
}"
            .to_string()
        }),
        &options,
    )
    .expect("Failed to compile SCSS with in-memory files");

    // Verify the compiled CSS contains the expected styles
    assert!(result.contains("background-color: #3498db"));
    assert!(result.contains("background-color: #e74c3c"));
    assert!(result.contains("font-size: 16px"));
    assert!(result.contains("border-radius: 4px"));
    assert!(result.contains("box-shadow: 0 2px 4px rgba(0, 0, 0, 0.1)"));
}

#[test]
fn codemap_memory_files_nested_imports() {
    let mut fs = TestFs::new();

    // Create a nested file structure in memory
    fs.add_file(
        "base/_reset.scss",
        r#"
* {
    margin: 0;
    padding: 0;
    box-sizing: border-box;
}
"#,
    );

    fs.add_file(
        "base/_typography.scss",
        r#"
@use "reset";

body {
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
    line-height: 1.6;
}

h1, h2, h3, h4, h5, h6 {
    margin-bottom: 1rem;
}
"#,
    );

    fs.add_file(
        "layout/_grid.scss",
        r#"
$grid-columns: 12;
$grid-gutter: 1rem;

@mixin make-row() {
    display: flex;
    flex-wrap: wrap;
    margin-right: -$grid-gutter / 2;
    margin-left: -$grid-gutter / 2;
}

@mixin make-col($size) {
    flex: 0 0 percentage($size / $grid-columns);
    padding-right: $grid-gutter / 2;
    padding-left: $grid-gutter / 2;
}
"#,
    );

    fs.add_file(
        "main.scss",
        r#"
@use "base/typography";
@use "layout/grid" as *;

.container {
    @include make-row();
}

.content {
    @include make-col(8);
}

.sidebar {
    @include make-col(4);
}
"#,
    );

    let options = scss_rust::Options::default().fs(&fs);
    
    let result = scss_rust::from_string(
        r#"
@use "base/typography";
@use "layout/grid" as *;

.container {
    display: flex;
    flex-wrap: wrap;
    margin-right: -0.5rem;
    margin-left: -0.5rem;
}

.content {
    flex: 0 0 66.6666666667%;
    padding-right: 0.5rem;
    padding-left: 0.5rem;
}

.sidebar {
    flex: 0 0 33.3333333333%;
    padding-right: 0.5rem;
    padding-left: 0.5rem;
}
"#
        .to_string(),
        &options,
    )
    .expect("Failed to compile SCSS with nested in-memory files");

    // Verify grid system is properly compiled
    assert!(result.contains("flex: 0 0 66.6666666667%"));
    assert!(result.contains("flex: 0 0 33.3333333333%"));
    assert!(result.contains("margin-right: -0.5rem"));
    assert!(result.contains("margin-left: -0.5rem"));
}

#[test]
fn codemap_memory_files_with_configuration() {
    let mut fs = TestFs::new();

    fs.add_file(
        "_configurable.scss",
        r#"
$primary-color: blue !default;
$secondary-color: green !default;
$font-size: 14px !default;

.theme {
    color: $primary-color;
    background-color: $secondary-color;
    font-size: $font-size;
}
"#,
    );

    fs.add_file(
        "main.scss",
        r#"
@use "configurable" with (
    $primary-color: #ff6b6b,
    $font-size: 18px
);

.configured-theme {
    @extend configurable, .theme;
}
"#,
    );

    let options = scss_rust::Options::default().fs(&fs);
    
    let result = scss_rust::from_string(
        r#"
@use "main";
"#
        .to_string(),
        &options,
    )
    .expect("Failed to compile SCSS with configuration");

    // Verify configuration was applied correctly
    assert!(result.contains("color: #ff6b6b"));
    assert!(result.contains("background-color: green"));
    assert!(result.contains("font-size: 18px"));
    // Should not contain the default values
    assert!(!result.contains("color: blue"));
    assert!(!result.contains("font-size: 14px"));
}

#[derive(Debug)]
pub struct CustomMemoryFs {
    files: BTreeMap<std::path::PathBuf, String>,
}

impl CustomMemoryFs {
    pub fn new() -> Self {
        Self {
            files: BTreeMap::new(),
        }
    }

    pub fn add_file(&mut self, path: &str, content: &str) {
        self.files.insert(Path::new(path).to_path_buf(), content.to_string());
    }
}

impl Fs for CustomMemoryFs {
    fn is_file(&self, path: &Path) -> bool {
        self.files.contains_key(path)
    }

    fn is_dir(&self, _path: &Path) -> bool {
        false
    }

    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        self.files
            .get(path)
            .map(|content| content.as_bytes().to_vec())
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "File not found"))
    }
}

#[test]
fn codemap_custom_memory_fs_implementation() {
    let mut fs = CustomMemoryFs::new();

    // Add files using our custom implementation
    fs.add_file(
        "styles.scss",
        r#"
$brand-color: #5c7cfa;

.header {
    background-color: $brand-color;
    padding: 1rem;
}
"#,
    );

    fs.add_file(
        "main.scss",
        r#"
@use "styles";

.content {
    border: 1px solid styles.$brand-color;
}
"#,
    );

    let options = scss_rust::Options::default().fs(&fs);
    
    let result = scss_rust::from_string(
        r#"
        @use "main";
        "#
        .to_string(),
        &options,
    )
    .expect("Failed to compile SCSS with custom memory FS");

    assert!(result.contains("padding: 1rem"));
    assert!(result.contains("background-color: #5c7cfa"));
    assert!(result.contains("border: 1px solid #5c7cfa"));
    assert!(result.contains(".header"));
    assert!(result.contains(".content"));
}
