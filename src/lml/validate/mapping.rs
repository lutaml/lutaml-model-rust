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
    match text(n, "mode", at)? {
        "inherit" | "unqualified" => {
            if n.get("uri").is_some() || n.get("prefix").is_some() {
                Err(failure(
                    ErrorReason::UnsupportedMapping,
                    at,
                    "uri/prefix only with mode uri",
                ))
            } else {
                Ok(())
            }
        }
        "uri" => {
            text(n, "uri", at)?;
            if let Some(v) = n.get("prefix") {
                string(v, &pointer(at, "prefix"))?;
            }
            Ok(())
        }
        _ => Err(failure(
            ErrorReason::UnsupportedMapping,
            &pointer(at, "mode"),
            "unknown namespace mode",
        )),
    }
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
    let group = matches!(
        n.get("kind").and_then(Node::string),
        Some("sequence" | "choice")
    );
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
    if group {
        for (i, member) in array(field(n, "members", at)?, &pointer(at, "members"))?
            .iter()
            .enumerate()
        {
            rule(member, names, &format!("{at}/members/{i}"), xml)?;
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
            "choice",
        ],
        at,
    )?;
    flags(n, at)?;
    // Root metadata (including mixed) is kept by the typed root instead of
    // throwing away fields the Ruby validator deliberately leaves declarative.
    if let Some(root) = n.get("root").filter(|v| **v != Node::Null) {
        object(root, &pointer(at, "root"))?;
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
    if let Some(groups) = n.get("sequence") {
        if let Some(groups) = groups.array() {
            for (i, group) in groups.iter().enumerate() {
                let loc = format!("{at}/sequence/{i}");
                rule(group, names, &loc, true)?;
                if !matches!(
                    group.get("kind").and_then(Node::string),
                    Some("sequence" | "choice")
                ) {
                    return Err(failure(
                        ErrorReason::UnsupportedMapping,
                        &pointer(&loc, "kind"),
                        "expected sequence or choice",
                    ));
                }
            }
        } else {
            compositor(groups, names, &pointer(at, "sequence"))?;
        }
    }
    if let Some(group) = n.get("choice") {
        compositor(group, names, &pointer(at, "choice"))?;
    }
    for key in ["root_mappings", "child_mappings"] {
        if let Some(value) = n.get(key) {
            object(value, &pointer(at, key))?;
        }
    }
    Ok(())
}
pub(super) fn compositor(n: &Node, names: &BTreeSet<&str>, at: &str) -> ContractResult<()> {
    keys(n, &["kind", "members", "min", "max"], at)?;
    if !["sequence", "choice"].contains(&text(n, "kind", at)?) {
        return Err(failure(
            ErrorReason::UnsupportedMapping,
            &pointer(at, "kind"),
            "unknown compositor",
        ));
    }
    bounds(n, at)?;
    for (i, member) in array(field(n, "members", at)?, &pointer(at, "members"))?
        .iter()
        .enumerate()
    {
        let loc = format!("{at}/members/{i}");
        if let Some(name) = member.string() {
            if !names.contains(name) {
                return Err(failure(
                    ErrorReason::DanglingReference,
                    &loc,
                    "unknown compositor member",
                ));
            }
        } else {
            compositor(member, names, &loc)?;
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
