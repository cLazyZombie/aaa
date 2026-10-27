//! coordinator의 프로젝트 모드. harness 권한·safety 설정과 독립이다.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AutoMode {
    pub on: bool,
    #[ts(type = "number")]
    pub since_ms: u64,
    #[ts(type = "number")]
    pub ended_ms: u64,
    pub by: String,
    #[ts(type = "number")]
    pub run: u64,
    pub decisions: u32,
}

/// 질문 답 전달 전부터 보존한다. outcome=pending이면 전달 여부를 확인하고 자동 재전송하지 않는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AutoDecision {
    pub id: String,
    #[ts(type = "number")]
    pub run: u64,
    pub subject: String,
    pub question: crate::ThreadQuestion,
    pub choice: String,
    pub pick: Option<u32>,
    pub reason: String,
    pub action: String,
    /// 답을 바꿔도 이미 실행한 작업은 host가 되돌리지 않는다. 아직 알 수 없으면 null.
    pub reversible: Option<bool>,
    pub decided_by: String,
    #[ts(type = "number")]
    pub at_ms: u64,
    pub outcome: String,
    pub error: Option<String>,
    pub reviewed: bool,
    pub reconsideration: Option<String>,
    pub files_base: Option<String>,
}

/// `post user --data --topic auto-decision`의 JSON 본문.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct AutoDecisionNote {
    pub question: String,
    pub answer: String,
    pub reason: String,
    pub action: String,
    pub reversible: bool,
}

pub const AUTO_MODE_EXCEPTIONS: &str = "비밀값·암호·토큰 입력; 로그인·OAuth·키체인 해제·GUI·실기 확인·청취; 폴더 신뢰·도구 권한 확인창; 되돌릴 수 없는 삭제(미병합 branch·사용자 파일·uploads·미push 작업); force push·main 기록 수정·check-public 우회; safety·profile·권한·시스템 설정·~/.config/aaa·자동화 승인; 결제·구독·유료 한도; release 게시·외부 이슈 글·메일; host 재시작·사용자 기기 서비스 설치; [사람] 표지·사용자가 직접 정한다고 지정한 질문";

/// 구조화 질문의 사용자 전용 표지. 비밀값은 이 API에 넣지 않고 전용 가림 입력칸을 쓴다.
pub fn auto_question_exception(q: &crate::ThreadQuestion) -> bool {
    let text =
        format!("{}\n{}\n{}", q.text, q.body.as_deref().unwrap_or_default(), q.after.as_deref().unwrap_or_default());
    ["[사람]", "[비밀값]", "[암호]", "[사용자 전용]", "[확인창]"].iter().any(|mark| text.contains(mark))
}
