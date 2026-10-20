//! host 전용 인증 금고. K의 KEK 위치·봉투 암호화와 같은 최소 저장 층이다.
use aaa_common::{redact, util};
use aaa_proto::{AccountCredential, AccountRefreshed, AccountView, ErrorBody, ErrorCode, SecretText};
use ring::{
    aead,
    rand::{SecureRandom, SystemRandom},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const MAX: usize = 128 * 1024;
fn fail() -> ErrorBody {
    ErrorBody::new(ErrorCode::BadRequest, "계정 인증을 처리하지 못했다. 입력 형식·저장 권한을 확인한다")
}
pub fn valid_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 64
        && n.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
        && redact::text(n) == n
}

#[derive(Serialize, Deserialize)]
struct Sealed {
    name: String,
    harness: String,
    version: u64,
    nonce: String,
    wrapped_nonce: String,
    data: String,
    wrapped: String,
}
impl Sealed {
    fn aad(&self) -> Vec<u8> {
        format!("aaa-account/v1\0{}\0{}\0{}", self.name, self.harness, self.version).into_bytes()
    }
    fn view(&self) -> AccountView {
        AccountView {
            name: self.name.clone(),
            harness: self.harness.clone(),
            version: self.version,
            email: None,
            plan: None,
            first_auth_ms: None,
            last_auth_ms: None,
            expires_ms: None,
            health: None,
        }
    }
}

fn random<const N: usize>() -> Result<[u8; N], ErrorBody> {
    let mut b = [0; N];
    SystemRandom::new().fill(&mut b).map_err(|_| fail())?;
    Ok(b)
}
fn cipher(key: &[u8]) -> Result<aead::LessSafeKey, ErrorBody> {
    Ok(aead::LessSafeKey::new(aead::UnboundKey::new(&aead::CHACHA20_POLY1305, key).map_err(|_| fail())?))
}
fn seal(key: &[u8], nonce: [u8; 12], aad: &[u8], value: &[u8]) -> Result<String, ErrorBody> {
    let mut b = value.to_vec();
    cipher(key)?
        .seal_in_place_append_tag(aead::Nonce::assume_unique_for_key(nonce), aead::Aad::from(aad), &mut b)
        .map_err(|_| fail())?;
    Ok(hex::encode(b))
}
fn open(key: &[u8], nonce: &str, aad: &[u8], value: &str) -> Result<Vec<u8>, ErrorBody> {
    let n: [u8; 12] = hex::decode(nonce).map_err(|_| fail())?.try_into().map_err(|_| fail())?;
    let mut b = hex::decode(value).map_err(|_| fail())?;
    Ok(cipher(key)?
        .open_in_place(aead::Nonce::assume_unique_for_key(n), aead::Aad::from(aad), &mut b)
        .map_err(|_| fail())?
        .to_vec())
}

