//! Pure, bounded CSV-to-inline-dataset import. This is an independent source
//! contract and does not change any HIR, MIR, composition, or scene schema.
//!
//! `retained_content_bytes` accounts for UTF-8 content in the supplied spec
//! (format, key, and column names), the returned header and dataset key, every
//! row's repeated map field names and string values, and every canonical key
//! retained in the duplicate-key set during import. It is a peak content charge:
//! the key set is dropped before return. Caller-owned CSV/JSON source bytes,
//! scalar values, container metadata/capacity, and the scanner's borrowed field
//! descriptors are excluded. Byte, row, column, and cell caps separately bound
//! those resources. No decoded cell is allocated until its charge is checked.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use schemars::JsonSchema;
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Number, Value};

use crate::{Dataset, Diagnostic, VizError, VizResult, value_as_key};

pub const CSV_IMPORT_FORMAT: &str = "vizir-csv-import/1";
pub const CSV_IMPORT_MAX_CSV_BYTES: usize = 8 * 1024 * 1024;
pub const CSV_IMPORT_MAX_SPEC_BYTES: usize = 256 * 1024;
pub const CSV_IMPORT_MAX_ROWS: usize = 65_536;
pub const CSV_IMPORT_MAX_COLUMNS: usize = 128;
pub const CSV_IMPORT_MAX_CELLS: usize = 1_048_576;
pub const CSV_IMPORT_MAX_CELL_BYTES: usize = 64 * 1024;
pub const CSV_IMPORT_MAX_RETAINED_BYTES: usize = 32 * 1024 * 1024;
pub const CSV_IMPORT_MAX_NAME_BYTES: usize = 256;

/// Limits may tighten, but never raise, the public hard caps. Zero is permitted
/// and causes any input requiring that resource to fail. Cells count data cells;
/// the mandatory header is separately bounded by the column and cell-byte caps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CsvImportLimits {
    pub max_csv_bytes: usize,
    pub max_spec_bytes: usize,
    pub max_rows: usize,
    pub max_columns: usize,
    pub max_cells: usize,
    pub max_cell_bytes: usize,
    pub max_retained_bytes: usize,
}

impl Default for CsvImportLimits {
    fn default() -> Self {
        Self {
            max_csv_bytes: CSV_IMPORT_MAX_CSV_BYTES,
            max_spec_bytes: CSV_IMPORT_MAX_SPEC_BYTES,
            max_rows: CSV_IMPORT_MAX_ROWS,
            max_columns: CSV_IMPORT_MAX_COLUMNS,
            max_cells: CSV_IMPORT_MAX_CELLS,
            max_cell_bytes: CSV_IMPORT_MAX_CELL_BYTES,
            max_retained_bytes: CSV_IMPORT_MAX_RETAINED_BYTES,
        }
    }
}

