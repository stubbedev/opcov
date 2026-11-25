use mago_names::ResolvedNames;
use mago_syntax::cst::{Access, ClassLikeConstantSelector, Expression, Literal};

const CLASS_CONSTANT: &[u8] = b"class";

pub(crate) fn string_literal(expression: &Expression<'_>) -> Option<String> {
    let Expression::Literal(Literal::String(string)) = expression else {
        return None;
    };
    return string
        .value
        .map(|value| return String::from_utf8_lossy(value).into_owned());
}

pub(crate) fn class_constant(
    names: &ResolvedNames<'_>,
    expression: &Expression<'_>,
) -> Option<String> {
    let Expression::Access(Access::ClassConstant(access)) = expression else {
        return None;
    };
    let (ClassLikeConstantSelector::Identifier(constant), Expression::Identifier(class)) =
        (&access.constant, access.class)
    else {
        return None;
    };
    return constant
        .value
        .eq_ignore_ascii_case(CLASS_CONSTANT)
        .then(|| return String::from_utf8_lossy(names.get(class)).into_owned());
}

pub(crate) fn class_or_string(
    names: &ResolvedNames<'_>,
    expression: &Expression<'_>,
) -> Option<String> {
    return class_constant(names, expression).or_else(|| return string_literal(expression));
}
