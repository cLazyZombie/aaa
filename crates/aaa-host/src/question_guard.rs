//! coordinator 완료 답의 질문 카드 누락을 inbox로 알린다. 턴·카드를 만들거나 막지 않는다.
use crate::Host;
use aaa_proto::{HarnessEvent, PostType};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::{collections::HashSet, sync::LazyLock};

pub(crate) const EVENT: &str = "question card missing";
pub(crate) const REMINDER: &str = "방금 답에서 사용자 결정을 물었지만 같은 턴의 질문 카드가 없습니다. aaa post user --request --needs-user --topic user-action --title '<질문 제목>' --body-file <질문 파일>로 질문 카드를 만드세요. 기존 카드가 있으면 열린 q-ID를 안내하세요.";

/// 코드·인용문을 지운 뒤 문단별로 본다. 기존 q-ID 하나가 다른 새 질문까지 가리지 않는다.
fn missing_question(text: &str, open: &HashSet<String>) -> bool {
    static ASK: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(concat!(
            r"(?i)(?:할까요|될까요|인가요|괜찮을까요|하시겠|원하시|어느|어떤|무엇|어떻게|해도 될|진행해도)[^\n]*[?？]|",
            r"(?:골라|선택해|알려|말씀해|정해|답해|확인해)[^\n]*(?:주세요|주시면|주십시오)|",
            r"(?:라고|이라고)(?:만)?\s*해\s*(?:주세요|주시면)|원하시면|원하신다면|",
            r"\b(?:shall i|should i|can i|would you like|do you want|which .{0,100}prefer|let me know|please choose)\b"
        ))
        .unwrap()
    });
    static QUOTES: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"`[^`\n]*`|「[^」]*」|“[^”]*”|"[^"\n]*"|'[^'\n]*'"#).unwrap());
    static CHOICE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?m)^\s*(?:[-*]\s*)?\*{0,2}[A-D](?:[.)]|\(추천\))?\*{0,2}\s+\S").unwrap());
    static INDIRECT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?:라고|라는|다고|냐고).*(?:보고|요약|물었|적었|썼|기록)|(?:예시|인용|질문 예)\s*[:：]").unwrap()
    });
    let mut clean = String::new();
    let mut fence: Option<(char, usize)> = None;
    for line in text.lines() {
        let t = line.trim_start();
        let mark = t.chars().next().filter(|c| matches!(c, '`' | '~'));
        let count = mark.map_or(0, |c| t.chars().take_while(|x| *x == c).count());
        if let Some((c, n)) = fence {
            if mark == Some(c) && count >= n {
                fence = None;
            }
            clean.push('\n');
            continue;
        }
        if count >= 3 {
            fence = Some((mark.unwrap(), count));
            clean.push('\n');
            continue;
        }
        if t.starts_with('>') || line.starts_with("    ") || line.starts_with('\t') || INDIRECT.is_match(t) {
            clean.push('\n');
            continue;
        }
        clean.push_str(&QUOTES.replace_all(line, ""));
        clean.push('\n');
    }
    clean.split("\n\n").any(|block| {
        let linked = block
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
            .filter_map(|s| aaa_proto::canonical_short_id(s, 'q'))
            .any(|id| open.contains(&id));
        !linked && (ASK.is_match(block) || CHOICE.find_iter(block).count() >= 2)
    })
}

impl Host {
    pub(crate) fn check_question_card(&self, mid: &str, ev: &HarnessEvent) -> anyhow::Result<()> {
        let Some(reply) = ev.turn_reply.as_ref() else { return Ok(()) };
        if !matches!(ev.harness.as_str(), "claude" | "codex")
            || reply.text.len() > 64_000
            || reply.started_ms > reply.finished_ms
            || reply.finished_ms < self.started_ms
        {
            return Ok(());
        }
        // host 정본으로 확인한다. 원격·worker·같은 cwd의 thread는 대상이 아니다.
        if self.inner.lock().unwrap().panes.get(&ev.session).is_none_or(|p| {
            p.machine_id != mid || matches!(p.info.role, Some(aaa_proto::Role::Worker | aaa_proto::Role::Evaluator))
        }) {
            return Ok(());
        }
        let Some(project) = self.coordinator_project_of_actor(&format!("local:cli@{}", ev.session)) else {
            return Ok(());
        };
        let key = format!("question-guard:{project}:{}", ev.session);
        let signature = hex::encode(Sha256::digest(serde_json::to_vec(reply)?));
        if let Some(last) = self.db.meta(&key)? {
            let (at, hash): (u64, String) = serde_json::from_str(&last)?;
            if at > reply.finished_ms || (at == reply.finished_ms && hash == signature) {
                return Ok(());
            }
        }
        let open_items = self.db.inbox_list(Some(&project), false)?;
        // topic은 강제하지 않는다. 실제 접수된 같은 pane의 열린 request만 인정한다.
        let mut posted = false;
        for item in open_items.iter().filter(|i| crate::asks::is_user_request(i)) {
            if let Some(post) = self.db.post_of_item(&project, &item.id)? {
                posted |= post.from.session.as_deref() == Some(&ev.session)
                    && post.from.verified == "coordinator-pane"
                    && post.input.to == "user"
                    && post.input.post_type == PostType::Request
                    && post.input.needs_user
                    && post.created_ms >= reply.started_ms
                    && post.created_ms <= reply.finished_ms;
            }
        }
        let open = self
            .question_catalog(false)?
            .into_iter()
            .filter(|q| q.project.as_deref() == Some(&project) && q.state == aaa_proto::QuestionState::Open)
            .map(|q| q.short_id.unwrap_or(q.id))
            .filter_map(|id| aaa_proto::canonical_short_id(&id, 'q'))
            .collect();
        if !posted && missing_question(&reply.text, &open) {
            self.inbox_once(&project, "coordinator-check", &ev.session, EVENT, REMINDER);
            self.db.audit(
                "question-guard",
                "missing_question_card",
                &format!("{project} pane={} turn_end={}", ev.session, reply.finished_ms),
            );
        }
        self.db.set_meta(&key, &serde_json::to_string(&(reply.finished_ms, signature))?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_decisions_are_distinct_from_quotes_and_existing_cards() {
        let open = HashSet::from(["q-1001".into()]);
        for text in [
            "진행할까요?",
            "원하시면 진행하겠습니다.",
            "진행이라고 해 주세요.",
            "A 진행 (추천)\nB 보류",
            "Which option do you prefer?",
            "원하는 방향을 알려 주세요.",
        ] {
            assert!(missing_question(text, &open), "{text}");
        }
        for text in [
            "검증을 마쳤습니다.",
            "> 진행할까요?",
            "```md\n진행할까요?\n```",
            "~~~md\n진행할까요?\n~~~",
            "보고: 「진행할까요?」",
            "진행할까요?라고 물었다는 보고입니다.",
            "[질문](#question=q-01001)에서 선택해 주세요.",
            "q-1001에서 진행할까요?",
        ] {
            assert!(!missing_question(text, &open), "{text}");
        }
        assert!(missing_question("q-1001에서 답해 주세요.\n\n추가로 배포할까요?", &open));
        assert!(missing_question("q-1002에서 선택해 주세요.", &open), "닫힌 카드 참조는 면제하지 않는다");
    }
}
