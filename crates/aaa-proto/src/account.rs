//! 계정 인증은 전용 요청에서만 전송한다. 일반 실행 spec·journal에는 참조만 둔다.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(type = "string")]
pub struct SecretText(pub String);

impl std::fmt::Debug for SecretText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[비밀]")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AccountView {
    pub name: String,
    pub harness: String,
    #[ts(type = "number")]
    pub version: u64,
    /// 공식 CLI가 로그인 때 알린 메일(FRONT_API 1.79, 관리 GUI W5a). 조작 기기와 host의 세션 밖 CLI에만 싣는다. 모르면 생략
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub email: Option<String>,
    /// 구독 plan(Codex 인증의 계정 정보). 메일과 같은 곳에만 싣는다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub plan: Option<String>,
    /// aaa가 처음 기록한 인증 시각(ms). 이 기록 전에 등록한 계정은 생략(모름)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub first_auth_ms: Option<u64>,
    /// 마지막으로 로그인에 성공한 시각(ms)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub last_auth_ms: Option<u64>,
    /// 인증에 적힌 만료 시각(ms). 공식 CLI가 알린 것만(Claude `setup-token`의 유효 기간). 모르면 생략
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub expires_ms: Option<u64>,
    /// 인증 상태. 옛 host는 생략한다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub health: Option<AccountHealth>,
}

/// 계정 인증 상태(plan §8.351). 판정은 실제 쓰임(Codex 갱신 거부)과 알려진 만료 시각뿐이다: 제공자에게 따로 묻지 않는다
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AccountHealth {
    /// 거부·만료를 본 적 없다(만료 시각을 모르면 이것이다)
    Ok,
    /// 만료 7일 전부터
    Expiring,
    /// 끊김: 만료 시각이 지났다
    Expired,
    /// 끊김: 제공자가 인증 갱신을 거부했다
    Rejected,
}

impl AccountHealth {
    pub fn broken(self) -> bool {
        matches!(self, AccountHealth::Expired | AccountHealth::Rejected)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountCredential {
    pub run: String,
    pub account: String,
    pub harness: String,
    pub version: u64,
    pub secret: SecretText,
    /// host가 허용한 thread 자기 폴더만 미리 신뢰한다. 생략하면 사용자 신뢰 창을 유지한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trust_cwd: Option<String>,
    /// 로그인 세대(A2). 옛 pane은 귀속 미확인으로 남긴다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_generation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountRefresh {
    pub run: String,
    pub session: String,
    pub id: String,
    pub version: u64,
    pub request: SecretText,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountRefreshed {
    pub id: String,
    pub version: u64,
    pub response: Option<SecretText>,
}

/// 인증값을 포함하지 않는 harness별 기본 설정.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AccountDefault {
    pub harness: String,
    pub account: String,
}

/// harness별 자동 배정에 사용하는 계정. 그룹·모델은 구판 자료 호환용이다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AccountPoolMember {
    pub account: String,
    /// 구판 호환용. 새 host는 계정 이름으로 정규화한다.
    #[serde(default)]
    pub group: String,
    /// 구판 호환용. 새 host는 모든 모델을 허용한다.
    #[serde(default)]
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AccountPoolView {
    pub revision: String,
    pub members: Vec<AccountPoolMember>,
    /// 구판 호환용 빈 목록. 프로젝트별 참여 설정은 사용하지 않는다.
    #[serde(default)]
    pub projects: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AccountTaskSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl AccountTaskSize {
    pub fn percent(self) -> u8 {
        match self {
            Self::Small => 5,
            Self::Medium => 15,
            Self::Large => 30,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AccountSelection {
    pub size: AccountTaskSize,
    pub group: String,
    pub unobserved: bool,
    pub reason: String,
    #[ts(type = "number")]
    pub selected_at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AccountStartState {
    Queued,
    Started,
    Failed,
    Cancelled,
}

/// 대기 작업의 원문·인증·사용자 신원은 조회 응답에 넣지 않는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AccountStartView {
    pub id: String,
    pub project: String,
    pub title: String,
    pub size: AccountTaskSize,
    pub state: AccountStartState,
    pub thread: Option<String>,
    pub reason: String,
    #[ts(type = "number")]
    pub created_at_ms: u64,
}

/// 공식 로그인 자식의 원본 출력은 공개하지 않는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AccountLoginView {
    /// 원격 어댑터의 고정 오류 문구. CLI 원문·인증 자료는 담지 않는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub error: Option<String>,
    pub id: String,
    pub account: String,
    pub harness: String,
    /// waiting·succeeded·failed·cancelled, 그리고 `mismatch`(다시 로그인에 다른 메일이 들어왔다: `account_login_confirm`을 기다린다)·
    /// `kept`(바꾸지 않고 옛 인증을 남겼다)
    pub state: String,
    /// 다시 로그인할 때 그 별칭에 기록된 메일(「이 메일로 로그인하세요」, FRONT_API 1.79). 조작 기기·세션 밖 CLI에만
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub expect_email: Option<String>,
    /// 로그인한 메일(끝난 뒤, 공식 CLI가 알렸을 때)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub email: Option<String>,
}

/// 시작한 조작 기기에만 전송하는 단기 인증 자료. Debug에는 실제 값이 없다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AccountLoginPrompt {
    /// device_code(Codex), paste_code(Claude).
    pub kind: String,
    pub url: SecretText,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub user_code: Option<SecretText>,
}

/// coordinator는 thread와 다른 이름 공간을 쓴다. 경로 탈출은 양쪽 모두 거부한다.
pub fn account_run_parts(run: &str) -> Option<(&str, &str)> {
    let (project, task) = run.split_once('/')?;
    let coordinator = task
        .strip_prefix("c-")
        .is_some_and(|id| id.len() == 26 && id.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()));
    (crate::valid_project(project) && (crate::valid_thread(task) || coordinator)).then_some((project, task))
}