/// 최종 파일·부모를 symlink로 바꾸어 금고 밖을 읽거나 쓰지 않는다.
fn regular(path: &Path) -> Result<(), ErrorBody> {
    let m = path.symlink_metadata().map_err(|_| fail())?;
    if !m.is_file() || m.len() > MAX as u64 * 4 {
        return Err(fail());
    }
    Ok(())
}
struct Store {
    root: PathBuf,
    key_path: PathBuf,
}
impl Store {
    fn key(&self, create: bool) -> Result<[u8; 32], ErrorBody> {
        if !self.key_path.exists() {
            if !create || self.root.read_dir().ok().is_some_and(|mut d| d.next().is_some()) {
                return Err(fail());
            }
            use std::io::Write;
            let k = random::<32>()?;
            let mut opts = std::fs::OpenOptions::new();
            opts.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600);
            }
            let mut f = opts.open(&self.key_path).map_err(|_| fail())?;
            f.write_all(&k).and_then(|_| f.sync_all()).map_err(|_| fail())?;
        }
        regular(&self.key_path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if self.key_path.metadata().map_err(|_| fail())?.permissions().mode() & 0o077 != 0 {
                return Err(fail());
            }
        }
        std::fs::read(&self.key_path).map_err(|_| fail())?.try_into().map_err(|_| fail())
    }
    fn path(&self, name: &str) -> Result<PathBuf, ErrorBody> {
        if !valid_name(name) {
            return Err(fail());
        }
        Ok(self.root.join(format!("{name}.json")))
    }
    fn record(&self, name: &str) -> Result<Sealed, ErrorBody> {
        let p = self.path(name)?;
        regular(&p)?;
        let r: Sealed = serde_json::from_slice(&std::fs::read(p).map_err(|_| fail())?).map_err(|_| fail())?;
        if r.name != name {
            return Err(fail());
        }
        Ok(r)
    }
    fn read(&self, name: &str) -> Result<AccountCredential, ErrorBody> {
        let r = self.record(name)?;
        let key = self.key(false)?;
        let aad = r.aad();
        let dek = open(&key, &r.wrapped_nonce, &aad, &r.wrapped)?;
        let value = String::from_utf8(open(&dek, &r.nonce, &aad, &r.data)?).map_err(|_| fail())?;
        redact::remember_auth(&r.harness, &value);
        Ok(AccountCredential {
            run: String::new(),
            account: r.name,
            harness: r.harness,
            version: r.version,
            secret: SecretText(value),
            trust_cwd: None,
        })
    }
    fn write(&self, name: &str, harness: &str, version: u64, value: &str) -> Result<AccountView, ErrorBody> {
        let path = self.path(name)?;
        if self.root.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(fail());
        }
        util::ensure_private_dir(&self.root).map_err(|_| fail())?;
        let key = self.key(true)?;
        let dek = random::<32>()?;
        let nonce = random::<12>()?;
        let wrap_nonce = random::<12>()?;
        let mut r = Sealed {
            name: name.into(),
            harness: harness.into(),
            version,
            nonce: hex::encode(nonce),
            wrapped_nonce: hex::encode(wrap_nonce),
            data: String::new(),
            wrapped: String::new(),
        };
        let aad = r.aad();
        r.data = seal(&dek, nonce, &aad, value.as_bytes())?;
        r.wrapped = seal(&key, wrap_nonce, &aad, &dek)?;
        // 임시 파일 이름은 매번 무작위이고 기존 파일을 따라가지 않는다.
        let temp = self.root.join(format!(".{}.tmp", util::new_id()));
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        use std::io::Write;
        let result = (|| {
            let mut f = opts.open(&temp)?;
            f.write_all(&serde_json::to_vec(&r).map_err(std::io::Error::other)?)?;
            f.sync_all()?;
            drop(f);
            std::fs::rename(&temp, &path)?;
            #[cfg(unix)]
            std::fs::File::open(&self.root)?.sync_all()?;
            Ok::<_, std::io::Error>(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(temp);
            return Err(fail());
        }
        Ok(r.view())
    }
}

/// Codex 갱신이 어떻게 끝났나(계정 상태 판정, plan §8.351). 제공자가 거부한 것(400·401)만 끊김이다: 망·시한·5xx는 모른다
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RefreshOutcome {
    /// 제공자가 새 인증을 줬다
    Renewed,
    /// 옛 판 요청에 금고의 더 새 판을 줬다(제공자에게 묻지 않았다: 상태를 바꾸지 않는다)
    Served,
    /// 제공자가 지금 금고 판의 갱신을 거부했다
    Rejected,
    Failed,
}

pub struct Accounts {
    pub(crate) logins: Mutex<HashMap<String, crate::account_login::LoginJob>>,
    store: Mutex<Store>,
    gates: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}