impl CsvImportLimits {
    pub fn validate(&self) -> VizResult<()> {
        let hard = Self::default();
        for (value, cap) in [
            (self.max_csv_bytes, hard.max_csv_bytes),
            (self.max_spec_bytes, hard.max_spec_bytes),
            (self.max_rows, hard.max_rows),
            (self.max_columns, hard.max_columns),
            (self.max_cells, hard.max_cells),
            (self.max_cell_bytes, hard.max_cell_bytes),
            (self.max_retained_bytes, hard.max_retained_bytes),
        ] {
            if value > cap {
                return Err(error(
                    "VIZ-CSV-0001",
                    "limits may only tighten the hard caps",
                    None,
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CsvImportSpec {
    #[schemars(extend("const" = CSV_IMPORT_FORMAT))]
    pub format: String,
    #[schemars(length(min = 1, max = 128))]
    pub columns: Vec<CsvImportColumn>,
    #[schemars(length(min = 1, max = 256), extend("pattern" = "^[^\\u0000-\\u001F\\u007F-\\u009F\\u2028\\u2029]+$(?![\\s\\S])"))]
    pub key: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CsvImportColumn {
    #[schemars(length(min = 1, max = 256), extend("pattern" = "^[^\\u0000-\\u001F\\u007F-\\u009F\\u2028\\u2029]+$(?![\\s\\S])"))]
    pub name: String,
    #[serde(rename = "type")]
    pub column_type: CsvColumnType,
}

#[derive(Debug, Clone, Copy, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CsvColumnType {
    String,
    Int64,
    Float64,
    Bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CsvImportResult {
    pub dataset: Dataset,
    pub header: Vec<String>,
    pub had_utf8_bom: bool,
    pub row_count: usize,
    pub retained_content_bytes: usize,
}

/// Separate schema root. Byte-length, unique-column-name, and key-reference
/// constraints also require `validate_csv_import_spec`; JSON Schema lengths are
/// Unicode character lengths and cannot express UTF-8 byte limits.
pub fn csv_import_spec_schema() -> Value {
    let mut schema = serde_json::to_value(schemars::schema_for!(CsvImportSpec))
        .expect("CSV import spec schema must serialize");
    schema["$schema"] = Value::String("https://json-schema.org/draft/2020-12/schema".to_owned());
    schema
}

/// Strict JSON reader. Checks source size first and bounds column counts and
/// owned names while decoding, including when limits are tightened. Unknown,
/// duplicate, missing, and null members are errors. No intermediate JSON value
/// or unbounded vector is constructed.
pub fn parse_csv_import_spec(bytes: &[u8], limits: &CsvImportLimits) -> VizResult<CsvImportSpec> {
    limits.validate()?;
    if bytes.len() > limits.max_spec_bytes {
        return Err(error(
            "VIZ-CSV-0010",
            "specification byte limit exceeded",
            None,
        ));
    }
    let mut decoder = serde_json::Deserializer::from_slice(bytes);
    let spec = SpecSeed { limits }
        .deserialize(&mut decoder)
        .map_err(spec_decode_error)?;
    decoder.end().map_err(spec_decode_error)?;
    Ok(spec)
}

fn spec_decode_error(err: serde_json::Error) -> VizError {
    error(
        "VIZ-CSV-0002",
        "invalid CSV import specification; expected the strict vizir-csv-import/1 contract",
        Some(format!("spec line {} column {}", err.line(), err.column())),
    )
}

pub fn validate_csv_import_spec(spec: &CsvImportSpec, limits: &CsvImportLimits) -> VizResult<()> {
    limits.validate()?;
    if spec.format != CSV_IMPORT_FORMAT {
        return Err(error(
            "VIZ-CSV-0002",
            "unsupported CSV import specification format",
            None,
        ));
    }
    if spec.columns.is_empty() || spec.columns.len() > limits.max_columns {
        return Err(error(
            "VIZ-CSV-0002",
            "column count is empty or exceeds the limit",
            None,
        ));
    }
    if !valid_name(&spec.key) {
        return Err(error("VIZ-CSV-0002", "invalid key field name", None));
    }
    let mut names = BTreeSet::new();
    for (index, column) in spec.columns.iter().enumerate() {
        if !valid_name(&column.name) || !names.insert(column.name.as_str()) {
            return Err(error(
                "VIZ-CSV-0002",
                "column names must be valid and unique",
                Some(format!("spec column {}", index + 1)),
            ));
        }
    }
    match spec.columns.iter().find(|column| column.name == spec.key) {
        Some(column)
            if matches!(
                column.column_type,
                CsvColumnType::String | CsvColumnType::Int64
            ) => {}
        _ => {
            return Err(error(
                "VIZ-CSV-0002",
                "key must name a string or int64 column",
                None,
            ));
        }
    }
    let mut budget = ContentBudget::new(limits.max_retained_bytes);
    charge_spec(&mut budget, spec)?;
    Ok(())
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= CSV_IMPORT_MAX_NAME_BYTES
        && !name.chars().any(|ch| {
            ch <= '\u{1f}'
                || ('\u{7f}'..='\u{9f}').contains(&ch)
                || ch == '\u{2028}'
                || ch == '\u{2029}'
        })
}

fn error(code: &str, message: &str, location: Option<String>) -> VizError {
    let mut diagnostic = Diagnostic::new(code, message);
    diagnostic.source = location;
    VizError::validation(&[diagnostic])
}

// Bounded generic serde decoding. These visitors do not echo attacker-controlled
// strings into diagnostics, reserve from a sequence size_hint, or decode an
// additional column once the count limit has been reached.
#[derive(Clone, Copy)]
enum Member {
    Format,
    Columns,
    Key,
    Name,
    Type,
}
impl<'de> Deserialize<'de> for Member {
    fn deserialize<D: Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        struct MemberVisitor;
        impl Visitor<'_> for MemberVisitor {
            type Value = Member;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a known CSV import member")
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Member, E> {
                match value {
                    "format" => Ok(Member::Format),
                    "columns" => Ok(Member::Columns),
                    "key" => Ok(Member::Key),
                    "name" => Ok(Member::Name),
                    "type" => Ok(Member::Type),
                    _ => Err(E::custom("unknown CSV import member")),
                }
            }
        }
        decoder.deserialize_identifier(MemberVisitor)
    }
}

impl<'de> Deserialize<'de> for CsvColumnType {
    fn deserialize<D: Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        struct TypeVisitor;
        impl Visitor<'_> for TypeVisitor {
            type Value = CsvColumnType;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("string, int64, float64, or bool")
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                match value {
                    "string" => Ok(CsvColumnType::String),
                    "int64" => Ok(CsvColumnType::Int64),
                    "float64" => Ok(CsvColumnType::Float64),
                    "bool" => Ok(CsvColumnType::Bool),
                    _ => Err(E::custom("unsupported CSV column type")),
                }
            }
        }
        decoder.deserialize_any(TypeVisitor)
    }
}

struct NameSeed<'a> {
    retained: &'a mut usize,
    max_retained: usize,
    is_format: bool,
}
impl Visitor<'_> for NameSeed<'_> {
    type Value = String;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a bounded CSV import name or format")
    }
    fn visit_str<E: de::Error>(self, value: &str) -> Result<String, E> {
        if (self.is_format && value != CSV_IMPORT_FORMAT) || (!self.is_format && !valid_name(value))
        {
            return Err(E::custom("invalid CSV import name or format"));
        }
        let charged = self
            .retained
            .checked_add(value.len())
            .filter(|n| *n <= self.max_retained)
            .ok_or_else(|| E::custom("CSV specification retained-content limit exceeded"))?;
        *self.retained = charged;
        Ok(value.to_owned())
    }
}
impl<'de> DeserializeSeed<'de> for NameSeed<'_> {
    type Value = String;
    fn deserialize<D: Deserializer<'de>>(self, decoder: D) -> Result<String, D::Error> {
        decoder.deserialize_any(self)
    }
}

