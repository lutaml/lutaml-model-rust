//! Wire types shared by loaders, emitters, generated runtimes and wasm adapters.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

pub type ModelId = String;
pub type ValueTypeId = String;
pub type SourceMap = BTreeMap<String, String>;
pub type ContractResult<T> = Result<T, Box<ContractError>>;
pub type ExactInteger = num_bigint::BigInt;
pub type ExactRational = num_rational::BigRational;

/// Neutral data deliberately has no floating-point variant. Exact values cross
/// the portable boundary as strings; these integers are structural metadata.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum NeutralValue {
    Null,
    Boolean(bool),
    Signed(i64),
    Unsigned(u64),
    String(String),
    Array(Vec<NeutralValue>),
    Object(BTreeMap<String, NeutralValue>),
}

// Decode through the arbitrary-precision JSON number representation. Serde's
// untagged fallback could otherwise mistake its private number map for an
// ordinary object and admit fractional structural metadata.
impl<'de> Deserialize<'de> for NeutralValue {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        fn convert(value: serde_json::Value) -> Result<NeutralValue, &'static str> {
            use serde_json::Value;
            match value {
                Value::Null => Ok(NeutralValue::Null),
                Value::Bool(v) => Ok(NeutralValue::Boolean(v)),
                Value::String(v) => Ok(NeutralValue::String(v)),
                Value::Number(v) => {
                    if let Some(v) = v.as_i64() {
                        Ok(NeutralValue::Signed(v))
                    } else if let Some(v) = v.as_u64() {
                        Ok(NeutralValue::Unsigned(v))
                    } else {
                        Err("structural numbers must be bounded integers; exact values require strings")
                    }
                }
                Value::Array(v) => v
                    .into_iter()
                    .map(convert)
                    .collect::<Result<Vec<_>, _>>()
                    .map(NeutralValue::Array),
                Value::Object(v) => v
                    .into_iter()
                    .map(|(key, value)| convert(value).map(|value| (key, value)))
                    .collect::<Result<BTreeMap<_, _>, _>>()
                    .map(NeutralValue::Object),
            }
        }
        convert(serde_json::Value::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

/// Instance fields distinguish absent, explicit null, and a present value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Presence<T> {
    #[default]
    Absent,
    Null,
    Value(T),
}

