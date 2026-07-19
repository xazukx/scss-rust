//! Source position tracking for error reporting.
//!
//! Every source file is parsed in its **own** coordinate space: byte offsets run
//! from `0` to the file's length, exactly as a user would count them in their
//! editor. A [`Span`] therefore carries the identity of the file it belongs to
//! (a small, `Copy` [`FileId`]) alongside file-relative `low`/`high` offsets. This
//! means a span is self-describing: reading a location out of it never requires
//! knowing where the file sits relative to any other file.
//!
//! The [`CodeMap`] is just a registry that maps a [`FileId`] back to its
//! [`CodeFile`] (source text, name, and line table) so a span can be rendered as
//! `file:line:column`. It performs no cross-file offset arithmetic, so error
//! locations cannot drift between files regardless of the order files are added.
//!
//! # Example
//! ```
//! use scss_rust::codemap::CodeMap;
//! use std::sync::Arc;
//! let mut codemap = CodeMap::new();
//! let file = codemap.add_file(Arc::new("test.rs".to_string()), Arc::new("fn test(){\n    println!(\"Hello\");\n}\n".to_string()));
//! // Offsets are relative to the start of *this* file.
//! let string_literal_span = file.span.subspan(24, 31);
//!
//! let location = codemap.look_up_span(string_literal_span);
//! assert_eq!(location.file.name(), "test.rs");
//! assert_eq!(location.begin.line, 1);
//! assert_eq!(location.begin.column, 13);
//! assert_eq!(location.end.line, 1);
//! assert_eq!(location.end.column, 20);
//! ```

use std::cmp;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::{Add, Deref, Sub};
use std::sync::Arc;

/// Identifies a single source file within a [`CodeMap`].
///
/// This is the registry index of the file. It is `Copy` so it can ride along
/// inside every [`Span`] without giving up `Span`'s `Copy`-ness.
#[derive(Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd, Debug)]
pub struct FileId(u32);

impl FileId {
    /// A span that is not tied to any file in a `CodeMap` (e.g. a synthetic or
    /// not-yet-registered span). Resolving it yields the `<unknown>` file.
    pub const DETACHED: FileId = FileId(u32::MAX);

    /// The registry index of this file.
    #[inline]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// A small, `Copy`, value representing a byte offset within a file.
///
/// The offset is relative to the start of the file the enclosing [`Span`] refers
/// to — never to a global, multi-file coordinate space.
#[derive(Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd, Debug)]
pub struct Pos(pub u32);

impl AsRef<u32> for Pos {
    fn as_ref(&self) -> &u32 {
        &self.0
    }
}

impl std::ops::Deref for Pos {
    type Target = u32;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Add<u64> for Pos {
    type Output = Pos;
    fn add(self, other: u64) -> Pos {
        Pos(self.0 + other as u32)
    }
}

impl Sub<Pos> for Pos {
    type Output = u64;
    fn sub(self, other: Pos) -> u64 {
        (self.0 - other.0) as u64
    }
}

/// A range of text within a single source file.
///
/// `low`/`high` are byte offsets **relative to the start of `file`**, so they map
/// directly onto positions a user would recognise in that file.
#[derive(Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd, Debug)]
pub struct Span {
    /// The file this span points into.
    file: FileId,

    /// The offset of the first byte of the span, relative to the file's start.
    low: Pos,

    /// The offset after the last byte of the span, relative to the file's start.
    high: Pos,
}

impl Span {
    /// Creates a span from explicit file-relative offsets.
    ///
    /// Most spans are derived from a file's [`CodeFile::span`] via [`Span::subspan`]
    /// and [`Span::merge`], which carry the [`FileId`] forward automatically; this
    /// constructor is only needed when building one from scratch.
    pub fn new(file: FileId, low: u32, high: u32) -> Span {
        Span {
            file,
            low: Pos(low),
            high: Pos(high),
        }
    }