struct ColumnSeed<'a> {
    retained: &'a mut usize,
    max_retained: usize,
}
impl<'de> Visitor<'de> for ColumnSeed<'_> {
    type Value = CsvImportColumn;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a CSV import column")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut name = None;
        let mut column_type = None;
        while let Some(member) = map.next_key::<Member>()? {
            match member {
                Member::Name if name.is_none() => {
                    name = Some(map.next_value_seed(NameSeed {
                        retained: self.retained,
                        max_retained: self.max_retained,
                        is_format: false,
                    })?)
                }
                Member::Type if column_type.is_none() => column_type = Some(map.next_value()?),
                _ => {
                    return Err(de::Error::custom("duplicate or unknown CSV column member"));
                }
            }
        }
        Ok(CsvImportColumn {
            name: name.ok_or_else(|| de::Error::missing_field("name"))?,
            column_type: column_type.ok_or_else(|| de::Error::missing_field("type"))?,
        })
    }
}
impl<'de> DeserializeSeed<'de> for ColumnSeed<'_> {
    type Value = CsvImportColumn;
    fn deserialize<D: Deserializer<'de>>(self, decoder: D) -> Result<Self::Value, D::Error> {
        decoder.deserialize_map(self)
    }
}
impl<'de> Deserialize<'de> for CsvImportColumn {
    fn deserialize<D: Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        ColumnSeed {
            retained: &mut 0,
            max_retained: CSV_IMPORT_MAX_RETAINED_BYTES,
        }
        .deserialize(decoder)
    }
}

