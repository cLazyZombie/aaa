//! coordinator 완료 답의 요청과 카드 연결을 검사한다. 원문은 진단에 복사하지 않는다.
use crate::Host;
use aaa_proto::HarnessEvent;
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, ops::Range, sync::LazyLock};

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
    choices_section: usize,
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

/// 교체·폐기·답변 불필요 설명의 참조만 제외한다. 같은 번호의 실제 답 안내는 남긴다.
fn answer_ids(text: &str, replacements: &HashMap<String, String>) -> Vec<String> {
    static HISTORY: LazyLock<Regex> = LazyLock::new(|| {
        let reference = r"(?:\[[^\]\n]*\]\([^()\n]*q-[0-9]+[^()\n]*\)|q-[0-9]+)";
        let replacement = format!(
            r"(?<old>{reference})[ \t]*에서[ \t]*(?<new>{reference})[ \t]*(?:으로|로)[ \t]*(?:바뀜|바뀌었습니다|교체됨|교체됐습니다)\b"
        );
        Regex::new(&format!(
            r"{replacement}|{reference}\s*에서\s*(?:바뀜|바뀌었습니다|교체됨|교체됐습니다)|옛\s*번호\s*[:：]?\s*{reference}|{reference}\s*\(옛\s*번호\)|{reference}\s*(?:(?:은|는|이|가)\s*)?(?:이미\s*)?(?:닫았|닫혔|폐기했)(?:습니다|어요|고|으며|으니|지만)|{reference}\s*(?:에는|에|에서는|에서|은|는)?\s*답하지\s*(?:않으셔도|않아도)\s*됩니다|{reference}\s*(?:은|는)\s*이\s*(?:질문이|시안이|카드가)\s*대신합니다\b"
        ))
        .unwrap()
    });
    static TARGET: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^\s*\)?\s*(?:에서|으로|로|에|을|를)[^.!?。\n|]{0,80}(?:답해|선택해|골라|정해|판단해)").unwrap()
    });
    ids(&HISTORY.replace_all(text, |c: &regex::Captures<'_>| {
        let matched = c.get(0).unwrap();
        if let (Some(old), Some(new)) = (c.name("old"), c.name("new")) {
            let old_ids = ids(old.as_str());
            let new_ids = ids(new.as_str());
            let at_token_start = !text[..old.start()].ends_with(|ch: char| ch.is_ascii_alphanumeric() || ch == '-');
            if at_token_start
                && let ([old_id], [new_id]) = (old_ids.as_slice(), new_ids.as_slice())
                && replacements.get(old_id) == Some(new_id)
            {
                return format!(" {}", &text[old.end()..matched.end()]);
            }
        } else if !TARGET.is_match(&text[matched.end()..]) {
            return " ".into();
        }
        matched.as_str().to_owned()
    }))
}

/// 간접 인용은 보고한 구절만 지운다. 같은 줄의 카드 안내·새 요청까지 지우지 않는다.
fn prose(line: &str) -> String {
    static QUOTES: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"`[^`\n]*`|「[^」]*」|“[^”]*”|"[^"\n]*"|'[^'\n]*'"#).unwrap());
    static INDIRECT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(^|[.!?？。;,，])[^.!?？。;,，\n]*[?？]?(?:라고|라는|다고|냐고)[^.!?？。;,，\n]*?(?:보고|요약|물었|적었|썼|기록)[가-힣]*|(?:예시|인용|질문 예)\s*[:：][^.!?？。;,，\n]*[?？]?",
        )
        .unwrap()
    });
    let unquoted = QUOTES.replace_all(line, "");
    INDIRECT.replace_all(&unquoted, "$1").into_owned()
}

struct ListItem {
    line: usize,
    indent: usize,
    open_card: bool,
}

fn indent_end(text: &str, column: usize) -> usize {
    text.bytes()
        .take_while(|b| matches!(b, b' ' | b'\t'))
        .fold(column, |col, b| if b == b'\t' { col + 4 - col % 4 } else { col + 1 })
}