    /// Makes a span from offsets relative to the start of this span, preserving
    /// the file identity.
    ///
    /// # Panics
    ///   * If `end < begin`
    ///   * If `end` is beyond the length of the span
    pub fn subspan(&self, begin: u64, end: u64) -> Span {
        assert!(end >= begin);
        assert!(self.low + end <= self.high);
        Span {
            file: self.file,
            low: self.low + begin,
            high: self.low + end,
        }
    }

    /// Checks if a span is contained within this span (and refers to the same file).
    pub fn contains(&self, other: Span) -> bool {
        self.file == other.file && self.low <= other.low && self.high >= other.high
    }

    /// The file this span points into.
    pub fn file(&self) -> FileId {
        self.file
    }

    /// The offset of the first byte of the span, relative to the file's start.
    pub fn low(&self) -> Pos {
        self.low
    }

    /// The offset after the last byte of the span, relative to the file's start.
    pub fn high(&self) -> Pos {
        self.high
    }

    /// The length in bytes of the text of the span
    pub fn len(&self) -> u64 {
        self.high - self.low
    }

    /// Whether the span is empty (zero-length).
    pub fn is_empty(&self) -> bool {
        self.high == self.low
    }

    /// Create a span that encloses both `self` and `other`.
    ///
    /// Both spans must refer to the same file; merging across files is a bug, as
    /// the resulting range would be meaningless.
    pub fn merge(&self, other: Span) -> Span {
        debug_assert_eq!(
            self.file, other.file,
            "cannot merge spans from different files"
        );
        Span {
            file: self.file,
            low: cmp::min(self.low, other.low),
            high: cmp::max(self.high, other.high),
        }
    }
}

/// A byte offset into the virtual concatenation of every file in a [`CodeMap`],
/// taken in registry ([`FileId`]) order with no separators: file 0 starts at 0,
/// file 1 starts at `len(file 0)`, and so on.
///
/// This is the "whole-codemap" counterpart to the file-local [`Pos`]. Convert
/// between the two with [`CodeMap::pos_to_global`] / [`CodeMap::pos_from_global`].
#[derive(Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd, Debug, Default)]
pub struct GlobalPos(pub u64);

impl Add<u64> for GlobalPos {
    type Output = GlobalPos;
    fn add(self, other: u64) -> GlobalPos {
        GlobalPos(self.0 + other)
    }
}

/// A range in the virtual concatenation of every file in a [`CodeMap`].
///
/// This is the "whole-codemap" counterpart to the file-local [`Span`]. Unlike
/// `Span` it carries no file identity, because it is expressed purely in global
/// coordinates. Convert with [`CodeMap::span_to_global`] /
/// [`CodeMap::span_from_global`].
#[derive(Copy, Clone, Hash, Eq, PartialEq, Debug)]
pub struct GlobalSpan {
    pub low: GlobalPos,
    pub high: GlobalPos,
}

impl GlobalSpan {
    pub fn new(low: u64, high: u64) -> GlobalSpan {
        GlobalSpan {
            low: GlobalPos(low),
            high: GlobalPos(high),
        }
    }

    /// The length in bytes of the range.
    pub fn len(&self) -> u64 {
        self.high.0 - self.low.0
    }

    /// Whether the range is empty (zero-length).
    pub fn is_empty(&self) -> bool {
        self.high.0 == self.low.0
    }
}

/// Associate a Span with a value of arbitrary type (e.g. an AST node).
#[derive(Clone, PartialEq, Eq, Hash, Debug, Copy)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Span,
}

impl<T> Spanned<T> {
    /// Maps a `Spanned<T>` to `Spanned<U>` by applying the function to the node,
    /// leaving the span untouched.
    pub fn map_node<U, F: FnOnce(T) -> U>(self, op: F) -> Spanned<U> {
        Spanned {
            node: op(self.node),
            span: self.span,
        }
    }
}

impl<T> Deref for Spanned<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.node
    }
}

/// A registry mapping [`FileId`]s to the source files they identify.
///
/// Each file owns its own 0-based coordinate space, so the `CodeMap` only has to
/// remember which file is which — it never assigns global offsets or searches by
/// position.
#[derive(Default, Debug)]
pub struct CodeMap {
    pub files: Vec<Arc<CodeFile>>,
}

