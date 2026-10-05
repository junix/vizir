use vizir_core::{NumberFormat, NumberNotation};

/// The exact strings used by both layout and Scene2D emission. A chart opts in
/// when either numeric axis requests formatting; otherwise legacy geometry and
/// text bytes are preserved.
pub(crate) struct NumericTickLabels {
    pub x: Vec<String>,
    pub y: Vec<String>,
}

impl NumericTickLabels {
    #[cfg(test)]
    pub fn new(
        x: Option<([f64; 2], Option<&NumberFormat>)>,
        y: Option<([f64; 2], Option<&NumberFormat>)>,
    ) -> Result<Option<Self>, String> {
        Self::new_with_measurement(x, y, false)
    }

    pub fn new_with_measurement(
        x: Option<([f64; 2], Option<&NumberFormat>)>,
        y: Option<([f64; 2], Option<&NumberFormat>)>,
        measured: bool,
    ) -> Result<Option<Self>, String> {
        if !measured
            && !x.is_some_and(|(_, format)| format.is_some())
            && !y.is_some_and(|(_, format)| format.is_some())
        {
            return Ok(None);
        }
        for (axis, domain) in [
            ("x", x.map(|(domain, _)| domain)),
            ("y", y.map(|(domain, _)| domain)),
        ] {
            if let Some([start, end]) = domain
                && (!start.is_finite() || !end.is_finite() || !(end - start).is_finite())
            {
                return Err(format!(
                    "VIZ-FORMAT-0002: {axis}-axis numeric ticks require finite domain endpoints and a finite span"
                ));
            }
        }
        let labels = Self {
            x: x.map(|(domain, format)| tick_labels(domain, false, format))
                .unwrap_or_default(),
            y: y.map(|(domain, format)| tick_labels(domain, true, format))
                .unwrap_or_default(),
        };
        for (axis, explicit, ticks) in [
            (
                "x",
                x.is_some_and(|(_, format)| format.is_some()),
                &labels.x,
            ),
            (
                "y",
                y.is_some_and(|(_, format)| format.is_some()),
                &labels.y,
            ),
        ] {
            if explicit && let Some(pair) = ticks.windows(2).find(|pair| pair[0] == pair[1]) {
                return Err(format!(
                    "VIZ-FORMAT-0001: {axis}-axis number_format produces indistinguishable neighboring tick labels {:?}; choose a different precision or notation",
                    pair[0]
                ));
            }
        }
        Ok(Some(labels))
    }
}

fn tick_labels(domain: [f64; 2], reverse: bool, format: Option<&NumberFormat>) -> Vec<String> {
    let [start, end] = if reverse {
        [domain[1], domain[0]]
    } else {
        domain
    };
    (0..=5)
        .map(|index| format_number(start + (end - start) * (index as f64 / 5.0), format))
        .collect()
}

pub(crate) fn format_number(value: f64, format: Option<&NumberFormat>) -> String {
    let Some(format) = format else {
        return legacy_format_number(value);
    };
    let precision = usize::from(format.precision);
    let text = match format.notation {
        NumberNotation::Scientific => format!("{value:.precision$e}"),
        NumberNotation::Fixed => format!("{value:.precision$}"),
    };
    // Inspect the rounded mantissa rather than the original value. Parsing the
    // full scientific string as f64 could incorrectly underflow a tiny nonzero
    // value to zero. Rust formatting is locale-independent.
    let mantissa = text.split('e').next().unwrap_or(&text);
    if let Some(unsigned) = mantissa.strip_prefix('-')
        && unsigned
            .chars()
            .all(|character| character == '0' || character == '.')
    {
        text[1..].to_owned()
    } else {
        text
    }
}

/// Shortest round-tripping default used by heatmap interval endpoints and cells.
/// Keep this separate from the intentionally abbreviated legacy axis formatter.
pub(crate) fn format_exact_float(value: f64, format: Option<&NumberFormat>) -> String {
    let value = if value == 0. { 0. } else { value };
    match format {
        Some(format) => format_number(value, Some(format)),
        None if value == 0. || (0.0001..1_000_000.).contains(&value.abs()) => value.to_string(),
        None => format!("{value:e}"),
    }
}

/// Preserve the evaluated Int64 value before any f64 paint projection.
pub(crate) fn format_integer(value: i64, format: Option<&NumberFormat>) -> String {
    let Some(format) = format else {
        return value.to_string();
    };
    let precision = usize::from(format.precision);
    match format.notation {
        NumberNotation::Fixed => {
            let integer = value.to_string();
            if precision == 0 {
                integer
            } else {
                format!("{integer}.{}", "0".repeat(precision))
            }
        }
        NumberNotation::Scientific => {
            let digits = value.unsigned_abs().to_string();
            let mut exponent = digits.len() - 1;
            let significant = precision + 1;
            let mut retained = if digits.len() <= significant {
                format!("{digits}{}", "0".repeat(significant - digits.len()))
            } else {
                let (keep, tail) = digits.split_at(significant);
                let mut magnitude: u64 = keep.parse().expect("at most 13 significant digits");
                let first = tail.as_bytes()[0];
                let above_half = first > b'5'
                    || (first == b'5' && tail.as_bytes()[1..].iter().any(|d| *d != b'0'));
                let exact_half = first == b'5' && tail.as_bytes()[1..].iter().all(|d| *d == b'0');
                if above_half || (exact_half && magnitude % 2 == 1) {
                    magnitude += 1;
                }
                let rounded = magnitude.to_string();
                if rounded.len() > significant {
                    exponent += 1;
                    rounded[..significant].to_owned()
                } else {
                    rounded
                }
            };
            if precision > 0 {
                retained.insert(1, '.');
            }
            let sign = if value < 0 { "-" } else { "" };
            format!("{sign}{retained}e{exponent}")
        }
    }
}

