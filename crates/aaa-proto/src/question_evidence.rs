//! 질문 카드 근거 위치 검사. 카드는 `### N.` 질문 칸 안의 글·그림만 싣고 `## Questions` 밖·첫 질문 앞의 본문은 버린다.
//! 밖에만 그림·영상·library 링크·r-ID가 있고 어느 질문 칸에도 근거가 없으면 사용자는 빈 카드로 판단하게 된다.
use crate::{ThreadQuestion, parse_report_questions, short_id_number};
use serde::{Deserialize, Serialize};

/// 근거로 세는 자료 하나(원문 주소 또는 r-ID)
#[derive(Debug, Clone, PartialEq, Eq)]
struct Evidence {
    kind: &'static str,
    target: String,
}

const VIDEO: [&str; 3] = [".mp4", ".webm", ".mov"];

/// 한 줄의 근거: `![설명](주소)` 그림·영상, `[글](library/…)` 링크, 맨 r-NNNN. inline code 속은 예시라 세지 않는다.
fn line_evidence(line: &str, out: &mut Vec<Evidence>) {
    let mut plain = String::with_capacity(line.len());
    for (i, part) in line.split('`').enumerate() {
        if i % 2 == 0 {
            plain.push_str(part);
        }
        plain.push(' ');
    }
    let mut rest = plain.as_str();
    let mut text = String::new();
    while let Some(at) = rest.find("](") {
        let head = &rest[..at];
        let open = head.rfind('[');
        let image = open.is_some_and(|o| head[..o].ends_with('!'));
        let tail = &rest[at + 2..];
        let end = tail.find(')').unwrap_or(tail.len());
        let href = tail[..end].split_whitespace().next().unwrap_or("").trim_matches(['<', '>']);
        text.push_str(open.map_or(head, |o| &head[..o.saturating_sub(usize::from(image))]));
        if let Some(o) = open {
            text.push(' ');
            text.push_str(&head[o + 1..]);
        }
        let lower = href.to_ascii_lowercase();
        let path = lower.split(['#', '?']).next().unwrap_or("");
        let library = path.trim_start_matches("./").starts_with("library/");
        let report = path.rsplit(['/', ':']).next().and_then(|n| short_id_number(n, 'r')).is_some();
        if image {
            let kind = if VIDEO.iter().any(|v| path.ends_with(v)) { "영상" } else { "그림" };
            out.push(Evidence { kind, target: href.to_string() });
        } else if library || report {
            out.push(Evidence { kind: "링크", target: href.to_string() });
        }
        rest = &tail[end.min(tail.len())..];
    }
    text.push_str(rest);
    for word in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-')) {
        if short_id_number(word, 'r').is_some() {
            out.push(Evidence { kind: "보고서", target: word.to_string() });
        }
    }
}

/// code fence 밖 줄의 근거
fn evidence(text: &str) -> Vec<Evidence> {
    let mut out = Vec::new();
    let mut fenced = false;
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if !fenced {
            line_evidence(line, &mut out);
        }
    }
    out
}

/// 본문 그림·링크가 가리키는 `library/` 아래 파일 경로(앞의 `library/`를 뗀 상대 경로, 처음 나온 순서, 중복 없음).
/// code fence·inline code 속 예시와 폴더(`/`로 끝남)는 뺀다. 받는 쪽이 그 판에 파일이 있는지 확인한다.
pub fn library_paths(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in evidence(text) {
        let path = e.target.split(['#', '?']).next().unwrap_or("").trim_start_matches("./");
        if let Some(rel) = path.strip_prefix("library/")
            && !rel.is_empty()
            && !rel.ends_with('/')
            && !out.iter().any(|p| p == rel)
        {
            out.push(rel.to_string());
        }
    }
    out
}

/// 질문 칸 안의 근거: 자료(그림·영상·링크·r-ID)와 카드에 그대로 보이는 표·code 블록. 표로 비교를 실은 질문은 밖의 상세 링크만으로 경고하지 않는다.
fn question_evidence(q: &ThreadQuestion) -> bool {
    let parts = [Some(q.text.as_str()), q.body.as_deref(), q.after.as_deref()].into_iter().flatten().chain(
        q.choices.iter().flat_map(|c| [Some(c.label.as_str()), c.description.as_deref(), c.body.as_deref()]).flatten(),
    );
    parts.into_iter().any(|p| {
        !evidence(p).is_empty()
            || p.lines().any(|l| {
                let t = l.trim_start();
                t.starts_with('|') || t.starts_with("```") || t.starts_with("~~~")
            })
    })
}

/// 질문 칸 밖에만 있는 근거의 요약. 카드 문구(사용자)와 고칠 방법(작성자)을 같은 진단에서 만든다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionEvidenceGap {
    /// 종류별 수(`그림 2개·링크 1개`)
    pub what: String,
    /// 앞의 서로 다른 주소·r-ID 최대 3개
    pub examples: Vec<String>,
}

impl QuestionEvidenceGap {
    /// 보낸 쪽에 돌려줄 진단과 고칠 방법
    pub fn message(&self) -> String {
        format!(
            "근거 확인 필요: {}({})가 질문 칸 밖에만 있다. 카드에는 `### N.` 질문 칸 안의 글·그림만 보인다. 판단에 쓰는 그림·영상·보고서 링크를 그 질문의 `### N.` 아래(선택지 위)로 옮긴다.",
            self.what,
            self.examples.join(", ")
        )
    }
}

