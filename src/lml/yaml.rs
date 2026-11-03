//! Narrow, owning adapter over raw YAML events. Raw scalar text avoids the
//! arbitrary-integer -> f64 fallback in serde_yaml's generic deserializer.
use super::{failure, pointer, Node};
use crate::contracts::{ContractResult, ErrorReason};
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use unsafe_libyaml as unsafe_yaml;

struct Parser<'input> {
    // The parser stores pointers into itself and the input. Heap allocation keeps
    // its address stable; the lifetime prevents it outliving the borrowed bytes.
    storage: Box<MaybeUninit<unsafe_yaml::yaml_parser_t>>,
    input: PhantomData<&'input [u8]>,
}
struct Event(unsafe_yaml::yaml_event_t);
impl Drop for Event {
    fn drop(&mut self) {
        // SAFETY: each Event owns one successfully initialized event and is never
        // copied. All scalar data is copied before this matching deletion.
        unsafe { unsafe_yaml::yaml_event_delete(&mut self.0) }
    }
}
impl Drop for Parser<'_> {
    fn drop(&mut self) {
        // SAFETY: construction initializes this allocation exactly once. No
        // other object owns the parser and the input still lives at this point.
        unsafe { unsafe_yaml::yaml_parser_delete(self.storage.as_mut_ptr()) }
    }
}
impl<'input> Parser<'input> {
    fn new(input: &'input [u8]) -> ContractResult<Self> {
        let mut storage = Box::<unsafe_yaml::yaml_parser_t>::new_uninit();
        // SAFETY: the allocation has the exact parser size/alignment and remains
        // stable until Drop. The borrowed input lives for the parser lifetime.
        unsafe {
            if unsafe_yaml::yaml_parser_initialize(storage.as_mut_ptr()).fail {
                return Err(failure(
                    ErrorReason::InvalidDocument,
                    "",
                    "YAML parser initialization failed",
                ));
            }
            unsafe_yaml::yaml_parser_set_encoding(
                storage.as_mut_ptr(),
                unsafe_yaml::YAML_UTF8_ENCODING,
            );
            unsafe_yaml::yaml_parser_set_input_string(
                storage.as_mut_ptr(),
                input.as_ptr(),
                input.len() as u64,
            );
        }
        Ok(Self {
            storage,
            input: PhantomData,
        })
    }
    fn next(&mut self, at: &str) -> ContractResult<Event> {
        let mut event = MaybeUninit::uninit();
        // SAFETY: the parser is initialized, pinned and exclusively borrowed.
        // On success yaml_parser_parse initializes all event fields. On failure
        // no event owner is created, matching libyaml's documented API usage.
        unsafe {
            if unsafe_yaml::yaml_parser_parse(self.storage.as_mut_ptr(), event.as_mut_ptr()).fail {
                let parser = self.storage.assume_init_ref();
                return Err(failure(
                    ErrorReason::InvalidDocument,
                    at,
                    format!(
                        "malformed YAML at line {}, column {}",
                        parser.problem_mark.line + 1,
                        parser.problem_mark.column + 1
                    ),
                ));
            }
            Ok(Event(event.assume_init()))
        }
    }
    fn node(&mut self, event: Event, at: &str, depth: usize) -> ContractResult<Node> {
        if depth > 128 {
            return Err(failure(
                ErrorReason::InvalidDocument,
                at,
                "maximum nesting exceeded",
            ));
        }
        // SAFETY: union fields are read only for their corresponding event type.
        // Value slices are parser-owned and remain valid until Event is dropped;
        // all returned Node data is copied into owned Rust allocations.
        unsafe {
            match event.0.type_ {
                unsafe_yaml::YAML_ALIAS_EVENT => Err(failure(
                    ErrorReason::InvalidDocument,
                    at,
                    "YAML aliases are not allowed",
                )),
                unsafe_yaml::YAML_SCALAR_EVENT => {
                    let scalar = &event.0.data.scalar;
                    properties(scalar.anchor, scalar.tag, at)?;
                    let bytes = std::slice::from_raw_parts(scalar.value, scalar.length as usize);
                    let value = std::str::from_utf8(bytes)
                        .map_err(|e| failure(ErrorReason::InvalidDocument, at, e.to_string()))?;
                    if scalar.style == unsafe_yaml::YAML_PLAIN_SCALAR_STYLE {
                        plain(value, at)
                    } else {
                        Ok(Node::String(value.into()))
                    }
                }
                unsafe_yaml::YAML_SEQUENCE_START_EVENT => {
                    let sequence = &event.0.data.sequence_start;
                    properties(sequence.anchor, sequence.tag, at)?;
                    let mut nodes = Vec::new();
                    loop {
                        let child = pointer(at, &nodes.len().to_string());
                        let event = self.next(&child)?;
                        if event.0.type_ == unsafe_yaml::YAML_SEQUENCE_END_EVENT {
                            break;
                        }
                        nodes.push(self.node(event, &child, depth + 1)?);
                    }
                    Ok(Node::Array(nodes))
                }
                unsafe_yaml::YAML_MAPPING_START_EVENT => {
                    let mapping = &event.0.data.mapping_start;
                    properties(mapping.anchor, mapping.tag, at)?;
                    let mut entries = Vec::new();
                    loop {
                        let key = self.next(at)?;
                        if key.0.type_ == unsafe_yaml::YAML_MAPPING_END_EVENT {
                            break;
                        }
                        let Node::String(key) = self.node(key, at, depth + 1)? else {
                            return Err(failure(
                                ErrorReason::InvalidDocument,
                                at,
                                "mapping keys must be strings",
                            ));
                        };
                        let child = pointer(at, &key);
                        if entries.iter().any(|(previous, _)| previous == &key) {
                            return Err(failure(
                                if at == "/models" || at == "/value_types" {
                                    ErrorReason::DuplicateId
                                } else {
                                    ErrorReason::InvalidDocument
                                },
                                &child,
                                "duplicate YAML mapping key",
                            ));
                        }
                        let value = self.next(&child)?;
                        entries.push((key, self.node(value, &child, depth + 1)?));
                    }
                    Ok(Node::Object(entries))
                }
                _ => Err(failure(
                    ErrorReason::InvalidDocument,
                    at,
                    "expected YAML data node",
                )),
            }
        }
    }
}
fn properties(anchor: *mut u8, tag: *mut u8, at: &str) -> ContractResult<()> {
    if !anchor.is_null() || !tag.is_null() {
        return Err(failure(
            ErrorReason::InvalidDocument,
            at,
            "YAML anchors and tags are not allowed",
        ));
    }
    Ok(())
}