impl CodeMap {
    /// Creates an empty `CodeMap`.
    pub fn new() -> CodeMap {
        Default::default()
    }

    /// Builds the line table (byte offset of the start of each line) and the
    /// whole-file span for a freshly registered file.
    fn build_file(id: FileId, name: Arc<String>, source: Arc<String>) -> Arc<CodeFile> {
        let high = Pos(source.len() as u32);
        let mut lines = vec![Pos(0)];
        lines.extend(source.match_indices('\n').map(|(p, _)| Pos((p + 1) as u32)));

        Arc::new(CodeFile {
            span: Span {
                file: id,
                low: Pos(0),
                high,
            },
            name,
            source,
            lines,
        })
    }

    /// Adds a file with the given name and contents. It will not replace files with the same name.
    /// Use the returned `File` and its `.span` property to create `Spans`
    /// representing substrings of the file.
    pub fn add_file(&mut self, name: Arc<String>, source: Arc<String>) -> Arc<CodeFile> {
        let id = FileId(self.files.len() as u32);
        let file = Self::build_file(id, name, source);
        self.files.push(file.clone());
        file
    }

    /// Adds a file with the given name and contents, replacing any existing file with the same name.
    ///
    /// The replacement keeps the original file's [`FileId`] so that spans created
    /// against earlier files remain valid (registry indices never shift).
    pub fn add_or_replace_file(&mut self, name: Arc<String>, source: Arc<String>) -> Arc<CodeFile> {
        if let Some(idx) = self.files.iter().position(|f| *f.name == *name) {
            let file = Self::build_file(FileId(idx as u32), name, source);
            self.files[idx] = file.clone();
            file
        } else {
            self.add_file(name, source)
        }
    }

    fn unknown_file() -> Arc<CodeFile> {
        Arc::new(CodeFile {
            span: Span {
                file: FileId::DETACHED,
                low: Pos(0),
                high: Pos(0),
            },
            name: Arc::new("<unknown>".to_owned()),
            source: Arc::new(String::new()),
            lines: vec![Pos(0)],
        })
    }

    /// Looks up the `File` with the specified name.
    pub fn get_file(&self, file_name: &str) -> Option<&Arc<CodeFile>> {
        self.files.iter().find(|file| **file.name == *file_name)
    }

    /// Looks up the `File` a span refers to, by its [`FileId`].
    pub fn get_file_of(&self, span: Span) -> Option<&Arc<CodeFile>> {
        self.files.get(span.file.index())
    }

    /// Gets the file, line, and column of a span's start position.
    pub fn look_up_pos(&self, span: Span) -> Loc {
        match self.get_file_of(span) {
            Some(file) => Loc {
                file: file.clone(),
                position: file.find_line_col(file.clamp(span.low)),
            },
            None => Loc {
                file: Self::unknown_file(),
                position: LineCol { line: 0, column: 0 },
            },
        }
    }

    /// Gets the file and its line and column ranges represented by a `Span`.
    ///
    /// The span names its own file directly via its [`FileId`], so resolution is a
    /// simple registry lookup — no offset subtraction and no risk of bleeding into
    /// a neighbouring file. As a final safety net the offsets are clamped into the
    /// file's bounds, so even a malformed span always yields a location inside the
    /// file it claims to belong to.
    pub fn look_up_span(&self, span: Span) -> SpanLoc {
        match self.get_file_of(span) {
            Some(file) => {
                let low = file.clamp(span.low);
                let high = file.clamp(cmp::max(span.high, low));
                SpanLoc {
                    file: file.clone(),
                    begin: file.find_line_col(low),
                    end: file.find_line_col(high),
                }
            }
            None => SpanLoc {
                file: Self::unknown_file(),
                begin: LineCol { line: 0, column: 0 },
                end: LineCol { line: 0, column: 0 },
            },
        }
    }

    // ---- Conversions between file-local and whole-codemap coordinates ---------
    //
    // Spans and positions are stored file-locally (each file is its own 0-based
    // space). These helpers project them onto, or recover them from, the virtual
    // concatenation of all files in registry order. Nothing in normal error
    // reporting needs the global view; it exists for callers that want to address
    // the whole corpus as one sequence.

