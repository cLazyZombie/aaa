//! host profile-file은 기존 host.toml의 schema/[profiles] 꼴만 가져온다.
use aaa_proto::install::InstallResult;

pub fn names(text: &str) -> Result<Vec<String>, Box<InstallResult>> {
    let invalid = || super::error("invalid_profile", 2);
    let doc: toml_edit::DocumentMut =
        text.parse().map_err(|source_error| aaa_common::diagnostic::context(source_error, invalid()))?;
    if doc.get("schema").and_then(|v| v.as_integer()) != Some(1)
        || doc.iter().any(|(key, _)| !matches!(key, "schema" | "profiles"))
    {
        return Err(invalid());
    }
    let profiles = doc.get("profiles").and_then(|v| v.as_table_like()).ok_or_else(invalid)?;
    if profiles.is_empty() {
        return Err(invalid());
    }
    for (_, item) in profiles.iter() {
        let table = item.as_table_like().ok_or_else(invalid)?;
        if table.iter().any(|(k, _)| {
            !matches!(k, "agent" | "harness" | "model" | "effort" | "args" | "env" | "command" | "description")
        }) {
            return Err(invalid());
        }
    }
    let host: aaa_config::HostConfig = toml_edit::de::from_str(text)
        .map_err(|source_error| aaa_common::diagnostic::context(source_error, invalid()))?;
    let mut names = vec![];
    for (name, p) in host.profiles {
        aaa_config::profile_edit::validate_profile(&name, &p)
            .map_err(|source_error| aaa_common::diagnostic::context(source_error, invalid()))?;
        if !matches!(p.harness.as_str(), "claude" | "codex")
            || !p.command.is_empty()
            || p.env.keys().any(|k| !matches!(k.as_str(), "LANG" | "LC_ALL" | "LC_CTYPE" | "TZ" | "TERM"))
            || p.args
                .iter()
                .chain(p.env.values())
                .any(|s| s.chars().any(char::is_control) || s.contains("$(") || s.contains('`'))
        {
            return Err(super::error("unsafe_profile", 2));
        }
        names.push(name);
    }
    Ok(names)
}
