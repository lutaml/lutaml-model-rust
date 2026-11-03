//! Exact lexical values used by generated consumers. No floating-point bridge.
use crate::contracts::{
    ContractError, ContractResult, ErrorReason, NeutralValue, TypeBinding, TypeBindings,
};
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, Zero};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;
use std::str::FromStr;

fn failure(reason: ErrorReason, detail: impl Into<String>) -> Box<ContractError> {
    Box::new(ContractError {
        reason,
        model_id: None,
        attribute: None,
        format: None,
        rule: None,
        location: "/value".into(),
        klass: None,
        detail: Some(detail.into()),
    })
}

/// Read a plain decimal lexical form without converting through f64.
pub fn parse_decimal(input: &str) -> ContractResult<BigRational> {
    let (negative, unsigned) = match input.as_bytes().first() {
        Some(b'-') => (true, &input[1..]),
        Some(b'+') => (false, &input[1..]),
        _ => (false, input),
    };
    let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    if (whole.is_empty() && fraction.is_empty())
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || !fraction.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(failure(
            ErrorReason::InvalidDocument,
            "expected a plain decimal string",
        ));
    }
    let digits = format!("{whole}{fraction}");
    let mut numerator = BigInt::from_str(&digits)
        .map_err(|_| failure(ErrorReason::InvalidDocument, "invalid decimal digits"))?;
    if negative {
        numerator = -numerator;
    }
    let scale = u32::try_from(fraction.len())
        .map_err(|_| failure(ErrorReason::InvalidDocument, "decimal scale is too large"))?;
    Ok(BigRational::new(numerator, BigInt::from(10u8).pow(scale)))
}

/// Render only terminating rationals. Refusing 1/3 is a contract, not rounding.
pub fn render_decimal(value: &BigRational) -> ContractResult<String> {
    let mut denominator = value.denom().clone();
    let mut twos = 0u32;
    let mut fives = 0u32;
    while (&denominator % 2u8).is_zero() {
        denominator /= 2u8;
        twos += 1;
    }
    while (&denominator % 5u8).is_zero() {
        denominator /= 5u8;
        fives += 1;
    }
    if denominator != BigInt::one() {
        return Err(failure(
            ErrorReason::NonterminatingRational,
            "denominator contains a factor other than 2 or 5",
        ));
    }
    let scale = twos.max(fives);
    let scaled =
        value.numer() * BigInt::from(2u8).pow(scale - twos) * BigInt::from(5u8).pow(scale - fives);
    let sign = if scaled.is_negative() { "-" } else { "" };
    let digits = scaled.abs().to_str_radix(10);
    if scale == 0 {
        return Ok(format!("{sign}{digits}"));
    }
    let scale = scale as usize;
    let padded = if digits.len() <= scale {
        format!("{}{}", "0".repeat(scale + 1 - digits.len()), digits)
    } else {
        digits
    };
    let split = padded.len() - scale;
    Ok(format!("{sign}{}.{}", &padded[..split], &padded[split..]))
}

/// Keeps authored precision, including trailing zeros and signed zero.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExactDecimal {
    lexical: String,
    value: BigRational,
}
impl ExactDecimal {
    pub fn parse(input: &str) -> ContractResult<Self> {
        Ok(Self {
            lexical: input.into(),
            value: parse_decimal(input)?,
        })
    }
    pub fn as_str(&self) -> &str {
        &self.lexical
    }
    pub fn rational(&self) -> &BigRational {
        &self.value
    }
    pub fn from_rational(value: BigRational) -> ContractResult<Self> {
        Ok(Self {
            lexical: render_decimal(&value)?,
            value,
        })
    }
}
impl Serialize for ExactDecimal {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.lexical)
    }
}
impl<'de> Deserialize<'de> for ExactDecimal {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let lexical = String::deserialize(deserializer)?;
        Self::parse(&lexical).map_err(serde::de::Error::custom)
    }
}

/// Arbitrary-size integer with a string-only serde boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExactInteger {
    lexical: String,
    value: BigInt,
}
impl ExactInteger {
    pub fn parse(input: &str) -> ContractResult<Self> {
        let unsigned = input
            .strip_prefix('-')
            .or_else(|| input.strip_prefix('+'))
            .unwrap_or(input);
        if unsigned.is_empty() || !unsigned.bytes().all(|b| b.is_ascii_digit()) {
            return Err(failure(
                ErrorReason::InvalidDocument,
                "expected an integer decimal string",
            ));
        }
        let value = BigInt::from_str(input)
            .map_err(|_| failure(ErrorReason::InvalidDocument, "invalid integer digits"))?;
        Ok(Self {
            lexical: input.into(),
            value,
        })
    }
    pub fn as_str(&self) -> &str {
        &self.lexical
    }
    pub fn as_bigint(&self) -> &BigInt {
        &self.value
    }
}
impl Serialize for ExactInteger {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.lexical)
    }
}
impl<'de> Deserialize<'de> for ExactInteger {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let lexical = String::deserialize(deserializer)?;
        Self::parse(&lexical).map_err(serde::de::Error::custom)
    }
}

pub fn require_binding<'a>(
    id: &str,
    bindings: &'a TypeBindings,
) -> ContractResult<&'a TypeBinding> {
    bindings
        .get(id)
        .filter(|binding| !binding.rust_type.trim().is_empty())
        .ok_or_else(|| {
            failure(
                ErrorReason::MissingTypeBinding,
                format!("missing value-type binding: {id}"),
            )
        })
}

/// Supported numeric facets are explicit; other facets cannot silently vanish.
pub fn validate_decimal_facets(
    value: &ExactDecimal,
    facets: &BTreeMap<String, NeutralValue>,
) -> ContractResult<()> {
    for (name, bound) in facets {
        let accepted = match name.as_str() {
            "minInclusive" | "minExclusive" | "maxInclusive" | "maxExclusive" => {
                let NeutralValue::String(text) = bound else {
                    return Err(failure(
                        ErrorReason::InvalidDocument,
                        "decimal facet bounds must be strings",
                    ));
                };
                let threshold = parse_decimal(text)?;
                match name.as_str() {
                    "minInclusive" => value.value >= threshold,
                    "minExclusive" => value.value > threshold,
                    "maxInclusive" => value.value <= threshold,
                    _ => value.value < threshold,
                }
            }
            "totalDigits" | "fractionDigits" => {
                let limit = match bound {
                    NeutralValue::Unsigned(n) => *n,
                    NeutralValue::Signed(n) if *n >= 0 => *n as u64,
                    _ => {
                        return Err(failure(
                            ErrorReason::InvalidDocument,
                            "digit facet must be a nonnegative integer",
                        ))
                    }
                };
                let rendered = render_decimal(&value.value)?;
                let unsigned = rendered.trim_start_matches('-');
                let digits = if name == "fractionDigits" {
                    unsigned.split_once('.').map_or(0, |(_, part)| part.len())
                } else {
                    unsigned
                        .chars()
                        .filter(|c| *c != '.')
                        .collect::<String>()
                        .trim_start_matches('0')
                        .len()
                        .max(1)
                };
                digits as u64 <= limit
            }
            _ => {
                return Err(failure(
                    ErrorReason::UnsupportedType,
                    format!("unsupported decimal facet: {name}"),
                ))
            }
        };
        if !accepted {
            return Err(failure(
                ErrorReason::InvalidDocument,
                format!("decimal violates {name}"),
            ));
        }
    }
    Ok(())
}