/// 근거가 질문 칸 밖에만 있으면 그 진단. 질문이 없거나 어느 질문 칸에 근거가 하나라도 있으면 None.
/// 판단은 카드가 실제로 싣는 질문 칸(본문·선택지·선택지 뒤 설명)과 같은 파서 결과로 한다.
pub fn question_evidence_gap(text: &str) -> Option<QuestionEvidenceGap> {
    let parsed = parse_report_questions(text);
    if parsed.items.is_empty() || parsed.items.iter().any(question_evidence) {
        return None;
    }
    let outside = evidence(text);
    if outside.is_empty() {
        return None;
    }
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for e in &outside {
        match counts.iter_mut().find(|(k, _)| *k == e.kind) {
            Some((_, n)) => *n += 1,
            None => counts.push((e.kind, 1)),
        }
    }
    let mut examples = Vec::new();
    for e in &outside {
        if !examples.contains(&e.target) && examples.len() < 3 {
            examples.push(e.target.clone());
        }
    }
    Some(QuestionEvidenceGap {
        what: counts.iter().map(|(k, n)| format!("{k} {n}개")).collect::<Vec<_>>().join("·"),
        examples,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pictures_only_above_questions_are_reported_with_the_fix() {
        // q-2677 꼴: 그림 여러 장을 Questions 위에 두고 질문 칸에는 글만 쓴다.
        let body = "시안 7장을 비교한다.\n\n![A](library/p/a.png)\n![B](library/p/b.png)\n![영상](library/p/c.mp4)\n\n\
                    [보고서](library/p/index.html) r-0012\n\n## Questions\n### 1. 어느 안\n- A 첫 안 (추천)\n- B 둘째 안\n";
        let gap = question_evidence_gap(body).expect("밖에만 근거").message();
        assert!(gap.contains("그림 2개·영상 1개·링크 1개·보고서 1개"), "{gap}");
        assert!(gap.contains("library/p/a.png, library/p/b.png, library/p/c.mp4"), "{gap}");
        assert!(gap.contains("`### N.` 아래(선택지 위)로 옮긴다"), "{gap}");
        // 질문 앞(## Questions 안, 첫 ### 앞)의 그림도 카드에 실리지 않는다.
        assert!(
            question_evidence_gap("## Questions\n![A](library/a.png)\n### 1. 진행\n- A 예\n- B 아니오\n").is_some()
        );
    }

    #[test]
    fn any_evidence_inside_a_question_or_its_choices_is_enough() {
        for inside in [
            "### 1. 어느 안\n![A](library/a.png)\n- A 예\n- B 아니오\n",
            "### 1. 어느 안\n[상세](library/details.md)\n- A 예\n- B 아니오\n",
            "### 1. 어느 안\nr-0012 보고서를 본다\n- A 예\n- B 아니오\n",
            "### 1. 어느 안\n- A 예\n  ![A안](library/a.png)\n- B 아니오\n",
            "### 1. 어느 안\n- A 예 — [근거](library/x/index.html#A)\n- B 아니오\n",
            "### 1. 색\n- A 예\n- B 아니오\n### 2. 어느 안\n[보고서](aaa:report/r-0012)\n- A 예\n- B 아니오\n",
            "### 1. 어느 안\n| 안 | 결과 |\n| --- | --- |\n| A | 빠름 |\n\n- A 예\n- B 아니오\n",
            "### 1. 어느 안\n```\n모델  모두 끄기\n```\n- A 예\n- B 아니오\n",
        ] {
            let body = format!("![위](library/up.png)\n\n## Questions\n{inside}");
            assert_eq!(question_evidence_gap(&body), None, "{body}");
        }
    }

    #[test]
    fn library_paths_skip_code_examples_folders_and_fragments() {
        let body = "![A](library/map/a.png)\n[보고서](./library/map/index.html#B) [폴더](library/map/)\n\
                    `![예](library/x.png)`\n```\n![예](library/y.png)\n```\n[다시](library/map/a.png) [외부](https://example.com/library/z.png)\n";
        assert_eq!(library_paths(body), ["map/a.png", "map/index.html"]);
    }

    #[test]
    fn plain_questions_code_examples_and_outside_words_are_not_evidence() {
        for body in [
            // 근거가 아무 데도 없는 단순 질문은 이 검사의 대상이 아니다.
            "[결정 필요] 진행\n\n## Report\n남은 것 없음\n\n## Questions\n### 1. 진행\n- A 예\n- B 아니오\n",
            // code 예시·외부 주소·글자 속 r-ID 모양은 근거가 아니다.
            "예: `![시안](library/a.png)`\n```md\n![시안](library/b.png)\n```\n[외부](https://example.com/a) arr-0001 r-0x\n\n## Questions\n### 1. 진행\n- A 예\n- B 아니오\n",
            // Questions가 없으면 카드도 없다.
            "![A](library/a.png)\n",
        ] {
            assert_eq!(question_evidence_gap(body), None, "{body}");
        }
    }
}
