//! Strict new-envelope decoding without tightening any legacy MIR deserializer.
use std::fmt;

use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Number, Value};
use vizir_core::{VizMir, VizResult};

use crate::theme::{ThemeContext, ThemedMir, theme_error};

pub const MAX_THEMED_MIR_JSON_BYTES: usize = 32 * 1024 * 1024;

/// Bounded byte input plus strict envelope decoding. JSON's default recursion
/// limit stays enabled. Compiler execution/traversal limits are checked later.
pub fn parse_themed_mir_json(source: &[u8]) -> VizResult<ThemedMir> {
    if source.len() > MAX_THEMED_MIR_JSON_BYTES {
        return Err(theme_error(
            "0004",
            "JSON input exceeds the 32 MiB parsing limit",
        ));
    }
    let mir: ThemedMir = serde_json::from_slice(source)?;
    mir.validate_context()?;
    Ok(mir)
}

impl<'de> Deserialize<'de> for ThemedMir {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            format: String,
            theme: ThemeContext,
            mir: VizMir,
        }
        // Parsing allocations are separate from compiler execution budgets. The
        // CLI and byte-reader bound input size before this recursive decoder.
        let input = UniqueValue::deserialize(deserializer)?.0;
        let fields: Fields =
            serde_json::from_value(input.clone()).map_err(serde::de::Error::custom)?;
        let mir = Self {
            format: fields.format,
            theme: fields.theme,
            mir: fields.mir,
        };
        let canonical = serde_json::to_value(&mir).map_err(serde::de::Error::custom)?;
        check_fields(&input, &canonical, &mut Vec::new()).map_err(serde::de::Error::custom)?;
        Ok(mir)
    }
}

// Some legacy internally tagged MIR enums accept unknown fields. Reject keys
// lost by typed decoding at this new boundary, but allow defaulted fields to be
// added and numeric spellings to normalize. Arbitrary inline row metadata is
// retained by serde_json::Value and therefore remains valid.
fn check_fields(input: &Value, canonical: &Value, path: &mut Vec<String>) -> Result<(), String> {
    let diagnostic = |path: &[String]| {
        format!(
            "VIZ-THEME-0007: unsupported field $.{} would be discarded",
            path.join(".")
        )
    };
    match (input, canonical) {
        (Value::Object(input), Value::Object(canonical)) => {
            for (key, value) in input {
                // CoordinateSpace2D.parent is the sole nullable MIR field
                // omitted on serialization. Preserve its established null form.
                if key == "parent"
                    && value.is_null()
                    && path.len() == 3
                    && path[0] == "mir"
                    && path[1] == "spaces"
                {
                    continue;
                }
                path.push(key.clone());
                let target = canonical.get(key).ok_or_else(|| diagnostic(path))?;
                check_fields(value, target, path)?;
                path.pop();
            }
        }
        (Value::Array(input), Value::Array(canonical)) => {
            if input.len() != canonical.len() {
                return Err(diagnostic(path));
            }
            for (i, (value, target)) in input.iter().zip(canonical).enumerate() {
                path.push(i.to_string());
                check_fields(value, target, path)?;
                path.pop();
            }
        }
        (Value::Object(_) | Value::Array(_), _) => return Err(diagnostic(path)),
        _ => {}
    }
    Ok(())
}

struct UniqueValue(Value);

impl<'de> Deserialize<'de> for UniqueValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct JsonVisitor;
        impl<'de> Visitor<'de> for JsonVisitor {
            type Value = UniqueValue;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("JSON without duplicate object keys")
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(UniqueValue(Value::Null))
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(UniqueValue(Value::Bool(v)))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(UniqueValue(Value::Number(v.into())))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(UniqueValue(Value::Number(v.into())))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
                Number::from_f64(v)
                    .map(|n| UniqueValue(Value::Number(n)))
                    .ok_or_else(|| E::custom("nonfinite JSON number"))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(UniqueValue(Value::String(v.to_owned())))
            }
            fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(UniqueValue(Value::String(v)))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element::<UniqueValue>()? {
                    values.push(value.0);
                }
                Ok(UniqueValue(Value::Array(values)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut values = Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if values.contains_key(&key) {
                        return Err(serde::de::Error::custom(format!(
                            "VIZ-THEME-0007: duplicate JSON object key {key:?}"
                        )));
                    }
                    values.insert(key, map.next_value::<UniqueValue>()?.0);
                }
                Ok(UniqueValue(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(JsonVisitor)
    }
}
