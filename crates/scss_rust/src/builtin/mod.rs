mod functions;
pub(crate) mod modules;

pub(crate) use functions::{
    DISALLOWED_PLAIN_CSS_FUNCTION_NAMES, GLOBAL_FUNCTIONS, color, list, map, math, meta, selector,
    string,
};

pub use functions::Builtin;

/// Imports common to all builtin fns
mod builtin_imports {
    pub(crate) use super::functions::{Builtin, GLOBAL_FUNCTIONS, GlobalFunctionMap};

    pub(crate) use crate::codemap::{Span, Spanned};

    #[cfg(feature = "random")]
    pub(crate) use rand::{RngExt, distr::Alphanumeric, rng};

    pub(crate) use crate::{
        Options,
        ast::{Argument, ArgumentDeclaration, ArgumentResult, MaybeEvaledArguments},
        color::Color,
        common::{BinaryOp, Brackets, Identifier, ListSeparator, QuoteKind},
        error::SassResult,
        evaluate::Visitor,
        unit::Unit,
        value::{CalculationArg, Number, SassFunction, SassMap, SassNumber, Value},
    };

    pub(crate) use std::{
        cmp::Ordering,
        collections::{BTreeMap, BTreeSet},
        sync::Arc,
    };
}