struct RejectExtra;
impl<'de> DeserializeSeed<'de> for RejectExtra {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, _: D) -> Result<(), D::Error> {
        Err(de::Error::custom("CSV column count limit exceeded"))
    }
}
struct ColumnsSeed<'a> {
    retained: &'a mut usize,
    limits: &'a CsvImportLimits,
}
impl<'de> Visitor<'de> for ColumnsSeed<'_> {
    type Value = Vec<CsvImportColumn>;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a nonempty bounded CSV column array")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut columns = Vec::new();
        loop {
            if columns.len() == self.limits.max_columns {
                seq.next_element_seed(RejectExtra)?;
                break;
            }
            match seq.next_element_seed(ColumnSeed {
                retained: self.retained,
                max_retained: self.limits.max_retained_bytes,
            })? {
                Some(column) => columns.push(column),
                None => break,
            }
        }
        if columns.is_empty() {
            return Err(de::Error::custom("CSV columns cannot be empty"));
        }
        Ok(columns)
    }
}
impl<'de> DeserializeSeed<'de> for ColumnsSeed<'_> {
    type Value = Vec<CsvImportColumn>;
    fn deserialize<D: Deserializer<'de>>(self, decoder: D) -> Result<Self::Value, D::Error> {
        decoder.deserialize_seq(self)
    }
}

struct SpecSeed<'a> {
    limits: &'a CsvImportLimits,
}
impl<'de> Visitor<'de> for SpecSeed<'_> {
    type Value = CsvImportSpec;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a strict CSV import specification")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut format = None;
        let mut columns = None;
        let mut key = None;
        let mut retained = 0;
        while let Some(member) = map.next_key::<Member>()? {
            match member {
                Member::Format if format.is_none() => {
                    format = Some(map.next_value_seed(NameSeed {
                        retained: &mut retained,
                        max_retained: self.limits.max_retained_bytes,
                        is_format: true,
                    })?)
                }
                Member::Key if key.is_none() => {
                    key = Some(map.next_value_seed(NameSeed {
                        retained: &mut retained,
                        max_retained: self.limits.max_retained_bytes,
                        is_format: false,
                    })?)
                }
                Member::Columns if columns.is_none() => {
                    columns = Some(map.next_value_seed(ColumnsSeed {
                        retained: &mut retained,
                        limits: self.limits,
                    })?)
                }
                _ => {
                    return Err(de::Error::custom("duplicate or unknown CSV import member"));
                }
            }
        }
        let spec = CsvImportSpec {
            format: format.ok_or_else(|| de::Error::missing_field("format"))?,
            columns: columns.ok_or_else(|| de::Error::missing_field("columns"))?,
            key: key.ok_or_else(|| de::Error::missing_field("key"))?,
        };
        validate_csv_import_spec(&spec, self.limits).map_err(de::Error::custom)?;
        Ok(spec)
    }
}
impl<'de> DeserializeSeed<'de> for SpecSeed<'_> {
    type Value = CsvImportSpec;
    fn deserialize<D: Deserializer<'de>>(self, decoder: D) -> Result<Self::Value, D::Error> {
        decoder.deserialize_map(self)
    }
}
impl<'de> Deserialize<'de> for CsvImportSpec {
    fn deserialize<D: Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        SpecSeed {
            limits: &CsvImportLimits::default(),
        }
        .deserialize(decoder)
    }
}

