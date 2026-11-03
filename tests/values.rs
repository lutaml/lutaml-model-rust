#[path = "../src/values/mod.rs"]
mod values;
use lutaml_model::contracts;
use lutaml_model::contracts::{ErrorReason, NeutralValue, TypeBinding, TypeBindings};
use num_bigint::BigInt;
use num_rational::BigRational;
use std::collections::BTreeMap;
use values::{
    parse_decimal, render_decimal, require_binding, validate_decimal_facets, ExactDecimal,
    ExactInteger,
};

#[test]
fn authored_decimal_strings_survive_serde_exactly() {
    for text in [
        "273.15", "459.67", "0.01", "1.2300", "-0.00", "+01.20", ".5", "1.",
    ] {
        let value = ExactDecimal::parse(text).unwrap();
        assert_eq!(value.as_str(), text);
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(json, serde_json::to_string(text).unwrap());
        assert_eq!(serde_json::from_str::<ExactDecimal>(&json).unwrap(), value);
    }
}
#[test]
fn decimal_arithmetic_never_uses_a_binary_float() {
    let result = parse_decimal("0.1").unwrap() + parse_decimal("0.2").unwrap();
    assert_eq!(render_decimal(&result).unwrap(), "0.3");
    assert_eq!(
        render_decimal(&(parse_decimal("20").unwrap() + parse_decimal("273.15").unwrap())).unwrap(),
        "293.15"
    );
    assert_eq!(ExactDecimal::from_rational(result).unwrap().as_str(), "0.3");
}
#[test]
fn rendering_handles_sign_and_scale() {
    for (n, d, text) in [
        (1, 8, "0.125"),
        (-1, 40, "-0.025"),
        (123, 10, "12.3"),
        (0, 7, "0"),
        (5, 1, "5"),
    ] {
        assert_eq!(
            render_decimal(&BigRational::new(n.into(), d.into())).unwrap(),
            text
        );
    }
}
#[test]
fn nonterminating_output_is_refused_by_name() {
    for d in [3, 7, 11] {
        let error = render_decimal(&BigRational::new(1.into(), d.into())).unwrap_err();
        assert_eq!(error.reason, ErrorReason::NonterminatingRational);
    }
}
#[test]
fn integer_strings_exceed_machine_and_javascript_ranges() {
    let text = "900719925474099312345678901234567890";
    let integer = ExactInteger::parse(text).unwrap();
    assert_eq!(integer.as_str(), text);
    assert_eq!(
        integer.as_bigint(),
        &BigInt::parse_bytes(text.as_bytes(), 10).unwrap()
    );
    let json = serde_json::to_string(&integer).unwrap();
    assert_eq!(
        serde_json::from_str::<ExactInteger>(&json).unwrap(),
        integer
    );
}
#[test]
fn numeric_json_cannot_cross_exact_string_boundaries() {
    for input in ["0.1", "1", "1e3", "null"] {
        assert!(serde_json::from_str::<ExactDecimal>(input).is_err());
        assert!(serde_json::from_str::<ExactInteger>(input).is_err());
    }
}
#[test]
fn invalid_decimal_and_integer_lexemes_fail() {
    for text in [
        "", "+", "-", ".", "1.2.3", " 1", "1 ", "1e2", "NaN", "Inf", "١",
    ] {
        assert!(ExactDecimal::parse(text).is_err(), "{text}");
        assert!(ExactInteger::parse(text).is_err(), "{text}");
    }
}
#[test]
fn custom_bindings_are_explicit() {
    let mut bindings = TypeBindings::new();
    assert_eq!(
        require_binding("xml_id", &bindings).unwrap_err().reason,
        ErrorReason::MissingTypeBinding
    );
    bindings.insert(
        "xml_id".into(),
        TypeBinding {
            rust_type: "XmlId".into(),
            codec: Some("XmlIdCodec".into()),
        },
    );
    assert_eq!(
        require_binding("xml_id", &bindings).unwrap().rust_type,
        "XmlId"
    );
}
#[test]
fn numeric_facets_use_exact_comparison_and_value_digits() {
    let value = ExactDecimal::parse("1.2300").unwrap();
    let facets = BTreeMap::from([
        ("minInclusive".into(), NeutralValue::String("1.23".into())),
        ("fractionDigits".into(), NeutralValue::Unsigned(2)),
        ("totalDigits".into(), NeutralValue::Unsigned(3)),
    ]);
    validate_decimal_facets(&value, &facets).unwrap();
    assert_eq!(value.rational(), &parse_decimal("1.23").unwrap());
    let bad = BTreeMap::from([("maxExclusive".into(), NeutralValue::String("1.23".into()))]);
    assert_eq!(
        validate_decimal_facets(&value, &bad).unwrap_err().reason,
        ErrorReason::InvalidDocument
    );
    let unknown = BTreeMap::from([("pattern".into(), NeutralValue::String(".*".into()))]);
    assert_eq!(
        validate_decimal_facets(&value, &unknown)
            .unwrap_err()
            .reason,
        ErrorReason::UnsupportedType
    );
}