    /// The total length, in bytes, of every file in the map concatenated together.
    pub fn global_len(&self) -> u64 {
        self.files.iter().map(|f| f.len()).sum()
    }

    /// The global start offset of `file` within the concatenated sequence, or
    /// `None` if the id is not registered.
    pub fn global_start_of(&self, file: FileId) -> Option<GlobalPos> {
        let idx = file.index();
        if idx >= self.files.len() {
            return None;
        }
        Some(GlobalPos(self.files[..idx].iter().map(|f| f.len()).sum()))
    }

    /// Converts a file-local position into its offset in the concatenated sequence.
    pub fn pos_to_global(&self, file: FileId, local: Pos) -> Option<GlobalPos> {
        let base = self.global_start_of(file)?;
        Some(self.files[file.index()].pos_to_global(local, base))
    }

    /// Converts a file-local [`Span`] into a [`GlobalSpan`] in the concatenated
    /// sequence.
    pub fn span_to_global(&self, span: Span) -> Option<GlobalSpan> {
        let base = self.global_start_of(span.file())?;
        let file = self.get_file_of(span)?;
        Some(GlobalSpan {
            low: file.pos_to_global(span.low(), base),
            high: file.pos_to_global(span.high(), base),
        })
    }

    /// Finds the file and file-local position a global offset falls in.
    ///
    /// An offset on a file boundary belongs to the file that *starts* there; the
    /// very end of the sequence maps to the end of the last file. Returns `None`
    /// if the offset is past the end of the whole sequence.
    pub fn pos_from_global(&self, global: GlobalPos) -> Option<(FileId, Pos)> {
        let (idx, base) = self.locate(global.0)?;
        let file = &self.files[idx];
        let local = file.global_to_local(global, base)?;
        Some((file.span.file(), local))
    }

    /// Converts a [`GlobalSpan`] back into a file-local [`Span`].
    ///
    /// The file is the one containing the span's start. If the range extends past
    /// that file (because it was built across the concatenation boundary), the end
    /// is clamped into the file so the result remains a valid single-file span.
    pub fn span_from_global(&self, global: GlobalSpan) -> Option<Span> {
        let (idx, base) = self.locate(global.low.0)?;
        let file = &self.files[idx];
        let low = file.global_to_local(global.low, base)?;
        let file_end = base + file.len();
        let clamped_high = cmp::min(global.high, file_end);
        let high = cmp::max(file.global_to_local(clamped_high, base)?, low);
        Some(Span::new(file.span.file(), *low, *high))
    }

    /// Locates the registry index and global start offset of the file a global
    /// position falls in, applying the boundary rule documented on
    /// [`CodeMap::pos_from_global`].
    fn locate(&self, global: u64) -> Option<(usize, GlobalPos)> {
        let mut start = 0u64;
        for (i, file) in self.files.iter().enumerate() {
            let end = start + file.len();
            let is_last = i + 1 == self.files.len();
            if global < end || (is_last && global <= end) {
                return Some((i, GlobalPos(start)));
            }
            start = end;
        }
        None
    }
}

/// A `CodeMap`'s record of a source file.
pub struct CodeFile {
    /// The span representing the entire file (always starting at offset 0).
    pub span: Span,

    /// The filename as it would be displayed in an error message.
    name: Arc<String>,

    /// Contents of the file.
    source: Arc<String>,

    /// File-relative byte offsets of line beginnings (the first is always 0).
    lines: Vec<Pos>,
}

impl CodeFile {
    /// Gets the name of the file
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Clamps a file-relative position into this file's bounds (`0..=len`).
    fn clamp(&self, pos: Pos) -> Pos {
        cmp::min(cmp::max(pos, self.span.low), self.span.high)
    }

