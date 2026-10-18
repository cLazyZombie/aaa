//! Codex 전역 연동. herdr install_codex·herdr-projects setup의 추가/제거 규칙을 따른다.
//! 설정을 자동으로 고치지 않는다. 명시적 install·doctor --fix만 쓴다.
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use toml_edit::DocumentMut;

const EVENTS: [&str; 3] = ["SessionStart", "UserPromptSubmit", "PostToolUse"];
const OWNED: &str = "aaa-codex-integration.json";
static SEQ: AtomicU64 = AtomicU64::new(0);

pub fn codex_home() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| aaa_config::paths::home().join(".codex"))
}

/// Unix는 sh 인용, Windows는 herdr의 PowerShell EncodedCommand 방식이다.
fn command(exe: &Path) -> String {
    #[cfg(not(windows))]
    {
        format!("'{}' hook --agent codex", exe.to_string_lossy().replace('\'', "'\\''"))
    }
    #[cfg(windows)]
    {
        windows_command(exe)
    }
}

#[cfg(any(windows, test))]
fn windows_command(exe: &Path) -> String {
    use base64::Engine;
    // EncodedCommand는 stderr를 CLIXML로 내보낸다. 모듈 첫 검색의 진행 출력도 hook 출력이 되므로 억제한다.
    // 기본 code page와 native stdin 전달 차이를 피한다. 비-aaa 세션은 stdin보다 먼저 끝낸다.
    let script = format!(
        "$ProgressPreference = 'SilentlyContinue'; \
         if (-not $env:AAA_SESSION_ID -or -not $env:AAA_HOOK_SOCK -or -not $env:AAA_HOOK_TOKEN) {{ exit 0 }}; \
         [Console]::InputEncoding = [System.Text.UTF8Encoding]::new($false); \
         [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false); \
         $OutputEncoding = [Console]::OutputEncoding; \
         [Console]::In.ReadToEnd() | & '{}' hook --agent codex; exit 0",
        exe.to_string_lossy().replace('\'', "''")
    );
    let bytes: Vec<_> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    format!(
        "powershell -NoProfile -NonInteractive -OutputFormat Text -EncodedCommand {}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

fn read(path: &Path) -> Result<Option<String>> {
    match fs::symlink_metadata(path) {
        Ok(m) => {
            ensure!(m.is_file() && !m.file_type().is_symlink(), "{}: 일반 파일만 편집한다", path.display());
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                ensure!(m.nlink() == 1, "{}: hard link는 편집하지 않는다", path.display());
            }
            Ok(Some(fs::read_to_string(path)?))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn parse_hooks(text: Option<&str>) -> Result<Value> {
    let v: Value = serde_json::from_str(text.unwrap_or("{}")).context("hooks.json을 읽지 못했다")?;
    ensure!(v.is_object(), "hooks.json은 object여야 한다");
    if let Some(hooks) = v.get("hooks") {
        ensure!(hooks.is_object(), "hooks는 object여야 한다");
        for event in EVENTS {
            if let Some(entries) = hooks.get(event) {
                ensure!(entries.is_array(), "hooks.{event}는 array여야 한다");
                for group in entries.as_array().unwrap() {
                    ensure!(group.is_object(), "hooks.{event} 항목은 object여야 한다");
                    if let Some(handlers) = group.get("hooks") {
                        ensure!(handlers.is_array(), "hooks.{event}의 hooks는 array여야 한다");
                    }
                }
            }
        }
    }
    Ok(v)
}

fn edit_hooks(mut v: Value, commands: &[String], current: &str, remove: bool) -> Value {
    if v.get("hooks").is_none() {
        if remove {
            return v;
        }
        v["hooks"] = json!({});
    }
    let hooks = v["hooks"].as_object_mut().unwrap();
    for event in EVENTS {
        if !hooks.contains_key(event) {
            if remove {
                continue;
            }
            hooks.insert(event.into(), json!([]));
        }
        let entries = hooks.get_mut(event).unwrap().as_array_mut().unwrap();
        let desired = if event == "SessionStart" {
            json!({"matcher":"^(startup|resume|clear|compact|fork)$","hooks":[{"type":"command","command":current,"timeout":10}]})
        } else {
            json!({"hooks":[{"type":"command","command":current,"timeout":10}]})
        };
        // 같은 명령도 소유 대상으로 삼는다. 옮긴 실행 파일은 기록한 완전 일치만 제거한다.
        let ours = |h: &Value| {
            h["type"] == "command"
                && h["command"].as_str().is_some_and(|s| s == current || commands.iter().any(|c| c == s))
        };
        let mut found = false;
        entries.retain_mut(|group| {
            if !remove && *group == desired && !found {
                found = true;
                return true;
            }
            let Some(handlers) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
                return true;
            };
            let before = handlers.len();
            handlers.retain(|h| !ours(h));
            before == handlers.len() || !handlers.is_empty()
        });
        if !remove && !found {
            entries.push(desired);
        }
    }
    v
}

fn config(text: Option<&str>) -> Result<(bool, String)> {
    let original = text.unwrap_or("");
    let mut doc: DocumentMut = original.parse().context("config.toml을 읽지 못했다")?;
    if let Some(features) = doc.get("features") {
        ensure!(features.is_table_like(), "features는 table이어야 한다");
        if let Some(hooks) = features.get("hooks") {
            ensure!(hooks.as_bool().is_some(), "features.hooks는 bool이어야 한다");
        }
    }
    let enabled = doc.get("features").and_then(|f| f.get("hooks")).and_then(|h| h.as_bool()) == Some(true);
    if enabled {
        return Ok((true, original.into()));
    }
    doc["features"]["hooks"] = toml_edit::value(true);
    Ok((false, doc.to_string()))
}

struct Files {
    home: PathBuf,
    hook_text: Option<String>,
    config_text: Option<String>,
    owned_text: Option<String>,
    hooks: Value,
    commands: Vec<String>,
    current: String,
}
impl Files {
    fn load(home: PathBuf, exe: &Path, read_config: bool) -> Result<Self> {
        let hook_text = read(&home.join("hooks.json"))?;
        let config_text = if read_config { read(&home.join("config.toml"))? } else { None };
        let owned_text = read(&home.join(OWNED))?;
        let hooks = parse_hooks(hook_text.as_deref())?;
        let commands =
            serde_json::from_str(owned_text.as_deref().unwrap_or("[]")).context("aaa 연동 소유 기록을 읽지 못했다")?;
        Ok(Self { home, hook_text, config_text, owned_text, hooks, commands, current: command(exe) })
    }
}

pub fn healthy(home: PathBuf, exe: &Path) -> Result<bool> {
    let f = Files::load(home, exe, true)?;
    let desired = edit_hooks(f.hooks.clone(), &f.commands, &f.current, false);
    Ok(desired == f.hooks && config(f.config_text.as_deref())?.0)
}

/// 파일마다 쓰기 전 백업·snapshot 비교·같은 폴더 rename. 여러 파일 전체의 transaction은 아니다.
pub fn configure(home: PathBuf, exe: &Path, remove: bool) -> Result<Vec<PathBuf>> {
    let mut f = Files::load(home, exe, !remove)?;
    let (enabled, new_config) = if remove { (true, String::new()) } else { config(f.config_text.as_deref())? };
    let hooks = edit_hooks(f.hooks.clone(), &f.commands, &f.current, remove);
    let hook_after = if hooks == f.hooks {
        f.hook_text.clone()
    } else {
        Some(format!("{}\n", serde_json::to_string_pretty(&hooks)?))
    };
    let mut changes = Vec::new();
    if !remove {
        if !f.commands.contains(&f.current) {
            f.commands.push(f.current);
        }
        let owned = format!("{}\n", serde_json::to_string_pretty(&f.commands)?);
        // 먼저 소유 기록을 보존한다. 뒤 파일 쓰기가 실패해도 이전·새 명령을 모두 제거할 수 있다.
        changes.push((f.home.join(OWNED), f.owned_text, Some(owned)));
    }
    changes.push((f.home.join("hooks.json"), f.hook_text, hook_after));
    // herdr uninstall_codex처럼 다른 도구도 쓰는 hooks 기능은 끄지 않는다.
    if !remove && !enabled {
        changes.push((f.home.join("config.toml"), f.config_text, Some(new_config)));
    }
    let mut backups = Vec::new();
    // 어느 설정도 쓰기 전에 모든 변경 대상의 snapshot·쓰기 권한을 확인한다.
    for (path, before, after) in &changes {
        if before != after && before.is_some() {
            ensure!(&read(path)? == before, "{}: 설치 중 파일이 바뀌었다", path.display());
            ensure!(!fs::metadata(path)?.permissions().readonly(), "{}: 읽기 전용 파일이다", path.display());
            OpenOptions::new().write(true).open(path)?;
        }
    }
    for (path, before, after) in changes {
        if before != after
            && let Some(after) = after
            && let Some(backup) = replace(&path, &before, &after)?
        {
            backups.push(backup);
        }
    }
    Ok(backups)
}

fn private_new(path: &Path) -> std::io::Result<fs::File> {
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(path)
}

fn replace(path: &Path, before: &Option<String>, after: &str) -> Result<Option<PathBuf>> {
    ensure!(&read(path)? == before, "{}: 설치 중 파일이 바뀌었다", path.display());
    fs::create_dir_all(path.parent().context("설정 폴더가 없다")?)?;
    let tag = format!("{}-{}-{}", aaa_common::util::now_ms(), std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed));
    let mut backup = None;
    if let Some(text) = before {
        let metadata = fs::metadata(path)?;
        ensure!(!metadata.permissions().readonly(), "{}: 읽기 전용 파일이다", path.display());
        // 쓰기 불가 설정을 parent rename 권한으로 우회하지 않는다.
        OpenOptions::new().write(true).open(path)?;
        let b = path.with_file_name(format!("{}.before-aaa-{tag}", path.file_name().unwrap().to_string_lossy()));
        let mut file = private_new(&b)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        backup = Some(b);
    }
    let tmp = path.with_file_name(format!(".aaa-integration-{tag}.tmp"));
    let mut file = private_new(&tmp)?;
    let result = (|| -> Result<()> {
        file.write_all(after.as_bytes())?;
        file.sync_all()?;
        drop(file);
        if before.is_some() {
            fs::set_permissions(&tmp, fs::metadata(path)?.permissions())?;
        }
        ensure!(&read(path)? == before, "{}: 설치 중 파일이 바뀌었다", path.display());
        fs::rename(&tmp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result?;
    Ok(backup)
}

#[cfg(test)]
mod tests {
    #[test]
    fn windows_command_preserves_special_path_and_utf8_stdin_stdout() {
        use base64::Engine;
        let cmd = super::windows_command(std::path::Path::new(r"C:\A b\한글 ' $ &\aaa.exe"));
        let encoded =
            cmd.strip_prefix("powershell -NoProfile -NonInteractive -OutputFormat Text -EncodedCommand ").unwrap();
        let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).unwrap();
        let utf16: Vec<_> = bytes.as_chunks::<2>().0.iter().map(|b| u16::from_le_bytes(*b)).collect();
        let script = String::from_utf16(&utf16).unwrap();
        assert_eq!(
            script,
            concat!(
                "$ProgressPreference = 'SilentlyContinue'; ",
                "if (-not $env:AAA_SESSION_ID -or -not $env:AAA_HOOK_SOCK -or -not $env:AAA_HOOK_TOKEN) { exit 0 }; ",
                "[Console]::InputEncoding = [System.Text.UTF8Encoding]::new($false); ",
                "[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false); ",
                "$OutputEncoding = [Console]::OutputEncoding; ",
                r"[Console]::In.ReadToEnd() | & 'C:\A b\한글 '' $ &\aaa.exe' hook --agent codex; exit 0"
            )
        );
    }
}
