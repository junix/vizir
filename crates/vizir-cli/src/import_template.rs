//! CSV-import-only decoding into a bounded JSON-shaped tree.
//!
//! The root has depth zero; each array item or mapping value adds one level.
//! Containers, scalar values, and mapping keys each consume one node. Mapping
//! keys consume UTF-8 bytes but do not add a value-nesting level. Aliases are
//! decoded through the same seeds, so every expansion consumes its full budget.

use std::fmt;

use serde::de::{self, DeserializeSeed, EnumAccess, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};
use vizir_core::{VizError, VizResult};

pub(crate) const MAX_TEMPLATE_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const MAX_TEMPLATE_DEPTH: usize = 64;
pub(crate) const MAX_TEMPLATE_NODES: usize = 262_144;
pub(crate) const MAX_TEMPLATE_STRING_BYTES: usize = 16 * 1024 * 1024;
/// Counts every scanner token, including delimiters, punctuation, and anchors.
/// Four per allowed owned node plus stream/document overhead accommodates the
/// supported YAML grammar while bounding the preflight's total work.
pub(crate) const MAX_TEMPLATE_TOKENS: usize = 4 * MAX_TEMPLATE_NODES + 16;

#[derive(Clone, Copy)]
struct Limits {
    input_bytes: usize,
    depth: usize,
    nodes: usize,
    string_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            input_bytes: MAX_TEMPLATE_BYTES,
            depth: MAX_TEMPLATE_DEPTH,
            nodes: MAX_TEMPLATE_NODES,
            string_bytes: MAX_TEMPLATE_STRING_BYTES,
        }
    }
}

#[derive(Clone, Copy)]
struct Failure {
    budget: bool,
    message: &'static str,
}

impl Failure {
    fn syntax(message: &'static str) -> Self {
        Self {
            budget: false,
            message,
        }
    }

    fn budget(message: &'static str) -> Self {
        Self {
            budget: true,
            message,
        }
    }

    fn diagnostic(self, location: Option<(usize, usize)>) -> VizError {
        let code = if self.budget {
            "VIZ-CSV-0202"
        } else {
            "VIZ-CSV-0201"
        };
        let location = location
            .map(|(line, column)| format!(" at line {line}, column {column}"))
            .unwrap_or_default();
        // Do not format a deserializer error: it may contain an arbitrarily
        // large scalar, a map key, or a path constructed from untrusted keys.
        VizError::Diagnostic(format!("{code}: {}{location}", self.message))
    }
}

struct Budget {
    limits: Limits,
    nodes: usize,
    string_bytes: usize,
    failure: Option<Failure>,
}

impl Budget {
    fn new(limits: Limits) -> Self {
        Self {
            limits,
            nodes: 0,
            string_bytes: 0,
            failure: None,
        }
    }

    fn fail<E: de::Error>(&mut self, failure: Failure) -> E {
        self.failure = Some(failure);
        E::custom(failure.message)
    }

    fn node<E: de::Error>(&mut self) -> Result<(), E> {
        let Some(next) = self
            .nodes
            .checked_add(1)
            .filter(|&n| n <= self.limits.nodes)
        else {
            return Err(self.fail(Failure::budget("template node limit exceeded")));
        };
        self.nodes = next;
        Ok(())
    }

    fn depth<E: de::Error>(&mut self, depth: usize) -> Result<(), E> {
        if depth > self.limits.depth {
            return Err(self.fail(Failure::budget("template depth limit exceeded")));
        }
        Ok(())
    }

    fn child_depth<E: de::Error>(&mut self, depth: usize) -> Result<usize, E> {
        depth
            .checked_add(1)
            .ok_or_else(|| self.fail(Failure::budget("template depth limit exceeded")))
    }

    fn string<E: de::Error>(&mut self, bytes: usize) -> Result<(), E> {
        let Some(next) = self
            .string_bytes
            .checked_add(bytes)
            .filter(|&n| n <= self.limits.string_bytes)
        else {
            return Err(self.fail(Failure::budget("template string-byte limit exceeded")));
        };
        self.string_bytes = next;
        Ok(())
    }

