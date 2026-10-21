//! 보고 첫 줄의 알림 의도·작업 단계. thread 묶음과 진행률은 기존 규칙으로 셈한다.
use crate::{Host, orch::ThreadRec};
use aaa_proto::ReportAttention;
use sha2::{Digest, Sha256};

pub(crate) fn marker(report: &str) -> Option<&'static str> {
    let first = report.trim_start_matches('\u{feff}').lines().next()?;
    // 새 안내는 [확인 필요]를 쓴다. 이미 회수한 보고의 옛 표지는 입력 호환용으로만 받는다.
    ["[완료]", "[결정 필요]", "[검수 대기]", "[확인 필요]", "[사람 필요]", "[병합 준비됨]", "[착지]", "[진행 중]"]
        .into_iter()
        .find(|m| first == *m || first.strip_prefix(m).is_some_and(|rest| rest.starts_with(char::is_whitespace)))
}

/// 단계는 첫 줄의 단독 표지만 인정한다. 완료 등 여러 표지를 겹쳐 쓰면 보수적으로 작업으로 센다.
pub(crate) fn stage(report: &str) -> Option<aaa_proto::ThreadStage> {
    let first = report.trim_start_matches('\u{feff}').lines().next()?;
    let tag = marker(report)?;
    let rest = first.strip_prefix(tag)?.trim_start();
    if rest.starts_with('[') {
        return None;
    }
    match tag {
        "[병합 준비됨]" => Some(aaa_proto::ThreadStage::MergeReady),
        "[착지]" => Some(aaa_proto::ThreadStage::Landing),
        _ => None,
    }
}

/// 사람을 기다리는 표지인가(알림 의도 「확인 필요」). 질문 카드(`## Questions`)도 이 보고에서만 읽는다(plan §8.329)
pub(crate) fn needs_user(report: &str) -> bool {
    matches!(marker(report), Some("[결정 필요]" | "[검수 대기]" | "[확인 필요]" | "[사람 필요]"))
}

impl Host {
    /// 이전 host가 회수한 명시 표지도 배포 직후 같은 한도로 센다. 사본이 없거나 hash가 다르면 제외하지 않는다.
    pub(crate) fn backfill_report_stages(&self) -> anyhow::Result<()> {
        for text in self.db.threads(None)? {
            let mut rec: ThreadRec = serde_json::from_str(&text)?;
            let stage = self.verified_report(&rec).as_deref().and_then(stage);
            if rec.report_stage != stage {
                rec.report_stage = stage;
                self.db.thread_save(&rec.project, &rec.id, rec.status.as_str(), &serde_json::to_string(&rec)?)?;
            }
        }
        Ok(())
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use aaa_proto::ThreadStage::{Landing, MergeReady};

    #[test]
    fn only_unambiguous_first_line_stages_release_capacity() {
        for (text, expected) in [
            ("[병합 준비됨] 전체 확인 완료\n## Next\n- 착지", Some(MergeReady)),
            ("\u{feff}[병합 준비됨]\r\n본문", Some(MergeReady)),
            ("[착지] 검증 실패 수리 중", Some(Landing)),
            ("[착지]", Some(Landing)),
            ("[완료] 전체 완료", None),
            ("[진행 중] 다음 단계 구현", None),
            ("[확인 필요] 사용자 결정", None),
            ("옛 보고\n[병합 준비됨]", None),
            ("\n[병합 준비됨]", None),
            (" [착지]", None),
            ("[병합 준비됨]제목", None),
            ("[완료][병합 준비됨]", None),
            ("[완료] [병합 준비됨]", None),
            ("[병합 준비됨][완료]", None),
            ("[병합 준비됨] [완료]", None),
            ("[착지] [진행 중]", None),
        ] {
            assert_eq!(stage(text), expected, "{text}");
        }
        assert!(!needs_user("[병합 준비됨] 착지 창 대기"));
    }
}