impl Accounts {
    pub fn new(state: &Path) -> Result<Self, ErrorBody> {
        let out = Self {
            store: Mutex::new(Store { root: state.join("accounts"), key_path: state.join("secrets.key") }),
            gates: Default::default(),
            logins: Default::default(),
        };
        // 재시작 뒤에도 과거 인증을 일반 기록에 다시 노출하지 않는다.
        // 키/한 계정이 잠겨도 계정 미지정 기능은 계속 제공한다.
        if let Ok(accounts) = out.list() {
            for a in accounts {
                let _ = out.credential(&a.name);
            }
        }
        Ok(out)
    }
    pub fn list(&self) -> Result<Vec<AccountView>, ErrorBody> {
        let s = self.store.lock().unwrap();
        if !s.root.exists() {
            return Ok(vec![]);
        }
        if s.root.symlink_metadata().map_err(|_| fail())?.file_type().is_symlink() {
            return Err(fail());
        }
        let mut out = vec![];
        for e in s.root.read_dir().map_err(|_| fail())? {
            let e = e.map_err(|_| fail())?;
            let name = e.file_name();
            let name = name.to_string_lossy();
            if let Some(n) = name.strip_suffix(".json") {
                out.push(s.record(n)?.view())
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }
    pub fn import(&self, name: &str, harness: &str, value: &str) -> Result<AccountView, ErrorBody> {
        redact::remember_auth(harness, value);
        validate(harness, value)?;
        let s = self.store.lock().unwrap();
        let p = s.path(name)?;
        let version = if p.exists() {
            let old = s.record(name)?;
            if old.harness != harness {
                return Err(fail());
            }
            old.version.checked_add(1).ok_or_else(fail)?
        } else {
            1
        };
        s.write(name, harness, version, value)
    }
    pub fn credential(&self, name: &str) -> Result<AccountCredential, ErrorBody> {
        self.store.lock().unwrap().read(name)
    }
    pub fn remove(&self, name: &str) -> Result<(), ErrorBody> {
        let s = self.store.lock().unwrap();
        let p = s.path(name)?;
        regular(&p)?;
        std::fs::remove_file(p).map_err(|_| fail())
    }
    pub(crate) async fn refresh(
        &self,
        name: &str,
        version: u64,
        request: &SecretText,
    ) -> (AccountRefreshed, RefreshOutcome) {
        let gate = self.gates.lock().unwrap().entry(name.into()).or_default().clone();
        let _guard = gate.lock().await;
        let mut outcome = RefreshOutcome::Failed;
        let result = self.refresh_inner(name, version, request, &mut outcome).await;
        match result {
            Ok((version, response)) => {
                (AccountRefreshed { id: String::new(), version, response: Some(SecretText(response)) }, outcome)
            }
            Err(_) => (AccountRefreshed { id: String::new(), version, response: None }, outcome),
        }
    }
    async fn refresh_inner(
        &self,
        name: &str,
        version: u64,
        request: &SecretText,
        outcome: &mut RefreshOutcome,
    ) -> Result<(u64, String), ErrorBody> {
        let c = self.credential(name)?;
        if c.harness != "codex" || version > c.version {
            return Err(fail());
        }
        let mut auth: serde_json::Value = serde_json::from_str(&c.secret.0).map_err(|_| fail())?;
        let req: serde_json::Value = serde_json::from_str(&request.0).map_err(|_| fail())?;
        if req.get("grant_type").and_then(|v| v.as_str()) != Some("refresh_token") {
            return Err(fail());
        }
        if version < c.version {
            *outcome = RefreshOutcome::Served;
            return Ok((c.version, token_response(&auth)?));
        }
        // 주소는 production에서 고정한다. 가짜 서버는 debug 실험실의 loopback에만 허용한다.
        let mut url = "https://auth.openai.com/oauth/token".to_string();
        if cfg!(debug_assertions)
            && let Ok(origin) = std::env::var("AAA_TEST_ACCOUNT_REFRESH_ORIGIN")
            && origin.starts_with("http://127.0.0.1:")
        {
            url = format!("{}/oauth/token", origin.trim_end_matches('/'));
        }
        let body=serde_json::to_vec(&serde_json::json!({"client_id":"app_EMoamEEZ73f0CkXaXp7hrann","grant_type":"refresh_token","refresh_token":auth["tokens"]["refresh_token"]})).map_err(|_|fail())?;
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            crate::http::request("POST", &url, &[("Content-Type", "application/json".into())], &body, MAX),
        )
        .await
        .map_err(|_| fail())?
        .map_err(|_| fail())?;
        redact::remember_auth("codex", &String::from_utf8_lossy(&response.body));
        if response.status != 200 {
            // 묻는 사이 다시 로그인해 금고 판이 올랐으면 거부는 옛 인증의 것이다(끊김이 아니다)
            if matches!(response.status, 400 | 401) && self.store.lock().unwrap().record(name)?.version == version {
                *outcome = RefreshOutcome::Rejected;
            }
            return Err(fail());
        }
        let tokens: serde_json::Value = serde_json::from_slice(&response.body).map_err(|_| fail())?;
        for k in ["access_token", "refresh_token"] {
            if let Some(v) = tokens.get(k).filter(|v| !v.is_null()) {
                let v = v.as_str().filter(|s| s.len() >= 12).ok_or_else(fail)?;
                auth["tokens"][k] = v.into();
            }
        }
        if let Some(v) = tokens.get("id_token").and_then(|v| v.as_str()) {
            auth["tokens"]["id_token"] = v.into();
        }
        auth["last_refresh"] = util::utc_iso(util::now_ms()).into();
        let value = serde_json::to_string(&auth).map_err(|_| fail())?;
        redact::remember_auth("codex", &value);
        let s = self.store.lock().unwrap();
        if s.record(name)?.version != version {
            return Err(fail());
        }
        let v = version.checked_add(1).ok_or_else(fail)?;
        s.write(name, "codex", v, &value)?;
        *outcome = RefreshOutcome::Renewed;
        Ok((v, token_response(&auth)?))
    }
}
fn token_response(auth: &serde_json::Value) -> Result<String, ErrorBody> {
    let t = auth.get("tokens").and_then(|v| v.as_object()).ok_or_else(fail)?;
    let mut out = serde_json::Map::new();
    for k in ["access_token", "refresh_token", "id_token"] {
        if let Some(v) = t.get(k) {
            out.insert(k.into(), v.clone());
        }
    }
    serde_json::to_string(&out).map_err(|_| fail())
}
fn validate(harness: &str, value: &str) -> Result<(), ErrorBody> {
    if value.len() < 12 || value.len() > MAX {
        return Err(fail());
    }
    match harness {
        "claude" if value.bytes().all(|b| b.is_ascii_graphic()) => Ok(()),
        "codex" => {
            let v: serde_json::Value = serde_json::from_str(value).map_err(|_| fail())?;
            for k in ["access_token", "refresh_token"] {
                if !v["tokens"][k].as_str().is_some_and(|s| s.len() >= 12) {
                    return Err(fail());
                }
            }
            Ok(())
        }
        _ => Err(fail()),
    }
}

impl crate::Host {
    pub(crate) async fn spawn_account_on(
        self: &Arc<Self>,
        mid: &str,
        spec: aaa_proto::SpawnSpec,
        credential: AccountCredential,
        actor: &str,
        expected_pane: Option<&str>,
    ) -> Result<String, ErrorBody> {
        let conn = self.live_conn(mid)?;
        self.check_health(mid)?;
        let (_, task) = aaa_proto::account_run_parts(&credential.run).ok_or_else(fail)?;
        let feature = if task.starts_with("c-") { "account_auth.v2" } else { "account_auth.v1" };
        if !self.machine_feature(mid, feature) {
            return Err(fail());
        }
        let mut binding = crate::account_policy::PaneAccount {
            run: credential.run.clone(),
            account: credential.account.clone(),
            harness: credential.harness.clone(),
            machine: mid.into(),
            pane: None,
            profile: spec.profile.clone(),
        };
        let previous = self.claim_account_run(&mut binding, expected_pane)?;
        self.db.audit(actor, "pane_run_account", &credential.account);
        match conn.request(aaa_proto::AgentRequest::SpawnAccount { spec, credential }).await {
            Ok(aaa_proto::AgentResponse::Session(info)) => {
                let id = info.id.clone();
                binding.pane = Some(id.clone());
                self.save_account_run(&binding)?;
                self.on_session(mid, info, false);
                Ok(id)
            }
            other => {
                if let Some(previous) = previous {
                    self.save_account_run(&previous)?;
                } else {
                    self.db.del_meta(&format!("account-run:{}", binding.run)).map_err(|_| fail())?;
                }
                match other {
                    Err(e) => Err(e),
                    _ => Err(fail()),
                }
            }
        }
    }
    pub(crate) async fn account_refresh(self: Arc<Self>, mid: String, request: aaa_proto::AccountRefresh) {
        let bound = self
            .account_run(&request.run)
            .ok()
            .flatten()
            .filter(|r| r.machine == mid && r.pane.as_deref().is_none_or(|p| p == request.session))
            .map(|r| r.account);
        let account = bound.or_else(|| {
            request
                .run
                .split_once('/')
                .and_then(|(p, t)| self.load(p, t).ok())
                .filter(|r| {
                    r.machine_id == mid
                        && (r.pane.as_deref() == Some(&request.session)
                            || r.status == aaa_proto::ThreadStatus::Starting)
                })
                .and_then(|r| r.account)
        });
        let mut result = match account {
            Some(a) => {
                let (result, outcome) = self.accounts.refresh(&a, request.version, &request.request).await;
                self.account_refresh_seen(&a, outcome, request.version);
                result
            }
            None => AccountRefreshed { id: String::new(), version: request.version, response: None },
        };
        result.id = request.id;
        if let Ok(conn) = self.live_conn(&mid) {
            let _ = conn.request(aaa_proto::AgentRequest::AccountRefreshed { session: request.session, result }).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Temp(PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn encrypted_accounts_reject_tampering_and_lock_without_stopping_other_accounts() {
        let state = Temp(std::env::temp_dir().join(format!("aaa-vault-{}", util::new_id())));
        util::ensure_private_dir(&state.0).unwrap();
        let accounts = Accounts::new(&state.0).unwrap();
        // 경로용 시간 ID와 무관한 엔트로피를 쓴다. 공통 ID 접두부는 비밀 조각이 아니다.
        let value = format!("test-oauth-{}", hex::encode(random::<32>().unwrap()));
        let first = accounts.import("work", "claude", &value).unwrap();
        assert_eq!(first.version, 1);
        assert!(accounts.credential("work").unwrap().secret.0 == value, "인증 왕복");
        let file = state.0.join("accounts/work.json");
        let sealed = std::fs::read(&file).unwrap();
        assert!(!sealed.windows(value.len()).any(|w| w == value.as_bytes()), "암호문에 원문 없음");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for (path, mode) in
                [(state.0.join("accounts"), 0o700), (state.0.join("secrets.key"), 0o600), (file.clone(), 0o600)]
            {
                assert_eq!(path.metadata().unwrap().permissions().mode() & 0o777, mode);
            }
        }
        for field in ["name", "harness", "version", "data", "wrapped"] {
            let mut changed: serde_json::Value = serde_json::from_slice(&sealed).unwrap();
            changed[field] = match field {
                "name" => "other".into(),
                "harness" => "codex".into(),
                "version" => 2.into(),
                _ => "00".into(),
            };
            std::fs::write(&file, serde_json::to_vec(&changed).unwrap()).unwrap();
            assert!(accounts.credential("work").is_err(), "AAD/암호문 바꿔치기 거부: {field}");
            assert!(Accounts::new(&state.0).is_ok(), "잠김 금고 때문에 서비스 전체가 죽지 않는다");
        }
        std::fs::write(&file, sealed).unwrap();
        std::fs::rename(state.0.join("secrets.key"), state.0.join("held.key")).unwrap();
        let locked = Accounts::new(&state.0).unwrap();
        assert!(locked.credential("work").is_err());
        assert!(locked.import("new", "claude", &value).is_err(), "새 키로 기존 금고를 덮지 않는다");
        std::fs::rename(state.0.join("held.key"), state.0.join("secrets.key")).unwrap();
        assert!(locked.credential("work").unwrap().secret.0 == value, "키 복구 뒤 인증 왕복");
        assert_eq!(locked.import("work", "claude", &format!("{value}-next")).unwrap().version, 2);
        locked.remove("work").unwrap();
        assert!(locked.credential("work").is_err());
    }
}
