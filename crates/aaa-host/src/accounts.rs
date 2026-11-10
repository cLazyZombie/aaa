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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    generation: Option<String>,
    nonce: String,
    wrapped_nonce: String,
    data: String,
    wrapped: String,
}
impl Sealed {
    fn auth_generation(&self) -> String {
        self.generation.clone().unwrap_or_else(|| format!("legacy-{}", self.version))
    }
    fn aad(&self) -> Vec<u8> {
        let mut aad = format!("aaa-account/v1\0{}\0{}\0{}", self.name, self.harness, self.version);
        if let Some(generation) = &self.generation {
            aad.push_str(&format!("\0{generation}"));
        }
        aad.into_bytes()
    }
    fn view(&self) -> AccountView {
        AccountView {
            plan_override: None,
            detected_plan: None,
            name: self.name.clone(),
            harness: self.harness.clone(),
            version: self.version,
            email: None,
            plan: None,
            first_auth_ms: None,
            last_auth_ms: None,
            expires_ms: None,
            health: None,
            token_expires_ms: None,
            refreshed_ms: None,
            refresh_failure: None,
        }
    }
}

fn random<const N: usize>() -> Result<[u8; N], ErrorBody> {
    let mut b = [0; N];
    SystemRandom::new().fill(&mut b).map_err(|source_error| aaa_common::diagnostic::context(source_error, fail()))?;
    Ok(b)
}
fn cipher(key: &[u8]) -> Result<aead::LessSafeKey, ErrorBody> {
    Ok(aead::LessSafeKey::new(
        aead::UnboundKey::new(&aead::CHACHA20_POLY1305, key)
            .map_err(|source_error| aaa_common::diagnostic::context(source_error, fail()))?,
    ))
}
fn seal(key: &[u8], nonce: [u8; 12], aad: &[u8], value: &[u8]) -> Result<String, ErrorBody> {
    let mut b = value.to_vec();
    cipher(key)?
        .seal_in_place_append_tag(aead::Nonce::assume_unique_for_key(nonce), aead::Aad::from(aad), &mut b)
        .map_err(|source_error| aaa_common::diagnostic::context(source_error, fail()))?;
    Ok(hex::encode(b))
}
fn open(key: &[u8], nonce: &str, aad: &[u8], value: &str) -> Result<Vec<u8>, ErrorBody> {
    let n: [u8; 12] = hex::decode(nonce)
        .map_err(|source_error| aaa_common::diagnostic::context(source_error, fail()))?
        .try_into()
        .map_err(|source_error| aaa_common::diagnostic::context(source_error, fail()))?;
    let mut b = hex::decode(value).map_err(|source_error| aaa_common::diagnostic::context(source_error, fail()))?;
    Ok(cipher(key)?
        .open_in_place(aead::Nonce::assume_unique_for_key(n), aead::Aad::from(aad), &mut b)
        .map_err(|source_error| aaa_common::diagnostic::context(source_error, fail()))?
        .to_vec())
}

