//! coordinator 완료 답의 요청과 카드 연결을 검사한다. 원문은 진단에 복사하지 않는다.
use crate::Host;
use aaa_proto::HarnessEvent;
use regex::Regex;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, sync::LazyLock};

pub(crate) const EVENT: &str = "question card missing";
pub(crate) const STALE_EVENT: &str = "question card outdated";
pub(crate) const REMINDER: &str = "사용자 답을 구하는 표현과 연결된 질문 카드를 확인하지 못했습니다. 실제 답이 필요한 요청이면 aaa post user --request --needs-user --topic user-action --title '<질문 제목>' --body-file <질문 파일>로 카드를 만드세요. 기존 카드 안내라면 해당 q-ID와 상태를 확인하세요.";
pub(crate) const STALE_REMINDER: &str = "안내한 질문 카드가 닫혔거나 교체됐습니다. 최신 카드와 이미 받은 답을 확인해 안내를 갱신하세요. 같은 질문을 새로 만들지 마세요.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CardState {
    Open,
    Answered,
    Closed,
}

impl CardState {
    fn label(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Answered => "answered",
            Self::Closed => "closed_or_replaced",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Evidence {
    rule: &'static str,
    line: usize,
    refs: Vec<(String, CardState)>,
    fresh: bool,
}

impl Evidence {
    fn summary(&self, reminder: &str) -> String {
        let rule = match self.rule {
            "question" => "질문 어미",
            "choices" => "선택지",
            "required_information" => "정보 요청",
            _ => "선택·답 요청",
        };
        let refs = self
            .refs
            .iter()
            .take(8)
            .map(|(id, state)| {
                format!(
                    "{id} {}",
                    match state {
                        CardState::Open => "열림",
                        CardState::Answered => "답함",
                        CardState::Closed => "닫힘·교체",
                    }
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("{reminder} (답변 {}행 · {rule}{}{})", self.line, if refs.is_empty() { "" } else { " · " }, refs)
    }

    fn diagnostic(&self, turn_end: u64) -> String {
        format!(
            "turn_end={turn_end} rule={} line={} refs={}",
            self.rule,
            self.line,
            self.refs.iter().map(|(id, state)| format!("{id}:{}", state.label())).collect::<Vec<_>>().join(",")
        )
    }
}

#[derive(Default)]
struct Inspection {
    missing: Option<Evidence>,
    stale: Option<Evidence>,
}

struct Block {
    text: String,
    line: usize,
    section: usize,
    question_group: Option<usize>,
}

fn ids(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
        .filter_map(|s| aaa_proto::canonical_short_id(s, 'q'))
        .fold(Vec::new(), |mut ids, id| {
            if !ids.contains(&id) {
                ids.push(id);
            }
            ids
        })
}

/// Markdown 전체를 해석하지 않는다. 인용·코드를 제외하고 제목과 문단의 연결만 보존한다.
fn blocks(text: &str) -> Vec<Block> {
    static QUOTES: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"`[^`\n]*`|「[^」]*」|“[^”]*”|"[^"\n]*"|'[^'\n]*'"#).unwrap());
    static INDIRECT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?:라고|라는|다고|냐고).*(?:보고|요약|물었|적었|썼|기록)|(?:예시|인용|질문 예)\s*[:：]").unwrap()
    });
    let mut out: Vec<Block> = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    let mut section = 0;
    let mut question_group = None;
    let mut separated = true;
    for (index, line) in text.lines().enumerate() {
        let t = line.trim_start();
        let mark = t.chars().next().filter(|c| matches!(c, '`' | '~'));
        let count = mark.map_or(0, |c| t.chars().take_while(|x| *x == c).count());
        if let Some((c, n)) = fence {
            if mark == Some(c) && count >= n {
                fence = None;
            }
            separated = true;
            continue;
        }
        if count >= 3 {
            fence = Some((mark.unwrap(), count));
            separated = true;
            continue;
        }
        if t.is_empty()
            || t.starts_with('>')
            || line.starts_with("    ")
            || line.starts_with('\t')
            || INDIRECT.is_match(t)
        {
            separated = true;
            continue;
        }
        let heading = t.trim_matches('*').trim_start_matches('#').trim();
        let title = t.starts_with('#') || t.starts_with("**") && t.ends_with("**");
        // 한 주제 안의 질문 1·2가 뒤의 카드 목록을 공유할 수 있다.
        let numbered = heading.chars().next().is_some_and(|c| c.is_ascii_digit()) && heading.contains(['.', ')']);
        if title && !heading.starts_with("질문") && !heading.starts_with("추천") && !numbered {
            section += 1;
            question_group = None;
            separated = true;
        }
        if fresh_request(t) {
            section += 1;
            question_group = None;
            separated = true;
        }
        if title && let Some(number) = heading.strip_prefix("질문") {
            let digits = number.trim_start().chars().take_while(|c| c.is_ascii_digit()).collect::<String>();
            if let Ok(number) = digits.parse() {
                question_group = Some(number);
                separated = true;
            }
        }
        let clean = QUOTES.replace_all(line, "");
        if separated || out.is_empty() {
            out.push(Block { text: clean.into_owned(), line: index + 1, section, question_group });
        } else {
            let last = out.last_mut().unwrap();
            last.text.push('\n');
            last.text.push_str(&clean);
        }
        separated = false;
    }
    out
}

fn fresh_tail(text: &str) -> Option<&str> {
    ["추가로", "별도로", "새 질문", "새로운 질문", "추가 질문", "함께 정할", "재승인", "다시 배포", "다시 승인"]
        .iter()
        .filter_map(|marker| text.rfind(marker))
        .max()
        .map(|at| &text[at..])
}

fn fresh_request(text: &str) -> bool {
    fresh_tail(text).is_some()
}

fn request(text: &str, required: bool) -> Option<(&'static str, usize)> {
    static QUESTION: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)[^\n.!?。;]*?(?:[가-힣]+까요|[가-힣]+나요|하시겠|원하시|어느|어떤|무엇|어떻게|해도 될|진행해도)[^\n.!?。;]*[?？]|\b(?:shall i|should i|can i|would you like|do you want|which .{0,100}prefer)\b[^\n]*[?？]").unwrap()
    });
    static ASK: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)(?:골라|선택해|알려|말씀해|정해|답해|확인해)[^\n.!?。;,:：]{0,80}(?:주세요|주십시오)|(?:라고|이라고)(?:만)?\s*해\s*(?:주세요|주십시오)|\b(?:let me know|please choose)\b").unwrap()
    });
    static CONDITIONAL: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)(?:원하시면|원하신다면|싶으시면|싶으면|시려면|시작하려면|뜻이 다르면|다른 뜻|만료되면|재부팅|끝나면|그 뒤에|보신 뒤|보시고 정해|나중에|흐리면|문제가.*면|카드가 뜨면|카드가 나오면|올라오면|옮길 때|때가 되면|이었다면|시간이 지나면|항목이 있으면|if you want|if you'd like|if needed)").unwrap()
    });
    static CHOICE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?m)^\s*(?:[-*]\s*)?\*{0,2}[A-D](?:[.)]|\(추천\))?\*{0,2}\s+\S").unwrap());
    if let Some(m) = QUESTION.find(text) {
        return Some(("question", text[..m.start()].bytes().filter(|b| *b == b'\n').count()));
    }
    // 조건부 제안은 실제 질문 어미와 분리한다. 한 문장의 제안이 다음 문장의 요청을 가리지 않는다.
    let mut line = 0;
    for sentence in text.split_inclusive(['.', '!', '?', '？', '\n']) {
        let future_permission = sentence.contains("해도 되면") && text.contains("여쭙겠습니다");
        if !CONDITIONAL.is_match(sentence) && !future_permission {
            if let Some(ask) = ASK.find(sentence) {
                let verification = ask.as_str().starts_with("확인해")
                    && !["할지", "될지", "여부", "맞는지", "괜찮은지", "승인", "결정"]
                        .iter()
                        .any(|s| sentence.contains(s));
                if !verification {
                    return Some(("request", line));
                }
            }
            if (sentence.contains("알려 주시면 좋은")
                || sentence.contains("정해 주실 것")
                || required && ["답해 주시면", "알려 주시면", "정해 주시면"].iter().any(|s| sentence.contains(s)))
                && !sentence.contains("나중")
            {
                return Some(("required_information", line));
            }
        }
        line += sentence.bytes().filter(|b| *b == b'\n').count();
    }
    let choices = CHOICE.find_iter(text).count();
    if choices >= 2
        && (text.contains("추천")
            || text.contains("선택지")
            || text.contains("고를 것")
            || text.lines().all(|l| l.chars().count() < 24 && !l.contains(['.', ':'])))
    {
        return Some(("choices", 0));
    }
    None
}

