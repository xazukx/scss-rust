use crate::ast::*;

pub(crate) use at_root_query::AtRootQueryParser;
pub use base::BaseParser;
pub use css::CssParser;
pub(crate) use keyframes::KeyframesSelectorParser;
pub(crate) use media_query::MediaQueryParser;
pub use sass::SassParser;
pub use scss::ScssParser;
pub use stylesheet::StylesheetParser;

mod at_root_query;
mod base;
mod css;
mod keyframes;
mod media_query;
mod sass;
mod scss;
mod stylesheet;
mod value;

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum DeclarationOrBuffer {
    Stmt(AstStmt),
    Buffer(Interpolation),
}

/// Names that functions are not allowed to have
pub(super) const RESERVED_IDENTIFIERS: [&str; 8] = [
    "calc",
    "element",
    "expression",
    "url",
    "and",
    "or",
    "not",
    "clamp",
];

#[derive(Debug, Clone)]
pub enum VariableDeclOrInterpolation {
    VariableDecl(AstVariableDecl),
    Interpolation(Interpolation),
}