/// 실제 목록 본문 행을 찾고, 같은 항목 첫 줄의 열린 카드에 이어지는 행을 표시한다.
fn list_text(
    text: &str,
    cards: &HashMap<String, CardState>,
    replacements: &HashMap<String, String>,
) -> HashMap<usize, bool> {
    let starts = std::iter::once(0).chain(text.match_indices('\n').map(|(at, _)| at + 1)).collect::<Vec<_>>();
    let line_at = |offset| starts.partition_point(|&start| start <= offset);
    let source_line = |line| text[starts[line - 1]..].lines().next().unwrap_or_default();
    let mut items: Vec<ListItem> = Vec::new();
    let mut excluded = 0;
    let mut lines = HashMap::new();
    for (event, span) in Parser::new(text).into_offset_iter() {
        match event {
            Event::Start(Tag::BlockQuote(_) | Tag::CodeBlock(_) | Tag::Heading { .. }) => excluded += 1,
            Event::End(TagEnd::BlockQuote(_) | TagEnd::CodeBlock | TagEnd::Heading(_)) => excluded -= 1,
            Event::Start(Tag::Item) => {
                let line = line_at(span.start);
                let marker = &text[span];
                let width = marker.find([' ', '\t', '\r', '\n']).unwrap_or(marker.len());
                let marker_end = indent_end(source_line(line), 0) + width;
                let indent = indent_end(&marker[width..], marker_end).max(marker_end + 1);
                items.push(ListItem { line, indent, open_card: false });
            }
            Event::End(TagEnd::Item) => {
                items.pop();
            }
            Event::Start(Tag::Link { dest_url, .. }) if excluded == 0 => {
                if let Some(item) = items.last_mut()
                    && line_at(span.start) == item.line
                    && let [id] = answer_ids(&prose(source_line(item.line)), replacements).as_slice()
                    && cards.get(id) == Some(&CardState::Open)
                    && (ids(&dest_url).contains(id) || ids(&text[span]).contains(id))
                {
                    item.open_card = true;
                }
            }
            Event::Text(_) if excluded == 0 => {
                if let Some(item) = items.last() {
                    let line = line_at(span.start);
                    if line == item.line {
                        lines.insert(line, false);
                    } else if indent_end(source_line(line), 0) >= item.indent {
                        lines.insert(line, item.open_card);
                    }
                }
            }
            _ => {}
        }
    }
    lines
}

/// 인용·코드를 제외하고 제목과 문단의 연결을 보존한다.
fn blocks(text: &str, list_text: &HashMap<usize, bool>) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    let mut section = 0;
    let mut choices_section = 0;
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
            || (line.starts_with("    ") || line.starts_with('\t')) && !list_text.contains_key(&(index + 1))
        {
            separated = true;
            continue;
        }
        let clean = prose(line);
        let t = clean.trim_start();
        if t.trim_matches(|c: char| c.is_whitespace() || ".!?？。;,，".contains(c)).is_empty() {
            separated = true;
            continue;
        }
        let heading = t.trim_matches('*').trim_start_matches('#').trim();
        let title = t.starts_with('#') || t.starts_with("**") && t.ends_with("**");
        // 한 주제 안의 질문 1·2가 뒤의 카드 목록을 공유할 수 있다.
        let numbered = heading.chars().next().is_some_and(|c| c.is_ascii_digit()) && heading.contains(['.', ')']);
        // 목록 제목을 건너 연결할 수 있는 것은 선택지뿐이다. 독립 승인 요청은 계속 분리한다.
        let presentation = matches!(heading, "선택지" | "고칠 방식" | "정할 것");
        if title && !heading.starts_with("질문") && !heading.starts_with("추천") && !numbered {
            section += 1;
            if !presentation {
                choices_section += 1;
            }
            question_group = None;
            separated = true;
        }
        if fresh_request(t) {
            section += 1;
            choices_section += 1;
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
        if separated || out.is_empty() {
            out.push(Block { text: clean, line: index + 1, section, choices_section, question_group });
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
        .flat_map(|&marker| text.match_indices(marker))
        // 알려진 완료 설명만 제외한다. 앞뒤의 실제 별도 요청 표지는 그대로 남긴다.
        .filter(|(at, marker)| *marker != "별도로" || !text[*at..].starts_with("별도로 정할 일로 넘겼습니다."))
        .map(|(at, _)| at)
        .max()
        .map(|at| &text[at..])
}

fn fresh_request(text: &str) -> bool {
    fresh_tail(text).is_some()
}

fn choice_prefix(line: &str) -> Option<usize> {
    static CHOICE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\s*(?:[-*]\s*)?\*{0,2}[A-D](?:[.)]|\(추천\))?:?\*{0,2}:?\s+").unwrap());
    CHOICE.find(line).map(|m| m.end()).filter(|&end| end < line.len())
}