/// 최종 파일·부모를 symlink로 바꾸어 금고 밖을 읽거나 쓰지 않는다.
fn regular(path: &Path) -> Result<(), ErrorBody> {
    let m = path.symlink_metadata().map_err(|source_error| aaa_common::diagnostic::context(source_error, fail()))?;
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
            if !create
                || self
                    .root
                    .read_dir()
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
                    .is_some_and(|mut d| d.next().is_some())
            {
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
            let mut f = opts
                .open(&self.key_path)
                .map_err(|source_error| aaa_common::diagnostic::context(source_error, fail()))?;
            f.write_all(&k)
                .and_then(|_| f.sync_all())
                .map_err(|source_error| aaa_common::diagnostic::context(source_error, fail()))?;
        }
        regular(&self.key_path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if self
                .key_path
                .metadata()
                .map_err(|source_error| aaa_common::diagnostic::context(source_error, fail()))?
                .permissions()
                .mode()
                & 0o077
                != 0
            {
                return Err(fail());
            }
        }
        std::fs::read(&self.key_path)
            .map_err(|source_error| aaa_common::diagnostic::context(source_error, fail()))?
            .try_into()
            .map_err(|source_error| aaa_common::diagnostic::context(source_error, fail()))
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
        let r: Sealed = serde_json::from_slice(
            &std::fs::read(p).map_err(|source_error| aaa_common::diagnostic::context(source_error, fail()))?,
        )
        .map_err(|source_error| aaa_common::diagnostic::context(source_error, fail()))?;
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
        let value = String::from_utf8(open(&dek, &r.nonce, &aad, &r.data)?)
            .map_err(|source_error| aaa_common::diagnostic::context(source_error, fail()))?;
        redact::remember_auth(&r.harness, &value);
        let generation = r.auth_generation();
        Ok(AccountCredential {
            run: String::new(),
            account: r.name,
            harness: r.harness,
            version: r.version,
            secret: SecretText(value),
            trust_cwd: None,
            auth_generation: Some(generation),
        })
    }
    fn write(
        &self,
        name: &str,
        harness: &str,
        version: u64,
        value: &str,
        generation: Option<String>,
    ) -> Result<AccountView, ErrorBody> {
        let path = self.path(name)?;
        if self
            .root
            .symlink_metadata()
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "성공 여부만 쓰는 분기의 하위 오류"))
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            return Err(fail());
        }
        util::ensure_private_dir(&self.root)
            .map_err(|source_error| aaa_common::diagnostic::context(source_error, fail()))?;
        let key = self.key(true)?;
        let dek = random::<32>()?;
        let nonce = random::<12>()?;
        let wrap_nonce = random::<12>()?;
        let mut r = Sealed {
            name: name.into(),
            harness: harness.into(),
            version,
            generation,
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
        if result
            .as_ref()
            .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
            .is_err()
        {
            aaa_common::diagnostic::ignored(std::fs::remove_file(temp), "정리 실패가 본 작업의 결과를 덮지 않게 한다");
            return Err(fail());
        }
        Ok(r.view())
    }
}

/// Codex 갱신이 어떻게 끝났나(계정 상태 판정, plan §8.351). 제공자가 refresh token을 거부한 것만 끊김이다: 망·시한·5xx·
/// 그 밖의 400은 모른다. 거부 판정은 Codex 0.167 `classify_refresh_token_failure`와 같다([`permanent`])
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RefreshOutcome {
    /// 제공자가 새 인증을 줬다
    Renewed,
    /// 옛 판 요청에 금고의 더 새 판을 줬다(제공자에게 묻지 않았다: 상태를 바꾸지 않는다)
    Served,
    /// 제공자가 지금 금고 판의 갱신을 거부했다(400·401)
    Rejected(RefreshFailure),
    /// 그 밖의 실패
    Failed(RefreshFailure),
}
impl RefreshOutcome {
    pub(crate) fn failure(&self) -> Option<&RefreshFailure> {
        match self {
            RefreshOutcome::Rejected(f) | RefreshOutcome::Failed(f) => Some(f),
            RefreshOutcome::Renewed | RefreshOutcome::Served => None,
        }
    }
}

/// 갱신이 실패한 까닭(값·본문 없음, t-1688). 알림·계정 목록·pane 중계가 쓴다
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RefreshFailure {
    /// `aaa`(aaa 안), `network`(host가 제공자에 닿지 못함), `provider`(제공자가 200이 아닌 상태)
    pub cause: &'static str,
    pub stage: &'static str,
    pub status: Option<u16>,
    pub code: Option<String>,
}
impl RefreshFailure {
    pub(crate) fn aaa(stage: &'static str) -> Self {
        Self { cause: "aaa", stage, status: None, code: None }
    }
    pub(crate) fn network(stage: &'static str) -> Self {
        Self { cause: "network", stage, status: None, code: None }
    }
    pub(crate) fn provider(status: u16, code: Option<String>) -> Self {
        Self { cause: "provider", stage: "provider", status: Some(status), code }
    }
    /// pane 중계가 돌려줄 HTTP 상태와 오류 코드: 제공자 것 그대로, 망은 502. aaa 안의 실패는 없음(중계가 503을 쓴다)
    fn relay(&self) -> (Option<u16>, Option<String>) {
        match self.cause {
            "provider" => (self.status, self.code.clone()),
            "network" => (Some(502), Some("aaa_provider_unreachable".into())),
            _ => (None, None),
        }
    }
}

pub struct Accounts {
    pub(crate) logins: Mutex<HashMap<String, crate::account_login::LoginJob>>,
    store: Mutex<Store>,
    usage_store: Mutex<Store>,
    gates: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}
