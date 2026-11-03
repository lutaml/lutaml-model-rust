mod contracts {
    pub use lutaml_model::contracts::*;
}
#[path = "../src/generator/mod.rs"]
mod generator;
use contracts::*;
use generator::RustEmitter;
use std::collections::BTreeMap;
fn attribute(name: &str, ty: TypeRef) -> Attribute {
    Attribute {
        name: name.into(),
        value_type: ty,
        required: false,
        collection: Collection::Scalar(ScalarCollection),
        default: DefaultValue::Absent,
        initialize_empty: false,
        discriminator: None,
        reference: None,
        enumeration: None,
    }
}
fn string(name: &str) -> Attribute {
    attribute(
        name,
        TypeRef::Builtin {
            name: BuiltinType::String,
        },
    )
}
fn model(name: &str, attributes: Vec<Attribute>) -> ModelDocument {
    ModelDocument {
        lml: 2,
        model: name.into(),
        register: "default".into(),
        attributes,
        mappings: Mappings::default(),
        ordered_content: None,
    }
}
fn bundle() -> Bundle {
    let mut title = string("title");
    title.required = true;
    let mut enabled = attribute(
        "enabled",
        TypeRef::Builtin {
            name: BuiltinType::Boolean,
        },
    );
    enabled.default = DefaultValue::Present(NeutralValue::Boolean(false));
    enabled.enumeration = Some(vec![
        NeutralValue::Boolean(false),
        NeutralValue::Boolean(true),
    ]);
    let mut tags = string("tags");
    tags.collection = Collection::Bounded {
        min: 1,
        max: Some(2),
        class_binding: None,
    };
    let child = attribute(
        "child",
        TypeRef::Model {
            id: "Node@default".into(),
        },
    );
    let amount = attribute(
        "amount",
        TypeRef::Builtin {
            name: BuiltinType::Decimal,
        },
    );
    let mut item = attribute(
        "item",
        TypeRef::Model {
            id: "Leaf@default".into(),
        },
    );
    item.discriminator = Some(Discriminator {
        attribute: "kind".into(),
        cases: BTreeMap::from([
            ("leaf".into(), "Leaf@default".into()),
            ("node".into(), "Node@default".into()),
        ]),
    });
    let mut node = model("Node", vec![title, enabled, tags, child, amount, item]);
    node.ordered_content = Some(OrderedContent {
        member: "__lml_content".into(),
        version: 1,
    });
    Bundle {
        lml_bundle: 1,
        roots: vec!["Node@default".into()],
        models: BTreeMap::from([
            ("Node@default".into(), node),
            ("Leaf@default".into(), model("Leaf", vec![string("text")])),
        ]),
        value_types: BTreeMap::new(),
        capabilities: vec![],
        provenance: None,
    }
}
fn parg() -> Vec<PargClassBinding> {
    vec![PargClassBinding {
        rule: "leaf".into(),
        model_id: "Leaf@default".into(),
        captures: BTreeMap::from([("body".into(), "text".into())]),
    }]
}
#[test]
fn static_sources_are_deterministic_and_match_compiled_fixture() {
    let b = bundle();
    let sources = RustEmitter
        .generate_models(&b, &TypeBindings::new())
        .unwrap();
    assert_eq!(
        sources,
        RustEmitter
            .generate_models(&b, &TypeBindings::new())
            .unwrap()
    );
    let binders = RustEmitter.generate_parg_bindings(&b, &parg()).unwrap();

    assert_eq!(
        sources["models.rs"],
        include_str!("fixtures/rust_generated/src/models.rs")
    );
    assert_eq!(
        binders["parg_bindings.rs"],
        include_str!("fixtures/rust_generated/src/parg_bindings.rs")
    );
}
#[test]
fn collisions_and_unknown_targets_are_explicit() {
    let mut b = bundle();
    b.models
        .insert("Other@default".into(), model("Node", vec![]));
    assert_eq!(
        RustEmitter
            .generate_models(&b, &TypeBindings::new())
            .unwrap_err()
            .reason,
        ErrorReason::ReservedMember
    );
    let mut b = bundle();
    b.models.get_mut("Leaf@default").unwrap().attributes = vec![string("a-b"), string("a_b")];
    assert_eq!(
        RustEmitter
            .generate_models(&b, &TypeBindings::new())
            .unwrap_err()
            .reason,
        ErrorReason::ReservedMember
    );
    let mut b = bundle();
    b.models.remove("Leaf@default");
    assert_eq!(
        RustEmitter
            .generate_models(&b, &TypeBindings::new())
            .unwrap_err()
            .reason,
        ErrorReason::DanglingReference
    );
}
#[test]
fn invalid_defaults_bounds_and_unsupported_types_refuse() {
    let mut b = bundle();
    b.models.get_mut("Leaf@default").unwrap().attributes[0].default =
        DefaultValue::Present(NeutralValue::Boolean(false));
    assert_eq!(
        RustEmitter
            .generate_models(&b, &TypeBindings::new())
            .unwrap_err()
            .reason,
        ErrorReason::NonSerializableDefault
    );
    let mut b = bundle();
    b.models.get_mut("Leaf@default").unwrap().attributes[0].collection = Collection::Bounded {
        min: 2,
        max: Some(1),
        class_binding: None,
    };
    assert_eq!(
        RustEmitter
            .generate_models(&b, &TypeBindings::new())
            .unwrap_err()
            .reason,
        ErrorReason::InvalidDocument
    );
    let mut b = bundle();
    b.models.get_mut("Leaf@default").unwrap().attributes[0].value_type = TypeRef::Builtin {
        name: BuiltinType::Hash,
    };
    assert_eq!(
        RustEmitter
            .generate_models(&b, &TypeBindings::new())
            .unwrap_err()
            .reason,
        ErrorReason::UnsupportedType
    );
}
#[test]
fn named_types_need_bindings_and_parg_is_statically_checked() {
    let mut b = bundle();
    b.value_types.insert(
        "Custom".into(),
        ValueType {
            base: BuiltinType::String,
            facets: BTreeMap::new(),
        },
    );
    b.models.get_mut("Leaf@default").unwrap().attributes[0].value_type = TypeRef::Value {
        id: "Custom".into(),
    };
    assert_eq!(
        RustEmitter
            .generate_models(&b, &TypeBindings::new())
            .unwrap_err()
            .reason,
        ErrorReason::MissingTypeBinding
    );
    let bindings = BTreeMap::from([(
        "Custom".into(),
        TypeBinding {
            rust_type: "crate::Custom".into(),
            codec: None,
        },
    )]);
    assert!(
        RustEmitter.generate_models(&b, &bindings).unwrap()["models.rs"]
            .contains("Presence<crate::Custom>")
    );
    let mut bindings = parg();
    bindings[0].captures.insert("bad".into(), "missing".into());
    assert_eq!(
        RustEmitter
            .generate_parg_bindings(&bundle(), &bindings)
            .unwrap_err()
            .reason,
        ErrorReason::InvalidDocument
    );
    let duplicate = vec![parg()[0].clone(), parg()[0].clone()];
    assert_eq!(
        RustEmitter
            .generate_parg_bindings(&bundle(), &duplicate)
            .unwrap_err()
            .reason,
        ErrorReason::ReservedMember
    );
}

