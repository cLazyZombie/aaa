//! Codex hook 명령·설정 생성. 전역 설치와 계정 전용 홈이 같은 OS 인용·사건 규칙을 쓴다.
use serde_json::{Value, json};
use std::path::Path;

pub const EVENTS: [&str; 3] = ["SessionStart", "UserPromptSubmit", "PostToolUse"];

/// Unix는 sh 인용, Windows는 herdr의 PowerShell EncodedCommand 방식이다.
pub fn command(exe: &Path) -> String {
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
    // 기본 code page와 native stdin 전달 차이를 피한다.
    // Codex 0.164는 선택한 turn shell로 hook을 실행한다. cmd를 명시해 PowerShell에서도 같은 guard를 쓴다.
    // 비-aaa 세션은 cmd에서 거른다. PowerShell 안의 guard만으로는 그 host의 기동 비용을 피하지 못한다.
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
        "cmd.exe /d /s /c \"if defined AAA_SESSION_ID if defined AAA_HOOK_SOCK if defined AAA_HOOK_TOKEN \
         powershell -NoProfile -NonInteractive -OutputFormat Text -EncodedCommand {}\"",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

pub fn group(event: &str, command: &str) -> Value {
    let hook = json!({"type":"command","command":command,"timeout":10});
    if event == "SessionStart" {
        json!({"matcher":"^(startup|resume|clear|compact|fork)$","hooks":[hook]})
    } else {
        json!({"hooks":[hook]})
    }
}

pub fn settings(exe: &Path) -> Value {
    let command = command(exe);
    let hooks: serde_json::Map<String, Value> =
        EVENTS.into_iter().map(|event| (event.into(), json!([group(event, &command)]))).collect();
    json!({"hooks": hooks})
}

/// aaa가 만든 세 handler만 내용 해시로 신뢰한다. 전역 hook·project hook 신뢰를 우회하지 않는다.
/// Codex rust-v0.164.0 hooks/engine/discovery.rs의 정규화와 config/fingerprint.rs의
/// 정렬 JSON SHA-256 형식을 따른다(Apache-2.0). 설정의 임의 명령은 입력받지 않는다.
pub fn trusted_state(exe: &Path, hook_path: &Path) -> Value {
    use sha2::{Digest, Sha256};
    let command = command(exe);
    let mut state = serde_json::Map::new();
    for (event, key) in [
        ("SessionStart", "session_start"),
        ("UserPromptSubmit", "user_prompt_submit"),
        ("PostToolUse", "post_tool_use"),
    ] {
        // TOML 정규화에서 None 필드는 생략되고 async의 기본값 false는 포함된다.
        let mut identity = group(event, &command);
        identity["event_name"] = key.into();
        identity["hooks"][0]["async"] = false.into();
        identity.sort_all_objects();
        let bytes = serde_json::to_vec(&identity).expect("hook identity JSON");
        state.insert(
            format!("{}:{key}:0:0", hook_path.display()),
            json!({"trusted_hash":format!("sha256:{}", hex::encode(Sha256::digest(bytes)))}),
        );
    }
    Value::Object(state)
}

#[cfg(test)]
mod tests {
    #[test]
    fn windows_command_preserves_special_path_and_utf8_stdin_stdout() {
        use base64::Engine;
        let cmd = super::windows_command(std::path::Path::new(r"C:\A b\한글 ' $ &\aaa.exe"));
        let encoded = cmd
            .strip_prefix(concat!(
                "cmd.exe /d /s /c \"if defined AAA_SESSION_ID if defined AAA_HOOK_SOCK if defined AAA_HOOK_TOKEN ",
                "powershell -NoProfile -NonInteractive -OutputFormat Text -EncodedCommand "
            ))
            .unwrap()
            .strip_suffix('"')
            .unwrap();
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