struct ContentBudget {
    used: usize,
    limit: usize,
}
impl ContentBudget {
    fn new(limit: usize) -> Self {
        Self { used: 0, limit }
    }
    fn charge(&mut self, bytes: usize, location: Option<String>) -> VizResult<()> {
        self.used = self
            .used
            .checked_add(bytes)
            .filter(|n| *n <= self.limit)
            .ok_or_else(|| {
                error(
                    "VIZ-CSV-0010",
                    "retained-content byte limit exceeded",
                    location,
                )
            })?;
        Ok(())
    }
}
fn charge_spec(budget: &mut ContentBudget, spec: &CsvImportSpec) -> VizResult<()> {
    budget.charge(spec.format.len(), None)?;
    budget.charge(spec.key.len(), None)?;
    for column in &spec.columns {
        budget.charge(column.name.len(), None)?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct Field<'a> {
    raw: &'a str,
    decoded_len: usize,
    escaped: bool,
    byte: usize,
}
impl Field<'_> {
    fn matches(&self, expected: &str) -> bool {
        let mut raw = self.raw.bytes();
        let mut wanted = expected.bytes();
        while let Some(byte) = raw.next() {
            if self.escaped && byte == b'"' {
                raw.next();
            }
            if wanted.next() != Some(byte) {
                return false;
            }
        }
        wanted.next().is_none()
    }
    fn into_string(self) -> String {
        if !self.escaped {
            return self.raw.to_owned();
        }
        let mut decoded = String::with_capacity(self.decoded_len);
        let raw = self.raw.as_bytes();
        let mut start = 0;
        let mut index = 0;
        while index < raw.len() {
            if raw[index] == b'"' {
                decoded.push_str(&self.raw[start..index]);
                decoded.push('"');
                index += 2;
                start = index;
            } else {
                index += 1;
            }
        }
        decoded.push_str(&self.raw[start..]);
        decoded
    }
}

