//! 보고 표지는 알림 의도만 나타낸다. thread 묶음과 진행률은 기존 규칙으로 셈한다.
use crate::{Host, orch::ThreadRec};
use aaa_proto::ReportAttention;
use sha2::{Digest, Sha256};

pub(crate) fn marker(report: &str) -> Option<&'static str> {
    let first = report.trim_start_matches('\u{feff}').lines().next()?;
    // 새 안내는 [확인 필요]를 쓴다. 이미 회수한 보고의 옛 표지는 입력 호환용으로만 받는다.
    ["[완료]", "[결정 필요]", "[검수 대기]", "[확인 필요]", "[사람 필요]", "[착지]", "[진행 중]"]
        .into_iter()
        .find(|m| first == *m || first.strip_prefix(m).is_some_and(|rest| rest.starts_with(char::is_whitespace)))
}

/// 사람을 기다리는 표지인가(알림 의도 「확인 필요」). 질문 카드(`## Questions`)도 이 보고에서만 읽는다(plan §8.329)
pub(crate) fn needs_user(report: &str) -> bool {
    matches!(marker(report), Some("[결정 필요]" | "[검수 대기]" | "[확인 필요]" | "[사람 필요]"))
}

impl Host {
    /// 사본 교체와 DB 갱신 사이의 다른 보고를 알림 의도로 읽지 않는다.
    pub(crate) fn verified_report(&self, r: &ThreadRec) -> Option<String> {
        let dir = self.project_dir(&r.project).ok()?;
        let bytes = std::fs::read(dir.join("threads").join(format!("{}.md", r.id))).ok()?;
        if Some(hex::encode(Sha256::digest(&bytes))) != r.report_hash {
            return None;
        }
        String::from_utf8(bytes).ok()
    }

    pub(crate) fn report_marker(&self, r: &ThreadRec) -> Option<&'static str> {
        marker(&self.verified_report(r)?)
    }

    pub(crate) fn report_attention(&self, r: &ThreadRec) -> Option<ReportAttention> {
        let report = self.verified_report(r)?;
        if needs_user(&report) {
            return Some(ReportAttention::NeedsUser);
        }
        (marker(&report) == Some("[완료]")).then_some(ReportAttention::Completed)
    }
}
