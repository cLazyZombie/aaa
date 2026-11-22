//! Front가 보는 머신·pane·배치·기기 타입.

use super::*;

// ───────────────────────── Front API ─────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum DeviceRole {
    /// 보기만 한다. 입력·resize·실행·업로드 불가
    Reader,
    /// 조작한다
    Controller,
    /// 같은 머신 사용자(UDS peer UID 확인)
    Local,
}

impl DeviceRole {
    pub fn can_control(self) -> bool {
        matches!(self, DeviceRole::Controller | DeviceRole::Local)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Connection {
    Connected,
    /// agent와 끊겼다. 세션은 계속 돌 수 있으나 aaa에서는 읽기 전용이다.
    Disconnected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MachineView {
    pub id: String,
    pub name: String,
    pub local: bool,
    pub connection: Connection,
    pub os: Option<String>,
    pub boot_id: Option<String>,
    pub dev_mode: bool,
    #[ts(type = "number | null")]
    pub last_seen_ms: Option<u64>,
    /// 마지막 건강 보고(§8.10). 연결 중이 아니면 오래된 값이다.
    pub health: Option<MachineHealth>,
    /// agent 시계 - host 시계(보고를 받은 때 기준)
    #[ts(type = "number | null")]
    pub clock_skew_ms: Option<i64>,
    /// 새 세션을 거부하는 이유(연결 중일 때만). 비었으면 정상이다.
    pub problems: Vec<HealthProblem>,
    /// 마지막 hello의 CPU 종류(`aarch64`·`x86_64`). 아래 넷과 같이 1.88부터 싣고, 옛 host·아직 붙지 않은 머신은 생략한다(plan §8.372)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub arch: Option<String>,
    /// 마지막 hello의 aaa 판(`aaa --version`의 판 글)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub software: Option<String>,
    /// 마지막 hello의 실행 파일 SHA-256(소문자 hex). 옛 agent는 보내지 않는다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sha256: Option<String>,
    /// 판이 host와 다르다(판을 모르면 false)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(optional, as = "Option<bool>")]
    pub software_differs: bool,
    /// 이 host 실행 중 마지막 연결이 알린 agent 기능(host 재시작 뒤 다시 붙기 전에는 비었다)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<String>>")]
    pub features: Vec<String>,
    /// 마지막 연결의 시작 자가 점검(§10.4 6) 요약. 점검이 hello보다 늦게 끝났으면 다음 연결 때 온다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub selfcheck: Option<MachineSelfcheck>,
    /// 지금 걸린 독점 예약(FRONT_API 1.148). 끝났거나 시한이 지난 예약은 싣지 않는다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reservation: Option<MachineReservation>,
    /// 사람이 정한 자동 배정 비중(FRONT_API 1.148). 없으면 건강 보고의 CPU 수를 쓴다. 0이면 자동 배정에서 뺀다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub weight: Option<u32>,
}

/// 머신 독점 예약(FRONT_API 1.148). 주인이 아닌 시작은 그 머신에 자동 배정하지 않고, 그 머신을 지정한 시작은 풀릴 때까지 기다린다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MachineReservation {
    pub project: String,
    /// 주인: `t-NNNN` thread 또는 `d-NNNN` 할 일
    pub holder: String,
    /// 주인이 할 일이면 그 ULID
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub note: Option<String>,
    /// 저절로 풀리는 시각(없으면 주인이 끝나거나 풀 때까지)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub until_ms: Option<u64>,
    /// 건 쪽: `사용자`·`coordinator`·`t-NNNN`(자기 머신을 건 thread)
    pub by: String,
    /// coordinator가 인용한 사용자 지시
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub user_request: Option<String>,
    #[ts(type = "number")]
    pub at_ms: u64,
}

/// agent 시작 자가 점검 요약(hello `selfcheck`에서 host가 읽은 것). 실패 이유는 agent가 쓴 글 그대로다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MachineSelfcheck {
    pub pass: bool,
    #[serde(default)]
    pub reasons: Vec<String>,
}

/// agent가 주기적으로 보내는 머신 건강(§8.10)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MachineHealth {
    /// 표시 전용 시스템 정보. 옛 agent는 생략하며 판 조회 실패로 세션을 막지 않는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub info: Option<MachineInfo>,
    /// agent 시계
    #[ts(type = "number")]
    pub at_ms: u64,
    /// 작업 root가 있는 디스크의 여유·전체(알 수 없으면 None)
    #[ts(type = "number | null")]
    pub disk_free_bytes: Option<u64>,
    #[ts(type = "number | null")]
    pub disk_total_bytes: Option<u64>,
    /// 1분 평균 부하 ×100과 CPU 수
    pub load_x100: Option<u32>,
    pub cpus: u32,
    /// 직전 표본부터의 CPU 사용률(0~100, 모든 코어 평균). 옛 agent는 보내지 않는다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cpu_pct: Option<u8>,
    /// 사용 메모리와 전체 바이트. 사용 = macOS 앱(internal−purgeable)+wired+압축, Linux 전체−MemAvailable, Windows 전체−ullAvailPhys
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub mem_used_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub mem_total_bytes: Option<u64>,
    /// 전원에 연결됐는가(배터리가 없는 머신은 true, 알 수 없으면 None)
    pub on_ac: Option<bool>,
    pub toolchain: Vec<ToolCheck>,
    /// agent가 본 이상(디스크 부족·toolchain 실패). 새 세션을 거부한다.
    pub problems: Vec<HealthProblem>,
    /// 거부하지는 않는 주의(배터리 전원 등)
    pub warnings: Vec<String>,
}

