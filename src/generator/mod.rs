//! Static Rust source emission. Schemas are data; only caller-supplied bindings
//! may name external Rust types. No grammar is interpreted at runtime.
use crate::contracts::*;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

#[derive(Default)]
pub struct RustEmitter;
fn error(reason: ErrorReason, id: &str, field: &str, detail: &str) -> Box<ContractError> {
    Box::new(ContractError {
        reason,
        model_id: Some(id.into()),
        attribute: (!field.is_empty()).then(|| field.into()),
        format: None,
        rule: None,
        location: format!(
            "/models/{}/attributes/{}",
            id.replace('~', "~0").replace('/', "~1"),
            field.replace('~', "~0").replace('/', "~1")
        ),
        klass: None,
        detail: Some(detail.into()),
    })
}
fn quote(s: &str) -> String {
    serde_json::to_string(s).expect("strings serialize")
}
/// Deterministic Rust member identifier. Emitters must still reject collisions
/// among members of the same model after applying this normalization.
pub fn member_identifier(s: &str) -> String {
    let mut result = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            result.push(c);
        } else if !result.ends_with('_') {
            result.push('_');
        }
    }
    if result == "_" {
        result.push_str("Lml");
    }
    if result.is_empty() || result.starts_with(|c: char| c.is_ascii_digit()) {
        result.insert(0, '_');
    }
    if [
        "Self", "self", "super", "crate", "type", "mod", "fn", "pub", "struct", "enum", "match",
        "ref", "move", "use", "impl", "trait", "where", "loop", "in", "as", "const", "static",
        "let", "mut", "async", "await", "dyn", "default", "return", "break", "continue", "if",
        "else", "while", "for", "true", "false", "extern", "unsafe", "union", "abstract", "become",
        "box", "do", "final", "macro", "override", "priv", "typeof", "unsized", "virtual", "yield",
        "try",
    ]
    .contains(&result.as_str())
    {
        result.push('_');
    }
    result
}
/// Stable model-ID to Rust type-name map, with runtime-name and model collisions
/// rejected using the same rules as generated structs and PARG constructors.
pub fn model_names(bundle: &Bundle) -> ContractResult<BTreeMap<String, String>> {
    let mut used: BTreeSet<String> = [
        "Presence",
        "ValidationError",
        "ContentToken",
        "TextSource",
        "QName",
        "ExactInteger",
        "ExactDecimal",
        "Serialize",
        "Deserialize",
        "String",
        "Vec",
        "Box",
        "Result",
        "Option",
        "Default",
        "bool",
        "str",
        "char",
        "i8",
        "i16",
        "i32",
        "i64",
        "i128",
        "isize",
        "u8",
        "u16",
        "u32",
        "u64",
        "u128",
        "usize",
        "f32",
        "f64",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    bundle
        .models
        .iter()
        .map(|(id, m)| {
            let name = member_identifier(&m.model);
            if !used.insert(name.clone()) {
                return Err(error(
                    ErrorReason::ReservedMember,
                    id,
                    "",
                    "Rust type name collision",
                ));
            }
            Ok((id.clone(), name))
        })
        .collect()
}
fn builtin(t: &BuiltinType, id: &str, field: &str) -> ContractResult<String> {
    Ok(match t {
        BuiltinType::String | BuiltinType::Token | BuiltinType::Symbol => "String",
        BuiltinType::Boolean => "bool",
        BuiltinType::Integer => "ExactInteger",
        BuiltinType::Decimal | BuiltinType::Rational => "ExactDecimal",
        BuiltinType::Qname => "QName",
        // Lexical formats require a declared validator/binding rather than an
        // unchecked String or a platform-dependent date/URI interpretation.
        _ => {
            return Err(error(
                ErrorReason::UnsupportedType,
                id,
                field,
                "builtin requires an explicit lexical codec",
            ))
        }
    }
    .into())
}
fn field_type(
    a: &Attribute,
    id: &str,
    names: &BTreeMap<String, String>,
    bundle: &Bundle,
    bindings: &TypeBindings,
) -> ContractResult<String> {
    let target = a
        .reference
        .as_ref()
        .map(|id| TypeRef::Model { id: id.clone() })
        .unwrap_or_else(|| a.value_type.clone());
    match target {
        TypeRef::Builtin { name } => builtin(&name, id, &a.name),
        TypeRef::Model { id: target } => names
            .get(&target)
            .map(|n| format!("Box<{n}>"))
            .ok_or_else(|| {
                error(
                    ErrorReason::DanglingReference,
                    id,
                    &a.name,
                    "missing model target",
                )
            }),
        TypeRef::Value { id: target } => {
            if !bundle.value_types.contains_key(&target) {
                return Err(error(
                    ErrorReason::UnknownType,
                    id,
                    &a.name,
                    "missing value type",
                ));
            }
            bindings
                .get(&target)
                .map(|b| b.rust_type.clone())
                .ok_or_else(|| {
                    error(
                        ErrorReason::MissingTypeBinding,
                        id,
                        &a.name,
                        "named value type needs trusted binding",
                    )
                })
        }
    }
}
fn shape(a: &Attribute, ty: &str) -> String {
    match a.collection {
        Collection::Scalar(_) => ty.into(),
        Collection::Bounded { .. } => format!("Vec<{ty}>"),
    }
}
impl ModelEmitter for RustEmitter {
    fn generate_models(
        &self,
        bundle: &Bundle,
        bindings: &TypeBindings,
    ) -> ContractResult<SourceMap> {
        let names = model_names(bundle)?;
        let mut output = include_str!("runtime.rs.txt").to_owned();
        let mut type_names: BTreeSet<String> = names.values().cloned().collect();
        for (id, m) in &bundle.models {
            if let Some(ordered) = &m.ordered_content {
                if ordered.member != "__lml_content" || ordered.version != 1 {
                    return Err(error(
                        ErrorReason::InvalidOrderedContent,
                        id,
                        "__lml_content",
                        "unsupported ordered-content declaration",
                    ));
                }
            }
            let name = &names[id];
            let mut fields = BTreeSet::new();
            let mut types = Vec::new();
            for a in &m.attributes {
                if a.name == "__lml_content" {
                    return Err(error(
                        ErrorReason::ReservedMember,
                        id,
                        &a.name,
                        "reserved ordered-content member",
                    ));
                }
                if !fields.insert(member_identifier(&a.name)) {
                    return Err(error(
                        ErrorReason::ReservedMember,
                        id,
                        &a.name,
                        "Rust field name collision",
                    ));
                }
                if let Collection::Bounded {
                    min,
                    max,
                    class_binding,
                } = &a.collection
                {
                    if max.is_some_and(|max| max < *min) {
                        return Err(error(
                            ErrorReason::InvalidDocument,
                            id,
                            &a.name,
                            "invalid cardinality",
                        ));
                    }
                    if class_binding.is_some() {
                        return Err(error(
                            ErrorReason::MissingTypeBinding,
                            id,
                            &a.name,
                            "custom collection binding unsupported",
                        ));
                    }
                }
                let mut ty = field_type(a, id, &names, bundle, bindings)?;
                if let Some(d) = &a.discriminator {
                    if a.enumeration.is_some()
                        || a.reference.is_some()
                        || !matches!(a.value_type, TypeRef::Model { .. })
                        || d.cases.is_empty()
                    {
                        return Err(error(
                            ErrorReason::InvalidDocument,
                            id,
                            &a.name,
                            "invalid discriminator declaration",
                        ));
                    }
                    ty = format!("{name}_{}_Union", member_identifier(&a.name));
                    if !type_names.insert(ty.clone()) {
                        return Err(error(
                            ErrorReason::ReservedMember,
                            id,
                            &a.name,
                            "union type collision",
                        ));
                    }
                    writeln!(output,"#[allow(non_camel_case_types)]\n#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]\n#[serde(tag = {})]\npub enum {ty} {{",quote(&d.attribute)).unwrap();
                    for (index, (tag, target)) in d.cases.iter().enumerate() {
                        let target_name = names.get(target).ok_or_else(|| {
                            error(
                                ErrorReason::DanglingReference,
                                id,
                                &a.name,
                                "missing discriminator target",
                            )
                        })?;
                        if bundle.models[target]
                            .attributes
                            .iter()
                            .any(|a| a.name == d.attribute)
                        {
                            return Err(error(
                                ErrorReason::ReservedMember,
                                id,
                                &a.name,
                                "discriminator tag collides with payload member",
                            ));
                        }
                        writeln!(
                            output,
                            "#[serde(rename = {})] Case{index}(Box<{target_name}>),",
                            quote(tag)
                        )
                        .unwrap();
                    }
                    output.push_str("}\n");
                    writeln!(output,"impl {ty} {{ pub fn validate(&self)->Result<(),ValidationError>{{ match self {{").unwrap();
                    for index in 0..d.cases.len() {
                        writeln!(output, "Self::Case{index}(v) => v.validate(),").unwrap();
                    }
                    output.push_str("} } }\n");
                }
                if let Some(values) = &a.enumeration {
                    if values.is_empty()
                        || values
                            .iter()
                            .enumerate()
                            .any(|(i, v)| values[..i].contains(v))
                    {
                        return Err(error(
                            ErrorReason::InvalidDocument,
                            id,
                            &a.name,
                            "empty or duplicate enum",
                        ));
                    }
                    // Validate each literal against the declared scalar before emitting.
                    for value in values {
                        validate_literal(a, value, id, bundle)?;
                    }
                    let en = format!("{name}_{}_Enum", member_identifier(&a.name));
                    if !type_names.insert(en.clone()) {
                        return Err(error(
                            ErrorReason::ReservedMember,
                            id,
                            &a.name,
                            "enum type collision",
                        ));
                    }
                    writeln!(output,"#[allow(non_camel_case_types)]\n#[derive(Clone, Debug, PartialEq, Eq)]\npub enum {en} {{").unwrap();
                    for index in 0..values.len() {
                        writeln!(output, "Case{index},").unwrap();
                    }
                    output.push_str("}\n");
                    writeln!(output,"impl Serialize for {en} {{ fn serialize<S:serde::Serializer>(&self,s:S)->Result<S::Ok,S::Error>{{ match self {{").unwrap();
                    for (index, v) in values.iter().enumerate() {
                        writeln!(output,"Self::Case{index} => {{ let v:{ty}=serde_json::from_str({}).map_err(serde::ser::Error::custom)?; v.serialize(s) }},",quote(&serde_json::to_string(v).unwrap())).unwrap();
                    }
                    output.push_str("} } }\n");
                    writeln!(output,"impl<'de> Deserialize<'de> for {en} {{ fn deserialize<D:serde::Deserializer<'de>>(d:D)->Result<Self,D::Error>{{ let v={ty}::deserialize(d)?;").unwrap();
                    for (index, v) in values.iter().enumerate() {
                        writeln!(output,"if v == serde_json::from_str::<{ty}>({}).map_err(serde::de::Error::custom)? {{ return Ok(Self::Case{index}); }}",quote(&serde_json::to_string(v).unwrap())).unwrap();
                    }
                    output.push_str("Err(serde::de::Error::custom(\"invalid enum value\")) } }\n");
                    ty = en;
                }
                if let DefaultValue::Present(v) = &a.default {
                    validate_default(a, v, id, bundle)?;
                }
                if a.initialize_empty && matches!(a.collection, Collection::Scalar(_)) {
                    return Err(error(
                        ErrorReason::InvalidDocument,
                        id,
                        &a.name,
                        "initialize_empty requires collection",
                    ));
                }
                types.push(ty);
            }
            writeln!(output,"#[allow(non_camel_case_types, non_snake_case)]\n#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]\n#[serde(deny_unknown_fields)]\npub struct {name} {{").unwrap();
            for (a, ty) in m.attributes.iter().zip(&types) {
                writeln!(output,"#[serde(rename = {}, default, skip_serializing_if = \"Presence::is_absent\")]\npub {}: Presence<{}>,",quote(&a.name),member_identifier(&a.name),shape(a,ty)).unwrap();
            }
            if m.ordered_content.is_some() {
                output.push_str("#[serde(default, skip_serializing_if = \"Presence::is_absent\")]\npub __lml_content: Presence<Vec<ContentToken>>,\n");
            }
            output.push_str("}\n");
            writeln!(
                output,
                "impl Default for {name} {{ fn default()->Self {{ Self {{"
            )
            .unwrap();
            for a in &m.attributes {
                writeln!(output, "{}: Presence::Absent,", member_identifier(&a.name)).unwrap();
            }
            if m.ordered_content.is_some() {
                output.push_str("__lml_content: Presence::Absent,\n");
            }
            output.push_str("} } }\n");
            writeln!(
                output,
                "impl {name} {{\npub fn apply_defaults(&mut self)->Result<(),ValidationError>{{"
            )
            .unwrap();
            for (a, ty) in m.attributes.iter().zip(&types) {
                let field = member_identifier(&a.name);
                if let DefaultValue::Present(v) = &a.default {
                    writeln!(output,"if self.{field}.is_absent() {{ self.{field}=serde_json::from_str::<Presence<{}>>({}).map_err(|_|invalid({}, {}, \"non_serializable_default\"))?; }}",shape(a,ty),quote(&serde_json::to_string(v).unwrap()),quote(id),quote(&a.name)).unwrap();
                } else if a.initialize_empty {
                    writeln!(output,"if self.{field}.is_absent() {{ self.{field}=Presence::Value(Vec::new()); }}").unwrap();
                }
            }
            output.push_str("Ok(()) }\npub fn validate(&self)->Result<(),ValidationError>{\n");
            for a in &m.attributes {
                let field = member_identifier(&a.name);
                let err = format!(
                    "invalid({}, {}, \"invalid_document\")",
                    quote(id),
                    quote(&a.name)
                );
                if a.required {
                    writeln!(output,"if matches!(self.{field},Presence::Absent|Presence::Null) {{ return Err({err}); }}").unwrap();
                }
                if let Collection::Bounded { min, max, .. } = a.collection {
                    let mut bounds = Vec::new();
                    if min > 0 {
                        bounds.push(format!("(v.len() as u64) < {min}u64"));
                    }
                    if let Some(max) = max {
                        bounds.push(format!("(v.len() as u64) > {max}u64"));
                    }
                    if !bounds.is_empty() {
                        writeln!(output,"if let Presence::Value(v)=&self.{field} {{ if {} {{ return Err({err}); }} }}",bounds.join(" || ")).unwrap();
                    }
                }
                if a.enumeration.is_none()
                    && (a.discriminator.is_some()
                        || a.reference.is_some()
                        || matches!(a.value_type, TypeRef::Model { .. }))
                {
                    match a.collection {
                        Collection::Scalar(_) => {
                            writeln!(
                                output,
                                "if let Presence::Value(v)=&self.{field} {{ v.validate()?; }}"
                            )
                            .unwrap();
                        }
                        Collection::Bounded { .. } => {
                            writeln!(output,"if let Presence::Value(values)=&self.{field} {{ for v in values {{ v.validate()?; }} }}").unwrap();
                        }
                    }
                }
            }
            if m.ordered_content.is_some() {
                let err = format!(
                    "invalid({}, \"__lml_content\", \"invalid_ordered_content\")",
                    quote(id)
                );
                writeln!(
                    output,
                    "if matches!(self.__lml_content,Presence::Null) {{ return Err({err}); }}"
                )
                .unwrap();
                output.push_str("if let Presence::Value(tokens)=&self.__lml_content { let mut seen=std::collections::BTreeSet::new(); for token in tokens { if let Some((member,index))=token.reference() { let count=match member {\n");
                for a in &m.attributes {
                    let field = member_identifier(&a.name);
                    let count = match a.collection {
                        Collection::Scalar(_) => "1",
                        Collection::Bounded { .. } => "v.len() as u64",
                    };
                    let pattern = match a.collection {
                        Collection::Scalar(_) => "_",
                        Collection::Bounded { .. } => "v",
                    };
                    writeln!(output,"{} => match &self.{field} {{ Presence::Value({pattern})=>{count}, _=>0 }},",quote(&a.name)).unwrap();
                }
                writeln!(output,"_ => return Err({err}), }}; if index>=count || !seen.insert((member,index)) {{ return Err({err}); }} }} }} }}").unwrap();
            }
            output.push_str("Ok(()) }\n}\n");
        }
        Ok(BTreeMap::from([("models.rs".into(), output)]))
    }
}
fn validate_literal(
    a: &Attribute,
    v: &NeutralValue,
    id: &str,
    _bundle: &Bundle,
) -> ContractResult<()> {
    let valid = match (&a.value_type, v) {
        (
            TypeRef::Builtin {
                name: BuiltinType::Boolean,
            },
            NeutralValue::Boolean(_),
        ) => true,
        (
            TypeRef::Builtin {
                name: BuiltinType::String | BuiltinType::Token | BuiltinType::Symbol,
            },
            NeutralValue::String(_),
        ) => true,
        (
            TypeRef::Builtin {
                name: BuiltinType::Integer,
            },
            NeutralValue::String(s),
        ) => {
            let d = s.strip_prefix('-').unwrap_or(s);
            !d.is_empty() && d.bytes().all(|c| c.is_ascii_digit())
        }
        (
            TypeRef::Builtin {
                name: BuiltinType::Decimal | BuiltinType::Rational,
            },
            NeutralValue::String(s),
        ) => {
            let d = s.strip_prefix('-').unwrap_or(s);
            let parts: Vec<_> = d.split('.').collect();
            parts.len() <= 2
                && parts
                    .iter()
                    .all(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit()))
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(error(
            ErrorReason::NonSerializableDefault,
            id,
            &a.name,
            "literal does not match declared type",
        ))
    }
}
fn validate_default(
    a: &Attribute,
    v: &NeutralValue,
    id: &str,
    bundle: &Bundle,
) -> ContractResult<()> {
    if matches!(v, NeutralValue::Null) {
        return if a.required {
            Err(error(
                ErrorReason::NonSerializableDefault,
                id,
                &a.name,
                "required default cannot be null",
            ))
        } else {
            Ok(())
        };
    }
    let values = match (&a.collection, v) {
        (Collection::Bounded { min, max, .. }, NeutralValue::Array(v)) => {
            if (v.len() as u64) < *min || max.is_some_and(|m| (v.len() as u64) > m) {
                return Err(error(
                    ErrorReason::NonSerializableDefault,
                    id,
                    &a.name,
                    "default cardinality",
                ));
            }
            v.as_slice()
        }
        (Collection::Bounded { .. }, _) => {
            return Err(error(
                ErrorReason::NonSerializableDefault,
                id,
                &a.name,
                "collection default must be array",
            ))
        }
        _ => std::slice::from_ref(v),
    };
    for v in values {
        validate_literal(a, v, id, bundle)?;
        if a.enumeration.as_ref().is_some_and(|vs| !vs.contains(v)) {
            return Err(error(
                ErrorReason::NonSerializableDefault,
                id,
                &a.name,
                "default outside enum",
            ));
        }
    }
    Ok(())
}

impl PargBindingEmitter for RustEmitter {
    fn generate_parg_bindings(
        &self,
        bundle: &Bundle,
        bindings: &[PargClassBinding],
    ) -> ContractResult<SourceMap> {
        let names = model_names(bundle)?;
        let mut output=String::from("// Compile-time PARG capture constructors; parsing and application behavior are external.\nuse super::models::*;\n");
        let mut used = BTreeSet::new();
        for binding in bindings {
            let model = bundle.models.get(&binding.model_id).ok_or_else(|| {
                error(
                    ErrorReason::DanglingReference,
                    &binding.model_id,
                    "",
                    "unknown PARG class",
                )
            })?;
            let function = format!("bind_{}", member_identifier(&binding.rule));
            if !used.insert(function.clone()) {
                return Err(error(
                    ErrorReason::ReservedMember,
                    &binding.model_id,
                    "",
                    "duplicate grammar rule function",
                ));
            }
            let mut members = BTreeSet::new();
            let mut args = Vec::new();
            let mut assignments = Vec::new();
            for (index, (capture, member)) in binding.captures.iter().enumerate() {
                if capture.is_empty() || !members.insert(member) {
                    return Err(error(
                        ErrorReason::InvalidDocument,
                        &binding.model_id,
                        member,
                        "invalid or duplicate capture member",
                    ));
                }
                let a = model
                    .attributes
                    .iter()
                    .find(|a| &a.name == member)
                    .ok_or_else(|| {
                        error(
                            ErrorReason::InvalidDocument,
                            &binding.model_id,
                            member,
                            "unknown capture member",
                        )
                    })?;
                if a.discriminator.is_some()
                    || a.enumeration.is_some()
                    || matches!(a.value_type, TypeRef::Value { .. })
                {
                    return Err(error(
                        ErrorReason::MissingTypeBinding,
                        &binding.model_id,
                        member,
                        "capture requires explicit scalar/model type",
                    ));
                }
                let ty = shape(
                    a,
                    &field_type(a, &binding.model_id, &names, bundle, &TypeBindings::new())?,
                );
                args.push(format!("capture_{index}: {ty}"));
                assignments.push(format!(
                    "{}: Presence::Value(capture_{index})",
                    member_identifier(member)
                ));
                writeln!(
                    output,
                    "// capture {} -> member {}",
                    quote(capture),
                    quote(member)
                )
                .unwrap();
            }
            let model_name = &names[&binding.model_id];
            if assignments.len() < model.attributes.len() || model.ordered_content.is_some() {
                assignments.push("..Default::default()".into());
            }
            writeln!(output,"pub fn {function}({})->Result<{model_name},ValidationError>{{ let mut value={model_name}{{ {} }}; value.apply_defaults()?; value.validate()?; Ok(value) }}",args.join(", "),assignments.join(", ")).unwrap();
        }
        Ok(BTreeMap::from([("parg_bindings.rs".into(), output)]))
    }
}