fn inspect(text: &str, cards: &HashMap<String, CardState>) -> Inspection {
    let blocks = blocks(text);
    let mut result = Inspection::default();
    for (index, block) in blocks.iter().enumerate() {
        let required = blocks.iter().filter(|b| b.section == block.section).any(|b| {
            ["카드는 없음", "카드로 올리지", "선택지는 두 가지", "해 주실 것"]
                .iter()
                .any(|marker| b.text.contains(marker))
        });
        let Some((rule, line)) = request(&block.text, required) else { continue };
        let own_ids = ids(fresh_tail(&block.text).unwrap_or(&block.text));
        let fresh = fresh_request(&block.text);
        let unresolved = own_ids.iter().any(|id| !cards.contains_key(id));
        let own = own_ids.iter().filter_map(|id| cards.get(id).map(|state| (id.clone(), *state))).collect::<Vec<_>>();
        // 새 요청이 같은 문단의 옛 q-ID 뒤에 붙어도 옛 카드로 가리지 않는다.
        let mut linked = if unresolved { Vec::new() } else { own };
        if linked.is_empty() && !fresh && !unresolved {
            // 요청 뒤 목록·설명을 지나 같은 주제의 카드 링크를 찾는다.
            linked = blocks[index + 1..]
                .iter()
                .take_while(|next| next.section == block.section)
                .flat_map(|next| ids(&next.text))
                .filter_map(|id| cards.get(&id).map(|state| (id, *state)))
                .collect();
            // 질문 1·2를 따로 묻고 링크를 뒤에 모았으면 같은 카드 하나로 둘을 면제하지 않는다.
            if let Some(group) = block.question_group {
                let groups = blocks
                    .iter()
                    .filter(|b| b.section == block.section)
                    .filter_map(|b| b.question_group)
                    .fold(Vec::new(), |mut groups, group| {
                        if !groups.contains(&group) {
                            groups.push(group);
                        }
                        groups
                    });
                if groups.len() > 1 {
                    let unique = linked.into_iter().fold(Vec::new(), |mut refs, item| {
                        if !refs.iter().any(|(id, _)| id == &item.0) {
                            refs.push(item);
                        }
                        refs
                    });
                    linked = groups
                        .iter()
                        .position(|g| *g == group)
                        .and_then(|i| unique.get(i))
                        .cloned()
                        .into_iter()
                        .collect();
                }
            }
            // 카드 아래의 선택지 목록은 같은 안내다. 일반 새 질문은 앞 카드로 면제하지 않는다.
            if linked.is_empty()
                && (rule == "choices"
                    || rule == "request"
                        && ["직접 답", "카드에", "카드에서", "그 질문", "위 질문", "해당 질문"]
                            .iter()
                            .any(|s| block.text.contains(s)))
            {
                linked = blocks[..index]
                    .iter()
                    .rev()
                    .take_while(|prev| prev.section == block.section)
                    .flat_map(|prev| ids(&prev.text))
                    .filter_map(|id| cards.get(&id).map(|state| (id, *state)))
                    .collect();
            }
        }
        // 첫 문단의 일반 카드 안내는 뒤의 여러 주제 카드 목록을 가리킬 수 있다.
        if linked.is_empty()
            && !fresh
            && !unresolved
            && rule == "request"
            && ["카드를 확인", "아래 질문", "아래 새 카드", "새 카드로 답", "새 번호로 답"]
                .iter()
                .any(|s| block.text.contains(s))
        {
            linked = blocks
                .iter()
                .flat_map(|b| ids(&b.text))
                .filter_map(|id| cards.get(&id).map(|state| (id, *state)))
                .collect();
        }
        let evidence = Evidence { rule, line: block.line + line, refs: linked, fresh };
        if evidence.refs.is_empty() {
            result.missing.get_or_insert(evidence);
        } else if evidence.refs.iter().any(|(_, state)| *state == CardState::Closed) {
            result.stale.get_or_insert(evidence);
        }
    }
    result
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
        let posted = self.db.question_guard_posted(&project, &ev.session, reply.started_ms, reply.finished_ms)?;
        let open = self.question_catalog(false)?;
        let mut cards = HashMap::new();
        let mut replacements = HashMap::<String, Vec<String>>::new();
        for id in ids(&reply.text).into_iter().take(128) {
            if open
                .iter()
                .any(|q| q.project.as_deref() == Some(&project) && q.short_id.as_ref().unwrap_or(&q.id) == &id)
            {
                cards.insert(id, CardState::Open);
            } else if let Some(answered) = self.db.question_guard_history(&project, &id)? {
                if !answered
                    && let Some(target) = self.db.question_target(Some(&project), &id)?
                    && target.kind == "thread"
                {
                    let latest = open
                        .iter()
                        .filter(|q| {
                            q.project.as_deref() == Some(&project) && q.thread.as_deref() == Some(&target.subject)
                        })
                        .take(8)
                        .map(|q| q.short_id.clone().unwrap_or_else(|| q.id.clone()))
                        .collect();
                    replacements.insert(id.clone(), latest);
                }
                cards.insert(id, if answered { CardState::Answered } else { CardState::Closed });
            }
        }
        let inspection = inspect(&reply.text, &cards);
        if let Some(evidence) = inspection.missing
            && (!posted || evidence.fresh)
        {
            self.inbox_once(&project, "coordinator-check", &ev.session, EVENT, &evidence.summary(REMINDER));
            self.db.audit(
                "question-guard",
                "missing_question_card",
                &format!("{project} pane={} {}", ev.session, evidence.diagnostic(reply.finished_ms)),
            );
        }
        if let Some(evidence) = inspection.stale {
            let latest = evidence
                .refs
                .iter()
                .filter_map(|(id, _)| replacements.get(id))
                .flatten()
                .take(8)
                .cloned()
                .collect::<Vec<_>>();
            let mut summary = evidence.summary(STALE_REMINDER);
            if !latest.is_empty() {
                summary.push_str(&format!(" 현재 카드: {}", latest.join(", ")));
            }
            self.inbox_once(&project, "coordinator-check", &ev.session, STALE_EVENT, &summary);
            self.db.audit(
                "question-guard",
                "outdated_question_card",
                &format!("{project} pane={} {}", ev.session, evidence.diagnostic(reply.finished_ms)),
            );
        }
        self.db.set_meta(&key, &serde_json::to_string(&(reply.finished_ms, signature))?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