pub fn parse(bytes: &[u8]) -> ContractResult<Node> {
    let mut parser = Parser::new(bytes)?;
    if parser.next("")?.0.type_ != unsafe_yaml::YAML_STREAM_START_EVENT
        || parser.next("")?.0.type_ != unsafe_yaml::YAML_DOCUMENT_START_EVENT
    {
        return Err(failure(
            ErrorReason::InvalidDocument,
            "",
            "expected one YAML document",
        ));
    }
    let first = parser.next("")?;
    let node = parser.node(first, "", 0)?;
    if parser.next("")?.0.type_ != unsafe_yaml::YAML_DOCUMENT_END_EVENT
        || parser.next("")?.0.type_ != unsafe_yaml::YAML_STREAM_END_EVENT
    {
        return Err(failure(
            ErrorReason::InvalidDocument,
            "",
            "multiple YAML documents are not allowed",
        ));
    }
    Ok(node)
}

fn plain(value: &str, at: &str) -> ContractResult<Node> {
    match value {
        "" | "null" | "Null" | "NULL" | "~" => return Ok(Node::Null),
        "true" | "True" | "TRUE" => return Ok(Node::Bool(true)),
        "false" | "False" | "FALSE" => return Ok(Node::Bool(false)),
        _ => {}
    }
    let unpositive = value.strip_prefix('+').unwrap_or(value);
    if value.starts_with('+') && unpositive.starts_with(['+', '-']) {
        return Ok(Node::String(value.into()));
    }
    let (negative, unsigned) = unpositive
        .strip_prefix('-')
        .map_or((false, unpositive), |s| (true, s));
    // YAML 1.2 radix forms are integers too. BigInt is used only for exact
    // lexical normalization; decimal values never pass through IEEE-754.
    for (prefix, radix) in [("0x", 16), ("0o", 8), ("0b", 2)] {
        if let Some(digits) = unsigned.strip_prefix(prefix) {
            if !digits.is_empty() && digits.chars().all(|c| c.is_digit(radix)) {
                let mut integer =
                    num_bigint::BigInt::parse_bytes(digits.as_bytes(), radix).unwrap();
                if negative {
                    integer = -integer;
                }
                return number(&integer.to_string(), at);
            }
            return Ok(Node::String(value.into()));
        }
    }
    if !unsigned.is_empty() && unsigned.bytes().all(|b| b.is_ascii_digit()) {
        // Leading-zero decimal scalars are strings under YAML 1.2, matching the
        // pinned serde_yaml scalar schema.
        if unsigned.len() > 1 && unsigned.starts_with('0') {
            return Ok(Node::String(value.into()));
        }
        let integer = num_bigint::BigInt::parse_bytes(unpositive.as_bytes(), 10).unwrap();
        return number(&integer.to_string(), at);
    }
    if [".inf", ".nan"].contains(&unsigned.to_ascii_lowercase().as_str()) {
        return Err(failure(
            ErrorReason::InvalidDocument,
            at,
            "nonfinite YAML numbers are not portable",
        ));
    }
    let lexical = if let Some(fraction) = unsigned.strip_prefix('.') {
        format!("0.{fraction}")
    } else {
        unsigned.to_owned()
    };
    let (mantissa, exponent) = lexical
        .find(['e', 'E'])
        .map_or((lexical.as_str(), ""), |i| lexical.split_at(i));
    let mantissa = if mantissa.ends_with('.') {
        format!("{mantissa}0")
    } else {
        mantissa.into()
    };
    let lexical = format!("{}{mantissa}{exponent}", if negative { "-" } else { "" });
    if (value.contains('.') || value.contains(['e', 'E']))
        && lexical.parse::<serde_json::Number>().is_ok()
    {
        return number(&lexical, at);
    }
    Ok(Node::String(value.into()))
}
fn number(value: &str, at: &str) -> ContractResult<Node> {
    value
        .parse::<serde_json::Number>()
        .map(Node::Number)
        .map_err(|e| failure(ErrorReason::InvalidDocument, at, e.to_string()))
}
