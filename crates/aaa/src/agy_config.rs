//! 사람이 실행 머신의 agent.toml에 개인 agy 구독을 등록한다.

use std::path::Path;
use std::process::ExitCode;

use aaa_config::{AgyAccount, profile_edit};
use anyhow::{Context, Result, ensure};
use toml_edit::{Array, DocumentMut, Item, Table, TableLike, value};

use crate::settings::read_config_text;

const LOGIN_PROMPT: &str = "agy Google 로그인 주소";

pub fn configure(dir: &Path, executable: String, subscription: String, json: bool) -> Result<ExitCode> {
    crate::person::require("`aaa config agy`(실행 머신 설정)")?;
    let command = [executable];
    profile_edit::validate_agy_command(&command).map_err(anyhow::Error::msg)?;
    ensure!(profile_edit::executable_available(&command[0], None), "`--command`의 agy 실행 파일을 실행할 수 없다");
    profile_edit::validate_name(&subscription).map_err(anyhow::Error::msg)?;

    let file = dir.join("agent.toml");
    let local_file = dir.join("agent.local.toml");
    let before = read_config_text(&file)?;
    let local = read_config_text(&local_file)?;
    if let Some(text) = &local {
        let doc = text.parse::<DocumentMut>().context("agent.local.toml 읽기 실패")?;
        let shadows =
            doc.get("agy").is_some_and(|a| a.get("subscription").is_some() || a.get("login_sha256").is_some())
                || doc.get("harnesses").and_then(|h| h.get("agy")).and_then(|a| a.get("command")).is_some();
        ensure!(!shadows, "agent.local.toml이 agy 값을 덮는다. 그 파일의 agy 값을 확인한다");
    }
    let mut doc = before.as_deref().unwrap_or_default().parse::<DocumentMut>().context("agent.toml 읽기 실패")?;
    if before.as_deref().is_none_or(|text| text.trim().is_empty()) {
        doc["schema"] = value(i64::from(aaa_config::SCHEMA));
    }
    let old_command = doc.get("harnesses").and_then(|h| h.get("agy")).and_then(|a| a.get("command"));
    let old_account = doc.get("agy");
    if old_command.is_some() || old_account.is_some() {
        let old_exe = old_command.and_then(Item::as_array).and_then(|a| a.get(0)).and_then(|v| v.as_str());
        if let Some(exe) = old_exe {
            eprintln!("현재 실행 경로 {exe:?}");
        }
        if let Some(alias) = old_account.and_then(|a| a.get("subscription")).and_then(Item::as_str) {
            eprintln!("현재 구독 별칭 {alias:?}");
        }
        if old_account.is_some_and(|a| a.get("login_sha256").is_some()) {
            eprintln!("현재 로그인 해시 등록됨");
        }
        eprintln!("새 실행 경로 {:?}\n새 구독 별칭 {subscription:?}", command[0]);
        if !inquire::Confirm::new("기존 agy 설정을 바꿀까요?").with_default(false).prompt()? {
            return Ok(ExitCode::SUCCESS);
        }
    }

    let login = inquire::Password::new(LOGIN_PROMPT)
        .with_display_mode(inquire::PasswordDisplayMode::Hidden)
        .without_confirmation()
        .prompt()?;
    let login = login.trim();
    ensure!(
        login.split_once('@').is_some_and(|(name, domain)| !name.is_empty() && !domain.is_empty())
            && !login.chars().any(char::is_whitespace)
            && login.bytes().filter(|&b| b == b'@').count() == 1,
        "agy Google 로그인 주소 형식이 아니다"
    );
    let account = AgyAccount { login_sha256: AgyAccount::hash_login(login), subscription };
    let harnesses = table(doc.as_table_mut(), "harnesses")?;
    let mut argv = Array::new();
    argv.push(command[0].as_str());
    table(harnesses, "agy")?.insert("command", value(argv));
    let agy = table(doc.as_table_mut(), "agy")?;
    agy.insert("subscription", value(&account.subscription));
    agy.insert("login_sha256", value(&account.login_sha256));
    let edited = doc.to_string();

    std::fs::create_dir_all(dir).with_context(|| format!("{} 만들기 실패", dir.display()))?;
    let lock = aaa_common::private_file::coordinated_lock(&dir.join("agent.toml.agy.lock"), true)?;
    let saved = (|| {
        ensure!(
            read_config_text(&file)? == before && read_config_text(&local_file)? == local,
            "agent 설정 파일이 바뀌었다. 현재 설정을 확인하고 다시 실행한다"
        );
        aaa_config::check_agent_edit(dir, &edited).map_err(|e| anyhow::anyhow!("{}", e.render()))?;
        if before.as_deref() != Some(&edited) {
            profile_edit::write_atomic(&file, edited.as_bytes())
                .with_context(|| format!("{} 쓰기 실패", file.display()))?;
        }
        Ok(())
    })();
    let unlocked = lock.unlock();
    saved?;
    unlocked?;
    let message = "agy 머신 설정을 저장했다";
    if json {
        println!("{}", serde_json::json!({"message": message, "file": file, "subscription": account.subscription}));
    } else {
        println!("{message}. `aaa integration install agy`를 실행하고 agent를 다시 시작한다.");
    }
    Ok(ExitCode::SUCCESS)
}

fn table<'a>(parent: &'a mut dyn TableLike, key: &str) -> Result<&'a mut dyn TableLike> {
    if !parent.contains_key(key) {
        let mut new = Table::new();
        new.set_implicit(true);
        parent.insert(key, Item::Table(new));
    }
    parent.get_mut(key).and_then(Item::as_table_like_mut).with_context(|| format!("agent.toml의 `{key}`가 표가 아니다"))
}