impl Accounts {
    pub fn new(state: &Path) -> Result<Self, ErrorBody> {
        let out = Self {
            store: Mutex::new(Store { root: state.join("accounts"), key_path: state.join("secrets.key") }),
            usage_store: Mutex::new(Store { root: state.join("usage-accounts"), key_path: state.join("secrets.key") }),
            gates: Default::default(),
            logins: Default::default(),
        };
        // 재시작 뒤에도 과거 인증을 일반 기록에 다시 노출하지 않는다.
        // 키/한 계정이 잠겨도 계정 미지정 기능은 계속 제공한다.
        if let Ok(accounts) = out.list() {
            for a in accounts {
                aaa_common::diagnostic::ignored(out.credential(&a.name), "보조 작업 실패 뒤 기존 처리를 계속한다");
            }
        }
        Ok(out)
    }
    pub fn list(&self) -> Result<Vec<AccountView>, ErrorBody> {
        let s = self.store.lock().unwrap();
        if !s.root.exists() {
            return Ok(vec![]);
        }
        if s.root
            .symlink_metadata()
            .map_err(|source_error| aaa_common::diagnostic::context(source_error, fail()))?
            .file_type()
            .is_symlink()
        {
            return Err(fail());
        }
        let mut out = vec![];
        for e in s.root.read_dir().map_err(|source_error| aaa_common::diagnostic::context(source_error, fail()))? {
            let e = e.map_err(|source_error| aaa_common::diagnostic::context(source_error, fail()))?;
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
        let previous = if p.exists() { Some(s.record(name)?.auth_generation()) } else { None };
        let view = s.write(name, harness, version, value, Some(util::new_id()))?;
        if let Some(generation) = previous {
            aaa_common::diagnostic::ignored(
                self.remove_usage(&generation),
                "옛 조회 인증 정리가 실행 인증 교체 결과를 덮지 않게 한다",
            );
        }
        Ok(view)
    }
    pub(crate) fn generation(&self, name: &str) -> Result<String, ErrorBody> {
        let r = self.store.lock().unwrap().record(name)?;
        Ok(r.auth_generation())
    }
    pub fn credential(&self, name: &str) -> Result<AccountCredential, ErrorBody> {
        self.store.lock().unwrap().read(name)
    }
    /// 실행 인증 세대가 조회 인증의 키다. 별칭 변경에는 유지되고 재등록·삭제에는 쓰이지 않는다.
    pub(crate) fn usage_credential(&self, generation: &str) -> Result<AccountCredential, ErrorBody> {
        let store = self.usage_store.lock().unwrap();
        if store.root.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(fail());
        }
        store.read(generation)
    }
    fn remove_usage(&self, generation: &str) -> Result<(), ErrorBody> {
        let s = self.usage_store.lock().unwrap();
        let path = s.path(generation)?;
        if path.try_exists().map_err(|_| fail())? {
            regular(&path)?;
            std::fs::remove_file(path).map_err(|_| fail())?;
        }
        Ok(())
    }
    pub(crate) fn store_usage(
        &self,
        generation: &str,
        value: &str,
        expected_version: Option<u64>,
    ) -> Result<(), ErrorBody> {
        redact::remember_auth("claude", value);
        // 실행 금고의 KEK가 없으면 새 조회 금고가 대체 키를 만들지 않는다.
        self.store.lock().unwrap().key(false)?;
        let s = self.usage_store.lock().unwrap();
        let old = if s.path(generation)?.exists() { Some(s.record(generation)?) } else { None };
        if expected_version.is_some_and(|v| old.as_ref().is_none_or(|r| r.version != v)) {
            return Err(fail());
        }
        let version = old.as_ref().map_or(Ok(1), |r| r.version.checked_add(1).ok_or_else(fail))?;
        let auth_generation =
            expected_version.and_then(|_| old.as_ref().map(Sealed::auth_generation)).unwrap_or_else(util::new_id);
        s.write(generation, "claude", version, value, Some(auth_generation))?;
        Ok(())
    }
    /// 옛 AAD로 열고 새 AAD로 봉투 전체를 재암호화한다. 인증 판·세대는 그대로다.
    /// 새 파일을 먼저 내구 저장하고 DB 확정 뒤 옛 파일을 치운다(복구 journal은 host 소유).
    pub(crate) fn stage_rename(&self, from: &str, to: &str) -> Result<(), ErrorBody> {
        let s = self.store.lock().unwrap();
        let path = s.path(to)?;
        if path.symlink_metadata().is_ok() {
            return Err(ErrorBody::new(aaa_proto::ErrorCode::NameTaken, "이미 쓰는 계정 이름이다"));
        }
        let old = s.record(from)?;
        let credential = s.read(from)?;
        s.write(to, &old.harness, old.version, &credential.secret.0, old.generation)?;
        Ok(())
    }
    pub fn remove(&self, name: &str) -> Result<(), ErrorBody> {
        let s = self.store.lock().unwrap();
        let p = s.path(name)?;
        regular(&p)?;
        let generation = s.record(name)?.auth_generation();
        std::fs::remove_file(p).map_err(|source_error| aaa_common::diagnostic::context(source_error, fail()))?;
        aaa_common::diagnostic::ignored(
            self.remove_usage(&generation),
            "옛 조회 인증 정리가 계정 삭제 결과를 덮지 않게 한다",
        );
        Ok(())
    }
    pub(crate) async fn refresh(
        &self,
        name: &str,
        version: u64,
        request: &SecretText,
    ) -> (AccountRefreshed, RefreshOutcome) {
        let gate = self.gates.lock().unwrap().entry(name.into()).or_default().clone();
        let _guard = gate.lock().await;
        match self.refresh_inner(name, version, request).await {
            Ok((version, response, outcome)) => (
                AccountRefreshed {
                    id: String::new(),
                    version,
                    response: Some(SecretText(response)),
                    status: None,
                    code: None,
                },
                outcome,
            ),
            Err(outcome) => {
                let (status, code) = outcome.failure().map(RefreshFailure::relay).unwrap_or_default();
                let reply = AccountRefreshed { id: String::new(), version, response: None, status, code };
                (reply, outcome)
            }
        }
    }
    /// host 선제 갱신(t-1688): 금고의 지금 판을 제공자에게 갱신한다. pane 요청과 같은 계정 관문을 쓰므로 겹치지 않는다.
    /// 돌려주는 판은 끝난 뒤 금고 판이다
    pub(crate) async fn renew_current(&self, name: &str) -> (u64, RefreshOutcome) {
        let gate = self.gates.lock().unwrap().entry(name.into()).or_default().clone();
        let _guard = gate.lock().await;
        let (version, auth) = match self.codex_auth(name) {
            Ok((c, auth)) => (c.version, auth),
            Err(outcome) => {
                let version = self.store.lock().unwrap().record(name).map(|r| r.version).unwrap_or_default();
                return (version, outcome);
            }
        };
        match self.renew(name, version, auth).await {
            Ok((version, _, outcome)) => (version, outcome),
            Err(outcome) => (version, outcome),
        }
    }
    #[cfg(test)]
    pub(crate) fn refresh_gate_for_test(&self, name: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.gates.lock().unwrap().entry(name.into()).or_default().clone()
    }
    fn codex_auth(&self, name: &str) -> Result<(AccountCredential, serde_json::Value), RefreshOutcome> {
        let c = self.credential(name).map_err(|e| failed(name, RefreshFailure::aaa("vault_read"), &e.message))?;
        if c.harness != "codex" {
            return Err(failed(name, RefreshFailure::aaa("harness"), "Codex 계정이 아니다"));
        }
        let auth =
            serde_json::from_str(&c.secret.0).map_err(|e| failed(name, RefreshFailure::aaa("vault_format"), e))?;
        Ok((c, auth))
    }
    /// 금고의 지금 판을 준다(제공자에게 묻지 않는다)
    fn serve(&self, name: &str) -> Result<(u64, String, RefreshOutcome), RefreshOutcome> {
        let (c, auth) = self.codex_auth(name)?;
        let response =
            token_response(&auth).map_err(|e| failed(name, RefreshFailure::aaa("vault_format"), &e.message))?;
        Ok((c.version, response, RefreshOutcome::Served))
    }
    async fn refresh_inner(
        &self,
        name: &str,
        version: u64,
        request: &SecretText,
    ) -> Result<(u64, String, RefreshOutcome), RefreshOutcome> {
        let (c, auth) = self.codex_auth(name)?;
        if version > c.version {
            return Err(failed(
                name,
                RefreshFailure::aaa("version"),
                format_args!("요청 판 {version} > 금고 판 {}", c.version),
            ));
        }
        // 옛 판 요청에는 금고의 최신 인증을 준다. 이 pane은 이미 이 계정 인증을 받은 실행이라 본문보다 판을 먼저 본다:
        // 다시 로그인하거나 host가 먼저 갱신한 뒤 도는 pane이 재시작 없이 새 인증을 받는다(t-1688)
        if version < c.version {
            return self.serve(name);
        }
        let req: serde_json::Value =
            serde_json::from_str(&request.0).map_err(|e| failed(name, RefreshFailure::aaa("request_format"), e))?;
        if req.get("grant_type").and_then(|v| v.as_str()) != Some("refresh_token") {
            return Err(failed(name, RefreshFailure::aaa("request_grant"), "grant_type이 refresh_token이 아니다"));
        }
        self.renew(name, version, auth).await
    }
    /// 금고 판 `version`의 refresh token으로 제공자에게 새 인증을 받아 다음 판으로 저장한다
    async fn renew(
        &self,
        name: &str,
        version: u64,
        mut auth: serde_json::Value,
    ) -> Result<(u64, String, RefreshOutcome), RefreshOutcome> {
        let current = || self.store.lock().unwrap().record(name).map(|r| r.version).ok();
        // 주소는 production에서 고정한다. 가짜 서버는 debug 실험실의 loopback에만 허용한다.
        let mut url = "https://auth.openai.com/oauth/token".to_string();
        if cfg!(debug_assertions)
            && let Ok(origin) = std::env::var("AAA_TEST_ACCOUNT_REFRESH_ORIGIN")
            && origin.starts_with("http://127.0.0.1:")
        {
            url = format!("{}/oauth/token", origin.trim_end_matches('/'));
        }
        let body=serde_json::to_vec(&serde_json::json!({"client_id":"app_EMoamEEZ73f0CkXaXp7hrann","grant_type":"refresh_token","refresh_token":auth["tokens"]["refresh_token"]})).map_err(|e| failed(name, RefreshFailure::aaa("request_build"), e))?;
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            crate::http::request("POST", &url, &[("Content-Type", "application/json".into())], &body, MAX),
        )
        .await
        .map_err(|e| failed(name, RefreshFailure::network("provider_timeout"), e))?
        .map_err(|e| failed(name, RefreshFailure::network("provider_connect"), e))?;
        redact::remember_auth("codex", &String::from_utf8_lossy(&response.body));
        if response.status != 200 {
            // 본문은 남기지 않는다(제공자가 인증 조각을 되돌려 줄 수 있다). 상태와 오류 코드만
            let failure = RefreshFailure::provider(response.status, provider_code(&response.body));
            tracing::warn!(
                account = name,
                stage = failure.stage,
                status = response.status,
                code = failure.code.as_deref().unwrap_or("-"),
                "제공자가 계정 인증 갱신을 받지 않았다"
            );
            // 묻는 사이 다시 로그인해 금고 판이 올랐으면 거부는 옛 인증의 것이다(끊김이 아니다): 새 판을 준다
            return match current() {
                Some(v) if v != version => self.serve(name),
                _ if permanent(response.status, failure.code.as_deref()) => Err(RefreshOutcome::Rejected(failure)),
                _ => Err(RefreshOutcome::Failed(failure)),
            };
        }
        let tokens: serde_json::Value = serde_json::from_slice(&response.body)
            .map_err(|e| failed(name, RefreshFailure::aaa("provider_response"), e))?;
        for k in ["access_token", "refresh_token"] {
            if let Some(v) = tokens.get(k).filter(|v| !v.is_null()) {
                let v = v
                    .as_str()
                    .filter(|s| s.len() >= 12)
                    .ok_or_else(|| failed(name, RefreshFailure::aaa("provider_response"), format_args!("{k} 형식")))?;
                auth["tokens"][k] = v.into();
            }
        }
        if let Some(v) = tokens.get("id_token").and_then(|v| v.as_str()) {
            auth["tokens"]["id_token"] = v.into();
        }
        auth["last_refresh"] = util::utc_iso(util::now_ms()).into();
        let value = serde_json::to_string(&auth).map_err(|e| failed(name, RefreshFailure::aaa("vault_format"), e))?;
        redact::remember_auth("codex", &value);
        let response =
            token_response(&auth).map_err(|e| failed(name, RefreshFailure::aaa("vault_format"), &e.message))?;
        let s = self.store.lock().unwrap();
        let record = s.record(name).map_err(|e| failed(name, RefreshFailure::aaa("vault_read"), &e.message))?;
        if record.version != version {
            // 갱신 중에 다시 로그인했다. 옛 인증으로 받은 새 인증은 버리고 새 로그인의 인증을 준다
            drop(s);
            return self.serve(name);
        }
        let v = version.checked_add(1).ok_or_else(|| failed(name, RefreshFailure::aaa("version"), "판 넘침"))?;
        s.write(name, "codex", v, &value, Some(record.auth_generation()))
            .map_err(|e| failed(name, RefreshFailure::aaa("vault_write"), &e.message))?;
        tracing::info!(account = name, version = v, "제공자에게서 새 계정 인증을 받아 저장했다");
        Ok((v, response, RefreshOutcome::Renewed))
    }
}
/// 갱신 실패를 계정·까닭·단계·하위 오류와 함께 WARN으로 남긴다(사용자 10-23: 원인이 로그로 다 잡혀야 한다).
/// 값·본문은 넣지 않는다. 기록은 LogWriter 가림을 한 번 더 거친다
fn failed(account: &str, failure: RefreshFailure, detail: impl std::fmt::Display) -> RefreshOutcome {
    tracing::warn!(account, cause = failure.cause, stage = failure.stage, error = %detail, "계정 인증 갱신 실패");
    RefreshOutcome::Failed(failure)
}

