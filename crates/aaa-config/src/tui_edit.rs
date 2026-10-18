//! TUI 설정 창(`prefix s`)이 host.toml `[tui]` 값 하나를 바꿔 쓴다(herdr 0.9.5 `src/config/write.rs` `ConfigEdit`·`write_edit`:
//! 설정 파일의 그 key만 고치고 다시 읽는다, plan §8.178).
//!
//! - 나머지 글(주석·차례·다른 표)은 그대로 둔다. 빈 파일이면 `schema = 1`부터 쓴다. 고친 글이 다시 읽히는지 본다.
//! - 쓰기는 임시 파일 뒤 rename(`profile_edit::write_atomic`)이다. host.local.toml이 같은 key를 덮으면 고친 값이 보이지 않으니
//!   부르는 쪽이 다시 읽어 맞대 본다.

use std::path::Path;

use toml_edit::{DocumentMut, Item, Table, Value};

use crate::HostConfig;

/// host.toml 글의 `[tui] key`를 `value`로 바꾼 글
pub fn set_in(text: &str, key: &str, value: Value) -> Result<String, String> {
    let base = if text.trim().is_empty() { format!("schema = {}\n", crate::SCHEMA) } else { text.to_string() };
    let mut doc = base.parse::<DocumentMut>().map_err(|e| format!("host.toml을 읽지 못했다: {e}"))?;
    if !doc.contains_key("tui") {
        doc["tui"] = Item::Table(Table::new());
    }
    doc["tui"].as_table_mut().ok_or("host.toml의 `tui`가 표가 아니다")?[key] = Item::Value(value);
    let out = doc.to_string();
    toml_edit::de::from_str::<HostConfig>(&out).map_err(|e| format!("고친 host.toml을 읽지 못했다: {e}"))?;
    Ok(out)
}

/// 설정 폴더의 host.toml `[tui] key`를 바꿔 쓴다(없으면 만든다)
pub fn set(config_dir: &Path, key: &str, value: Value) -> Result<(), String> {
    let file = config_dir.join("host.toml");
    let text = match std::fs::read_to_string(&file) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("{}을 읽지 못했다: {e}", file.display())),
    };
    let edited = set_in(&text, key, value)?;
    crate::profile_edit::write_atomic(&file, edited.as_bytes())
        .map_err(|e| format!("{}에 쓰지 못했다: {e}", file.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_rest_and_starts_an_empty_file_with_schema() {
        let text = "schema = 1\n# 웹\n[web]\nlisten = \"\"\n\n[tui]\n# 색\ntheme = \"catppuccin\" # 기본\nmouse_capture = false\n";
        let out = set_in(text, "theme", "terminal".into()).unwrap();
        assert!(out.contains("# 웹\n[web]\nlisten = \"\""), "{out}");
        assert!(out.contains("# 색\ntheme = \"terminal\""), "{out}");
        assert!(out.contains("mouse_capture = false"), "{out}");
        let out = set_in(&out, "status_indicators", "symbols".into()).unwrap();
        let c: HostConfig = toml_edit::de::from_str(&out).unwrap();
        assert_eq!((c.tui.theme.as_str(), c.tui.status_indicators), ("terminal", crate::StatusIndicators::Symbols));
        // 빈 파일: schema부터(없으면 다음 읽기가 schema 오류)
        let out = set_in("", "theme", "nord".into()).unwrap();
        assert!(out.starts_with("schema = 1\n"), "{out}");
        let d = std::env::temp_dir().join(format!("aaa-tui-edit-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        set(&d, "theme", "nord".into()).unwrap();
        assert_eq!(crate::load_host(&d).unwrap().config.tui.theme, "nord");
        set(&d, "status_indicators", "symbols".into()).unwrap();
        let t = crate::load_host(&d).unwrap().config.tui;
        assert_eq!((t.theme.as_str(), t.status_indicators), ("nord", crate::StatusIndicators::Symbols));
        // 받지 않는 값은 쓰지 않는다
        assert!(set_in("", "status_indicators", "blink".into()).is_err());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
