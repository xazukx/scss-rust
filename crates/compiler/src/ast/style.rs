use crate::codemap::Spanned;

use crate::{interner::InternedString, value::Value};

/// A style: `color: red`
#[derive(Clone, Debug)]
pub struct Style {
    pub property: InternedString,
    pub value: Box<Spanned<Value>>,
    pub declared_as_custom_property: bool,
}