    /// Gets the line number of a file-relative `Pos`.
    ///
    /// The lines are 0-indexed (first line is numbered 0)
    ///
    /// # Panics
    ///
    ///  * If `pos` is not within this file's span
    pub fn find_line(&self, pos: Pos) -> usize {
        assert!(pos >= self.span.low);
        assert!(pos <= self.span.high);
        match self.lines.binary_search(&pos) {
            Ok(i) => i,
            Err(i) => i - 1,
        }
    }

    /// Gets the line and column of a file-relative `Pos`.
    ///
    /// # Panics
    ///
    /// * If `pos` is not with this file's span
    /// * If `pos` points to a byte in the middle of a UTF-8 character
    pub fn find_line_col(&self, pos: Pos) -> LineCol {
        let line = self.find_line(pos);
        let line_span = self.line_span(line);
        let byte_col = pos - line_span.low;
        let column = self.source_slice(line_span)[..byte_col as usize]
            .chars()
            .count();

        LineCol { line, column }
    }

    /// Gets the full source text of the file
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Gets the source text of a Span.
    ///
    /// # Panics
    ///
    ///   * If `span` is not entirely within this file.
    pub fn source_slice(&self, span: Span) -> &str {
        assert!(self.span.contains(span));
        &self.source[(span.low.0 as usize)..(span.high.0 as usize)]
    }

    /// Gets the span representing a line by line number.
    ///
    /// The line number is 0-indexed (first line is numbered 0). The returned span includes the
    /// line terminator.
    ///
    /// # Panics
    ///
    ///  * If the line number is out of range
    pub fn line_span(&self, line: usize) -> Span {
        assert!(line < self.lines.len());
        let high = if let Some(high) = self.lines.get(line + 1) {
            *high
        } else {
            self.span.high
        };
        Span {
            file: self.span.file,
            low: self.lines[line],
            high,
        }
    }

    /// Gets the source text of a line.
    ///
    /// The string returned does not include the terminating \r or \n characters.
    ///
    /// # Panics
    ///
    ///  * If the line number is out of range
    pub fn source_line(&self, line: usize) -> &str {
        self.source_slice(self.line_span(line))
            .trim_end_matches(&['\n', '\r'][..])
    }

    /// Gets the number of lines in the file
    pub fn num_lines(&self) -> usize {
        self.lines.len()
    }

    /// The length of the file in bytes.
    pub fn len(&self) -> u64 {
        u64::from(self.span.high.0)
    }

    /// Whether the file is empty (has no source bytes).
    pub fn is_empty(&self) -> bool {
        self.span.high.0 == 0
    }

    /// Projects a file-local position onto whole-codemap coordinates, given this
    /// file's global start offset (see [`CodeMap::global_start_of`]).
    ///
    /// The position is clamped into the file first, so the result is always inside
    /// `base..=base + len`.
    pub fn pos_to_global(&self, local: Pos, base: GlobalPos) -> GlobalPos {
        base + u64::from(self.clamp(local).0)
    }

    /// Recovers a file-local position from whole-codemap coordinates, given this
    /// file's global start offset. Returns `None` if `global` does not fall within
    /// this file's slice of the sequence.
    pub fn global_to_local(&self, global: GlobalPos, base: GlobalPos) -> Option<Pos> {
        if global < base {
            return None;
        }
        let local = global.0 - base.0;
        if local > self.len() {
            return None;
        }
        Some(Pos(local as u32))
    }
}

impl fmt::Debug for CodeFile {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "File({:?})", self.name)
    }
}

impl PartialEq for CodeFile {
    /// Compares by identity
    fn eq(&self, other: &CodeFile) -> bool {
        std::ptr::eq(self, other)
    }
}

impl Eq for CodeFile {}

impl Hash for CodeFile {
    fn hash<H: Hasher>(&self, hasher: &mut H) {
        self.span.hash(hasher);
    }
}

/// A line and column.
#[derive(Copy, Clone, Hash, Eq, PartialEq, Debug)]
pub struct LineCol {
    /// The line number within the file (0-indexed).
    pub line: usize,

    /// The column within the line (0-indexed).
    pub column: usize,
}

/// A file, and a line and column within it.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct Loc {
    pub file: Arc<CodeFile>,
    pub position: LineCol,
}

