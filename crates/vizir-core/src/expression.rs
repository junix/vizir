use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::Diagnostic;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum SpatialUnit {
    SceneUnit,
    Pixel,
    Point,
    Millimeter,
    NormalizedViewWidth,
    NormalizedViewHeight,
    DataUnit,
    WorldMeter,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ValueType {
    Null,
    Bool,
    Int64,
    Float64,
    String,
    Color,
    Length { space: String, unit: SpatialUnit },
    Position2 { space: String },
    Array { items: Box<ValueType> },
    Option { item: Box<ValueType> },
    Record { fields: BTreeMap<String, ValueType> },
}

impl ValueType {
    pub fn option(item: ValueType) -> Self {
        Self::Option {
            item: Box::new(item),
        }
    }

    fn is_numeric(&self) -> bool {
        matches!(self, Self::Int64 | Self::Float64)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "kebab-case",
    deny_unknown_fields
)]
pub enum LiteralValue {
    Null,
    Bool(bool),
    Int64(i64),
    Float64(f64),
    String(String),
    Color(String),
}

impl LiteralValue {
    pub fn value_type(&self) -> ValueType {
        match self {
            Self::Null => ValueType::Null,
            Self::Bool(_) => ValueType::Bool,
            Self::Int64(_) => ValueType::Int64,
            Self::Float64(_) => ValueType::Float64,
            Self::String(_) => ValueType::String,
            Self::Color(_) => ValueType::Color,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PureFunction {
    Abs,
    Min,
    Max,
    Clamp,
    Length,
    Lower,
    Upper,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "op", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Expression {
    Literal {
        value: LiteralValue,
    },
    Field {
        row: String,
        field: String,
    },
    Parameter {
        id: String,
    },
    Signal {
        id: String,
    },
    Array {
        items: Vec<Expression>,
    },
    Record {
        fields: BTreeMap<String, Expression>,
    },
    Add {
        left: Box<Expression>,
        right: Box<Expression>,
    },
    Subtract {
        left: Box<Expression>,
        right: Box<Expression>,
    },
    Multiply {
        left: Box<Expression>,
        right: Box<Expression>,
    },
    Divide {
        left: Box<Expression>,
        right: Box<Expression>,
    },
    Equal {
        left: Box<Expression>,
        right: Box<Expression>,
    },
    LessThan {
        left: Box<Expression>,
        right: Box<Expression>,
    },
    And {
        args: Vec<Expression>,
    },
    Or {
        args: Vec<Expression>,
    },
    Not {
        arg: Box<Expression>,
    },
    IsNull {
        arg: Box<Expression>,
    },
    If {
        condition: Box<Expression>,
        then_value: Box<Expression>,
        else_value: Box<Expression>,
    },
    Call {
        function: PureFunction,
        args: Vec<Expression>,
    },
    Convert {
        value: Box<Expression>,
        to: ValueType,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TypedExpression {
    pub result_type: ValueType,
    pub expression: Expression,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TypeEnvironment {
    pub rows: BTreeMap<String, BTreeMap<String, ValueType>>,
    pub parameters: BTreeMap<String, ValueType>,
    pub signals: BTreeMap<String, ValueType>,
}

pub fn type_expression(
    expression: Expression,
    environment: &TypeEnvironment,
) -> Result<TypedExpression, Diagnostic> {
    let result_type = infer_type(&expression, environment, "expression")?;
    Ok(TypedExpression {
        result_type,
        expression,
    })
}

fn infer_type(
    expression: &Expression,
    environment: &TypeEnvironment,
    path: &str,
) -> Result<ValueType, Diagnostic> {
    match expression {
        Expression::Literal {
            value: LiteralValue::Float64(value),
        } if !value.is_finite() => Err(expression_error(
            "VIZ-EXPR-0009",
            "floating-point literals must be finite",
            path,
        )),
        Expression::Literal {
            value: LiteralValue::Color(value),
        } if !portable_color(value) => Err(expression_error(
            "VIZ-EXPR-0010",
            format!("invalid portable color literal {value:?}"),
            path,
        )),
        Expression::Literal { value } => Ok(value.value_type()),
        Expression::Field { row, field } => environment
            .rows
            .get(row)
            .and_then(|fields| fields.get(field))
            .cloned()
            .ok_or_else(|| {
                expression_error(
                    "VIZ-EXPR-0001",
                    format!("unknown field {field:?} on row variable {row:?}"),
                    path,
                )
            }),
        Expression::Parameter { id } => environment.parameters.get(id).cloned().ok_or_else(|| {
            expression_error("VIZ-EXPR-0002", format!("unknown parameter {id:?}"), path)
        }),
        Expression::Signal { id } => environment.signals.get(id).cloned().ok_or_else(|| {
            expression_error("VIZ-EXPR-0003", format!("unknown signal {id:?}"), path)
        }),
        Expression::Array { items } => {
            let Some(first) = items.first() else {
                return Err(expression_error(
                    "VIZ-EXPR-0004",
                    "an empty array needs an explicit type",
                    path,
                ));
            };
            let mut item_type = infer_type(first, environment, &format!("{path}.items[0]"))?;
            for (index, item) in items.iter().enumerate().skip(1) {
                let next = infer_type(item, environment, &format!("{path}.items[{index}]"))?;
                item_type = unify(&item_type, &next).ok_or_else(|| {
                    type_mismatch(
                        &item_type,
                        &next,
                        &format!("{path}.items[{index}]"),
                        "array items must have one compatible type",
                    )
                })?;
            }
            Ok(ValueType::Array {
                items: Box::new(item_type),
            })
        }
        Expression::Record { fields } => fields
            .iter()
            .map(|(name, value)| {
                infer_type(value, environment, &format!("{path}.fields.{name}"))
                    .map(|value_type| (name.clone(), value_type))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()
            .map(|fields| ValueType::Record { fields }),
        Expression::Add { left, right } | Expression::Subtract { left, right } => {
            additive_type(left, right, environment, path)
        }
        Expression::Multiply { left, right } | Expression::Divide { left, right } => {
            numeric_binary_type(left, right, environment, path)
        }
        Expression::Equal { left, right } | Expression::LessThan { left, right } => {
            let left_type = infer_type(left, environment, &format!("{path}.left"))?;
            let right_type = infer_type(right, environment, &format!("{path}.right"))?;
            if unify(&left_type, &right_type).is_none() {
                return Err(type_mismatch(
                    &left_type,
                    &right_type,
                    path,
                    "comparison operands must have compatible types",
                ));
            }
            Ok(ValueType::Bool)
        }
        Expression::And { args } | Expression::Or { args } => {
            if args.is_empty() {
                return Err(expression_error(
                    "VIZ-EXPR-0005",
                    "boolean operation requires at least one argument",
                    path,
                ));
            }
            for (index, arg) in args.iter().enumerate() {
                require_type(
                    infer_type(arg, environment, &format!("{path}.args[{index}]"))?,
                    ValueType::Bool,
                    &format!("{path}.args[{index}]"),
                )?;
            }
            Ok(ValueType::Bool)
        }
        Expression::Not { arg } => {
            require_type(
                infer_type(arg, environment, &format!("{path}.arg"))?,
                ValueType::Bool,
                &format!("{path}.arg"),
            )?;
            Ok(ValueType::Bool)
        }
        Expression::IsNull { arg } => {
            infer_type(arg, environment, &format!("{path}.arg"))?;
            Ok(ValueType::Bool)
        }
        Expression::If {
            condition,
            then_value,
            else_value,
        } => {
            require_type(
                infer_type(condition, environment, &format!("{path}.condition"))?,
                ValueType::Bool,
                &format!("{path}.condition"),
            )?;
            let then_type = infer_type(then_value, environment, &format!("{path}.then-value"))?;
            let else_type = infer_type(else_value, environment, &format!("{path}.else-value"))?;
            unify(&then_type, &else_type).ok_or_else(|| {
                type_mismatch(
                    &then_type,
                    &else_type,
                    path,
                    "conditional branches must have compatible types",
                )
            })
        }
        Expression::Call { function, args } => function_type(*function, args, environment, path),
        Expression::Convert { value, to } => {
            let from = infer_type(value, environment, &format!("{path}.value"))?;
            if conversion_is_allowed(&from, to) {
                Ok(to.clone())
            } else {
                Err(expression_error(
                    "VIZ-EXPR-0006",
                    format!("conversion from {from:?} to {to:?} is not defined"),
                    path,
                ))
            }
        }
    }
}

fn additive_type(
    left: &Expression,
    right: &Expression,
    environment: &TypeEnvironment,
    path: &str,
) -> Result<ValueType, Diagnostic> {
    let left_type = infer_type(left, environment, &format!("{path}.left"))?;
    let right_type = infer_type(right, environment, &format!("{path}.right"))?;
    if left_type.is_numeric() && right_type.is_numeric() {
        return Ok(numeric_result(&left_type, &right_type));
    }
    if left_type == right_type && matches!(left_type, ValueType::Length { .. }) {
        return Ok(left_type);
    }
    Err(type_mismatch(
        &left_type,
        &right_type,
        path,
        "addition and subtraction require numeric values or lengths in the same space and unit",
    ))
}

fn numeric_binary_type(
    left: &Expression,
    right: &Expression,
    environment: &TypeEnvironment,
    path: &str,
) -> Result<ValueType, Diagnostic> {
    let left_type = infer_type(left, environment, &format!("{path}.left"))?;
    let right_type = infer_type(right, environment, &format!("{path}.right"))?;
    if left_type.is_numeric() && right_type.is_numeric() {
        Ok(numeric_result(&left_type, &right_type))
    } else {
        Err(type_mismatch(
            &left_type,
            &right_type,
            path,
            "multiplication and division require numeric operands",
        ))
    }
}

fn numeric_result(left: &ValueType, right: &ValueType) -> ValueType {
    if left == &ValueType::Float64 || right == &ValueType::Float64 {
        ValueType::Float64
    } else {
        ValueType::Int64
    }
}

fn function_type(
    function: PureFunction,
    args: &[Expression],
    environment: &TypeEnvironment,
    path: &str,
) -> Result<ValueType, Diagnostic> {
    let argument_types = args
        .iter()
        .enumerate()
        .map(|(index, arg)| infer_type(arg, environment, &format!("{path}.args[{index}]")))
        .collect::<Result<Vec<_>, _>>()?;
    match function {
        PureFunction::Abs if argument_types.len() == 1 && argument_types[0].is_numeric() => {
            Ok(argument_types[0].clone())
        }
        PureFunction::Min | PureFunction::Max
            if argument_types.len() >= 2 && argument_types.iter().all(ValueType::is_numeric) =>
        {
            Ok(if argument_types.contains(&ValueType::Float64) {
                ValueType::Float64
            } else {
                ValueType::Int64
            })
        }
        PureFunction::Clamp
            if argument_types.len() == 3 && argument_types.iter().all(ValueType::is_numeric) =>
        {
            Ok(if argument_types.contains(&ValueType::Float64) {
                ValueType::Float64
            } else {
                ValueType::Int64
            })
        }
        PureFunction::Length
            if argument_types.len() == 1
                && matches!(
                    argument_types[0],
                    ValueType::String | ValueType::Array { .. }
                ) =>
        {
            Ok(ValueType::Int64)
        }
        PureFunction::Lower | PureFunction::Upper
            if argument_types.as_slice() == [ValueType::String] =>
        {
            Ok(ValueType::String)
        }
        _ => Err(expression_error(
            "VIZ-EXPR-0007",
            format!("invalid arguments {argument_types:?} for pure function {function:?}"),
            path,
        )),
    }
}

fn conversion_is_allowed(from: &ValueType, to: &ValueType) -> bool {
    from == to
        || (from.is_numeric() && to.is_numeric())
        || matches!(
            (from, to),
            (ValueType::Bool, ValueType::String)
                | (ValueType::String, ValueType::Bool)
                | (ValueType::String, ValueType::Color)
                | (ValueType::Color, ValueType::String)
        )
}

fn unify(left: &ValueType, right: &ValueType) -> Option<ValueType> {
    if left == right {
        return Some(left.clone());
    }
    if left.is_numeric() && right.is_numeric() {
        return Some(ValueType::Float64);
    }
    match (left, right) {
        (ValueType::Null, value) | (value, ValueType::Null) => {
            Some(ValueType::option(value.clone()))
        }
        (ValueType::Option { item }, value) | (value, ValueType::Option { item })
            if item.as_ref() == value =>
        {
            Some(ValueType::option(value.clone()))
        }
        _ => None,
    }
}

fn require_type(actual: ValueType, expected: ValueType, path: &str) -> Result<(), Diagnostic> {
    if actual == expected {
        Ok(())
    } else {
        Err(type_mismatch(
            &expected,
            &actual,
            path,
            "expression has the wrong type",
        ))
    }
}

fn type_mismatch(left: &ValueType, right: &ValueType, path: &str, message: &str) -> Diagnostic {
    expression_error(
        "VIZ-EXPR-0008",
        format!("{message}: left is {left:?}, right is {right:?}"),
        path,
    )
}

fn expression_error(code: &str, message: impl Into<String>, path: &str) -> Diagnostic {
    Diagnostic::new(code, message).at(path)
}

fn portable_color(value: &str) -> bool {
    value == "transparent"
        || (matches!(value.len(), 7 | 9)
            && value.starts_with('#')
            && value[1..]
                .chars()
                .all(|character| character.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parameter(id: &str) -> Expression {
        Expression::Parameter { id: id.to_owned() }
    }

    #[test]
    fn numeric_expressions_promote_int_to_float() {
        let mut environment = TypeEnvironment::default();
        environment
            .parameters
            .insert("count".to_owned(), ValueType::Int64);
        environment
            .parameters
            .insert("ratio".to_owned(), ValueType::Float64);
        let typed = type_expression(
            Expression::Add {
                left: Box::new(parameter("count")),
                right: Box::new(parameter("ratio")),
            },
            &environment,
        )
        .expect("numeric expression should type-check");
        assert_eq!(typed.result_type, ValueType::Float64);
    }

    #[test]
    fn lengths_from_different_spaces_do_not_unify() {
        let mut environment = TypeEnvironment::default();
        environment.parameters.insert(
            "view-offset".to_owned(),
            ValueType::Length {
                space: "space/view".to_owned(),
                unit: SpatialUnit::SceneUnit,
            },
        );
        environment.parameters.insert(
            "world-offset".to_owned(),
            ValueType::Length {
                space: "space/world".to_owned(),
                unit: SpatialUnit::WorldMeter,
            },
        );
        let diagnostic = type_expression(
            Expression::Add {
                left: Box::new(parameter("view-offset")),
                right: Box::new(parameter("world-offset")),
            },
            &environment,
        )
        .unwrap_err();
        assert_eq!(diagnostic.code, "VIZ-EXPR-0008");
        assert!(diagnostic.message.contains("same space and unit"));
    }

    #[test]
    fn field_reference_resolves_through_named_row_scope() {
        let mut environment = TypeEnvironment::default();
        environment.rows.insert(
            "row/sales".to_owned(),
            BTreeMap::from([("revenue".to_owned(), ValueType::Float64)]),
        );
        let typed = type_expression(
            Expression::Field {
                row: "row/sales".to_owned(),
                field: "revenue".to_owned(),
            },
            &environment,
        )
        .expect("field should resolve");
        assert_eq!(typed.result_type, ValueType::Float64);
    }

    #[test]
    fn diagnostic_codes_pin_every_rejection_branch() {
        // Every VIZ-EXPR code must keep its exact code, message, and source
        // path; a code that drifts silently breaks tooling that matches on it.
        let cases = [
            (
                "non-finite float literal",
                Expression::Literal {
                    value: LiteralValue::Float64(f64::NAN),
                },
                "VIZ-EXPR-0009",
                "floating-point literals must be finite",
            ),
            (
                "unportable color literal",
                Expression::Literal {
                    value: LiteralValue::Color("red".to_owned()),
                },
                "VIZ-EXPR-0010",
                "invalid portable color literal \"red\"",
            ),
            (
                "unknown field",
                Expression::Field {
                    row: "row/missing".to_owned(),
                    field: "x".to_owned(),
                },
                "VIZ-EXPR-0001",
                "unknown field \"x\" on row variable \"row/missing\"",
            ),
            (
                "unknown parameter",
                Expression::Parameter {
                    id: "missing".to_owned(),
                },
                "VIZ-EXPR-0002",
                "unknown parameter \"missing\"",
            ),
            (
                "unknown signal",
                Expression::Signal {
                    id: "missing".to_owned(),
                },
                "VIZ-EXPR-0003",
                "unknown signal \"missing\"",
            ),
            (
                "empty array",
                Expression::Array { items: Vec::new() },
                "VIZ-EXPR-0004",
                "an empty array needs an explicit type",
            ),
            (
                "empty boolean operator",
                Expression::Or { args: Vec::new() },
                "VIZ-EXPR-0005",
                "boolean operation requires at least one argument",
            ),
            (
                "undefined conversion",
                Expression::Convert {
                    value: Box::new(Expression::Literal {
                        value: LiteralValue::Bool(true),
                    }),
                    to: ValueType::Int64,
                },
                "VIZ-EXPR-0006",
                "conversion from Bool to Int64 is not defined",
            ),
            (
                "function arity violation",
                Expression::Call {
                    function: PureFunction::Min,
                    args: vec![Expression::Literal {
                        value: LiteralValue::Int64(1),
                    }],
                },
                "VIZ-EXPR-0007",
                "invalid arguments [Int64] for pure function Min",
            ),
        ];
        for (name, expression, code, message) in cases {
            let diagnostic = type_expression(expression, &TypeEnvironment::default()).unwrap_err();
            assert_eq!(diagnostic.code, code, "{name}");
            assert_eq!(diagnostic.message, message, "{name}");
            assert_eq!(diagnostic.source.as_deref(), Some("expression"), "{name}");
        }
    }

    #[test]
    fn comparisons_logic_and_conditionals_check_operand_types() {
        let mut environment = TypeEnvironment::default();
        environment
            .parameters
            .insert("flag".to_owned(), ValueType::Bool);
        environment
            .parameters
            .insert("count".to_owned(), ValueType::Int64);
        environment
            .parameters
            .insert("ratio".to_owned(), ValueType::Float64);
        environment
            .parameters
            .insert("name".to_owned(), ValueType::String);

        let ok_cases = [
            (
                "equal numeric operands unify to bool",
                Expression::Equal {
                    left: Box::new(parameter("count")),
                    right: Box::new(parameter("ratio")),
                },
                ValueType::Bool,
            ),
            (
                "less-than on matching ints",
                Expression::LessThan {
                    left: Box::new(parameter("count")),
                    right: Box::new(Expression::Literal {
                        value: LiteralValue::Int64(1),
                    }),
                },
                ValueType::Bool,
            ),
            (
                "and over booleans stays boolean",
                Expression::And {
                    args: vec![
                        parameter("flag"),
                        Expression::Not {
                            arg: Box::new(parameter("flag")),
                        },
                    ],
                },
                ValueType::Bool,
            ),
            (
                "is-null accepts any operand type",
                Expression::IsNull {
                    arg: Box::new(parameter("name")),
                },
                ValueType::Bool,
            ),
            (
                "if branches unify int and float",
                Expression::If {
                    condition: Box::new(parameter("flag")),
                    then_value: Box::new(parameter("count")),
                    else_value: Box::new(parameter("ratio")),
                },
                ValueType::Float64,
            ),
        ];
        for (name, expression, expected) in ok_cases {
            let typed = type_expression(expression, &environment)
                .unwrap_or_else(|error| panic!("{name} should type-check: {error:?}"));
            assert_eq!(typed.result_type, expected, "{name}");
        }

        let error_cases = [
            (
                "equal on incompatible operands",
                Expression::Equal {
                    left: Box::new(parameter("count")),
                    right: Box::new(parameter("name")),
                },
                "comparison operands must have compatible types: \
                 left is Int64, right is String",
            ),
            (
                "not on a non-boolean",
                Expression::Not {
                    arg: Box::new(parameter("count")),
                },
                "expression has the wrong type: left is Bool, right is Int64",
            ),
            (
                "if branches must unify",
                Expression::If {
                    condition: Box::new(parameter("flag")),
                    then_value: Box::new(parameter("count")),
                    else_value: Box::new(parameter("name")),
                },
                "conditional branches must have compatible types: \
                 left is Int64, right is String",
            ),
        ];
        for (name, expression, message) in error_cases {
            let diagnostic = type_expression(expression, &environment).unwrap_err();
            assert_eq!(diagnostic.code, "VIZ-EXPR-0008", "{name}");
            assert_eq!(diagnostic.message, message, "{name}");
        }
    }

    #[test]
    fn arithmetic_operators_promote_numerics_and_preserve_lengths() {
        let mut environment = TypeEnvironment::default();
        environment
            .parameters
            .insert("count".to_owned(), ValueType::Int64);
        environment
            .parameters
            .insert("ratio".to_owned(), ValueType::Float64);
        let length = ValueType::Length {
            space: "space/view".to_owned(),
            unit: SpatialUnit::SceneUnit,
        };
        environment
            .parameters
            .insert("view-a".to_owned(), length.clone());
        environment
            .parameters
            .insert("view-b".to_owned(), length.clone());

        let multiplied = type_expression(
            Expression::Multiply {
                left: Box::new(parameter("count")),
                right: Box::new(parameter("ratio")),
            },
            &environment,
        )
        .expect("mixed numeric multiply should promote");
        assert_eq!(multiplied.result_type, ValueType::Float64);

        let divided = type_expression(
            Expression::Divide {
                left: Box::new(parameter("count")),
                right: Box::new(Expression::Literal {
                    value: LiteralValue::Int64(2),
                }),
            },
            &environment,
        )
        .expect("integer division stays integral at the type level");
        assert_eq!(divided.result_type, ValueType::Int64);

        let subtracted = type_expression(
            Expression::Subtract {
                left: Box::new(parameter("view-a")),
                right: Box::new(parameter("view-b")),
            },
            &environment,
        )
        .expect("subtracting lengths in one space keeps the length type");
        assert_eq!(subtracted.result_type, length);

        let diagnostic = type_expression(
            Expression::Multiply {
                left: Box::new(parameter("count")),
                right: Box::new(Expression::Literal {
                    value: LiteralValue::String("x".to_owned()),
                }),
            },
            &environment,
        )
        .unwrap_err();
        assert_eq!(diagnostic.code, "VIZ-EXPR-0008");
        assert_eq!(
            diagnostic.message,
            "multiplication and division require numeric operands: \
             left is Int64, right is String"
        );
    }

    #[test]
    fn array_items_unify_through_optionals() {
        let null_and_int = type_expression(
            Expression::Array {
                items: vec![
                    Expression::Literal {
                        value: LiteralValue::Null,
                    },
                    Expression::Literal {
                        value: LiteralValue::Int64(1),
                    },
                ],
            },
            &TypeEnvironment::default(),
        )
        .expect("null next to int64 should lift the item type into an option");
        assert_eq!(
            null_and_int.result_type,
            ValueType::Array {
                items: Box::new(ValueType::option(ValueType::Int64))
            }
        );

        let mut environment = TypeEnvironment::default();
        environment.rows.insert(
            "row/sales".to_owned(),
            BTreeMap::from([("maybe".to_owned(), ValueType::option(ValueType::Int64))]),
        );
        let optional_field = type_expression(
            Expression::Array {
                items: vec![
                    Expression::Field {
                        row: "row/sales".to_owned(),
                        field: "maybe".to_owned(),
                    },
                    Expression::Literal {
                        value: LiteralValue::Int64(1),
                    },
                ],
            },
            &environment,
        )
        .expect("an optional field unifies with its bare item type");
        assert_eq!(
            optional_field.result_type,
            ValueType::Array {
                items: Box::new(ValueType::option(ValueType::Int64))
            }
        );

        let diagnostic = type_expression(
            Expression::Array {
                items: vec![
                    Expression::Literal {
                        value: LiteralValue::Int64(1),
                    },
                    Expression::Literal {
                        value: LiteralValue::String("x".to_owned()),
                    },
                ],
            },
            &TypeEnvironment::default(),
        )
        .unwrap_err();
        assert_eq!(diagnostic.code, "VIZ-EXPR-0008");
        assert_eq!(
            diagnostic.message,
            "array items must have one compatible type: \
             left is Int64, right is String"
        );
        assert_eq!(diagnostic.source.as_deref(), Some("expression.items[1]"));
    }

    #[test]
    fn conversions_allow_only_the_portable_pairs() {
        let cases = [
            (
                "numeric widening",
                LiteralValue::Int64(1),
                ValueType::Float64,
                true,
            ),
            (
                "numeric narrowing is explicit",
                LiteralValue::Float64(1.5),
                ValueType::Int64,
                true,
            ),
            (
                "string to color",
                LiteralValue::String("#112233".to_owned()),
                ValueType::Color,
                true,
            ),
            (
                "color to string",
                LiteralValue::Color("#112233".to_owned()),
                ValueType::String,
                true,
            ),
            (
                "bool to string",
                LiteralValue::Bool(true),
                ValueType::String,
                true,
            ),
            (
                "string to bool",
                LiteralValue::String("true".to_owned()),
                ValueType::Bool,
                true,
            ),
            (
                "identity conversion",
                LiteralValue::Int64(1),
                ValueType::Int64,
                true,
            ),
            (
                "string to number",
                LiteralValue::String("1".to_owned()),
                ValueType::Int64,
                false,
            ),
            (
                "null to number",
                LiteralValue::Null,
                ValueType::Int64,
                false,
            ),
        ];
        for (name, value, target, allowed) in cases {
            let expression = Expression::Convert {
                value: Box::new(Expression::Literal { value }),
                to: target.clone(),
            };
            let outcome = type_expression(expression, &TypeEnvironment::default());
            assert_eq!(outcome.is_ok(), allowed, "{name}");
            if allowed {
                assert_eq!(outcome.unwrap().result_type, target, "{name}");
            }
        }
    }

    #[test]
    fn pure_functions_check_arity_and_widen_numeric_results() {
        let int = || Expression::Literal {
            value: LiteralValue::Int64(1),
        };
        let float = || Expression::Literal {
            value: LiteralValue::Float64(1.5),
        };
        let ok_cases = [
            (
                "abs preserves int64",
                Expression::Call {
                    function: PureFunction::Abs,
                    args: vec![int()],
                },
                ValueType::Int64,
            ),
            (
                "abs preserves float64",
                Expression::Call {
                    function: PureFunction::Abs,
                    args: vec![float()],
                },
                ValueType::Float64,
            ),
            (
                "min widens to float64",
                Expression::Call {
                    function: PureFunction::Min,
                    args: vec![int(), float()],
                },
                ValueType::Float64,
            ),
            (
                "clamp widens through any float operand",
                Expression::Call {
                    function: PureFunction::Clamp,
                    args: vec![int(), int(), float()],
                },
                ValueType::Float64,
            ),
            (
                "length of a string is int64",
                Expression::Call {
                    function: PureFunction::Length,
                    args: vec![Expression::Literal {
                        value: LiteralValue::String("abc".to_owned()),
                    }],
                },
                ValueType::Int64,
            ),
            (
                "lower keeps the string type",
                Expression::Call {
                    function: PureFunction::Lower,
                    args: vec![Expression::Literal {
                        value: LiteralValue::String("ABC".to_owned()),
                    }],
                },
                ValueType::String,
            ),
        ];
        for (name, expression, expected) in ok_cases {
            let typed = type_expression(expression, &TypeEnvironment::default())
                .unwrap_or_else(|error| panic!("{name} should type-check: {error:?}"));
            assert_eq!(typed.result_type, expected, "{name}");
        }

        let diagnostic = type_expression(
            Expression::Call {
                function: PureFunction::Upper,
                args: vec![int()],
            },
            &TypeEnvironment::default(),
        )
        .unwrap_err();
        assert_eq!(diagnostic.code, "VIZ-EXPR-0007");
        assert_eq!(
            diagnostic.message,
            "invalid arguments [Int64] for pure function Upper"
        );
    }
}