    fn container_item<E: de::Error>(&mut self, length: usize) -> Result<(), E> {
        if length.checked_add(1).is_none_or(|n| n > self.limits.nodes) {
            return Err(self.fail(Failure::budget("template container-length limit exceeded")));
        }
        Ok(())
    }

    fn owned_string<E: de::Error>(&mut self, value: &str) -> Result<String, E> {
        // Charge before cloning borrowed or temporary lexical scalar text.
        self.string(value.len())?;
        Ok(value.to_owned())
    }

    fn error(&self, location: Option<(usize, usize)>) -> VizError {
        self.failure
            .unwrap_or_else(|| Failure::syntax("invalid template syntax"))
            .diagnostic(location)
    }
}

/// Parse exactly one JSON or YAML document without first building an unbounded
/// `serde_yaml::Value` or `serde_json::Value`. This performs no I/O.
pub(crate) fn parse_template(bytes: &[u8], json: bool) -> VizResult<Value> {
    parse_with_limits(bytes, json, Limits::default())
}

fn parse_with_limits(bytes: &[u8], json: bool, limits: Limits) -> VizResult<Value> {
    if bytes.len() > limits.input_bytes {
        return Err(Failure::budget("template input-byte limit exceeded").diagnostic(None));
    }
    let mut budget = Budget::new(limits);
    if json {
        let mut decoder = serde_json::Deserializer::from_slice(bytes);
        let value = ValueSeed {
            budget: &mut budget,
            depth: 0,
        }
        .deserialize(&mut decoder)
        .map_err(|error| budget.error(Some((error.line(), error.column()))))?;
        decoder.end().map_err(|error| {
            Failure::syntax("template must contain exactly one JSON document")
                .diagnostic(Some((error.line(), error.column())))
        })?;
        Ok(value)
    } else {
        reject_yaml_tags(bytes)?;
        let mut documents = serde_yaml::Deserializer::from_slice(bytes);
        let document = documents.next().ok_or_else(|| {
            Failure::syntax("template must contain exactly one YAML document").diagnostic(None)
        })?;
        let value = ValueSeed {
            budget: &mut budget,
            depth: 0,
        }
        .deserialize(document)
        .map_err(|error| budget.error(error.location().map(|p| (p.line(), p.column()))))?;
        if documents.next().is_some() {
            return Err(
                Failure::syntax("template must contain exactly one YAML document").diagnostic(None),
            );
        }
        Ok(value)
    }
}

struct ValueSeed<'a> {
    budget: &'a mut Budget,
    depth: usize,
}

impl<'de> DeserializeSeed<'de> for ValueSeed<'_> {
    type Value = Value;

    fn deserialize<D: de::Deserializer<'de>>(self, decoder: D) -> Result<Value, D::Error> {
        self.budget.depth(self.depth)?;
        self.budget.node()?;
        decoder.deserialize_any(ValueVisitor {
            budget: self.budget,
            depth: self.depth,
        })
    }
}

struct ValueVisitor<'a> {
    budget: &'a mut Budget,
    depth: usize,
}