impl fmt::Display for Loc {
    /// Formats the location as `filename:line:column`, with a 1-indexed
    /// line and column.
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(
            f,
            "{}:{}:{}",
            self.file.name,
            self.position.line + 1,
            self.position.column + 1
        )
    }
}

/// A file, and a line and column range within it.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct SpanLoc {
    pub file: Arc<CodeFile>,
    pub begin: LineCol,
    pub end: LineCol,
}

impl fmt::Display for SpanLoc {
    /// Formats the span as `filename:start_line:start_column: end_line:end_column`,
    /// or if the span is zero-length, `filename:line:column`, with a 1-indexed line and column.
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        if self.begin == self.end {
            write!(
                f,
                "{}:{}:{}",
                self.file.name,
                self.begin.line + 1,
                self.begin.column + 1
            )
        } else {
            write!(
                f,
                "{}:{}:{}: {}:{}",
                self.file.name,
                self.begin.line + 1,
                self.begin.column + 1,
                self.end.line + 1,
                self.end.column + 1
            )
        }
    }
}

#[test]
fn test_codemap() {
    let mut codemap = CodeMap::new();
    let f1 = codemap.add_file(
        Arc::new("test1.rs".to_string()),
        Arc::new("abcd\nefghij\nqwerty".to_string()),
    );
    let f2 = codemap.add_file(
        Arc::new("test2.rs".to_string()),
        Arc::new("foo\nbar".to_string()),
    );

    // Each file's span resolves to itself, and both start at offset 0.
    assert_eq!(f1.span.low(), Pos(0));
    assert_eq!(f2.span.low(), Pos(0));
    assert_eq!(
        codemap.get_file_of(f1.span).map(|f| f.name()),
        Some("test1.rs")
    );
    assert_eq!(
        codemap.get_file_of(f2.span).map(|f| f.name()),
        Some("test2.rs")
    );

    let x = f1.span.subspan(5, 10);
    let f = codemap.get_file_of(x);
    assert_eq!(f.map(|f| f.name.as_str()), Some("test1.rs"));
    if let Some(f) = f {
        assert_eq!(
            f.find_line_col(f.span.low()),
            LineCol { line: 0, column: 0 }
        );
        assert_eq!(
            f.find_line_col(f.span.low() + 4),
            LineCol { line: 0, column: 4 }
        );
        assert_eq!(
            f.find_line_col(f.span.low() + 5),
            LineCol { line: 1, column: 0 }
        );
        assert_eq!(
            f.find_line_col(f.span.low() + 16),
            LineCol { line: 2, column: 4 }
        );
    } else {
        panic!("file not found");
    }
}

#[test]
fn test_issue2() {
    let mut codemap = CodeMap::new();
    let content = "a \nxyz\r\n";
    let file = codemap.add_file(Arc::new("<test>".to_owned()), Arc::new(content.to_owned()));

    let span = file.span.subspan(2, 3);
    assert_eq!(
        codemap.look_up_span(span),
        SpanLoc {
            file: file.clone(),
            begin: LineCol { line: 0, column: 2 },
            end: LineCol { line: 1, column: 0 }
        }
    );

    assert_eq!(file.source_line(0), "a ");
    assert_eq!(file.source_line(1), "xyz");
    assert_eq!(file.source_line(2), "");
}

#[test]
fn test_multibyte() {
    let mut codemap = CodeMap::new();
    let content = "65°00′N 18°00′W 汉语\n🔬";
    let file = codemap.add_file(Arc::new("<test>".to_owned()), Arc::new(content.to_owned()));

    assert_eq!(
        codemap.look_up_pos(file.span.subspan(21, 21)),
        Loc {
            file: file.clone(),
            position: LineCol {
                line: 0,
                column: 15
            }
        }
    );
    assert_eq!(
        codemap.look_up_pos(file.span.subspan(28, 28)),
        Loc {
            file: file.clone(),
            position: LineCol {
                line: 0,
                column: 18
            }
        }
    );
    assert_eq!(
        codemap.look_up_pos(file.span.subspan(33, 33)),
        Loc {
            file: file.clone(),
            position: LineCol { line: 1, column: 1 }
        }
    );
}

