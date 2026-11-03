use super::*;

pub(super) fn namespace(n: &Node, at: &str) -> ContractResult<()> {
    match text(n, "mode", at)? {
        "inherit" | "unqualified" => keys(n, &["mode"], at),
        "uri" => {
            keys(n, &["mode", "uri", "prefix"], at)?;
            text(n, "uri", at)?;
            if let Some(p) = n.get("prefix") {
                string(p, &pointer(at, "prefix"))?;
            }
            Ok(())
        }
        _ => Err(failure(
            ErrorReason::UnsupportedMapping,
            at,
            "unknown namespace mode",
        )),
    }
}
pub(super) fn render(n: &Node, at: &str) -> ContractResult<()> {
    keys(n, &["nil", "empty", "default"], at)?;
    for key in ["nil", "empty"] {
        if matches!(field(n, key, at)?, Node::Bool(_)) {
            continue;
        }
        let value = text(n, key, at)?;
        if !["omit", "nil", "blank", "as_nil", "as_blank"].contains(&value) {
            return Err(failure(
                ErrorReason::UnsupportedMapping,
                &pointer(at, key),
                "unsupported render policy",
            ));
        }
    }
    boolean(field(n, "default", at)?, &pointer(at, "default"))
}
pub(super) fn mapped_member<'a>(
    n: &'a Node,
    names: &BTreeSet<&str>,
    at: &str,
) -> ContractResult<&'a str> {
    let member = text(n, "to", at)?;
    if !names.contains(member) {
        return Err(failure(
            ErrorReason::DanglingReference,
            &pointer(at, "to"),
            "mapping target attribute missing",
        ));
    }
    Ok(member)
}
pub(super) fn xml(n: &Node, names: &BTreeSet<&str>, at: &str) -> ContractResult<()> {
    keys(n, &["root", "namespace", "rules", "sequence", "choice"], at)?;
    let root = field(n, "root", at)?;
    if root != &Node::Null {
        keys(root, &["element"], &pointer(at, "root"))?;
        text(root, "element", &pointer(at, "root"))?;
    }
    namespace(field(n, "namespace", at)?, &pointer(at, "namespace"))?;
    for (i, rule) in array(field(n, "rules", at)?, &pointer(at, "rules"))?
        .iter()
        .enumerate()
    {
        let loc = format!("{at}/rules/{i}");
        let result = (|| {
            keys(
                rule,
                &[
                    "kind",
                    "name",
                    "to",
                    "namespace",
                    "prefix",
                    "cdata",
                    "render",
                    "discriminator",
                ],
                &loc,
            )?;
            let kind = text(rule, "kind", &loc)?;
            match kind {
                "attribute" | "element" => {
                    let aliases = field(rule, "name", &loc)?;
                    strings(aliases, &pointer(&loc, "name"), false)?;
                    if aliases.array().unwrap().is_empty() {
                        return Err(invalid(&pointer(&loc, "name"), "empty aliases"));
                    }
                }
                "content" => {
                    if rule.get("name").is_some() {
                        return Err(failure(
                            ErrorReason::UnsupportedMapping,
                            &pointer(&loc, "name"),
                            "content rule cannot have name",
                        ));
                    }
                }
                _ => {
                    return Err(failure(
                        ErrorReason::UnsupportedMapping,
                        &pointer(&loc, "kind"),
                        "unsupported XML rule/compositor",
                    ))
                }
            }
            mapped_member(rule, names, &loc)?;
            namespace(field(rule, "namespace", &loc)?, &pointer(&loc, "namespace"))?;
            if let Some(p) = rule.get("prefix") {
                string(p, &pointer(&loc, "prefix"))?;
            }
            if let Some(c) = rule.get("cdata") {
                boolean(c, &pointer(&loc, "cdata"))?;
            }
            if let Some(r) = rule.get("render") {
                render(r, &pointer(&loc, "render"))?;
            }
            if let Some(d) = rule.get("discriminator") {
                discriminator(d, &pointer(&loc, "discriminator"))?;
            }
            Ok(())
        })();
        result.map_err(|mut e: Box<crate::contracts::ContractError>| {
            e.rule = Some(i.to_string());
            e.attribute = rule.get("to").and_then(Node::string).map(str::to_owned);
            e
        })?;
    }
    for key in ["sequence", "choice"] {
        if let Some(group) = n.get(key) {
            compositor(group, names, &pointer(at, key))?;
            if group.get("kind").and_then(Node::string) != Some(key) {
                return Err(failure(
                    ErrorReason::UnsupportedMapping,
                    &pointer(at, key),
                    "compositor kind mismatch",
                ));
            }
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
    keys(n, &["rules", "root_mappings", "child_mappings"], at)?;
    for key in ["root_mappings", "child_mappings"] {
        if let Some(v) = n.get(key) {
            object(v, &pointer(at, key))?;
        }
    }
    for (i, rule) in array(field(n, "rules", at)?, &pointer(at, "rules"))?
        .iter()
        .enumerate()
    {
        let loc = format!("{at}/rules/{i}");
        let result = (|| {
            keys(rule, &["key", "to", "render"], &loc)?;
            mapped_member(rule, names, &loc)?;
            let aliases = field(rule, "key", &loc)?;
            strings(aliases, &pointer(&loc, "key"), false)?;
            if aliases.array().unwrap().is_empty() {
                return Err(invalid(&pointer(&loc, "key"), "empty key aliases"));
            }
            if let Some(r) = rule.get("render") {
                render(r, &pointer(&loc, "render"))?;
            }
            Ok(())
        })();
        result.map_err(|mut e: Box<crate::contracts::ContractError>| {
            e.rule = Some(i.to_string());
            e.attribute = rule.get("to").and_then(Node::string).map(str::to_owned);
            e
        })?;
    }
    Ok(())
}