fn requests(text: &str, required: bool, previous_block: Option<&str>) -> Vec<(&'static str, usize)> {
    static QUESTION: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)[^\n.!?。;]*?(?:[가-힣]+까요|[가-힣]+나요|하시겠|원하시|어느|어떤|무엇|어떻게|해도 될|진행해도)[^\n.!?。;]*[?？]|\b(?:shall i|should i|can i|would you like|do you want|which .{0,100}prefer)\b[^\n]*[?？]").unwrap()
    });
    static ASK: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)(?:골라|선택해|알려|말씀해|정해|답해|확인해)[^\n.!?。;,:：]{0,80}(?:주세요|주십시오)|(?:라고|이라고)(?:만)?\s*해\s*(?:주세요|주십시오)|\b(?:let me know|please choose)\b").unwrap()
    });
    static CONDITIONAL: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)(?:원하시면|원하신다면|싶으시면|싶으면|시려면|시작하려면|뜻이 다르면|다른 뜻|만료되면|재부팅|끝나면|그 뒤에|보신 뒤|보시고 정해|나중에|흐리면|문제가.*면|카드가 뜨면|카드가 나오면|올라오면|옮길 때|때가 되면|이었다면|시간이 지나면|항목이 있으면|if you want|if you'd like|if needed)").unwrap()
    });
    static OPTIONAL_FEEDBACK: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?:필요하면|필요하다면|모델\s*을\s*바꾸려면|지우려면|삭제하려면|다르면|다르게\s*생각하시면|다르게 생각하신 거라면|다르게\s*뜻하셨(?:으면|다면)|다른 작업을 뜻하셨다면|다른\s*[가-힣 ]{1,40}(?:을|를)\s*말씀하신\s*거라면|다른\s*[가-힣]{1,24}(?:이라면|라면)\s*그때|뜻이셨다면|겹쳐야\s*한다면|(?:오류|증상)(?:\s*(?:문구|메시지))?(?:가|이)\s*있었다면|(?:경우|문제|오류|증상)(?:가|이)\s*있으면)\s*(?:따로\s*)?(?:말씀해|알려)\s*(?:주세요|주십시오)|^\s*(?:말씀해|알려)\s*(?:주세요|주십시오)\s*[,，]\s*(?:필요하면|필요하다면)").unwrap()
    });
    static DECIDED_NAME: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?:^|[.!?？。\n])\s*(?:(?:폴더|디렉터리)\s+)?이름(?:은|을)\s+(?:제가\s+)?(?:[\w/-]*로\s+)?(?:정했습니다|확정했습니다)[.!。]?\s*$").unwrap()
    });
    static NAME_FEEDBACK: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?:^|[,，])\s*다른\s+이름이\s+좋으시면\s*[,，]?\s*(?:말씀해|알려)\s*(?:주세요|주십시오)\b")
            .unwrap()
    });
    static NO_INFORMATION_NEEDED: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?:정해\s*주실\s*것|알려\s*주시면\s*좋은\s*것)(?:은|이|도)?\s*없(?:습니다|어요|고|으며|지만|는데|다)",
        )
        .unwrap()
    });
    let mut out = QUESTION.find_iter(text).map(|m| ("question", m.start())).collect::<Vec<_>>();
    // 조건부 제안은 실제 질문 어미와 분리한다. 한 문장의 제안이 다음 문장의 요청을 가리지 않는다.
    let mut offset = 0;
    let mut previous = previous_block.unwrap_or_default();
    let mask = |m: &regex::Captures<'_>| " ".repeat(m[0].len());
    for sentence in text.split_inclusive(['.', '!', '?', '？', '\n']) {
        // 직전 이름 확정 안내에 붙은 의견 구절만 같은 바이트 길이의 공백으로 바꾼다.
        let actionable = if NAME_FEEDBACK.is_match(sentence) && DECIDED_NAME.is_match(previous) {
            NAME_FEEDBACK.replace_all(sentence, mask)
        } else {
            std::borrow::Cow::Borrowed(sentence)
        };
        if !sentence.trim().is_empty() {
            previous = sentence;
        }
        // 피드백 구절만 지운다. 같은 문장의 별개 승인·선택 요청까지 면제하지 않는다.
        let actionable = OPTIONAL_FEEDBACK.replace_all(&actionable, mask);
        let actionable = NO_INFORMATION_NEEDED.replace_all(&actionable, mask);
        let sentence = actionable.as_ref();
        let future_permission = sentence.contains("해도 되면") && text.contains("여쭙겠습니다");
        if !CONDITIONAL.is_match(sentence) && !future_permission {
            for ask in ASK.find_iter(sentence) {
                let verification = ask.as_str().starts_with("확인해")
                    && !["할지", "될지", "여부", "맞는지", "괜찮은지", "승인", "결정"]
                        .iter()
                        .any(|s| sentence.contains(s));
                if !verification {
                    out.push(("request", offset + ask.start()));
                }
            }
            if (sentence.contains("알려 주시면 좋은")
                || sentence.contains("정해 주실 것")
                || required && ["답해 주시면", "알려 주시면", "정해 주시면"].iter().any(|s| sentence.contains(s)))
                && !sentence.contains("나중")
            {
                out.push(("required_information", offset));
            }
        }
        offset += sentence.len();
    }
    let choices = text.lines().filter(|line| choice_prefix(line).is_some()).count();
    if choices >= 2
        && (text.contains("추천")
            || text.contains("선택지")
            || text.contains("고를 것")
            || text.lines().all(|l| l.chars().count() < 24 && !l.contains(['.', ':'])))
    {
        out.push(("choices", 0));
    }
    out
}