impl<'de> Visitor<'de> for ValueVisitor<'_> {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an untagged JSON-shaped template value")
    }

    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Value, E> {
        Ok(Value::Number(Number::from(value)))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Value, E> {
        Ok(Value::Number(Number::from(value)))
    }

    fn visit_i128<E: de::Error>(self, value: i128) -> Result<Value, E> {
        i64::try_from(value)
            .map(|value| Value::Number(Number::from(value)))
            .map_err(|_| {
                self.budget.fail(Failure::syntax(
                    "template integer is outside the supported range",
                ))
            })
    }

    fn visit_u128<E: de::Error>(self, value: u128) -> Result<Value, E> {
        u64::try_from(value)
            .map(|value| Value::Number(Number::from(value)))
            .map_err(|_| {
                self.budget.fail(Failure::syntax(
                    "template integer is outside the supported range",
                ))
            })
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Value, E> {
        Number::from_f64(value).map(Value::Number).ok_or_else(|| {
            self.budget
                .fail(Failure::syntax("template numbers must be finite"))
        })
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Value, E> {
        self.budget.owned_string(value).map(Value::String)
    }

    fn visit_borrowed_str<E: de::Error>(self, value: &'de str) -> Result<Value, E> {
        self.visit_str(value)
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<Value, E> {
        // JSON/YAML normally supply borrowed or temporary string events. If a
        // decoder supplies ownership, validate it immediately and do not copy.
        self.budget.string(value.len())?;
        Ok(Value::String(value))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Value, A::Error> {
        let depth = self.budget.child_depth(self.depth)?;
        let mut values = Vec::new();
        // Never reserve from an untrusted size_hint. Each child is fully charged
        // before its storage is inserted; aliases use precisely the same seed.
        while let Some(value) = sequence.next_element_seed(ValueSeed {
            budget: self.budget,
            depth,
        })? {
            self.budget.container_item(values.len())?;
            values.push(value);
        }
        Ok(Value::Array(values))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut mapping: A) -> Result<Value, A::Error> {
        let depth = self.budget.child_depth(self.depth)?;
        let mut values = Map::new();
        while let Some(key) = mapping.next_key_seed(KeySeed {
            budget: self.budget,
        })? {
            if values.contains_key(&key) {
                return Err(self
                    .budget
                    .fail(Failure::syntax("duplicate template mapping key")));
            }
            self.budget.container_item(values.len())?;
            let value = mapping.next_value_seed(ValueSeed {
                budget: self.budget,
                depth,
            })?;
            values.insert(key, value);
        }
        Ok(Value::Object(values))
    }

    fn visit_enum<A: EnumAccess<'de>>(self, _tag: A) -> Result<Value, A::Error> {
        Err(self
            .budget
            .fail(Failure::syntax("YAML tags are not allowed in templates")))
    }
}

struct KeySeed<'a> {
    budget: &'a mut Budget,
}

impl<'de> DeserializeSeed<'de> for KeySeed<'_> {
    type Value = String;

    fn deserialize<D: de::Deserializer<'de>>(self, decoder: D) -> Result<String, D::Error> {
        self.budget.node()?;
        // deserialize_string/next_key::<String>() would let serde_yaml coerce
        // numeric and boolean keys. Only actual string events are accepted.
        decoder.deserialize_any(KeyVisitor {
            budget: self.budget,
        })
    }
}

struct KeyVisitor<'a> {
    budget: &'a mut Budget,
}

impl KeyVisitor<'_> {
    fn reject<E: de::Error>(self) -> E {
        self.budget
            .fail(Failure::syntax("template mapping keys must be strings"))
    }
}

impl<'de> Visitor<'de> for KeyVisitor<'_> {
    type Value = String;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a string mapping key")
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<String, E> {
        self.budget.owned_string(value)
    }

    fn visit_borrowed_str<E: de::Error>(self, value: &'de str) -> Result<String, E> {
        self.visit_str(value)
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<String, E> {
        self.budget.string(value.len())?;
        Ok(value)
    }

    fn visit_unit<E: de::Error>(self) -> Result<String, E> {
        Err(self.reject())
    }
    fn visit_none<E: de::Error>(self) -> Result<String, E> {
        Err(self.reject())
    }
    fn visit_bool<E: de::Error>(self, _value: bool) -> Result<String, E> {
        Err(self.reject())
    }
    fn visit_i64<E: de::Error>(self, _value: i64) -> Result<String, E> {
        Err(self.reject())
    }
    fn visit_u64<E: de::Error>(self, _value: u64) -> Result<String, E> {
        Err(self.reject())
    }
    fn visit_i128<E: de::Error>(self, _value: i128) -> Result<String, E> {
        Err(self.reject())
    }
    fn visit_u128<E: de::Error>(self, _value: u128) -> Result<String, E> {
        Err(self.reject())
    }
    fn visit_f64<E: de::Error>(self, _value: f64) -> Result<String, E> {
        Err(self.reject())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, _value: A) -> Result<String, A::Error> {
        Err(self.reject())
    }
    fn visit_map<A: MapAccess<'de>>(self, _value: A) -> Result<String, A::Error> {
        Err(self.reject())
    }
    fn visit_enum<A: EnumAccess<'de>>(self, _value: A) -> Result<String, A::Error> {
        Err(self.reject())
    }
}