/// agent가 10분마다 수집한 표시 전용 정보. 실패한 값은 None이다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MachineInfo {
    #[ts(type = "number")]
    pub at_ms: u64,
    pub os_version: Option<String>,
    pub cpu_model: Option<String>,
    pub claude: Option<String>,
    pub codex: Option<String>,
    pub installation: Option<String>,
    pub work_root: String,
}

/// 건강 이상 하나. `code`는 바뀌지 않는 종류(`disk`·`toolchain:<이름>`·`clock`)이고 `text`는 지금 값이 든 설명이다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct HealthProblem {
    pub code: String,
    pub text: String,
}

/// agent.toml `[health.toolchain]`의 점검 명령 하나
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ToolCheck {
    pub name: String,
    pub ok: bool,
    /// 출력 첫 줄(실패면 이유)
    pub version: String,
}

/// pane 조작 lease(대화형 front 하나가 키 입력과 PTY 크기를 정한다). 연결이 끊기면 풀린다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LeaseView {
    /// 보유한 front 연결 ID(front는 Welcome·attach 때 받은 자기 ID와 비교한다)
    pub front: String,
    /// 사람이 읽는 이름(예: `웹(ab12)`)
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PaneView {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub responsibility: Option<Responsibility>,
    /// host에 고정한 인증 별칭. 이전 판 thread의 인증은 ThreadView.account로 확인한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub account: Option<String>,
    pub id: String,
    pub machine_id: String,
    pub machine: String,
    pub name: String,
    pub argv: Vec<String>,
    pub cwd: Option<String>,
    #[serde(flatten)]
    pub state: SessionState,
    pub connection: Connection,
    /// 지금 입력할 수 없으면 true(끊김·종료·권한). lease는 따로 본다.
    pub readonly: bool,
    pub cols: u16,
    pub rows: u16,
    pub lease: Option<LeaseView>,
    pub role: Option<Role>,
    /// host 머신의 프로젝트 홈에서 도는 coordinator의 소속. host가 canonical cwd로 판정한다.
    /// 옛 host는 보내지 않으며 front는 이름·경로로 소속을 추측하지 않는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub coordinator_project: Option<String>,
    /// harness 상태(hook을 보내는 agent 세션만)
    pub agent: Option<Box<AgentStatus>>,
    /// 사람이 붙인 이름이 있으면 그 이름(`name`과 같다). 웹 메뉴가 「이름 지우기」를 보일지 정한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub label: Option<String>,
    /// 재부팅 뒤 다시 띄우지 못한 까닭(herdr `restore_error`). 그 pane은 중단으로 남는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub restore_error: Option<String>,
    /// 이 pane이 대신한 옛 pane ID(`pane resume`·재부팅 복원은 새 세션을 띄우고 옛 기록을 지운다, 오래된 것부터). 옛 ID를 보던
    /// 화면은 이것으로 새 pane을 따라간다(QA Q19).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<String>>", optional)]
    pub replaced: Vec<String>,
}

// ───────────────────────── layout(host 정본, §4.4) ─────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum SplitDir {
    /// 좌우로 나눈다(a가 왼쪽)
    Horizontal,
    /// 위아래로 나눈다(a가 위)
    Vertical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "node", rename_all = "snake_case")]