fn sentence_start(text: &str, offset: usize, links: &[Range<usize>]) -> usize {
    text[..offset]
        .rmatch_indices(['.', '!', '?', '？', '。', ';', '\n'])
        .find(|(at, separator)| {
            let before = links.partition_point(|link| link.start <= *at);
            *separator == "\n" || before == 0 || !links[before - 1].contains(at)
        })
        .map_or(0, |(at, separator)| at + separator.len())
}

/// 선택지 첫 문장의 요청은 붙어 있는 선택지 묶음 바로 앞의 답변 안내에 연결한다.
fn option_intro_end(text: &str, offset: usize, start: usize) -> Option<usize> {
    let line_start = text[..offset].rfind('\n')? + 1;
    let mut lines = text[line_start..].lines();
    if start > line_start + choice_prefix(lines.next()?)? {
        return None;
    }
    let mut intro_end = line_start - 1;
    let mut previous = text[..intro_end].rsplit('\n');
    let mut intro = previous.next()?;
    if choice_prefix(intro).is_none() {
        choice_prefix(lines.next()?)?;
    } else {
        while choice_prefix(intro).is_some() {
            let prior = previous.next()?;
            intro_end -= intro.len() + 1;
            intro = prior;
        }
    }
    let clean = intro.trim_end_matches(|c: char| c.is_whitespace() || ".!。".contains(c));
    static INSTRUCTION: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?:골라|선택해|답해)\s*(?:주세요|주십시오)$").unwrap());
    INSTRUCTION.is_match(clean).then_some(intro_end - (intro.len() - clean.len()))
}

/// 답변 방법만 적힌 구절은 앞선 카드를 가리킨다. 별도 결정 내용은 허용하지 않는다.
fn reply_method_only(text: &str) -> bool {
    static METHOD: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^\s*카드나\s*채팅(?:\(\s*(?:[1-9][A-D])?\s*(?:등)?\s*\))?으로\s*답해\s*(?:주세요|주십시오)\s*[.!?？。]*\s*$").unwrap()
    });
    METHOD.is_match(text)
}