// serde_yaml intentionally resolves standard tags before calling a visitor.
// Therefore visit_enum alone cannot reject !!str, !!map, or URI tags. Use its
// exact underlying lexer in a separate bounded pass, without constructing an
// event tree, copying token text, or interpreting YAML spelling ourselves.
fn reject_yaml_tags(bytes: &[u8]) -> VizResult<()> {
    yaml_tokens::check(bytes, MAX_TEMPLATE_TOKENS)
}

mod yaml_tokens {
    use std::marker::PhantomData;
    use std::mem::MaybeUninit;

    use unsafe_libyaml as unsafe_yaml;

    use super::{Failure, MAX_TEMPLATE_NODES, MAX_TEMPLATE_STRING_BYTES, VizResult};

    struct Scanner<'a> {
        // set_input_string installs a pointer to the parser itself as callback
        // data. The parser must stay at this stable heap address until deleted.
        raw: Box<MaybeUninit<unsafe_yaml::yaml_parser_t>>,
        _input: PhantomData<&'a [u8]>,
    }

    struct Token {
        raw: MaybeUninit<unsafe_yaml::yaml_token_t>,
    }

    impl<'a> Scanner<'a> {
        fn new(bytes: &'a [u8]) -> VizResult<Self> {
            let mut raw = Box::new(MaybeUninit::uninit());
            // SAFETY: raw is correctly aligned, writable storage for one parser.
            // initialize fully initializes it. unsafe-libyaml 0.2.11 initializes
            // all owned buffers before returning; delete is valid thereafter.
            let success = unsafe { unsafe_yaml::yaml_parser_initialize(raw.as_mut_ptr()) };
            let mut scanner = Self {
                raw,
                _input: PhantomData,
            };
            if success.fail {
                return Err(
                    Failure::syntax("cannot initialize YAML template scanner").diagnostic(None)
                );
            }
            // SAFETY: parser is initialized and will not move; bytes remains
            // alive for the scanner's entire lifetime. The bounded input length
            // fits both u64 and isize. Even an empty Rust slice is non-null.
            unsafe {
                unsafe_yaml::yaml_parser_set_encoding(
                    scanner.raw.as_mut_ptr(),
                    unsafe_yaml::YAML_UTF8_ENCODING,
                );
                unsafe_yaml::yaml_parser_set_input_string(
                    scanner.raw.as_mut_ptr(),
                    bytes.as_ptr(),
                    bytes.len() as u64,
                );
            }
            Ok(scanner)
        }

        fn next(&mut self) -> VizResult<Token> {
            let mut raw = MaybeUninit::uninit();
            // SAFETY: scanner is initialized, input is alive, and raw points to
            // writable token storage. scan initializes raw even on failure.
            // It is never mixed with the parser/load APIs.
            let success =
                unsafe { unsafe_yaml::yaml_parser_scan(self.raw.as_mut_ptr(), raw.as_mut_ptr()) };
            let token = Token { raw };
            if success.fail {
                // SAFETY: initialized parser's public error prefix is valid.
                let mark = unsafe { self.raw.assume_init_ref().problem_mark };
                return Err(
                    Failure::syntax("invalid YAML template syntax").diagnostic(location(mark))
                );
            }
            Ok(token)
        }
    }

    impl Drop for Scanner<'_> {
        fn drop(&mut self) {
            // SAFETY: exactly this guard owns the initialized, unmoved parser;
            // tokens still queued internally are cleaned by parser_delete.
            unsafe { unsafe_yaml::yaml_parser_delete(self.raw.as_mut_ptr()) };
        }
    }

    impl Token {
        fn get(&self) -> &unsafe_yaml::yaml_token_t {
            // SAFETY: Token is constructed only after scan initializes storage.
            unsafe { self.raw.assume_init_ref() }
        }
    }

    impl Drop for Token {
        fn drop(&mut self) {
            // SAFETY: scan initialized this token (also on failure), and exactly
            // this guard owns it. No token-owned pointer is retained elsewhere.
            unsafe { unsafe_yaml::yaml_token_delete(self.raw.as_mut_ptr()) };
        }
    }

    fn location(mark: unsafe_yaml::yaml_mark_t) -> Option<(usize, usize)> {
        Some((
            usize::try_from(mark.line).ok()?.checked_add(1)?,
            usize::try_from(mark.column).ok()?.checked_add(1)?,
        ))
    }

    pub(super) fn check(bytes: &[u8], token_limit: usize) -> VizResult<()> {
        let mut scanner = Scanner::new(bytes)?;
        let mut tokens = 0usize;
        let mut concrete_nodes = 0usize;
        let mut document_present = false;
        loop {
            // Charge before asking the lexer to allocate its next token.
            tokens = tokens
                .checked_add(1)
                .filter(|&n| n <= token_limit)
                .ok_or_else(|| {
                    Failure::budget("template YAML token limit exceeded").diagnostic(None)
                })?;
            let token = scanner.next()?;
            let raw = token.get();
            match raw.type_ {
                unsafe_yaml::YAML_TAG_TOKEN | unsafe_yaml::YAML_TAG_DIRECTIVE_TOKEN => {
                    return Err(Failure::syntax("YAML tags are not allowed in templates")
                        .diagnostic(location(raw.start_mark)));
                }
                unsafe_yaml::YAML_DOCUMENT_START_TOKEN => document_present = true,
                unsafe_yaml::YAML_SCALAR_TOKEN
                | unsafe_yaml::YAML_ALIAS_TOKEN
                | unsafe_yaml::YAML_FLOW_SEQUENCE_START_TOKEN
                | unsafe_yaml::YAML_FLOW_MAPPING_START_TOKEN
                | unsafe_yaml::YAML_BLOCK_SEQUENCE_START_TOKEN
                | unsafe_yaml::YAML_BLOCK_MAPPING_START_TOKEN => {
                    document_present = true;
                    // This counts concrete input nodes before serde_yaml loads
                    // events. The tree seed separately counts their expansions.
                    concrete_nodes = concrete_nodes
                        .checked_add(1)
                        .filter(|&n| n <= MAX_TEMPLATE_NODES)
                        .ok_or_else(|| {
                            Failure::budget("template node limit exceeded")
                                .diagnostic(location(raw.start_mark))
                        })?;
                    if raw.type_ == unsafe_yaml::YAML_SCALAR_TOKEN {
                        // SAFETY: the token discriminant selects the scalar
                        // member. Read its length only, never its raw pointer.
                        let length = unsafe { raw.data.scalar.length };
                        if length > MAX_TEMPLATE_STRING_BYTES as u64 {
                            return Err(Failure::budget("template lexical scalar limit exceeded")
                                .diagnostic(location(raw.start_mark)));
                        }
                    }
                }
                unsafe_yaml::YAML_STREAM_END_TOKEN => {
                    return if document_present {
                        Ok(())
                    } else {
                        Err(
                            Failure::syntax("template must contain exactly one YAML document")
                                .diagnostic(None),
                        )
                    };
                }
                unsafe_yaml::YAML_NO_TOKEN => {
                    return Err(
                        Failure::syntax("invalid YAML template token stream").diagnostic(None)
                    );
                }
                _ => {}
            }
            // Token buffers are freed here before the next scanner call. The
            // only unavoidable temporary scalar allocation is bounded by the
            // 8 MiB source and checked above; no scalar is copied by this pass.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounded(input: &str, json: bool, nodes: usize, string_bytes: usize) -> VizResult<Value> {
        parse_with_limits(
            input.as_bytes(),
            json,
            Limits {
                nodes,
                string_bytes,
                ..Limits::default()
            },
        )
    }

    fn error(input: &str, json: bool) -> String {
        parse_template(input.as_bytes(), json)
            .unwrap_err()
            .to_string()
    }

    #[test]
    fn containers_values_and_keys_each_consume_one_node() {
        for json in [false, true] {
            let input = r#"{"a":["é",""],"b":{}}"#;
            assert!(bounded(input, json, 7, 4).is_ok());
            assert!(
                bounded(input, json, 6, 4)
                    .unwrap_err()
                    .to_string()
                    .contains("node limit")
            );
            assert!(
                bounded(input, json, 7, 3)
                    .unwrap_err()
                    .to_string()
                    .contains("string-byte limit")
            );
            assert!(bounded("{}", json, 1, 0).is_ok());
            assert!(bounded("[]", json, 1, 0).is_ok());
            assert!(bounded("null", json, 1, 0).is_ok());
            assert!(bounded("null", json, 0, 0).is_err());
            assert!(bounded(r#"{"key":null}"#, json, 3, 3).is_ok());
            assert!(bounded(r#"{"key":null}"#, json, 2, 3).is_err());
            assert!(bounded(r#"{"key":null}"#, json, 3, 2).is_err());
        }
    }

    #[test]
    fn decoded_utf8_and_escaped_keys_are_charged() {
        for json in [false, true] {
            let input = r#"{"\u00e9":"\u00e9"}"#;
            assert!(bounded(input, json, 3, 4).is_ok());
            assert!(bounded(input, json, 3, 3).is_err());
            assert!(bounded(r#""\u00e9""#, json, 1, 2).is_ok());
            assert!(bounded(r#""\u00e9""#, json, 1, 1).is_err());
        }
    }

    #[test]
    fn input_byte_boundary_is_inclusive() {
        for json in [false, true] {
            let input = b"\"x\"";
            assert!(
                parse_with_limits(
                    input,
                    json,
                    Limits {
                        input_bytes: 3,
                        ..Limits::default()
                    }
                )
                .is_ok()
            );
            assert!(
                parse_with_limits(
                    input,
                    json,
                    Limits {
                        input_bytes: 2,
                        ..Limits::default()
                    }
                )
                .is_err()
            );
        }
        let mut input = vec![b' '; MAX_TEMPLATE_BYTES];
        input[..4].copy_from_slice(b"null");
        assert!(parse_template(&input, true).is_ok());
        input.push(b' ');
        assert!(
            parse_template(&input, true)
                .unwrap_err()
                .to_string()
                .contains("input-byte limit")
        );
    }

    #[test]
    fn depth_boundary_counts_value_edges_from_root_zero() {
        for json in [false, true] {
            for depth in [0, MAX_TEMPLATE_DEPTH] {
                let input = format!("{}null{}", "[".repeat(depth), "]".repeat(depth));
                assert!(
                    parse_template(input.as_bytes(), json).is_ok(),
                    "{json} {depth}"
                );
            }
            let input = format!(
                "{}null{}",
                "[".repeat(MAX_TEMPLATE_DEPTH + 1),
                "]".repeat(MAX_TEMPLATE_DEPTH + 1)
            );
            assert!(error(&input, json).contains("depth limit"));
            assert!(
                parse_with_limits(
                    b"{}",
                    json,
                    Limits {
                        depth: 0,
                        ..Limits::default()
                    }
                )
                .is_ok()
            );
            assert!(
                parse_with_limits(
                    b"[]",
                    json,
                    Limits {
                        depth: 0,
                        ..Limits::default()
                    }
                )
                .is_ok()
            );
            assert!(
                parse_with_limits(
                    b"[null]",
                    json,
                    Limits {
                        depth: 0,
                        ..Limits::default()
                    }
                )
                .is_err()
            );
            assert!(
                parse_with_limits(
                    br#"{"a":null}"#,
                    json,
                    Limits {
                        depth: 0,
                        ..Limits::default()
                    }
                )
                .is_err()
            );
        }
    }

    #[test]
    fn keys_require_actual_string_events_including_aliases() {
        for input in [
            "1: value",
            "-1: value",
            "1.5: value",
            "18446744073709551616: value",
            "-9223372036854775809: value",
            "true: value",
            "false: value",
            "null: value",
            "~: value",
            "? [a, b]\n: value",
            "? {a: b}\n: value",
            "first: &key 1\n*key: value",
        ] {
            assert!(
                error(input, false).contains("keys must be strings"),
                "{input}"
            );
        }
        for input in [
            "'1': value",
            "'true': value",
            "'null': value",
            "first: &key '1'\n*key: value",
        ] {
            assert!(parse_template(input.as_bytes(), false).is_ok(), "{input}");
        }
        let value = parse_template(br#"{"1":1,"true":true,"null":null}"#, true).unwrap();
        assert_eq!(value["1"], 1);
    }

    #[test]
    fn duplicates_are_rejected_at_every_level_and_after_unescaping() {
        for json in [false, true] {
            for input in [
                r#"{"a":1,"a":2}"#,
                r#"{"outer":[{"a":1,"a":2}]}"#,
                r#"{"a":1,"\u0061":2}"#,
            ] {
                assert!(
                    error(input, json).contains("duplicate template mapping key"),
                    "{input}"
                );
            }
        }
        for input in [
            "a: 1\na: 2",
            "outer:\n  a: 1\n  a: 2",
            "first: &key same\nsame: 1\n*key: 2",
        ] {
            assert!(
                error(input, false).contains("duplicate template mapping key"),
                "{input}"
            );
        }
    }

    #[test]
    fn all_yaml_tag_spellings_and_tag_directives_are_rejected() {
        for input in [
            "!custom value",
            "! value",
            "!!str value",
            "!!int 1",
            "!!float 1.5",
            "!!bool true",
            "!!null null",
            "!!seq [a]",
            "!!map {a: b}",
            "!!timestamp 2020-01-01",
            "!<tag:yaml.org,2002:str> value",
            "!<https://example.com/type> value",
            "{key: !!str value}",
            "[!custom value]",
            "key: &value !!str value",
            "&value !!str value",
            "!!str key: value",
            "? !!str key\n: value",
            "%TAG !e! tag:example.com,2000:\n---\na: !e!foo bar",
            "%TAG !e! tag:example.com,2000:\n---\na: no_tag_used",
        ] {
            assert!(
                error(input, false).contains("tags are not allowed"),
                "{input}"
            );
        }
    }

    #[test]
    fn literal_exclamations_in_all_scalar_styles_remain_strings() {
        for (input, expected) in [
            ("'!tag'", "!tag"),
            ("\"!tag\"", "!tag"),
            ("'one '' !two'", "one ' !two"),
            ("hello !world", "hello !world"),
            ("hello!world", "hello!world"),
            ("hello\n!world", "hello !world"),
            ("|\n  !tag\n", "!tag\n"),
            (">\n  !tag\n  again\n", "!tag again\n"),
        ] {
            assert_eq!(
                parse_template(input.as_bytes(), false).unwrap(),
                Value::String(expected.into()),
                "{input}"
            );
        }
        let value = parse_template(
            b"key: hello\n  !world\nother: |2-\n  !!str literal\nthird: '!also literal'\n",
            false,
        )
        .unwrap();
        assert_eq!(value["key"], "hello !world");
        assert_eq!(value["other"], "!!str literal");
        assert_eq!(value["third"], "!also literal");
    }

    #[test]
    fn exactly_one_document_is_required() {
        for input in ["{} {}", "null\nnull", "{} trailing", "", " "] {
            assert!(parse_template(input.as_bytes(), true).is_err(), "{input}");
        }
        for input in [
            "---\na: 1\n---\nb: 2",
            "a: 1\n...\n---\nb: 2",
            "---\n---",
            "",
            " ",
            "# only a comment\n",
            "[unterminated",
            "{a: b",
        ] {
            assert!(parse_template(input.as_bytes(), false).is_err(), "{input}");
        }
        assert_eq!(
            parse_template(b"---\nnull\n...\n", false).unwrap(),
            Value::Null
        );
        assert_eq!(parse_template(b"---\n...\n", false).unwrap(), Value::Null);
        assert!(parse_template(b"%YAML 1.2\n---\na: 1\n", false).is_ok());
        assert!(parse_template(&[0xff], false).is_err());
        assert!(parse_template(&[0xff], true).is_err());
    }

    #[test]
    fn scalar_numeric_categories_and_signed_float_zero_are_preserved() {
        for json in [false, true] {
            let value = parse_template(
                b"[-9223372036854775808,18446744073709551615,1.0,-0.0,0.0,null,true,false]",
                json,
            )
            .unwrap();
            assert_eq!(value[0].as_i64(), Some(i64::MIN));
            assert_eq!(value[1].as_u64(), Some(u64::MAX));
            assert!(value[2].as_number().unwrap().is_f64());
            assert_eq!(value[2].as_f64(), Some(1.0));
            assert!(value[3].as_f64().unwrap().is_sign_negative());
            assert!(!value[4].as_f64().unwrap().is_sign_negative());
            assert!(value[5].is_null());
            assert_eq!(value[6], true);
            assert_eq!(value[7], false);
        }
        for input in [".nan", ".inf", "-.inf", "+.inf"] {
            assert!(error(input, false).contains("numbers must be finite"));
        }
        assert!(parse_template(b"1e999", true).is_err());
        for input in ["18446744073709551616", "-9223372036854775809"] {
            assert!(error(input, false).contains("integer is outside the supported range"));
        }
    }

    #[test]
    fn yaml_merge_spelling_is_not_interpreted_as_a_merge() {
        let value = parse_template(b"base: &base {a: 1}\ncopy: {<<: *base}\n", false).unwrap();
        assert_eq!(value["copy"]["<<"]["a"], 1);
        assert!(value["copy"].get("a").is_none());
    }

    #[test]
    fn each_alias_expansion_is_charged_for_nodes_and_strings() {
        let input = "seed: &a [abc, de]\ncopy: [*a, *a]";
        assert!(bounded(input, false, 13, 23).is_ok());
        assert!(
            bounded(input, false, 12, 23)
                .unwrap_err()
                .to_string()
                .contains("node limit")
        );
        assert!(
            bounded(input, false, 13, 22)
                .unwrap_err()
                .to_string()
                .contains("string-byte limit")
        );
        assert!(parse_template(b"&a [*a]", false).is_err());
        assert!(parse_template(b"[*missing]", false).is_err());
    }

    #[test]
    fn aliases_reach_the_real_expanded_string_boundary_without_large_input() {
        let scalar = "x".repeat(MAX_TEMPLATE_STRING_BYTES / 16);
        let mut input = format!("[&a \"{scalar}\"{}]", ", *a".repeat(15));
        assert!(input.len() < MAX_TEMPLATE_BYTES);
        let value = parse_template(input.as_bytes(), false).unwrap();
        assert_eq!(value.as_array().unwrap().len(), 16);
        drop(value);
        input.pop();
        input.push_str(", *a]");
        assert!(error(&input, false).contains("string-byte limit"));
    }

    #[test]
    fn aliases_reach_the_real_expanded_node_boundary_without_large_input() {
        // 8191 arrays of 32 nodes each, 31 scalar leaves, and the root array:
        // 8191 * 32 + 31 + 1 = 262144. Only about 8200 input nodes are needed.
        let group = ["null"; 31].join(",");
        let mut input = format!("[&a [{group}]{}{}]", ",*a".repeat(8190), ",null".repeat(31));
        assert!(input.len() < MAX_TEMPLATE_BYTES);
        let value = parse_template(input.as_bytes(), false).unwrap();
        assert_eq!(value.as_array().unwrap().len(), 8191 + 31);
        drop(value);
        input.pop();
        input.push_str(",null]");
        assert!(error(&input, false).contains("node limit"));
    }

    #[test]
    fn token_limit_is_checked_before_the_next_token() {
        assert!(yaml_tokens::check(b"[a]", 5).is_ok());
        assert!(
            yaml_tokens::check(b"[a]", 4)
                .unwrap_err()
                .to_string()
                .contains("token limit")
        );
        assert!(yaml_tokens::check(b"null", 3).is_ok());
        assert!(yaml_tokens::check(b"null", 2).is_err());
    }

    #[test]
    fn counters_and_container_lengths_do_not_wrap() {
        type Error = serde::de::value::Error;
        let mut budget = Budget::new(Limits {
            nodes: usize::MAX,
            string_bytes: usize::MAX,
            ..Limits::default()
        });
        budget.nodes = usize::MAX;
        assert!(budget.node::<Error>().is_err());
        budget.string_bytes = usize::MAX;
        assert!(budget.string::<Error>(1).is_err());
        assert!(budget.container_item::<Error>(usize::MAX).is_err());
        assert!(budget.child_depth::<Error>(usize::MAX).is_err());
    }

    #[test]
    fn error_messages_do_not_include_untrusted_scalar_or_path_text() {
        let key = "untrusted".repeat(10000);
        for json in [false, true] {
            let duplicate = format!("{{\"{key}\":1,\"{key}\":2}}");
            let message = error(&duplicate, json);
            assert!(message.len() < 200, "diagnostic length {}", message.len());
            assert!(!message.contains("untrusted"));
            let invalid = format!("{{\"{key}\":[[");
            let message = error(&invalid, json);
            assert!(message.len() < 200);
            assert!(!message.contains("untrusted"));
        }
    }
}