pub enum LayoutNode {
    Pane {
        pane: String,
    },
    Split {
        dir: SplitDir,
        /// a 쪽 비율(천분율, 1..=999)
        ratio: u16,
        a: Box<LayoutNode>,
        b: Box<LayoutNode>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Tab {
    pub id: String,
    pub name: String,
    pub root: LayoutNode,
    /// pane 하나를 탭 가득 보인다(herdr `Tab.zoomed`). 보이는 pane은 front마다 그 front의 포커스 pane이다.
    /// 나누기·닫기·옮기기로 탭의 pane이 바뀌면 풀린다.
    #[serde(default)]
    pub zoomed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Workspace {
    pub name: String,
    pub tabs: Vec<Tab>,
}

/// layout 바꾸기. 새 pane이 필요한 동작은 host가 띄운 뒤 layout에 넣는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "do", rename_all = "snake_case")]
pub enum LayoutChange {
    /// 새 탭에 새 pane. `name`은 pane(세션) 이름, `label`은 탭 이름(없으면 자리 번호).
    /// cwd를 주지 않으면 host `[terminal] new_cwd`를 따른다(follow면 `from` pane이 지금 있는 폴더, plan §8.76).
    /// `cols`·`rows`는 화면이 붙기 전 처음 PTY 크기(기본 120×32)
    NewTab {
        machine: Option<String>,
        name: Option<String>,
        argv: Vec<String>,
        cwd: Option<String>,
        #[serde(default)]
        cols: Option<u16>,
        #[serde(default)]
        rows: Option<u16>,
        #[serde(default)]
        label: Option<String>,
        #[serde(default)]
        from: Option<String>,
    },
    /// pane 자리를 나눠 새 pane을 b 쪽에 둔다. `ratio`는 a 쪽 천분율(기본 500, herdr `pane split --ratio`).
    /// cwd를 주지 않으면 `[terminal] new_cwd`(follow면 나눈 pane의 폴더)
    Split {
        pane: String,
        dir: SplitDir,
        machine: Option<String>,
        argv: Vec<String>,
        cwd: Option<String>,
        #[serde(default)]
        ratio: Option<u16>,
    },
    /// 이미 있는 pane을 새 탭으로 넣는다
    AddTab { pane: String },
    /// layout에서 뺀다(kill이면 세션도 끝낸다)
    Close { pane: String, kill: bool },
    /// pane 가장자리를 그 방향으로 옮긴다(herdr `pane resize`): 그쪽 가장자리의 나눔(없으면 반대쪽)의 비율을
    /// 오른쪽·아래면 늘리고 왼쪽·위면 줄인다. `amount`는 천분율(기본 50, 최대 500), 비율은 100..=900으로 묶는다.
    Resize { pane: String, direction: PaneDirection, amount: Option<u16> },
    /// 탭 이름을 바꾼다
    RenameTab { tab: String, name: String },
    /// 탭을 닫는다. 그 안 pane도 끝낸다(herdr `tab close`)
    CloseTab { tab: String },
    /// pane을 지금 자리에서 빼 새 탭에 둔다(herdr `pane move --new-tab`)
    MoveToNewTab { pane: String, name: Option<String> },
    /// 탭 가득 보이기(herdr `pane zoom`). `zoomed`가 없으면 뒤집는다. pane이 하나뿐인 탭은 그대로다(plan §8.91)
    Zoom {
        pane: String,
        #[serde(default)]
        zoomed: Option<bool>,
    },
    /// 같은 탭의 두 pane 자리를 바꾼다(herdr `pane swap`의 source·target 꼴). 칸과 비율은 그대로다.
    /// 방향 이웃은 화면 크기를 아는 front가 찾는다(host는 front마다 다른 칸을 모른다).
    Swap { pane: String, with: String },
    /// 나눔 하나의 비율을 바로 정한다(herdr `layout.set_split_ratio`, 나눔 경계 끌기). `path`는 탭 뿌리에서 그 나눔까지
    /// (false = a, true = b), `ratio`는 a 쪽 천분율이고 100..=900으로 묶는다(plan §8.106)
    SplitRatio { tab: String, path: Vec<bool>, ratio: u16 },
    /// 탭을 `index` 앞으로 옮긴다(herdr `tab.move`의 `insert_index`: 0..=탭 수, 옮기기 전 차례로 센다)
    MoveTab { tab: String, index: u16 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PendingEnroll {
    pub name: String,
    pub machine_id: String,
    /// 상대 인증서 fingerprint 앞부분(참고용). 확인 코드는 agent 화면에만 보인다.
    pub fingerprint_prefix: String,
    #[ts(type = "number")]
    pub since_ms: u64,
}

/// SSH 머신 추가 작업 하나(웹 머신 추가 M2, plan §8.384). host DB에 남아 웹 창을 닫았다 열어도 같은 작업을 본다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MachineSetupView {
    pub job: String,
    /// 사람이 넣은 SSH 대상(별칭 또는 `user@host:port`)
    pub target: String,
    pub phase: MachineSetupPhase,
    /// `ssh -G`로 푼 접속(사용자·주소·포트, ProxyJump를 거치는가)
    pub ssh: Option<SshResolved>,
    /// 접속 때 받은 host key(인증 전). 사람이 지문을 보고 믿는다
    pub host_keys: Vec<HostKeyView>,
    /// 받은 키가 이미 믿는 키인가: `known` | `new` | `changed`(fail-closed)
    pub host_key_state: Option<String>,
    pub detected: Option<MachineDetected>,
    /// 실패·멈춤의 까닭(사람이 읽는 글)
    pub error: Option<String>,
    #[ts(type = "number")]
    pub created_ms: u64,
    #[ts(type = "number")]
    pub updated_ms: u64,
    /// 사람이 넣은 설치 설정(M3, plan §8.420). 옛 host는 생략한다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub settings: Option<MachineSetupSettings>,
    /// 원격 install이 낸 계획 요약(사람이 확인한다). 경로·계정이 든 계획 원문은 host 작업 폴더에만 둔다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, as = "Option<MachineSetupReview>")]
    pub review: Option<Box<MachineSetupReview>>,
    /// 원격 install 적용의 진행과 결과
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, as = "Option<MachineSetupInstall>")]
    pub install: Option<Box<MachineSetupInstall>>,
}

/// 새 머신의 install 설정(웹 폼). host가 install choices(role agent·등록 grant)로 옮긴다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MachineSetupSettings {
    /// 새 머신 이름(host 규칙, 이미 있는 이름은 거부)
    pub name: String,
    /// 새 머신이 닿을 host agent 연결 주소(`enroll_guide` 후보 중 하나)
    pub host: String,
    /// 작업 root(worktree 위치, 원격 절대 경로)
    pub work_root: String,
    /// 작업 root가 있는 외장 디스크 mount(비우면 내장 디스크)
    pub work_mount: Option<String>,
    /// 그 머신에서 쓸 harness(비우면 none)
    pub harness: Vec<crate::install::InstallHarness>,
    pub login: crate::install::InstallLogin,
}

/// 원격 `install-probe`가 낸 계획의 공개 요약
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MachineSetupReview {
    /// 사람이 확인하는 계획 hash(적용 요청에 그대로 실어 보낸다)
    pub plan_sha256: String,
    /// 보낸 aaa 실행 파일(원격에서 같은 SHA·판을 확인했다)
    pub binary_sha256: String,
    pub binary_version: String,
    /// 바꿀 것·검사·사람이 할 일(install 공개 결과 그대로)
    pub changes: Vec<crate::install::InstallChange>,
    pub checks: Vec<crate::install::InstallCheck>,
    pub needs_user: Vec<crate::install::InstallNeedsUser>,
    /// 승인 범위(operation 이름만, 원격 경로는 싣지 않는다)
    pub operations: Vec<String>,
    /// Windows 서비스 암호의 전용 가림 입력을 보인다. 값은 이 요약·작업에 절대 싣지 않는다.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(optional, as = "Option<bool>")]
    pub service_password: bool,
}

/// 웹 확인 창 → host 메모리 → SSH stdin 전용. Debug는 가리고 사용한 소유 버퍼는 지운다.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(type = "string")]
pub struct MachineSetupPassword(pub String);

impl std::fmt::Debug for MachineSetupPassword {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[비밀]")
    }
}