fn legacy_format_number(value: f64) -> String {
    if value.abs() >= 1000.0 {
        format!("{:.1}k", value / 1000.0)
    } else if value.fract().abs() < 0.001 {
        format!("{value:.0}")
    } else if value.abs() < 10.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.0}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_formats_preserve_precision_and_suppress_only_rounded_negative_zero() {
        let fixed = NumberFormat {
            notation: NumberNotation::Fixed,
            precision: 3,
        };
        let scientific = NumberFormat {
            notation: NumberNotation::Scientific,
            precision: 2,
        };
        for value in [-0.0, -0.0004, 0.0] {
            assert_eq!(format_number(value, Some(&fixed)), "0.000");
        }
        assert_eq!(format_number(-0.0006, Some(&fixed)), "-0.001");
        assert_eq!(format_number(-0.0, Some(&scientific)), "0.00e0");
        assert_eq!(format_number(-1e-150, Some(&scientific)), "-1.00e-150");
        assert_eq!(format_number(1e150, Some(&scientific)), "1.00e150");
        assert_eq!(format_number(1.25, Some(&fixed)), "1.250");
        assert_eq!(
            format_number(f64::from_bits(1), Some(&scientific)),
            "4.94e-324"
        );
    }

    #[test]
    fn formatted_mir_cannot_emit_nonfinite_numeric_text() {
        let format = NumberFormat {
            notation: NumberNotation::Scientific,
            precision: 2,
        };
        for domain in [[-1e308, 1e308], [f64::NAN, 1.0], [0.0, f64::INFINITY]] {
            let error = NumericTickLabels::new(Some((domain, Some(&format))), None)
                .err()
                .unwrap();
            assert!(error.contains("VIZ-FORMAT-0002"));
        }
    }

    #[test]
    fn zero_and_maximum_precision_are_exactly_honored() {
        for (precision, fixed, scientific) in [
            (0, "-2", "-2e0"),
            (12, "-2.000000000000", "-2.000000000000e0"),
        ] {
            assert_eq!(
                format_number(
                    -2.0,
                    Some(&NumberFormat {
                        notation: NumberNotation::Fixed,
                        precision
                    })
                ),
                fixed
            );
            assert_eq!(
                format_number(
                    -2.0,
                    Some(&NumberFormat {
                        notation: NumberNotation::Scientific,
                        precision
                    })
                ),
                scientific
            );
        }
    }
}

#[cfg(test)]
mod value_label_tests {
    use super::*;
    #[test]
    fn exact_integer_default_fixed_and_scientific_ties_to_even() {
        for value in [
            0,
            -1,
            9007199254740991,
            9007199254740992,
            9007199254740993,
            i64::MIN,
            i64::MAX,
        ] {
            assert_eq!(format_integer(value, None), value.to_string());
            assert_eq!(
                format_integer(
                    value,
                    Some(&NumberFormat {
                        notation: NumberNotation::Fixed,
                        precision: 12
                    })
                ),
                format!("{value}.000000000000")
            );
        }
        for (value, precision, expected) in [
            (25, 0, "2e1"),
            (35, 0, "4e1"),
            (-25, 0, "-2e1"),
            (-35, 0, "-4e1"),
            (245, 1, "2.4e2"),
            (255, 1, "2.6e2"),
            (999, 1, "1.0e3"),
            (2501, 0, "3e3"),
            (0, 3, "0.000e0"),
            (12, 3, "1.200e1"),
            (i64::MIN, 12, "-9.223372036855e18"),
            (i64::MAX, 12, "9.223372036855e18"),
        ] {
            assert_eq!(
                format_integer(
                    value,
                    Some(&NumberFormat {
                        notation: NumberNotation::Scientific,
                        precision
                    })
                ),
                expected,
                "{value}, {precision}"
            );
        }
    }
    #[test]
    fn float_default_roundtrips_all_extremes_without_collapsing_to_zero() {
        for value in [
            f64::from_bits(1),
            -f64::from_bits(1),
            f64::MIN_POSITIVE,
            f64::MAX,
            -f64::MAX,
            1e-99,
            -1e-99,
            0.00001,
            1000001.,
        ] {
            let text = format_exact_float(value, None);
            assert_eq!(text.parse::<f64>().unwrap().to_bits(), value.to_bits());
            assert!(text.len() <= 512);
        }
        assert_eq!(format_exact_float(-0., None), "0");
        assert_eq!(
            format_exact_float(
                -0.00001,
                Some(&NumberFormat {
                    notation: NumberNotation::Fixed,
                    precision: 2
                })
            ),
            "0.00"
        );
    }
}