/// 같은 항목의 보충 정보 요청에만 앞 카드 링크를 적용한다.
fn supplementary_information(prefix: &str, request: &str) -> bool {
    static ADDITION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"도\s*(?:함께\s*)?$").unwrap());
    static DECISION: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"승인|허가|결정|여부|(?:어느|어떤)\s*안|해도|(?:[할될갈말]|고를|고칠|바꿀)\s*(?:지|건지|것인지)")
            .unwrap()
    });
    (request.starts_with("알려") || request.starts_with("말씀해"))
        && ADDITION.is_match(prefix)
        && !DECISION.is_match(prefix)
}

/// 카드 목록의 위치를 가리키는 일반 안내만 답 전체에 연결한다.
/// 다른 요청이 같은 문장에 있어도 해당 '답해 주세요' 위치에만 적용한다.
fn summary_card_count(text: &str, offset: usize) -> Option<usize> {
    static SUMMARY: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"질문\s*(?<count>[0-9]+(?:\s*개)?|하나|한\s*개|둘|두\s*개|셋|세\s*개)\s*에\s*(?<ask>답해\s*(?:주세요|주십시오))").unwrap()
    });
    SUMMARY.captures_iter(text).find_map(|c| {
        if c.name("ask")?.start() != offset {
            return None;
        }
        let count = c["count"].split_whitespace().collect::<String>();
        Some(match count.as_str() {
            "하나" | "한개" => 1,
            "둘" | "두개" => 2,
            "셋" | "세개" => 3,
            _ => count.trim_end_matches('개').parse().unwrap_or(usize::MAX),
        })
    })
}