#[test]
fn runtime_names_and_invalid_enum_literals_reject() {
    for name in ["String", "bool", "Presence"] {
        let mut b = bundle();
        b.models.get_mut("Leaf@default").unwrap().model = name.into();
        assert_eq!(
            RustEmitter
                .generate_models(&b, &TypeBindings::new())
                .unwrap_err()
                .reason,
            ErrorReason::ReservedMember
        );
    }
    let mut b = bundle();
    b.models.get_mut("Leaf@default").unwrap().attributes[0].enumeration =
        Some(vec![NeutralValue::Boolean(false)]);
    assert_eq!(
        RustEmitter
            .generate_models(&b, &TypeBindings::new())
            .unwrap_err()
            .reason,
        ErrorReason::NonSerializableDefault
    );
    let mut b = bundle();
    b.models.get_mut("Leaf@default").unwrap().attributes[0].enumeration = Some(vec![
        NeutralValue::String("same".into()),
        NeutralValue::String("same".into()),
    ]);
    assert_eq!(
        RustEmitter
            .generate_models(&b, &TypeBindings::new())
            .unwrap_err()
            .reason,
        ErrorReason::InvalidDocument
    );
}
#[test]
fn fields_keep_wire_identity_and_initialization_is_explicit() {
    let mut b = bundle();
    let mut empty = string("type");
    empty.collection = Collection::Bounded {
        min: 0,
        max: None,
        class_binding: None,
    };
    empty.initialize_empty = true;
    b.models.get_mut("Leaf@default").unwrap().attributes =
        vec![empty, string("dash-name"), string("_")];
    let source = RustEmitter
        .generate_models(&b, &TypeBindings::new())
        .unwrap();
    assert!(source["models.rs"].contains("rename = \"type\""));
    assert!(source["models.rs"].contains("pub type_: Presence<Vec<String>>"));
    assert!(source["models.rs"].contains("pub dash_name: Presence<String>"));
    assert!(source["models.rs"].contains("pub _Lml: Presence<String>"));
    assert!(source["models.rs"].contains("self.type_=Presence::Value(Vec::new())"));
}

