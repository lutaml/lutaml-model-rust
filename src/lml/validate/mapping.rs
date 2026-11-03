use super::*;
fn mapping_keys(n: &Node, allowed: &[&str], at: &str) -> ContractResult<()> {
    keys(n, allowed, at).map_err(|mut e| {
        if e.reason == ErrorReason::InvalidDocument {
            e.reason = ErrorReason::UnsupportedMapping;
        }
        e
    })
}
pub(super) fn namespace(n: &Node, at: &str) -> ContractResult<()> {
    mapping_keys(n, &["mode", "uri", "prefix"], at)?;
    let mode = text(n, "mode", at)?;
    if !matches!(mode, "inherit" | "unqualified" | "uri") {
        return Err(failure(
            ErrorReason::UnsupportedMapping,
            &pointer(at, "mode"),
            "unknown namespace mode",
        ));
    }
    if let Some(v) = n.get("prefix") {
        prefix(v, &pointer(at, "prefix"))?;
    }
    if mode == "uri" {
        text(n, "uri", at)?;
        Ok(())
    } else if n.get("uri").is_some() || n.get("prefix").is_some() {
        Err(failure(
            ErrorReason::UnsupportedMapping,
            at,
            "uri/prefix only with mode uri",
        ))
    } else {
        Ok(())
    }
}
/// XML prefix lexical rule: `\A[A-Za-z_][A-Za-z0-9_.-]*\z` (Ruby XML_PREFIX).
fn prefix(n: &Node, at: &str) -> ContractResult<()> {
    let ok = n.string().is_some_and(|s| {
        let mut chars = s.chars();
        chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c))
    });
    if ok {
        Ok(())
    } else {
        Err(invalid(
            at,
            "prefix must match \\A[A-Za-z_][A-Za-z0-9_.-]*\\z",
        ))
    }
}
/// Root metadata scalars: object, non-empty `element`, boolean `mixed`.
fn root(n: &Node, at: &str) -> ContractResult<()> {
    object(n, at)?;
    if let Some(element) = n.get("element") {
        string(element, &pointer(at, "element"))?;
    }
    if let Some(mixed) = n.get("mixed") {
        boolean(mixed, &pointer(at, "mixed")).map_err(|mut e| {
            e.reason = ErrorReason::UnsupportedMapping;
            e
        })?;
    }
    Ok(())
}
pub(super) fn render(n: &Node, at: &str) -> ContractResult<()> {
    mapping_keys(n, &["nil", "empty", "default"], at)?;
    for (key, value) in object(n, at)? {
        if !matches!(value, Node::Bool(_) | Node::String(_)) {
            return Err(failure(
                ErrorReason::UnsupportedMapping,
                &pointer(at, key),
                "render policy must be a string or boolean",
            ));
        }
    }
    Ok(())
}
fn flags(n: &Node, at: &str) -> ContractResult<()> {
    for key in ["ordered", "mixed_content"] {
        if let Some(value) = n.get(key) {
            boolean(value, &pointer(at, key)).map_err(|mut e| {
                e.reason = ErrorReason::UnsupportedMapping;
                e
            })?;
        }
    }
    if let Some(ns) = n.get("namespace") {
        namespace(ns, &pointer(at, "namespace"))?;
    }
    Ok(())
}
fn compositor(n: &Node, names: &BTreeSet<&str>, at: &str, xml: bool) -> ContractResult<()> {
    rule(n, names, at, xml)?;
    if !matches!(
        n.get("kind").and_then(Node::string),
        Some("sequence" | "choice")
    ) {
        return Err(failure(
            ErrorReason::UnsupportedMapping,
            &pointer(at, "kind"),
            "expected sequence or choice",
        ));
    }
    for (i, member) in array(field(n, "members", at)?, &pointer(at, "members"))?
        .iter()
        .enumerate()
    {
        let loc = format!("{at}/members/{i}");
        object(member, &loc)?;
        if matches!(
            member.get("kind").and_then(Node::string),
            Some("sequence" | "choice")
        ) {
            compositor(member, names, &loc, xml)?;
        } else {
            rule(member, names, &loc, xml)?;
        }
    }
    Ok(())
}
fn rule(n: &Node, names: &BTreeSet<&str>, at: &str, xml: bool) -> ContractResult<()> {
    mapping_keys(
        n,
        &[
            "kind",
            "name",
            "key",
            "to",
            "namespace",
            "prefix",
            "cdata",
            "render",
            "discriminator",
            "members",
            "min",
            "max",
            "child_mappings",
            "root_mappings",
            "polymorphic_map",
        ],
        at,
    )?;
    if xml {
        if let Some(kind) = n.get("kind") {
            if !["attribute", "element", "content", "sequence", "choice"]
                .contains(&string(kind, &pointer(at, "kind"))?)
            {
                return Err(failure(
                    ErrorReason::UnsupportedMapping,
                    &pointer(at, "kind"),
                    "unknown XML rule kind",
                ));
            }
        }
    }
    if let Some(to) = n.get("to") {
        let name = string(to, &pointer(at, "to"))?;
        if !names.contains(name) {
            return Err(failure(
                ErrorReason::DanglingReference,
                &pointer(at, "to"),
                "mapping target attribute missing",
            ));
        }
    }
    for key in ["name", "key"] {
        if let Some(value) = n.get(key) {
            strings(value, &pointer(at, key), false)?;
        }
    }
    if let Some(ns) = n.get("namespace") {
        namespace(ns, &pointer(at, "namespace"))?;
    }
    if let Some(value) = n.get("cdata") {
        boolean(value, &pointer(at, "cdata"))?;
    }
    if let Some(policy) = n.get("render") {
        render(policy, &pointer(at, "render"))?;
    }
    if let Some(d) = n.get("discriminator") {
        discriminator(d, &pointer(at, "discriminator"))?;
    }
    if let Some(map) = n.get("polymorphic_map") {
        for (key, value) in object(map, &pointer(at, "polymorphic_map"))? {
            string(value, &pointer(&pointer(at, "polymorphic_map"), key))?;
        }
    }
    for key in ["child_mappings", "root_mappings"] {
        if let Some(value) = n.get(key) {
            object(value, &pointer(at, key))?;
        }
    }
    Ok(())
}
pub(super) fn xml(n: &Node, names: &BTreeSet<&str>, at: &str) -> ContractResult<()> {
    mapping_keys(
        n,
        &[
            "root",
            "namespace",
            "rules",
            "root_mappings",
            "child_mappings",
            "ordered",
            "mixed_content",
            "sequence",
        ],
        at,
    )?;
    flags(n, at)?;
    // Root metadata (including mixed) is kept by the typed root instead of
    // throwing away fields the Ruby validator deliberately leaves declarative.
    if let Some(value) = n.get("root") {
        root(value, &pointer(at, "root"))?;
    }
    if let Some(rules) = n.get("rules") {
        for (i, r) in array(rules, &pointer(at, "rules"))?.iter().enumerate() {
            let loc = format!("{at}/rules/{i}");
            rule(r, names, &loc, true).map_err(|mut e| {
                e.rule = Some(i.to_string());
                e.attribute = r.get("to").and_then(Node::string).map(str::to_owned);
                e
            })?;
        }
    }
    // Ruby's only compositor shape: an array of sequence/choice groups whose
    // members are rule objects or nested groups (no `choice` key, no bare
    // compositor object, no attribute-name members).
    if let Some(groups) = n.get("sequence") {
        for (i, group) in array(groups, &pointer(at, "sequence"))?.iter().enumerate() {
            compositor(group, names, &format!("{at}/sequence/{i}"), true)?;
        }
    }
    for key in ["root_mappings", "child_mappings"] {
        if let Some(value) = n.get(key) {
            object(value, &pointer(at, key))?;
        }
    }
    Ok(())
}
pub(super) fn key_mapping(n: &Node, names: &BTreeSet<&str>, at: &str) -> ContractResult<()> {
    mapping_keys(
        n,
        &[
            "root",
            "namespace",
            "rules",
            "root_mappings",
            "child_mappings",
            "ordered",
            "mixed_content",
            "sequence",
        ],
        at,
    )?;
    flags(n, at)?;
    if let Some(value) = n.get("root") {
        root(value, &pointer(at, "root"))?;
    }
    if n.get("sequence").is_some() {
        return Err(failure(
            ErrorReason::UnsupportedMapping,
            &pointer(at, "sequence"),
            "sequence is xml only",
        ));
    }
    for key in ["root_mappings", "child_mappings"] {
        if let Some(value) = n.get(key) {
            object(value, &pointer(at, key))?;
        }
    }
    if let Some(rules) = n.get("rules") {
        for (i, r) in array(rules, &pointer(at, "rules"))?.iter().enumerate() {
            let loc = format!("{at}/rules/{i}");
            rule(r, names, &loc, false).map_err(|mut e| {
                e.rule = Some(i.to_string());
                e.attribute = r.get("to").and_then(Node::string).map(str::to_owned);
                e
            })?;
        }
    }
    Ok(())
}