fn inspect(text: &str, cards: &HashMap<String, CardState>, replacements: &HashMap<String, String>) -> Inspection {
    let list_text = list_text(text, cards, replacements);
    let blocks = blocks(text, &list_text);
    let answer_ids = |text| answer_ids(text, replacements);
    let reply_refs = blocks.iter().flat_map(|b| answer_ids(&b.text)).fold(Vec::new(), |mut refs, id| {
        if let Some(state) = cards.get(&id)
            && !refs.iter().any(|(seen, _)| seen == &id)
        {
            refs.push((id, *state));
        }
        refs
    });
    let mut result = Inspection::default();
    for (index, block) in blocks.iter().enumerate() {
        let required = blocks.iter().filter(|b| b.section == block.section).any(|b| {
            ["카드는 없음", "카드로 올리지", "선택지는 두 가지", "해 주실 것"]
                .iter()
                .any(|marker| b.text.contains(marker))
        });
        let previous_block =
            blocks[..index].last().filter(|prev| prev.section == block.section).map(|prev| prev.text.as_str());
        let requests = requests(&block.text, required, previous_block);
        if requests.is_empty() {
            continue;
        }
        // 링크 원문은 q-ID 확인에 쓰고 링크 안의 문장부호만 경계에서 제외한다.
        let mut links: Vec<Range<usize>> = Vec::new();
        for (event, span) in Parser::new(&block.text).into_offset_iter() {
            if matches!(event, Event::Start(Tag::Link { .. }))
                && links.last().is_none_or(|outer| span.start >= outer.end)
            {
                links.push(span);
            }
        }
        for (rule, offset) in requests {
            let same_section = |b: &Block| {
                if rule == "choices" { b.choices_section == block.choices_section } else { b.section == block.section }
            };
            // 앞 문장의 카드 안내가 같은 줄의 별개 질문까지 면제하지 않는다.
            let start = sentence_start(&block.text, offset, &links);
            let line = block.line + block.text[..offset].bytes().filter(|b| *b == b'\n').count();
            let tail = fresh_tail(&block.text);
            let fresh = tail.is_some();
            let mut own_ids = answer_ids(tail.unwrap_or(&block.text[start..]));
            if own_ids.is_empty()
                && !fresh
                && rule == "request"
                && let Some(end) = option_intro_end(&block.text, offset, start)
            {
                let intro = sentence_start(&block.text, end, &links);
                own_ids = answer_ids(&block.text[intro..end]);
            }
            let unresolved = own_ids.iter().any(|id| !cards.contains_key(id));
            let own =
                own_ids.iter().filter_map(|id| cards.get(id).map(|state| (id.clone(), *state))).collect::<Vec<_>>();
            // 새 요청이 같은 문단의 옛 q-ID 뒤에 붙어도 옛 카드로 가리지 않는다.
            let mut linked = if unresolved { Vec::new() } else { own };
            if linked.is_empty() && !fresh && !unresolved {
                if rule == "request"
                    && list_text.get(&line) == Some(&true)
                    && supplementary_information(&block.text[start..offset], &block.text[offset..])
                {
                    continue;
                }
                // 요청 뒤 목록·설명을 지나 같은 주제의 카드 링크를 찾는다.
                linked = blocks[index + 1..]
                    .iter()
                    .take_while(|next| same_section(next))
                    .flat_map(|next| answer_ids(&next.text))
                    .filter_map(|id| cards.get(&id).map(|state| (id, *state)))
                    .collect();
                // 질문 1·2를 따로 묻고 링크를 뒤에 모았으면 같은 카드 하나로 둘을 면제하지 않는다.
                if let Some(group) = block.question_group {
                    let groups = blocks.iter().filter(|b| same_section(b)).filter_map(|b| b.question_group).fold(
                        Vec::new(),
                        |mut groups, group| {
                            if !groups.contains(&group) {
                                groups.push(group);
                            }
                            groups
                        },
                    );
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
                            && (reply_method_only(&block.text[start..])
                                || ["직접 답", "카드에", "카드에서", "그 질문", "위 질문", "해당 질문"]
                                    .iter()
                                    .any(|s| block.text[start..].contains(s))))
                {
                    linked = answer_ids(&block.text[..start])
                        .into_iter()
                        .chain(
                            blocks[..index]
                                .iter()
                                .rev()
                                .take_while(|prev| same_section(prev))
                                .flat_map(|prev| answer_ids(&prev.text)),
                        )
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
                    .any(|s| block.text[start..].contains(s))
            {
                linked = reply_refs.clone();
            }
            if !fresh
                && !unresolved
                && rule == "request"
                && let Some(count) = summary_card_count(&block.text, offset)
            {
                // 같은 카드의 링크·별칭을 여러 번 적어도 질문 수를 채우지 못한다.
                linked = if reply_refs.len() >= count { reply_refs.clone() } else { Vec::new() };
            }
            let evidence = Evidence { rule, line, refs: linked, fresh };
            if evidence.refs.is_empty() {
                if result.missing.as_ref().is_none_or(|previous| evidence.fresh && !previous.fresh) {
                    result.missing = Some(evidence);
                }
            } else if evidence.refs.iter().any(|(_, state)| *state == CardState::Closed) {
                result.stale.get_or_insert(evidence);
            }
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
        let mut confirmed_replacements = HashMap::new();
        let referenced = ids(&reply.text).into_iter().take(128).collect::<Vec<_>>();
        for id in &referenced {
            if open.iter().any(|q| q.project.as_deref() == Some(&project) && q.short_id.as_ref().unwrap_or(&q.id) == id)
            {
                cards.insert(id.clone(), CardState::Open);
            } else if let Some(answered) = self.db.question_guard_history(&project, id)? {
                if !answered
                    && let Some(target) = self.db.question_target(Some(&project), id)?
                    && target.kind == "thread"
                {
                    let latest: Vec<_> = open
                        .iter()
                        .filter(|q| {
                            q.project.as_deref() == Some(&project) && q.thread.as_deref() == Some(&target.subject)
                        })
                        .map(|q| q.short_id.clone().unwrap_or_else(|| q.id.clone()))
                        .collect();
                    // 같은 thread 안의 다른 질문이나 순서가 바뀐 기존 카드는 교체 근거가 아니다.
                    if target.revision.strip_prefix("retired:") == Some(id.as_str()) {
                        for candidate in &latest {
                            if referenced.contains(candidate)
                                && aaa_proto::cmp_short_ids(candidate, id).is_gt()
                                && let Some(next) = self.db.question_target(Some(&project), candidate)?
                                && next.position == target.position
                                && !next.revision.starts_with("retired:")
                            {
                                confirmed_replacements.insert(id.clone(), candidate.clone());
                                break;
                            }
                        }
                    }
                    replacements.insert(id.clone(), latest);
                }
                cards.insert(id.clone(), if answered { CardState::Answered } else { CardState::Closed });
            }
        }
        let inspection = inspect(&reply.text, &cards, &confirmed_replacements);
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