#[test]
fn shared_ruby_exact_value_vectors_match_without_rounding() {
    let vectors: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/exact-vectors.json")).unwrap();
    for vector in vectors["decimals"].as_array().unwrap() {
        let input = vector["input"].as_str().unwrap();
        let expected = vector["canonical"].as_str().unwrap();
        assert_eq!(
            render_decimal(&parse_decimal(input).unwrap()).unwrap(),
            expected,
            "{input}"
        );
        // Authored precision is retained independently of canonical numeric form.
        assert_eq!(ExactDecimal::parse(input).unwrap().as_str(), input);
    }
    for vector in vectors["rationals"].as_array().unwrap() {
        let numerator = vector["numerator"]
            .as_str()
            .unwrap()
            .parse::<BigInt>()
            .unwrap();
        let denominator = vector["denominator"]
            .as_str()
            .unwrap()
            .parse::<BigInt>()
            .unwrap();
        let value = BigRational::new(numerator, denominator);
        if let Some(expected) = vector["canonical"].as_str() {
            assert_eq!(render_decimal(&value).unwrap(), expected, "{value}");
        } else {
            assert_eq!(vector["refusal"], "nonterminating_rational");
            assert_eq!(
                render_decimal(&value).unwrap_err().reason,
                ErrorReason::NonterminatingRational
            );
        }
    }
}

#[test]
fn portable_float_matches_ruby_vectors_and_spelling_verdicts() {
    let vectors: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/float_wire_vectors.json")).unwrap();
    let encode = vectors["encode"].as_array().unwrap();
    assert_eq!(encode.len(), 37);
    for vector in encode {
        let bits = u64::from_str_radix(vector["bits"].as_str().unwrap(), 16).unwrap();
        let wire = vector["wire"].as_str().unwrap();
        assert_eq!(values::float_wire(f64::from_bits(bits)), wire);
        let parsed = values::PortableFloat::parse(wire).unwrap();
        assert_eq!(parsed.get().to_bits(), bits, "{wire}");
        assert_eq!(
            serde_json::to_string(&parsed).unwrap(),
            serde_json::to_string(wire).unwrap()
        );
    }
    let verdicts = vectors["canonical"].as_array().unwrap();
    assert_eq!(verdicts.len(), 21);
    for verdict in verdicts {
        let text = verdict["text"].as_str().unwrap();
        let expected = verdict["canonical"].as_bool().unwrap();
        assert_eq!(values::canonical_float(text), expected, "{text:?}");
        assert_eq!(
            serde_json::from_str::<values::PortableFloat>(&serde_json::to_string(text).unwrap())
                .is_ok(),
            expected,
            "{text:?}"
        );
    }
}

#[test]
fn portable_float_refuses_json_numbers_and_non_finite_values() {
    assert!(serde_json::from_str::<values::PortableFloat>("0.71").is_err());
    assert!(serde_json::from_str::<values::PortableFloat>("1").is_err());
    assert!(values::PortableFloat::new(f64::NAN).is_err());
    assert!(values::PortableFloat::new(f64::INFINITY).is_err());
    assert!(values::PortableFloat::new(f64::NEG_INFINITY).is_err());
    // Exponent thresholds: fixed for -4 < decimal point <= 15.
    assert_eq!(values::float_wire(1e-4), "0.0001");
    assert_eq!(values::float_wire(1e-5), "1.0e-5");
    assert_eq!(values::float_wire(9.99e14), "999000000000000.0");
    assert_eq!(values::float_wire(1e15), "1.0e15");
}