struct Scanner<'a> {
    text: &'a str,
    offset: usize,
    record: usize,
    limits: &'a CsvImportLimits,
}
impl<'a> Scanner<'a> {
    fn location(&self, column: usize, byte: usize) -> Option<String> {
        Some(location(self.record, column, byte))
    }
    fn fail(&self, code: &str, message: &str, column: usize, byte: usize) -> VizError {
        error(code, message, self.location(column, byte))
    }
    fn add_cell_bytes(
        &self,
        size: &mut usize,
        amount: usize,
        column: usize,
        byte: usize,
    ) -> VizResult<()> {
        *size = size
            .checked_add(amount)
            .filter(|n| *n <= self.limits.max_cell_bytes)
            .ok_or_else(|| {
                self.fail(
                    "VIZ-CSV-0010",
                    "decoded cell byte limit exceeded",
                    column,
                    byte,
                )
            })?;
        Ok(())
    }
    fn next_record(&mut self) -> VizResult<Option<Vec<Field<'a>>>> {
        let bytes = self.text.as_bytes();
        if self.offset == bytes.len() {
            return Ok(None);
        }
        self.record += 1;
        if bytes[self.offset] == b'\n'
            || (bytes[self.offset] == b'\r' && bytes.get(self.offset + 1) == Some(&b'\n'))
        {
            return Err(self.fail(
                "VIZ-CSV-0005",
                "blank records are not permitted",
                1,
                self.offset,
            ));
        }
        let mut fields = Vec::new();
        loop {
            let column = fields.len() + 1;
            if column > self.limits.max_columns {
                return Err(self.fail(
                    "VIZ-CSV-0010",
                    "CSV column count limit exceeded",
                    column,
                    self.offset,
                ));
            }
            let byte = self.offset;
            let mut decoded_len = 0;
            let mut escaped = false;
            let start;
            let end;
            if bytes.get(self.offset) == Some(&b'"') {
                self.offset += 1;
                start = self.offset;
                loop {
                    match bytes.get(self.offset).copied() {
                        None => {
                            return Err(self.fail(
                                "VIZ-CSV-0005",
                                "unterminated quoted field",
                                column,
                                self.offset,
                            ));
                        }
                        Some(0) => {
                            return Err(self.fail(
                                "VIZ-CSV-0005",
                                "NUL is not permitted",
                                column,
                                self.offset,
                            ));
                        }
                        Some(b'"') => {
                            if bytes.get(self.offset + 1) == Some(&b'"') {
                                self.add_cell_bytes(&mut decoded_len, 1, column, self.offset)?;
                                escaped = true;
                                self.offset += 2;
                            } else {
                                end = self.offset;
                                self.offset += 1;
                                break;
                            }
                        }
                        Some(b'\r') => {
                            if bytes.get(self.offset + 1) != Some(&b'\n') {
                                return Err(self.fail(
                                    "VIZ-CSV-0005",
                                    "bare CR is not permitted",
                                    column,
                                    self.offset,
                                ));
                            }
                            self.add_cell_bytes(&mut decoded_len, 2, column, self.offset)?;
                            self.offset += 2;
                        }
                        Some(_) => {
                            self.add_cell_bytes(&mut decoded_len, 1, column, self.offset)?;
                            self.offset += 1;
                        }
                    }
                }
                if !matches!(
                    bytes.get(self.offset),
                    None | Some(b',') | Some(b'\n') | Some(b'\r')
                ) {
                    return Err(self.fail(
                        "VIZ-CSV-0005",
                        "unexpected byte after closing quote",
                        column,
                        self.offset,
                    ));
                }
            } else {
                start = self.offset;
                while let Some(&next) = bytes.get(self.offset) {
                    match next {
                        b',' | b'\n' | b'\r' => break,
                        b'"' => {
                            return Err(self.fail(
                                "VIZ-CSV-0005",
                                "quote must begin a field",
                                column,
                                self.offset,
                            ));
                        }
                        0 => {
                            return Err(self.fail(
                                "VIZ-CSV-0005",
                                "NUL is not permitted",
                                column,
                                self.offset,
                            ));
                        }
                        _ => {
                            self.add_cell_bytes(&mut decoded_len, 1, column, self.offset)?;
                            self.offset += 1;
                        }
                    }
                }
                end = self.offset;
            }
            fields.push(Field {
                raw: &self.text[start..end],
                decoded_len,
                escaped,
                byte,
            });
            match bytes.get(self.offset) {
                None => return Ok(Some(fields)),
                Some(b',') => self.offset += 1,
                Some(b'\n') => {
                    self.offset += 1;
                    return Ok(Some(fields));
                }
                Some(b'\r') => {
                    if bytes.get(self.offset + 1) != Some(&b'\n') {
                        return Err(self.fail(
                            "VIZ-CSV-0005",
                            "bare CR is not permitted",
                            column,
                            self.offset,
                        ));
                    }
                    self.offset += 2;
                    return Ok(Some(fields));
                }
                _ => unreachable!("field scanner stops only at a separator"),
            }
        }
    }
}

fn location(record: usize, column: usize, byte: usize) -> String {
    format!("csv record {record} column {column} byte {byte}")
}

