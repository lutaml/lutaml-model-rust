mod contracts {
    pub use lutaml_model::contracts::*;
}
#[path = "../src/codecs/mod.rs"]
mod codecs;
#[path = "../src/generator/mod.rs"]
mod generator;
use codecs::RustJsonEmitter;
use contracts::*;
use std::collections::BTreeMap;
fn attribute(name: &str, t: BuiltinType) -> Attribute {
    Attribute {
        name: name.into(),
        value_type: TypeRef::Builtin { name: t },
        required: false,
        collection: Collection::Scalar(ScalarCollection),
        default: DefaultValue::Absent,
        initialize_empty: false,
        discriminator: None,
        reference: None,
        enumeration: None,
    }
}
fn model(name: &str, attributes: Vec<Attribute>, rules: Vec<KeyRule>) -> ModelDocument {
    ModelDocument {
        lml: 2,
        model: name.into(),
        register: "default".into(),
        attributes,
        mappings: Mappings {
            json: Some(KeyMapping {
                rules,
                root_mappings: None,
                child_mappings: None,
            }),
            ..Default::default()
        },
        ordered_content: None,
    }
}
fn rule(
    name: &str,
    aliases: &[&str],
    nil: RenderAction,
    empty: RenderAction,
    default: bool,
) -> KeyRule {
    KeyRule {
        key: aliases.iter().map(|s| s.to_string()).collect(),
        to: name.into(),
        render: Some(RenderPolicy {
            nil,
            empty,
            default,
        }),
    }
}
fn keep(name: &str, aliases: &[&str]) -> KeyRule {
    rule(name, aliases, RenderAction::Nil, RenderAction::Blank, true)
}
fn reference(name: &str, id: &str) -> Attribute {
    let mut a = attribute(name, BuiltinType::String);
    a.value_type = TypeRef::Model { id: id.into() };
    a
}
fn fixture() -> Bundle {
    let item = model(
        "Item",
        vec![
            attribute("id", BuiltinType::String),
            attribute("name", BuiltinType::String),
            attribute("amount", BuiltinType::Decimal),
        ],
        vec![
            keep("id", &["wire_id"]),
            keep("name", &["wire_name", "old_name"]),
            keep("amount", &["price"]),
        ],
    );
    let mut enabled = attribute("enabled", BuiltinType::Boolean);
    enabled.default = DefaultValue::Present(NeutralValue::Boolean(false));
    let mut entries = attribute("entries", BuiltinType::String);
    entries.collection = Collection::Bounded {
        min: 0,
        max: Some(3),
        class_binding: None,
    };
    let mut selected = reference("selected", "Item@default");
    selected.discriminator = Some(Discriminator {
        attribute: "kind".into(),
        cases: BTreeMap::from([("item".into(), "Item@default".into())]),
    });
    let mut record = model(
        "Record",
        vec![
            attribute("title", BuiltinType::String),
            enabled,
            entries,
            attribute("count", BuiltinType::Integer),
            attribute("amount", BuiltinType::Decimal),
            reference("child", "Item@default"),
            selected,
        ],
        vec![
            keep("title", &["label", "old_label"]),
            rule(
                "enabled",
                &["enabled"],
                RenderAction::Omit,
                RenderAction::Blank,
                false,
            ),
            rule(
                "entries",
                &["items"],
                RenderAction::Nil,
                RenderAction::Omit,
                true,
            ),
            keep("count", &["count"]),
            keep("amount", &["amount"]),
            keep("child", &["child"]),
            keep("selected", &["selected"]),
        ],
    );
    record.ordered_content = Some(OrderedContent {
        member: "__lml_content".into(),
        version: 1,
    });
    let mut collection = reference("records", "Item@default");
    collection.collection = Collection::Bounded {
        min: 0,
        max: None,
        class_binding: None,
    };
    let mut root = model(
        "Root",
        vec![collection.clone()],
        vec![keep("records", &["ignored"])],
    );
    root.mappings.json.as_mut().unwrap().root_mappings = Some(BTreeMap::from([(
        "id".into(),
        NeutralValue::String("key".into()),
    )]));
    let mut children = model(
        "Children",
        vec![collection],
        vec![keep("records", &["records", "old_records"])],
    );
    children.mappings.json.as_mut().unwrap().child_mappings = Some(BTreeMap::from([
        ("id".into(), NeutralValue::String("key".into())),
        (
            "name".into(),
            NeutralValue::Array(vec![
                NeutralValue::String("path".into()),
                NeutralValue::String("name".into()),
            ]),
        ),
        ("amount".into(), NeutralValue::String("price".into())),
    ]));
    Bundle {
        lml_bundle: 1,
        roots: vec!["Record@default".into()],
        models: BTreeMap::from([
            ("Item@default".into(), item),
            ("Record@default".into(), record),
            ("Root@default".into(), root),
            ("Children@default".into(), children),
        ]),
        value_types: BTreeMap::new(),
        capabilities: vec!["json".into()],
        provenance: None,
    }
}
#[test]
fn generation_is_deterministic_and_rejects_ambiguous_aliases() {
    let b = fixture();
    let output = RustJsonEmitter
        .generate_json_codecs(&b, &TypeBindings::new())
        .unwrap();
    assert_eq!(
        output,
        RustJsonEmitter
            .generate_json_codecs(&b, &TypeBindings::new())
            .unwrap()
    );
    assert_eq!(output.keys().collect::<Vec<_>>(), vec!["json_codecs.rs"]);
    let mut b = fixture();
    b.models
        .get_mut("Record@default")
        .unwrap()
        .mappings
        .json
        .as_mut()
        .unwrap()
        .rules[1]
        .key = vec!["label".into()];
    assert_eq!(
        RustJsonEmitter
            .generate_json_codecs(&b, &TypeBindings::new())
            .unwrap_err()
            .reason,
        ErrorReason::UnsupportedMapping
    );
}
#[test]
fn ambiguous_keyed_descriptors_reject_without_artifacts() {
    let mut b = fixture();
    let m = b.models.get_mut("Children@default").unwrap();
    let mut a = m.attributes[0].clone();
    a.name = "other".into();
    m.attributes.push(a);
    m.mappings
        .json
        .as_mut()
        .unwrap()
        .rules
        .push(keep("other", &["other"]));
    assert_eq!(
        RustJsonEmitter
            .generate_json_codecs(&b, &TypeBindings::new())
            .unwrap_err()
            .reason,
        ErrorReason::UnsupportedMapping
    );
}
#[test]
fn overlapping_keyed_paths_and_custom_codec_signatures_reject() {
    let mut b = fixture();
    let paths = b
        .models
        .get_mut("Children@default")
        .unwrap()
        .mappings
        .json
        .as_mut()
        .unwrap()
        .child_mappings
        .as_mut()
        .unwrap();
    paths.insert("amount".into(), NeutralValue::String("path".into()));
    assert_eq!(
        RustJsonEmitter
            .generate_json_codecs(&b, &TypeBindings::new())
            .unwrap_err()
            .reason,
        ErrorReason::UnsupportedMapping
    );
    let binding = TypeBinding {
        rust_type: "String".into(),
        codec: Some("arbitrary_callable".into()),
    };
    assert_eq!(
        RustJsonEmitter
            .generate_json_codecs(&fixture(), &BTreeMap::from([("unused".into(), binding)]))
            .unwrap_err()
            .reason,
        ErrorReason::UnsupportedMapping
    );
}
#[test]
fn emitted_consumer_compiles_and_executes_json_behavior() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = root.join("target/json-generated-consumer");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    let b = fixture();
    let models = generator::RustEmitter
        .generate_models(&b, &TypeBindings::new())
        .unwrap();
    let json = RustJsonEmitter
        .generate_json_codecs(&b, &TypeBindings::new())
        .unwrap();
    std::fs::write(dir.join("src/models.rs"), &models["models.rs"]).unwrap();
    std::fs::write(dir.join("src/json_codecs.rs"), &json["json_codecs.rs"]).unwrap();
    std::fs::write(
        dir.join("src/values.rs"),
        include_str!("../src/values/mod.rs"),
    )
    .unwrap();
    std::fs::write(dir.join("src/lib.rs"),format!("pub mod contracts {{ pub use lutaml_model::contracts::*; }}\npub mod values;\npub mod models;\npub mod json_codecs;\n{}",include_str!("../src/codecs/consumer_tests.rs.txt"))).unwrap();
    let manifest = format!(
        r#"[package]
name="generated-json-consumer"
version="0.0.0"
edition="2021"
[dependencies]
lutaml-model={{path={}}}
serde={{version="=1.0.219",features=["derive"]}}
serde_json={{version="=1.0.140",features=["arbitrary_precision"]}}
num-bigint="=0.4.6"
num-rational="=0.4.2"
num-traits="=0.2.19"
"#,
        serde_json::to_string(root.to_str().unwrap()).unwrap()
    );
    std::fs::write(dir.join("Cargo.toml"), manifest).unwrap();
    // The outer targeted Cargo invocation holds the host memory lock. This child
    // inherits that lease and uses one build worker rather than reacquiring it.
    let result = std::process::Command::new(env!("CARGO"))
        .args(["test", "--offline", "--manifest-path"])
        .arg(dir.join("Cargo.toml"))
        .env("CARGO_BUILD_JOBS", "1")
        .env("CARGO_TARGET_DIR", root.join("target/json-consumer-build"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "generated consumer failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(stdout.contains("7 passed"), "{stdout}");
}
