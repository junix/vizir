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