/// Defaults use the portable boolean tag, including an explicit null value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DefaultValue {
    Absent,
    Present(NeutralValue),
}
impl Serialize for DefaultValue {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map =
            serializer.serialize_map(Some(if matches!(self, Self::Absent) { 1 } else { 2 }))?;
        map.serialize_entry("present", &matches!(self, Self::Present(_)))?;
        if let Self::Present(value) = self {
            map.serialize_entry("value", value)?;
        }
        map.end()
    }
}
impl<'de> Deserialize<'de> for DefaultValue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut map = BTreeMap::<String, NeutralValue>::deserialize(deserializer)?;
        let present = map.remove("present");
        let value = map.remove("value");
        if !map.is_empty() {
            return Err(serde::de::Error::custom("unknown default member"));
        }
        match (present, value) {
            (Some(NeutralValue::Boolean(false)), None) => Ok(Self::Absent),
            (Some(NeutralValue::Boolean(true)), Some(value)) => Ok(Self::Present(value)),
            _ => Err(serde::de::Error::custom(
                "default requires present:false or present:true with value",
            )),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TypeRef {
    Builtin { name: BuiltinType },
    Model { id: ModelId },
    Value { id: ValueTypeId },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuiltinType {
    String,
    Token,
    Uri,
    Email,
    Hostname,
    Ip,
    Boolean,
    Integer,
    Decimal,
    Rational,
    Date,
    Time,
    DateTime,
    TimeWithoutDate,
    Duration,
    Qname,
    Base64Binary,
    HexBinary,
    Symbol,
    Hash,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Collection {
    Scalar(ScalarCollection),
    Bounded {
        min: u64,
        max: Option<u64>,
        #[serde(rename = "class", skip_serializing_if = "Option::is_none")]
        class_binding: Option<String>,
    },
}
/// Scalar collection metadata is exactly false, never true.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScalarCollection;
impl Serialize for ScalarCollection {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bool(false)
    }
}
impl<'de> Deserialize<'de> for ScalarCollection {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        if bool::deserialize(d)? {
            Err(serde::de::Error::custom("scalar collection must be false"))
        } else {
            Ok(Self)
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Discriminator {
    pub attribute: String,
    pub cases: BTreeMap<String, ModelId>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attribute {
    pub name: String,
    #[serde(rename = "type")]
    pub value_type: TypeRef,
    pub required: bool,
    pub collection: Collection,
    pub default: DefaultValue,
    pub initialize_empty: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discriminator: Option<Discriminator>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<ModelId>,
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    pub enumeration: Option<Vec<NeutralValue>>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValueType {
    pub base: BuiltinType,
    pub facets: BTreeMap<String, NeutralValue>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderedContent {
    pub member: String,
    pub version: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TextSource {
    Member { attribute: String, index: u64 },
    Literal { literal: String },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContentToken {
    Element {
        attribute: String,
        index: u64,
    },
    Text {
        #[serde(flatten)]
        source: TextSource,
    },
    Cdata {
        #[serde(flatten)]
        source: TextSource,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum Namespace {
    Inherit,
    Unqualified,
    Uri {
        uri: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        prefix: Option<String>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderAction {
    Omit,
    Nil,
    Blank,
    AsNil,
    AsBlank,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderPolicy {
    pub nil: RenderAction,
    pub empty: RenderAction,
    pub default: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum XmlRuleKind {
    Attribute,
    Element,
    Content,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct XmlRule {
    pub kind: XmlRuleKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<Vec<String>>,
    pub to: String,
    pub namespace: Namespace,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cdata: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub render: Option<RenderPolicy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discriminator: Option<Discriminator>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct XmlRoot {
    pub element: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Compositor {
    Sequence {
        members: Vec<CompositorMember>,
        min: u64,
        max: Option<u64>,
    },
    Choice {
        members: Vec<CompositorMember>,
        min: u64,
        max: Option<u64>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CompositorMember {
    Attribute(String),
    Group(Box<Compositor>),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct XmlMapping {
    pub root: Option<XmlRoot>,
    pub namespace: Namespace,
    pub rules: Vec<XmlRule>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence: Option<Compositor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub choice: Option<Compositor>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyRule {
    pub key: Vec<String>,
    pub to: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub render: Option<RenderPolicy>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyMapping {
    pub rules: Vec<KeyRule>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_mappings: Option<BTreeMap<String, NeutralValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub child_mappings: Option<BTreeMap<String, NeutralValue>>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mappings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xml: Option<XmlMapping>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json: Option<KeyMapping>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub yaml: Option<KeyMapping>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelDocument {
    pub lml: u32,
    pub model: String,
    pub register: String,
    pub attributes: Vec<Attribute>,
    pub mappings: Mappings,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ordered_content: Option<OrderedContent>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bundle {
    pub lml_bundle: u32,
    pub roots: Vec<ModelId>,
    pub models: BTreeMap<ModelId, ModelDocument>,
    pub value_types: BTreeMap<ValueTypeId, ValueType>,
    pub capabilities: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<BTreeMap<String, NeutralValue>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorReason {
    UnsupportedVersion,
    InvalidDocument,
    DuplicateId,
    DanglingReference,
    UnknownType,
    UnsupportedType,
    MissingTypeBinding,
    UnsupportedFormat,
    UnsupportedMapping,
    NonSerializableDefault,
    LambdaDefault,
    CustomInitialize,
    ImperativeElementOrder,
    InvalidOrderedContent,
    ReservedMember,
    NonterminatingRational,
    UnknownDiscriminator,
    RoundTripMismatch,
    ChecksumMismatch,
    CapabilityMissing,
    NotSerializable,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractError {
    pub reason: ErrorReason,
    pub model_id: Option<ModelId>,
    pub attribute: Option<String>,
    pub format: Option<String>,
    pub rule: Option<String>,
    pub location: String,
    pub klass: Option<String>,
    pub detail: Option<String>,
}
impl fmt::Display for ContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} at {}", self.reason, self.location)?;
        if let Some(detail) = &self.detail {
            write!(f, ": {detail}")?;
        }
        Ok(())
    }
}
impl std::error::Error for ContractError {}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Target {
    Ruby,
    Ts,
    Rust,
    Wasm,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorpusPin {
    pub generation: String,
    pub sha256: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilePin {
    pub path: String,
    pub sha256: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyPin {
    pub name: String,
    pub revision: String,
    pub sha256: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub manifest: u32,
    pub lml_checksum: String,
    pub schema_version: u32,
    pub generator_version: String,
    pub target: Target,
    pub capabilities: Vec<String>,
    pub corpora: Vec<CorpusPin>,
    pub files: Vec<FilePin>,
    pub dependencies: Vec<DependencyPin>,
}
/// Trusted target bindings are supplied by the caller, never by schema code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeBinding {
    pub rust_type: String,
    pub codec: Option<String>,
}
pub type TypeBindings = BTreeMap<ValueTypeId, TypeBinding>;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputFormat {
    Json,
    Yaml,
}
pub trait BundleLoader {
    fn load_bundle(&self, bytes: &[u8], format: InputFormat) -> ContractResult<Bundle>;
}
pub trait ModelEmitter {
    fn generate_models(
        &self,
        bundle: &Bundle,
        bindings: &TypeBindings,
    ) -> ContractResult<SourceMap>;
}
pub trait JsonCodecEmitter {
    fn generate_json_codecs(
        &self,
        bundle: &Bundle,
        bindings: &TypeBindings,
    ) -> ContractResult<SourceMap>;
}
pub trait XmlCodecEmitter {
    fn generate_xml_codecs(
        &self,
        bundle: &Bundle,
        bindings: &TypeBindings,
    ) -> ContractResult<SourceMap>;
}
/// Compile-time PARG lowering hook. Bind stable grammar rule/capture names to
/// LML class IDs and members before emitting Rust; this carries no runtime
/// grammar or executable behavior. Application/typed behavior stays external.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PargClassBinding {
    pub rule: String,
    pub model_id: ModelId,
    pub captures: BTreeMap<String, String>,
}
pub trait PargBindingEmitter {
    fn generate_parg_bindings(
        &self,
        bundle: &Bundle,
        bindings: &[PargClassBinding],
    ) -> ContractResult<SourceMap>;
}