/// 다시 로그인해야 풀리는 거부인가(Codex 0.167 manager.rs와 같은 분류): 401, 400 `invalid_grant`, refresh token의
/// 만료·재사용·폐기 코드. 그 밖의 400(요청 형식 등)은 사람이 다시 로그인해도 풀리지 않으므로 끊김으로 보지 않는다
fn permanent(status: u16, code: Option<&str>) -> bool {
    status == 401
        || (status == 400 && code.is_some_and(|c| c.eq_ignore_ascii_case("invalid_grant")))
        || matches!(code, Some("refresh_token_expired" | "refresh_token_reused" | "refresh_token_invalidated"))
}

/// 제공자 오류 본문의 오류 코드만 읽는다(Codex `TokenErrorDetail`과 같은 자리: `error` 글, `error.code`, `code`).
/// 설명·메시지는 읽지 않는다. 짧은 낱말 꼴이 아니면 버린다
pub(crate) fn provider_code(body: &[u8]) -> Option<String> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    let code = v
        .get("error")
        .and_then(|e| e.as_str())
        .or_else(|| v.get("error").and_then(|e| e.get("code")).and_then(|c| c.as_str()))
        .or_else(|| v.get("code").and_then(|c| c.as_str()))?;
    (!code.is_empty() && code.len() <= 64 && code.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b)))
        .then(|| code.to_string())
}