#[test]
fn test_positions_are_file_local_regardless_of_order() {
    // The same file content yields the same location no matter where it sits in
    // the registry, because spans are file-relative and name their own file.
    let render = |first: &str, second: &str| {
        let mut codemap = CodeMap::new();
        codemap.add_file(
            Arc::new("first.scss".to_owned()),
            Arc::new(first.to_owned()),
        );
        let target = codemap.add_file(
            Arc::new("target.scss".to_owned()),
            Arc::new(second.to_owned()),
        );
        // Bytes 13..34 of `target` — the `@use` rule on its line 2.
        codemap.look_up_span(target.span.subspan(13, 34))
    };

    let content = "/* File 2 */\n@use \"New Style Comp\";\na { b: c }\n";
    let small_first = render("x", content);
    let big_first = render(&"y\n".repeat(500), content);

    assert_eq!(small_first.file.name(), "target.scss");
    assert_eq!(small_first.begin, LineCol { line: 1, column: 0 });
    assert_eq!(
        small_first.end,
        LineCol {
            line: 1,
            column: 21
        }
    );
    // Identical despite a wildly different preceding file.
    assert_eq!(small_first.begin, big_first.begin);
    assert_eq!(small_first.end, big_first.end);
}

#[test]
fn test_span_clamped_to_its_file() {
    let mut codemap = CodeMap::new();
    let file = codemap.add_file(
        Arc::new("a.scss".to_owned()),
        Arc::new("abc\ndef".to_owned()),
    );

    // A malformed span whose end runs past the file's length. It must still
    // resolve to a location inside this file rather than panicking.
    let bad_span = Span::new(file.span.file(), 0, 999);
    let loc = codemap.look_up_span(bad_span);

    assert_eq!(loc.file.name(), "a.scss");
    assert_eq!(loc.begin, LineCol { line: 0, column: 0 });
    assert_eq!(loc.end, LineCol { line: 1, column: 3 });
}

// Files: a.scss (7 bytes), b.scss (3 bytes), c.scss (5 bytes).
// Concatenated, they occupy global ranges [0,7), [7,10), [10,15).
#[cfg(test)]
fn sample_map() -> CodeMap {
    let mut m = CodeMap::new();
    m.add_file(
        Arc::new("a.scss".to_owned()),
        Arc::new("abc\ndef".to_owned()),
    );
    m.add_file(Arc::new("b.scss".to_owned()), Arc::new("xyz".to_owned()));
    m.add_file(Arc::new("c.scss".to_owned()), Arc::new("hello".to_owned()));
    m
}

#[cfg(test)]
fn file_ids(m: &CodeMap) -> Vec<FileId> {
    m.files.iter().map(|f| f.span.file()).collect()
}

#[test]
fn test_global_offsets_are_sequential() {
    let m = sample_map();
    let ids = file_ids(&m);
    assert_eq!(m.global_start_of(ids[0]), Some(GlobalPos(0)));
    assert_eq!(m.global_start_of(ids[1]), Some(GlobalPos(7)));
    assert_eq!(m.global_start_of(ids[2]), Some(GlobalPos(10)));
    assert_eq!(m.global_len(), 15);
}

#[test]
fn test_pos_global_round_trip() {
    let m = sample_map();
    let ids = file_ids(&m);

    for (i, file) in m.files.iter().enumerate() {
        // For non-last files, the EOF position aliases the next file's start, so
        // only interior positions round-trip to the same file-local pair.
        let max = if i + 1 == m.files.len() {
            file.len()
        } else {
            file.len() - 1
        };
        for local in 0..=max as u32 {
            let g = m.pos_to_global(ids[i], Pos(local)).unwrap();
            assert_eq!(m.pos_from_global(g), Some((ids[i], Pos(local))));
        }
    }
}

