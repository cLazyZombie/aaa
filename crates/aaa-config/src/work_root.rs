//! 머신별 작업 경로. 인스턴스 지원도 이 기본값 계산 한 곳을 확장한다.
use std::path::{Component, Path, PathBuf};

/// 설정 문법 검사. 다른 OS의 설정 파일도 검증하므로 Windows 절대 경로를 인정한다.
pub fn validate(value: &str, allow_root: bool) -> Result<(), String> {
    if value.is_empty() {
        return Ok(());
    }
    let windows_drive = value.as_bytes().get(1) == Some(&b':') && value.as_bytes()[0].is_ascii_alphabetic();
    let absolute = value.starts_with('/')
        || value.starts_with("~/")
        || value.starts_with("\\\\")
        || windows_drive && value.as_bytes().get(2).is_some_and(|c| matches!(c, b'/' | b'\\'));
    if !absolute || value.contains('\0') || value.split(['/', '\\']).any(|part| part == "..") {
        return Err("절대 경로 또는 ~/…여야 하며 상위 경로(..)를 포함할 수 없다".into());
    }
    if !allow_root && (value == "/" || windows_drive && value[2..].trim_matches(['/', '\\']).is_empty()) {
        return Err("작업 root는 디스크 최상위일 수 없다".into());
    }
    Ok(())
}

/// 빈 root는 기존 기본값 그대로다. 명시한 root에는 다른 접미사를 붙이지 않는다.
pub fn resolve(value: &str, home: &Path) -> Result<PathBuf, String> {
    validate(value, false)?;
    let path = if value.is_empty() {
        home.join("aaa-work")
    } else if let Some(rest) = value.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(value)
    };
    if !path.is_absolute() {
        return Err("이 머신에서 사용할 수 있는 절대 경로가 아니다".into());
    }
    Ok(path.components().collect())
}

pub fn mount(value: &str, home: &Path) -> Result<Option<PathBuf>, String> {
    validate(value, true)?;
    if value.is_empty() {
        return Ok(None);
    }
    let path = value.strip_prefix("~/").map(|p| home.join(p)).unwrap_or_else(|| PathBuf::from(value));
    if !path.is_absolute() || path.components().any(|c| c == Component::ParentDir) {
        return Err("이 머신에서 사용할 수 있는 mount 절대 경로가 아니다".into());
    }
    Ok(Some(path.components().collect()))
}

/// agent.local.toml의 두 key만 바꾼다. 주석·다른 설정·기존 값의 주석을 보존한다.
pub fn edit(text: &str, root: &str, mount: &str) -> Result<String, String> {
    validate(root, false)?;
    validate(mount, true)?;
    use toml_edit::{DocumentMut, Item, Table};
    let base = if text.trim().is_empty() { format!("schema = {}\n", crate::SCHEMA) } else { text.to_owned() };
    let mut doc = base.parse::<DocumentMut>().map_err(|e| format!("agent.local.toml을 읽지 못했다: {e}"))?;
    if !doc.contains_key("agent") {
        doc["agent"] = Item::Table(Table::new());
    }
    let table = doc["agent"].as_table_like_mut().ok_or("agent.local.toml의 agent가 표가 아니다")?;
    for (key, value) in [("work_root", root), ("work_mount", mount)] {
        let mut next = toml_edit::Value::from(value);
        if let Some(previous) = table.get(key).and_then(Item::as_value) {
            *next.decor_mut() = previous.decor().clone();
        }
        table.insert(key, Item::Value(next));
    }
    let out = doc.to_string();
    toml_edit::de::from_str::<crate::AgentConfig>(&out)
        .map_err(|e| format!("agent.local.toml 형식이 올바르지 않다: {e}"))?;
    Ok(out)
}