#[test]
fn ordered_content_reserved_members_and_versions_reject() {
    let mut b = bundle();
    b.models
        .get_mut("Leaf@default")
        .unwrap()
        .attributes
        .push(string("__lml_content"));
    assert_eq!(
        RustEmitter
            .generate_models(&b, &TypeBindings::new())
            .unwrap_err()
            .reason,
        ErrorReason::ReservedMember
    );
    let mut b = bundle();
    b.models
        .get_mut("Node@default")
        .unwrap()
        .ordered_content
        .as_mut()
        .unwrap()
        .version = 2;
    assert_eq!(
        RustEmitter
            .generate_models(&b, &TypeBindings::new())
            .unwrap_err()
            .reason,
        ErrorReason::InvalidOrderedContent
    );
}

#[test]
fn public_naming_api_matches_static_models_and_rejects_collisions() {
    let b = bundle();
    let names = generator::model_names(&b).unwrap();
    assert_eq!(names["Node@default"], "Node");
    assert_eq!(names["Leaf@default"], "Leaf");
    assert_eq!(generator::member_identifier("wire-name"), "wire_name");
    assert_eq!(generator::member_identifier("type"), "type_");
    assert_eq!(generator::member_identifier("_"), "_Lml");
    let mut b = bundle();
    b.models.get_mut("Leaf@default").unwrap().model = "Node".into();
    assert_eq!(
        generator::model_names(&b).unwrap_err().reason,
        ErrorReason::ReservedMember
    );
}

#[test]
fn rule_only_discriminators_fail_explicitly_instead_of_losing_static_types() {
    let mut b = bundle();
    let rule = KeyRule {
        key: vec!["text".into()],
        to: "text".into(),
        discriminator: Some(Discriminator {
            attribute: "kind".into(),
            cases: BTreeMap::from([("node".into(), "Node@default".into())]),
        }),
        ..Default::default()
    };
    b.models.get_mut("Leaf@default").unwrap().mappings.json = Some(KeyMapping {
        rules: vec![rule],
        ..Default::default()
    });
    assert_eq!(
        RustEmitter
            .generate_models(&b, &TypeBindings::new())
            .unwrap_err()
            .reason,
        ErrorReason::UnsupportedMapping
    );
}
