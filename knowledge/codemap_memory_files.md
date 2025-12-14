# Using CodeMap for In-Memory SCSS Files

## Overview

While `CodeMap` is an internal component of the SCSS compiler that tracks file locations and spans for error reporting, users can work with in-memory SCSS files through the public API using the `Fs` trait. The compiler automatically uses `CodeMap` internally when reading files through a custom filesystem implementation.

## Key Concepts

- **CodeMap**: Internal structure that tracks file contents and provides span information for error reporting
- **Fs Trait**: Public interface for custom filesystem implementations
- **TestFs**: Example implementation that stores files in memory using a `BTreeMap`

## Usage Pattern

### 1. Create a Custom Filesystem

Implement the `Fs` trait or use `TestFs` for testing:

```rust
use scss_rust::{Fs, Options};
use std::path::Path;
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct MemoryFs {
    files: BTreeMap<std::path::PathBuf, String>,
}

impl MemoryFs {
    pub fn new() -> Self {
        Self {
            files: BTreeMap::new(),
        }
    }

    pub fn add_file(&mut self, path: &str, content: &str) {
        self.files.insert(Path::new(path).to_path_buf(), content.to_string());
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
        self.files.get(path)
            .map(|content| content.as_bytes().to_vec())
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "File not found"))
    }
}
```

### 2. Configure Compiler Options

Use the custom filesystem with compiler options:

```rust
let mut fs = MemoryFs::new();

// Add files to memory
fs.add_file("variables.scss", r#"
$primary-color: #3498db;
$font-size: 16px;
"#);

fs.add_file("mixins.scss", r#"
@mixin button-style($bg-color) {
    background-color: $bg-color;
    padding: 10px 20px;
    border: none;
    border-radius: 4px;
}
"#);

fs.add_file("main.scss", r#"
@use "variables" as vars;
@use "mixins" as *;

.button {
    @include button-style(vars.$primary-color);
    font-size: vars.$font-size;
}
"#);

let options = Options::default().fs(Box::new(fs));
let result = scss_rust::from_string("main.scss".to_string(), &options)?;
```

### 3. File Resolution with @use

The `@use` statements work with in-memory files the same way as filesystem files:

- Relative paths are resolved based on the current file's location
- File extensions (.scss, .sass) are automatically discovered
- Private members (starting with `_` or `-`) are not accessible
- Namespaces are created from filenames (or explicitly with `as`)

## Internal CodeMap Usage

When the compiler processes files through the `Fs` trait, it automatically:

1. Calls `code_map.add_file(name, content)` for each file read
2. Creates span information for error reporting
3. Tracks file relationships for import resolution

The `CodeMap` is not directly exposed to users but enables proper error messages with file locations and line numbers.

## Best Practices

1. **Use TestFs for testing**: The `TestFs` implementation in `crates/lib/tests/macros.rs` is optimized for test scenarios
2. **Implement Fs for production**: Create custom `Fs` implementations for real-world in-memory file needs
3. **File naming**: Use consistent naming conventions - `filename.scss` and `_filename.scss` are treated differently
4. **Path resolution**: Ensure relative paths work correctly by setting up proper file hierarchies in memory

## Error Handling

With in-memory files, error messages include proper location information thanks to CodeMap:

```
Error: Undefined variable.
  ,--> main.scss:4:15
  |
4 |     color: vars.$undefined-color;
  |               ^^^^^^^^^^^^^^^^
```

This works automatically because the compiler internally tracks file locations through CodeMap.