/// Import without file, path, URL, network, locale, trimming, NA, or type
/// inference. Record/column locations are one-based; byte offsets are zero-based
/// in the original input, including any initial BOM.
pub fn import_csv(
    bytes: &[u8],
    spec: &CsvImportSpec,
    limits: &CsvImportLimits,
) -> VizResult<CsvImportResult> {
    validate_csv_import_spec(spec, limits)?;
    if bytes.len() > limits.max_csv_bytes {
        return Err(error(
            "VIZ-CSV-0010",
            "CSV source byte limit exceeded",
            None,
        ));
    }
    let text = std::str::from_utf8(bytes).map_err(|err| {
        error(
            "VIZ-CSV-0004",
            "CSV source must be valid UTF-8",
            Some(format!("csv byte {}", err.valid_up_to())),
        )
    })?;
    let had_utf8_bom = text.starts_with('\u{feff}');
    let mut scanner = Scanner {
        text,
        offset: if had_utf8_bom { 3 } else { 0 },
        record: 0,
        limits,
    };
    let header_fields = scanner
        .next_record()?
        .ok_or_else(|| error("VIZ-CSV-0006", "mandatory CSV header is missing", None))?;
    if header_fields.len() != spec.columns.len() {
        return Err(error(
            "VIZ-CSV-0006",
            "header must match the ordered specification columns",
            Some(location(1, 1, if had_utf8_bom { 3 } else { 0 })),
        ));
    }
    for (index, (field, column)) in header_fields.iter().zip(&spec.columns).enumerate() {
        if !field.matches(&column.name) {
            return Err(error(
                "VIZ-CSV-0006",
                "header must match the ordered specification columns",
                Some(location(1, index + 1, field.byte)),
            ));
        }
    }
    let mut budget = ContentBudget::new(limits.max_retained_bytes);
    charge_spec(&mut budget, spec)?;
    budget.charge(spec.key.len(), None)?;
    for column in &spec.columns {
        budget.charge(column.name.len(), None)?;
    }
    let header = spec
        .columns
        .iter()
        .map(|column| column.name.clone())
        .collect();
    let mut dataset = Dataset {
        key: spec.key.clone(),
        rows: Vec::new(),
    };
    let key_index = spec
        .columns
        .iter()
        .position(|column| column.name == spec.key)
        .expect("validated key");
    let mut seen_keys = BTreeSet::new();
    let mut cell_count = 0usize;
    while scanner.offset < bytes.len() {
        if dataset.rows.len() >= limits.max_rows {
            return Err(error(
                "VIZ-CSV-0010",
                "CSV row count limit exceeded",
                Some(location(scanner.record + 1, 1, scanner.offset)),
            ));
        }
        cell_count = cell_count
            .checked_add(spec.columns.len())
            .filter(|n| *n <= limits.max_cells)
            .ok_or_else(|| {
                error(
                    "VIZ-CSV-0010",
                    "CSV data cell count limit exceeded",
                    Some(location(scanner.record + 1, 1, scanner.offset)),
                )
            })?;
        let fields = scanner
            .next_record()?
            .expect("unconsumed input has a record");
        if fields.len() != spec.columns.len() {
            return Err(error(
                "VIZ-CSV-0007",
                "data record width does not match the header",
                Some(location(scanner.record, 1, fields[0].byte)),
            ));
        }
        // Charge all repeated names and string bytes before any owned row/map/
        // decoded field is allocated. Numeric and bool values are scalars.
        for (index, (field, column)) in fields.iter().zip(&spec.columns).enumerate() {
            budget.charge(
                column.name.len(),
                Some(location(scanner.record, index + 1, field.byte)),
            )?;
            if column.column_type == CsvColumnType::String {
                budget.charge(
                    field.decoded_len,
                    Some(location(scanner.record, index + 1, field.byte)),
                )?;
            }
        }
        let key_field = fields[key_index];
        let key_len = match spec.columns[key_index].column_type {
            CsvColumnType::String if key_field.decoded_len > 0 => key_field.decoded_len,
            CsvColumnType::Int64 => {
                let value = parse_value(key_field, CsvColumnType::Int64).map_err(|message| {
                    error(
                        "VIZ-CSV-0008",
                        message,
                        Some(location(scanner.record, key_index + 1, key_field.byte)),
                    )
                })?;
                int_key_length(value.as_i64().expect("int64 key"))
            }
            _ => {
                return Err(error(
                    "VIZ-CSV-0009",
                    "string data keys must be nonempty",
                    Some(location(scanner.record, key_index + 1, key_field.byte)),
                ));
            }
        };
        // The canonical duplicate-key copy is also charged before owning a row.
        budget.charge(
            key_len,
            Some(location(scanner.record, key_index + 1, key_field.byte)),
        )?;
        let mut row = BTreeMap::new();
        for (index, (field, column)) in fields.iter().zip(&spec.columns).enumerate() {
            let value = parse_value(*field, column.column_type).map_err(|message| {
                error(
                    "VIZ-CSV-0008",
                    message,
                    Some(location(scanner.record, index + 1, field.byte)),
                )
            })?;
            if index == key_index {
                let key = value_as_key(&value).expect("validated scalar key");
                if !seen_keys.insert(key) {
                    return Err(error(
                        "VIZ-CSV-0009",
                        "duplicate canonical data key",
                        Some(location(scanner.record, index + 1, field.byte)),
                    ));
                }
            }
            row.insert(column.name.clone(), value);
        }
        dataset.rows.push(row);
    }
    if dataset.rows.is_empty() {
        return Err(error(
            "VIZ-CSV-0007",
            "CSV must contain at least one data record",
            None,
        ));
    }
    Ok(CsvImportResult {
        row_count: dataset.rows.len(),
        dataset,
        header,
        had_utf8_bom,
        retained_content_bytes: budget.used,
    })
}

