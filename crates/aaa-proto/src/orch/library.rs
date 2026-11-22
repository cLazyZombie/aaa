//! 보고서·library 사본과 정리 대상 타입.

use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LibFile {
    pub path: String,
    #[ts(type = "number")]
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Skipped {
    pub path: String,
    pub reason: String,
    /// 복사 상한을 넘어 agent에 남긴 일반 파일의 크기(이것만 나중에 지연 스트리밍으로 볼 수 있다)
    #[serde(default)]
    #[ts(type = "number | null")]
    pub lazy_size: Option<u64>,
}

/// thread 결과물 파일 하나(host 사본 또는 agent에 남긴 것)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LibraryFile {
    /// library 안 상대 경로
    pub path: String,
    #[ts(type = "number")]
    pub size: u64,
    /// host에 사본이 없다: 볼 때 agent에서 스트리밍한다(작업 폴더가 남아 있을 때만)
    pub on_agent: bool,
    /// 파일 보기가 고를 MIME·종류(파일 보기 `FileInfo`와 같은 표). 옛 목록 파일에는 없어 host가 줄 때 채운다
    #[serde(default)]
    pub mime: String,
    #[serde(default)]
    pub kind: crate::msg::FileKind,
}

/// 웹에서 보는 보고서·결과물. `base`는 짧은 수명의 읽기 전용 경로(`/v/<cap>/`)이고 그 아래에 `path`를 붙인다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LibraryView {
    /// FRONT_API 1.95. 같은 이름·thread의 보고 판(최신 번호부터).
    #[serde(default)]
    pub versions: Vec<String>,
    /// FRONT_API 1.95. 보고의 특정 판을 여는 프로젝트별 영속 번호(r-0001부터).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub report_id: Option<String>,

    pub project: String,
    pub thread: String,
    /// 회수한 report.md(없으면 None)
    pub report: Option<String>,
    pub files: Vec<LibraryFile>,
    pub base: String,
    #[ts(type = "number")]
    pub expires_at_ms: u64,
    /// FRONT_API 1.36. 조작 권한에만 보이는 보고서 공유 링크. 앱 안 보기는 만료·폐기와 무관하다.
    #[serde(default)]
    pub link: Option<LinkView>,
    /// FRONT_API 1.56. 보고서를 낸 thread(`LibraryReport.from_thread`와 같다). 열람 머리에 보인다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub from_thread: Option<String>,
}

/// FRONT_API 1.36. 보고서 하나의 host 사본만 읽는 서명 링크(§8.197).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LinkView {
    /// FRONT_API 1.95. 보고의 특정 판을 여는 프로젝트별 영속 번호(r-0001부터).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub report_id: Option<String>,

    pub id: String,
    pub project: String,
    pub thread: String,
    pub base: String,
    /// 공개했을 때만 있는 절대 HTTPS 주소. loopback은 밖 알림에 싣지 않는다.
    pub url: Option<String>,
    #[ts(type = "number")]
    pub created_at_ms: u64,
    #[ts(type = "number")]
    pub expires_at_ms: u64,
    #[ts(type = "number | null")]
    pub last_opened_at_ms: Option<u64>,
    pub revoked: bool,
    /// host에 회수한 일반 파일의 합계 바이트
    #[ts(type = "number")]
    pub size: u64,
}

/// FRONT_API 1.38. thread 결과물과 별도로 올린 보고서의 전체 목록 한 줄.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LibraryReport {
    /// FRONT_API 1.122. 연결된 할 일 저장소 이름, 없으면 원본 thread 저장소의 마지막 이름. 모르면 생략한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub repo: Option<String>,

    /// FRONT_API 1.114. 보관함의 불변 판. 자동 정리와 상한 계산에서 제외한다.
    #[serde(default)]
    pub archived: bool,
    /// FRONT_API 1.95. 특정 판 사본의 보존 용량. size는 기존 최신 사본 크기다.
    #[serde(default)]
    #[ts(type = "number")]
    pub history_size: u64,
    /// FRONT_API 1.95. 같은 이름·thread의 보고 판(최신 번호부터).
    #[serde(default)]
    pub versions: Vec<String>,
    /// FRONT_API 1.95. 보고의 특정 판을 여는 프로젝트별 영속 번호(r-0001부터).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub report_id: Option<String>,

    pub project: String,
    pub name: String,
    #[ts(type = "number")]
    pub size: u64,
    #[ts(type = "number")]
    pub updated_at_ms: u64,
    pub live: bool,
    /// 읽기 실패·상한 초과도 목록에서 감추지 않는다.
    pub problem: Option<String>,
    pub link: Option<LinkView>,
    /// FRONT_API 1.43. 완료 보고 또는 사용자에게 보낸 보고서(「내 보고서」). 자동 표시는 같은 사본을 쓴다.
    #[serde(default)]
    pub to_user: bool,
    /// FRONT_API 1.43. `--to-user`로 보낼 때 준 제목
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub title: Option<String>,
    /// FRONT_API 1.43. 사용자 보고서를 보낸(다시 보낸) 뒤 아직 열지 않았다
    #[serde(default)]
    pub unseen: bool,
    /// FRONT_API 1.43. 진행 중 thread 또는 완료 보관이 참조하는 사본이라 지울 수 없다
    #[serde(default)]
    pub busy: bool,
    /// FRONT_API 1.56. 보고서를 낸 thread(`t-NNNN`, 사용자 요청 10-20). 근원을 모르면 없다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub from_thread: Option<String>,
}

/// 보고서 하나(프로젝트·이름). FRONT_API 1.43 `LibraryDelete`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LibraryRef {
    pub project: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LibrarySkip {
    pub name: String,
    pub reason: String,
}

/// report.md 본문과 library 목록(파일 본문은 `FileRead`로 나눠 받는다)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportSnapshot {
    pub hash: String,
    pub report: B64,
    pub files: Vec<LibFile>,
    pub skipped: Vec<Skipped>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cleanup {
    pub worktree_removed: bool,
    pub branch_deleted: bool,
    pub notes: Vec<String>,
}

/// cleanup_probe.v1: 같은 저장소에서 정리할 자리·branch의 부재를 묶어 확인한다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanupTarget {
    pub cwd: String,
    pub branch: Option<String>,
}

/// FRONT_API 1.40. 자동 정리와 dry-run이 공유하는 판정 결과(크기는 실시간 원본 제외).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LibraryPrunePlan {
    /// FRONT_API 1.114. 상한 밖에서 따로 보존하는 사본 용량.
    #[serde(default)]
    #[ts(type = "number")]
    pub archived_size: u64,
    #[ts(type = "number")]
    pub size: u64,
    #[ts(type = "number")]
    pub remaining_size: u64,
    #[ts(type = "number")]
    pub protected_size: u64,
    #[ts(type = "number")]
    pub max_size: u64,
    #[ts(type = "number")]
    pub max_days: u64,
    pub candidates: Vec<LibraryPruneEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LibraryPruneEntry {
    pub project: String,
    pub name: String,
    #[ts(type = "number")]
    pub size: u64,
    /// 「기한」 또는 「용량」. 기한 정리 뒤 용량을 판정한다.
    pub reason: String,
}
