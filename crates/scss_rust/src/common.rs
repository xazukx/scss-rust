use std::cmp::Ordering;
use std::fmt::{self, Display, Write};
use std::hash::{Hash, Hasher};

use crate::interner::InternedString;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum UnaryOp {
    Plus,
    Neg,
    Div,
    Not,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BinaryOp {
    SingleEq,
    Equal,
    NotEqual,
    GreaterThan,
    GreaterThanEqual,
    LessThan,
    LessThanEqual,
    Plus,
    Minus,
    Mul,
    Div,
    Rem,
    And,
    Or,
}

impl BinaryOp {
    pub fn precedence(self) -> u8 {
        match self {
            Self::SingleEq => 0,
            Self::Or => 1,
            Self::And => 2,
            Self::Equal | Self::NotEqual => 3,
            Self::GreaterThan | Self::GreaterThanEqual | Self::LessThan | Self::LessThanEqual => 4,
            Self::Plus | Self::Minus => 5,
            Self::Mul | Self::Div | Self::Rem => 6,
        }
    }
}

impl Display for BinaryOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BinaryOp::SingleEq => write!(f, "="),
            BinaryOp::Equal => write!(f, "=="),
            BinaryOp::NotEqual => write!(f, "!="),
            BinaryOp::GreaterThanEqual => write!(f, ">="),
            BinaryOp::LessThanEqual => write!(f, "<="),
            BinaryOp::GreaterThan => write!(f, ">"),
            BinaryOp::LessThan => write!(f, "<"),
            BinaryOp::Plus => write!(f, "+"),
            BinaryOp::Minus => write!(f, "-"),
            BinaryOp::Mul => write!(f, "*"),
            BinaryOp::Div => write!(f, "/"),
            BinaryOp::Rem => write!(f, "%"),
            BinaryOp::And => write!(f, "and"),
            BinaryOp::Or => write!(f, "or"),
        }
    }
}

/// Strings can either have quotes or not
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum QuoteKind {
    Quoted,
    None,
}

impl Display for QuoteKind {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Quoted => f.write_char('"'),
            Self::None => Ok(()),
        }
    }
}

/// Lists can either be bracketed or not
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Brackets {
    None,
    Bracketed,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ListSeparator {
    Space,
    Comma,
    Slash,
    Undecided,
}

impl ListSeparator {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Space | Self::Undecided => " ",
            Self::Comma => ", ",
            Self::Slash => " / ",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Space | Self::Undecided => "space",
            Self::Comma => "comma",
            Self::Slash => "slash",
        }
    }
}

/// In Sass, underscores and hyphens are considered equal when inside identifiers.
///
/// The first field (`original`) preserves the original spelling and is what
/// gets displayed / resolved as `as_str`. The second field (`canonical`) is
/// interned with underscores normalized to hyphens, and all of `PartialEq`,
/// `Eq`, `Hash`, `PartialOrd`, and `Ord` operate on it — so two identifiers
/// that differ only in `_` vs `-` compare and hash equal.
#[derive(Clone, Copy)]
pub struct Identifier {
    original: InternedString,
    canonical: InternedString,
}

impl fmt::Debug for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Identifier")
            .field(&self.original.to_string())
            .finish()
    }
}

impl PartialEq for Identifier {
    fn eq(&self, other: &Self) -> bool {
        self.canonical == other.canonical
    }
}

impl Eq for Identifier {}

impl Hash for Identifier {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.canonical.hash(state);
    }
}

impl Ord for Identifier {
    fn cmp(&self, other: &Self) -> Ordering {
        self.canonical.cmp(&other.canonical)
    }
}

impl PartialOrd for Identifier {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Identifier {
    fn from_str(s: &str) -> Self {
        let original = InternedString::get_or_intern(s);
        let canonical = InternedString::get_or_intern(s.replace('_', "-"));
        Identifier {
            original,
            canonical,
        }
    }

    pub fn is_public(&self) -> bool {
        // Identifiers that start with `_` or `-` are considered private in
        // Sass; check the canonical form (which normalizes `_` to `-`) so
        // both prefixes are caught uniformly.
        !self.canonical_str().starts_with('-')
    }
}

impl From<String> for Identifier {
    fn from(s: String) -> Identifier {
        Self::from_str(&s)
    }
}

impl From<&String> for Identifier {
    fn from(s: &String) -> Identifier {
        Self::from_str(s)
    }
}

impl From<&str> for Identifier {
    fn from(s: &str) -> Identifier {
        Self::from_str(s)
    }
}

impl Display for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.original)
    }
}

impl Identifier {
    pub fn as_str(&self) -> &str {
        self.original.resolve_ref()
    }

    /// Returns the canonical form of this identifier (underscores normalized
    /// to hyphens). Use this for keying into maps that store identifiers in
    /// their hyphenated form (e.g. `GLOBAL_FUNCTIONS`).
    pub fn canonical_str(&self) -> &str {
        self.canonical.resolve_ref()
    }
}

/// Returns `name` without a vendor prefix (-moz-, -webkit-, etc).
///
/// If `name` has no vendor prefix, it's returned as-is.
pub(crate) fn unvendor(name: &str) -> &str {
    let bytes = name.as_bytes();

    if bytes.len() < 2 {
        return name;
    }

    if bytes.first() != Some(&b'-') || bytes.get(1_usize) == Some(&b'-') {
        return name;
    }

    for i in 2..bytes.len() {
        if bytes.get(i) == Some(&b'-') {
            return &name[i + 1..];
        }
    }

    name
}