fn int_key_length(value: i64) -> usize {
    let mut magnitude = value.unsigned_abs();
    let mut length = usize::from(value < 0) + 1;
    while magnitude >= 10 {
        magnitude /= 10;
        length += 1;
    }
    length
}

fn parse_value(field: Field<'_>, column_type: CsvColumnType) -> Result<Value, &'static str> {
    if column_type == CsvColumnType::String {
        return Ok(Value::String(field.into_string()));
    }
    if field.escaped {
        return Err("typed value has invalid literal syntax");
    }
    let raw = field.raw;
    match column_type {
        CsvColumnType::String => unreachable!(),
        CsvColumnType::Int64 => {
            if !int_grammar(raw.as_bytes()) {
                return Err("int64 requires a canonical integer literal");
            }
            raw.parse::<i64>()
                .map(|number| Value::Number(Number::from(number)))
                .map_err(|_| "int64 value is outside the signed 64-bit range")
        }
        CsvColumnType::Float64 => {
            if !json_number_grammar(raw.as_bytes()) {
                return Err("float64 requires JSON number syntax");
            }
            let number = raw
                .parse::<f64>()
                .map_err(|_| "float64 conversion failed")?;
            if !number.is_finite() {
                return Err("float64 must be finite");
            }
            if number == 0.0
                && raw
                    .bytes()
                    .take_while(|byte| *byte != b'e' && *byte != b'E')
                    .any(|byte| (b'1'..=b'9').contains(&byte))
            {
                return Err("nonzero float64 significand underflows to zero");
            }
            Number::from_f64(number)
                .map(Value::Number)
                .ok_or("float64 must be finite")
        }
        CsvColumnType::Bool => match raw {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            _ => Err("bool must be exactly true or false"),
        },
    }
}

fn integer_end(bytes: &[u8]) -> Option<usize> {
    let mut index = usize::from(bytes.first() == Some(&b'-'));
    match bytes.get(index) {
        Some(b'0') => Some(index + 1),
        Some(b'1'..=b'9') => {
            index += 1;
            while bytes.get(index).is_some_and(u8::is_ascii_digit) {
                index += 1;
            }
            Some(index)
        }
        _ => None,
    }
}
fn int_grammar(bytes: &[u8]) -> bool {
    integer_end(bytes) == Some(bytes.len())
}
fn json_number_grammar(bytes: &[u8]) -> bool {
    let Some(mut index) = integer_end(bytes) else {
        return false;
    };
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let first = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if first == index {
            return false;
        }
    }
    if matches!(bytes.get(index), Some(b'e' | b'E')) {
        index += 1;
        if matches!(bytes.get(index), Some(b'+' | b'-')) {
            index += 1;
        }
        let first = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if first == index {
            return false;
        }
    }
    index == bytes.len()
}