fn token_response(auth: &serde_json::Value) -> Result<String, ErrorBody> {
    let t = auth.get("tokens").and_then(|v| v.as_object()).ok_or_else(fail)?;
    let mut out = serde_json::Map::new();
    for k in ["access_token", "refresh_token", "id_token"] {
        if let Some(v) = t.get(k) {
            out.insert(k.into(), v.clone());
        }
    }
    serde_json::to_string(&out).map_err(|source_error| aaa_common::diagnostic::context(source_error, fail()))
}
fn validate(harness: &str, value: &str) -> Result<(), ErrorBody> {
    if value.len() < 12 || value.len() > MAX {
        return Err(fail());
    }
    match harness {
        "claude" if value.bytes().all(|b| b.is_ascii_graphic()) => Ok(()),
        "codex" => {
            let v: serde_json::Value = serde_json::from_str(value)
                .map_err(|source_error| aaa_common::diagnostic::context(source_error, fail()))?;
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
                    self.db
                        .del_meta(&format!("account-run:{}", binding.run))
                        .map_err(|source_error| aaa_common::diagnostic::context(source_error, fail()))?;
                }
                match other {
                    Err(e) => Err(e),
                    _ => Err(fail()),
                }
            }
        }
    }
    pub(crate) async fn account_refresh(self: Arc<Self>, mid: String, request: aaa_proto::AccountRefresh) {
        let _account_read = self.account_rename_gate.read().await;
        let bound = self
            .account_run(&request.run)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()
            .flatten()
            .filter(|r| r.machine == mid && r.pane.as_deref().is_none_or(|p| p == request.session))
            .map(|r| r.account);
        let account = bound.or_else(|| {
            request
                .run
                .split_once('/')
                .and_then(|(p, t)| {
                    self.load(p, t)
                        .inspect_err(|error| {
                            aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                        })
                        .ok()
                })
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
                // 갱신을 받지 못한 pane은 멈춘다: 알림·회복 소식에 그 thread를 싣는다(t-1688)
                let thread = request.run.split_once('/').map(|(_, t)| t);
                self.account_refresh_seen(&a, &outcome, request.version, thread);
                result
            }
            None => {
                tracing::warn!(machine = %mid, run = %request.run, "인증 갱신 요청의 계정 연결을 찾지 못했다");
                AccountRefreshed {
                    id: String::new(),
                    version: request.version,
                    response: None,
                    status: None,
                    code: None,
                }
            }
        };
        result.id = request.id;
        if let Ok(conn) = self.live_conn(&mid) {
            aaa_common::diagnostic::ignored(
                conn.request(aaa_proto::AgentRequest::AccountRefreshed { session: request.session, result }).await,
                "보조 작업 실패 뒤 기존 처리를 계속한다",
            );
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
    fn query_vault_does_not_replace_a_missing_execution_key_and_is_removed_with_account() {
        let state = Temp(std::env::temp_dir().join(format!("aaa-query-key-{}", util::new_id())));
        let accounts = Accounts::new(&state.0).unwrap();
        let execution = aaa_test_support::token("query-key-execution");
        accounts.import("work", "claude", &execution).unwrap();
        let generation = accounts.generation("work").unwrap();
        let raw = serde_json::json!({"access_token":aaa_test_support::token("query-key-access"), "refresh_token":aaa_test_support::token("query-key-refresh"), "expires_ms":util::now_ms()+3_600_000,"scopes":["user:profile"]}).to_string();
        std::fs::rename(state.0.join("secrets.key"), state.0.join("held.key")).unwrap();
        assert!(accounts.store_usage(&generation, &raw, None).is_err());
        assert!(!state.0.join("secrets.key").exists(), "교체 KEK를 생성하지 않는다");
        std::fs::rename(state.0.join("held.key"), state.0.join("secrets.key")).unwrap();
        accounts.store_usage(&generation, &raw, None).unwrap();
        assert_eq!(accounts.credential("work").unwrap().secret.0, execution);
        accounts.remove("work").unwrap();
        assert!(accounts.usage_credential(&generation).is_err());
    }

    #[test]
    fn legacy_vault_aad_reads_and_reseals_without_changing_auth_generation() {
        let state = Temp(std::env::temp_dir().join(format!("aaa-legacy-vault-{}", util::new_id())));
        util::ensure_private_dir(&state.0).unwrap();
        let accounts = Accounts::new(&state.0).unwrap();
        let value = &*aaa_test_support::token("legacy-auth");
        {
            let s = accounts.store.lock().unwrap();
            util::ensure_private_dir(&s.root).unwrap();
            let key = s.key(true).unwrap();
            let dek = random::<32>().unwrap();
            let nonce = random::<12>().unwrap();
            let wrap_nonce = random::<12>().unwrap();
            // A2 전 파일의 AAD 계약을 직접 고정한다. 새 aad()로 fixture를 만들지 않는다.
            let aad =
                b"aaa-account/v1\0legacy\0claude\0".iter().copied().chain(b"7".iter().copied()).collect::<Vec<_>>();
            let old = Sealed {
                name: "legacy".into(),
                harness: "claude".into(),
                version: 7,
                generation: None,
                nonce: hex::encode(nonce),
                wrapped_nonce: hex::encode(wrap_nonce),
                data: seal(&dek, nonce, &aad, value.as_bytes()).unwrap(),
                wrapped: seal(&key, wrap_nonce, &aad, &dek).unwrap(),
            };
            util::write_private(&s.path("legacy").unwrap(), &serde_json::to_vec(&old).unwrap()).unwrap();
        }
        let old = accounts.credential("legacy").unwrap();
        assert_eq!(old.secret.0, value);
        assert_eq!(old.auth_generation.as_deref(), Some("legacy-7"));
        {
            let s = accounts.store.lock().unwrap();
            let generation = Some(s.record("legacy").unwrap().auth_generation());
            s.write("legacy", "claude", 8, value, generation).unwrap();
        }
        assert_eq!(accounts.generation("legacy").unwrap(), "legacy-7", "암호문 판 교체는 로그인 교체가 아니다");
        assert_eq!(accounts.credential("legacy").unwrap().auth_generation, old.auth_generation);
        accounts.import("legacy", "claude", value).unwrap();
        assert_ne!(accounts.generation("legacy").unwrap(), "legacy-7", "다시 로그인은 새 세대");
    }
    #[test]
    fn encrypted_accounts_reject_tampering_and_lock_without_stopping_other_accounts() {
        let state = Temp(std::env::temp_dir().join(format!("aaa-vault-{}", util::new_id())));
        util::ensure_private_dir(&state.0).unwrap();
        let accounts = Accounts::new(&state.0).unwrap();
        // 경로용 시간 ID와 무관한 엔트로피를 쓴다. 공통 ID 접두부는 비밀 조각이 아니다.
        let value = aaa_test_support::fresh_token();
        let first = accounts.import("work", "claude", &value).unwrap();
        assert_eq!(first.version, 1);
        let generation = accounts.generation("work").unwrap();
        assert_eq!(accounts.credential("work").unwrap().auth_generation.as_deref(), Some(generation.as_str()));
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
        for field in ["name", "harness", "version", "data", "wrapped", "generation"] {
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
        assert_eq!(locked.import("work", "claude", &aaa_test_support::fresh_token()).unwrap().version, 2);
        assert_ne!(locked.generation("work").unwrap(), generation, "다시 로그인은 새 세대");
        locked.remove("work").unwrap();
        assert!(locked.credential("work").is_err());
        locked.import("work", "claude", &value).unwrap();
        assert_ne!(locked.generation("work").unwrap(), generation, "같은 별칭 삭제·재등록도 새 세대");
    }

    // 필요: 끊김(다시 로그인·선제 갱신 멈춤·자동 배정 제외)은 Codex가 영구 실패로 보는 거부에만 쓴다. 오류 코드는
    // 제공자 본문의 세 자리에서 짧은 낱말만 읽는다(설명·메시지는 읽지 않는다)(t-1688).
    #[test]
    fn only_refresh_token_rejections_are_permanent_and_only_the_code_is_read() {
        assert!(
            permanent(401, None)
                && permanent(400, Some("invalid_grant"))
                && permanent(403, Some("refresh_token_reused"))
        );
        assert!(!permanent(400, Some("invalid_request")) && !permanent(400, None) && !permanent(503, None));
        let code = |body: &str| provider_code(body.as_bytes());
        assert_eq!(code(r#"{"error":"invalid_grant","error_description":"x"}"#).as_deref(), Some("invalid_grant"));
        assert_eq!(
            code(r#"{"error":{"message":"m","code":"refresh_token_expired"}}"#).as_deref(),
            Some("refresh_token_expired")
        );
        assert_eq!(code(r#"{"code":"rate_limited"}"#).as_deref(), Some("rate_limited"));
        assert_eq!(code(r#"{"error":"has space"}"#), None);
        assert_eq!(code("<html>"), None);
    }

    // 필요: 11-09 회복 경로(t-1688). 다시 로그인으로 금고 판이 오르면 옛 판 pane은 요청 본문이 깨져 있어도
    // 제공자에게 묻지 않고 최신 인증을 받아야 한다(재시작 없이 회복). 지금 판의 깨진 본문은 거절한다.
    #[tokio::test]
    async fn an_old_version_is_served_before_the_request_body_is_read() {
        let state = Temp(std::env::temp_dir().join(format!("aaa-refresh-order-{}", util::new_id())));
        let accounts = Accounts::new(&state.0).unwrap();
        let auth = |n: &str| {
            serde_json::json!({"tokens":{"access_token":aaa_test_support::token(&format!("order-access-{n}")),"refresh_token":aaa_test_support::token(&format!("order-refresh-{n}"))}}).to_string()
        };
        accounts.import("work", "codex", &auth("1")).unwrap();
        accounts.import("work", "codex", &auth("2")).unwrap();
        let broken = SecretText(r#"{"client_id":"app_x","grant_type":***********}"#.into());
        let (served, outcome) = accounts.refresh("work", 1, &broken).await;
        assert_eq!((served.version, outcome), (2, RefreshOutcome::Served));
        let tokens: serde_json::Value = serde_json::from_str(&served.response.unwrap().0).unwrap();
        assert_eq!(tokens["access_token"], aaa_test_support::token("order-access-2"));
        let (failed, outcome) = accounts.refresh("work", 2, &broken).await;
        assert_eq!((failed.response, failed.status), (None, None), "aaa 안의 실패는 중계가 503으로 돌려준다");
        assert_eq!(outcome, RefreshOutcome::Failed(RefreshFailure::aaa("request_format")));
    }
}