#[test]
fn test_pos_from_global_boundaries() {
    let m = sample_map();
    let ids = file_ids(&m);

    // A boundary offset belongs to the file that *starts* there.
    assert_eq!(m.pos_from_global(GlobalPos(7)), Some((ids[1], Pos(0))));
    assert_eq!(m.pos_from_global(GlobalPos(10)), Some((ids[2], Pos(0))));
    // The very end of the sequence maps to the end of the last file.
    assert_eq!(m.pos_from_global(GlobalPos(15)), Some((ids[2], Pos(5))));
    // Past the end is unmapped.
    assert_eq!(m.pos_from_global(GlobalPos(16)), None);
    // The EOF of a non-last file aliases the next file's start.
    let a_eof = m.pos_to_global(ids[0], Pos(7)).unwrap();
    assert_eq!(a_eof, GlobalPos(7));
    assert_eq!(m.pos_from_global(a_eof), Some((ids[1], Pos(0))));
}

#[test]
fn test_span_global_round_trip() {
    let m = sample_map();

    for file in &m.files {
        // The whole-file span.
        let whole = file.span;
        let g = m.span_to_global(whole).unwrap();
        assert_eq!(m.span_from_global(g), Some(whole));

        // An interior sub-span.
        let sub = file.span.subspan(1, 3);
        let g = m.span_to_global(sub).unwrap();
        assert_eq!(m.span_from_global(g), Some(sub));
    }

    // Concrete check of the projected coordinates for b.scss (global base 7).
    let ids = file_ids(&m);
    let b_sub = m.files[1].span.subspan(0, 3);
    assert_eq!(m.span_to_global(b_sub), Some(GlobalSpan::new(7, 10)));
    assert_eq!(
        m.span_from_global(GlobalSpan::new(7, 10)).map(|s| s.file()),
        Some(ids[1])
    );
}

#[test]
fn test_span_from_global_clamps_across_boundary() {
    let m = sample_map();
    let ids = file_ids(&m);

    // Global [5, 9) starts in a.scss (offset 5) but would run into b.scss. The
    // result is clamped to a.scss so it stays a valid single-file span.
    let span = m.span_from_global(GlobalSpan::new(5, 9)).unwrap();
    assert_eq!(span.file(), ids[0]);
    assert_eq!(span.low(), Pos(5));
    assert_eq!(span.high(), Pos(7));

    assert_eq!(m.span_from_global(GlobalSpan::new(100, 200)), None);
}

#[test]
fn test_codefile_global_conversions() {
    let m = sample_map();
    let b = &m.files[1]; // "xyz"
    let base = m.global_start_of(b.span.file()).unwrap();

    assert_eq!(base, GlobalPos(7));
    assert_eq!(b.len(), 3);
    assert!(!b.is_empty());

    assert_eq!(b.pos_to_global(Pos(1), base), GlobalPos(8));
    assert_eq!(b.global_to_local(GlobalPos(8), base), Some(Pos(1)));
    assert_eq!(b.global_to_local(GlobalPos(7), base), Some(Pos(0)));
    assert_eq!(b.global_to_local(GlobalPos(10), base), Some(Pos(3))); // EOF
    assert_eq!(b.global_to_local(GlobalPos(6), base), None); // before file
    assert_eq!(b.global_to_local(GlobalPos(11), base), None); // after file
}

#[test]
fn test_global_conversions_reject_unknown_file() {
    let m = sample_map();
    assert_eq!(m.global_start_of(FileId::DETACHED), None);
    assert_eq!(m.pos_to_global(FileId::DETACHED, Pos(0)), None);
    assert_eq!(m.span_to_global(Span::new(FileId::DETACHED, 0, 0)), None);
}

#[test]
fn test_empty_file_conversions() {
    let mut m = CodeMap::new();
    let f = m.add_file(Arc::new("e.scss".to_owned()), Arc::new(String::new()));
    assert!(f.is_empty());
    assert_eq!(f.len(), 0);
    assert_eq!(m.global_len(), 0);
    assert_eq!(m.global_start_of(f.span.file()), Some(GlobalPos(0)));
    assert_eq!(
        m.pos_from_global(GlobalPos(0)),
        Some((f.span.file(), Pos(0)))
    );
}