impl Drop for MachineSetupPassword {
    fn drop(&mut self) {
        // SAFETY: 소유한 UTF-8 저장소를 지운 뒤 문자열을 더 읽지 않는다.
        for byte in unsafe { self.0.as_bytes_mut() } {
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MachineSetupInstall {
    pub installation_id: Option<String>,
    /// 마지막 진행 사건(사람이 읽는 글)
    pub last_event: Option<String>,
    /// 끝난 install 결과(공개 envelope)
    pub result: Option<crate::install::InstallResult>,
    /// 등록된 머신 이름(grant 등록 뒤)
    pub machine: Option<String>,
    /// 끝 확인 shell pane이 적은 줄(작업 폴더·git·harness·PATH)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<String>>")]
    pub checks: Vec<String>,
    /// 끝 확인 pane(사람이 열어 본다)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub check_pane: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum MachineSetupPhase {
    /// 접속해 host key를 받는 중
    Connecting,
    /// 처음 보는 host key: 사람이 지문을 확인한다
    Trust,
    /// 믿는 키로 로그인해 OS·CPU를 읽는 중
    Detecting,
    /// 감지가 안 됐다: 사람이 OS·CPU를 고른다
    Choose,
    /// 감지(또는 고름) 끝, 지원하는 대상이다. 사람이 설치 설정을 넣는다
    Ready,
    /// 감지했지만 release 대상이 없다(예: Windows ARM64)
    Unsupported,
    Failed,
    /// aaa를 보내 확인하고 원격 install이 계획을 낸다(M3)
    Planning,
    /// 계획이 나왔다: 사람이 바꿀 것을 보고 「설치하고 등록」을 누른다
    Review,
    /// 서명 승인 → 원격 install 적용·등록 중
    Installing,
    /// install이 사람 일을 남겼다(그 머신에서 하고 새로 시작한다)
    NeedsUser,
    /// 설치·등록 끝
    Installed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SshResolved {
    pub user: String,
    pub host: String,
    pub port: u16,
    pub proxy_jump: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct HostKeyView {
    /// `ssh-ed25519` 등
    pub kind: String,
    /// `SHA256:<base64>`(ssh-keygen -l와 같은 꼴)
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MachineDetected {
    /// `macos` | `linux` | `windows`(MachineView.os와 같은 말)
    pub os: String,
    /// `aarch64` | `x86_64`
    pub arch: String,
    /// 원격이 말한 그대로(`Darwin arm64`, `Windows AMD64`). 사람이 고른 것이면 빈 글
    pub raw: String,
    /// 사람이 골랐다(감지 실패)
    pub chosen: bool,
    pub home: Option<String>,
    pub shell: Option<String>,
    /// 원격에 이미 있는 aaa 판(`aaa --version` 첫 줄)
    pub aaa: Option<String>,
}

/// 「직접 설치」 안내(plan §8.383): 새 머신이 host agent 연결에 닿을 주소 후보와, 새 머신 화면에 나올 host 지문
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EnrollGuide {
    /// host가 실제로 연 agent 연결 주소(`agent_listen`). 닫혀 있으면 None이고 새 머신은 붙지 못한다
    pub listen: Option<String>,
    /// 새 머신에서 칠 `<주소>:<포트>` 후보. 앞이 추천이다. 빈 목록이면 host 밖에서 닿을 주소가 없다(닫힘·loopback)
    pub addresses: Vec<EnrollAddress>,
    /// host 인증서 SHA-256 전체(소문자 hex). 새 머신 `aaa agent enroll` 화면은 앞 16자를 보인다
    pub host_fingerprint: String,
    /// host 판(`aaa --version`). 새 머신에 같은 판을 넣는다
    pub software: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EnrollAddress {
    pub address: String,
    /// 어디서 온 후보인가: `tailnet`(tailscale 이름)·`listen`(연 주소 그대로)·`hostname`(이 머신 이름)
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PublishState {
    /// missing | stopped | running | error
    pub tailscale: String,
    pub dns_name: Option<String>,
    /// aaa 웹을 이미 proxy하는 HTTPS 주소
    pub url: Option<String>,
    /// aaa가 만든 serve 포트(끌 때 이 포트만 끈다)
    pub owned_port: Option<u16>,
    pub web_port: u16,
    pub message: Option<String>,
}

/// pairing한 웹 기기 하나(herdr-web-ui `PairedDevice`). 「이 기기」 표시는 웹이 `/api/v1/me`의 id로 정한다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DeviceView {
    pub id: String,
    pub label: String,
    /// reader 또는 controller
    pub role: DeviceRole,
    #[ts(type = "number")]
    pub created_at_ms: u64,
    /// 마지막으로 들어온 때(1분 단위로 적는다). 아직 한 번도 안 들어왔으면 없다
    #[ts(type = "number | null")]
    pub last_seen_ms: Option<u64>,
    /// 지금 공개 주소의 passkey가 있다(관리 GUI W7). 옛 host는 보내지 않는다
    #[serde(default)]
    pub passkey: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct HostStatus {
    pub host_id: String,
    pub software: String,
    pub proto: Version,
    pub dev_mode: bool,
    pub agent_listen: Option<String>,
    pub web_listen: Option<String>,
    pub machines: Vec<MachineView>,
}
