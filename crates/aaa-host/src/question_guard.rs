//! coordinator 완료 답의 요청과 카드 연결을 검사한다. 원문은 진단에 복사하지 않는다.
use crate::Host;
use aaa_proto::HarnessEvent;
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    sync::LazyLock,
};

pub(crate) const EVENT: &str = "question card missing";
pub(crate) const STALE_EVENT: &str = "question card outdated";
pub(crate) const REMINDER: &str = "사용자 답을 구하는 표현과 연결된 질문 카드를 확인하지 못했습니다. 실제 답이 필요한 요청이면 aaa post user --request --needs-user --topic user-action --title '<질문 제목>' --body-file <질문 파일>로 카드를 만드세요. 기존 카드 안내라면 해당 q-ID와 상태를 확인하세요.";
pub(crate) const STALE_REMINDER: &str = "안내한 질문 카드에 이미 답했거나 카드가 닫혔습니다. 최신 카드와 이미 받은 답을 확인해 안내를 갱신하세요. 같은 질문을 새로 만들지 마세요.";

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

const CARD_REFERENCE: &str = r"(?:\[[^\]\n]*\]\([^()\n]*q-[0-9]+[^()\n]*\)|q-[0-9]+)";

/// 교체·폐기·답변 불필요 설명의 참조만 제외한다. 같은 번호의 실제 답 안내는 남긴다.
fn answer_ids(text: &str, replacements: &HashMap<String, String>) -> Vec<String> {
    static HISTORY: LazyLock<Regex> = LazyLock::new(|| {
        let reference = CARD_REFERENCE;
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

/// 거둔 번호와 바로 이어서 답하라고 지정한 새 번호의 상태를 확인한다.
fn mask_withdrawn_card(text: &mut String, next_paragraph: &str, cards: &HashMap<String, CardState>) {
    static WITHDRAWAL: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(&format!(
            r"(?<old>{CARD_REFERENCE})\*{{0,2}}[ \t]*(?:은|는|이|가)[ \t]*상황이\s*바뀌어\s*(?:닫았|폐기했)습니다\*{{0,2}}[.。]"
        ))
        .unwrap()
    });
    static SUCCESSOR: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(&format!(
            r"(?x)^\*{{0,2}}\s*(?:(?:이제|대신|지금\s*답할\s*(?:것은|건))\s+)?\*{{0,2}}\s*
            (?:새\s+(?:질문\s+)?카드|지금\s*답할\s*카드는)\*{{0,2}}\s*[:：]?\s*\*{{0,2}}(?<new>{CARD_REFERENCE})\*{{0,2}}\s*
            (?:(?:에서|으로|로|에)\s*(?:(?:답해|선택해)\s*(?:주세요|주십시오)|답하시면\s*됩니다)
            |하나\*{{0,2}}\s*(?:입니다|만\s*답하시면\s*됩니다))\b"
        ))
        .unwrap()
    });
    let history = WITHDRAWAL.captures_iter(text).filter_map(|c| {
        let old = c.name("old")?;
        if text[..old.start()].ends_with(|ch: char| ch.is_ascii_alphanumeric() || ch == '-') {
            return None;
        }
        let following = text[c.get(0)?.end()..].trim_start();
        let following = if following.is_empty() { next_paragraph.trim_start() } else { following };
        let new = SUCCESSOR.captures(following)?.name("new")?;
        let old_ids = ids(old.as_str());
        let new_ids = ids(new.as_str());
        let ([old_id], [new_id]) = (old_ids.as_slice(), new_ids.as_slice()) else { return None };
        (cards.get(old_id) == Some(&CardState::Closed)
            && cards.get(new_id) == Some(&CardState::Open)
            && aaa_proto::cmp_short_ids(new_id, old_id).is_gt())
        .then(|| old.range())
    });
    for span in history.collect::<Vec<_>>() {
        text.replace_range(span.clone(), &" ".repeat(span.len()));
    }
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
    has_link: bool,
}

fn indent_end(text: &str, column: usize) -> usize {
    text.bytes()
        .take_while(|b| matches!(b, b' ' | b'\t'))
        .fold(column, |col, b| if b == b'\t' { col + 4 - col % 4 } else { col + 1 })
}

#[derive(Clone, Copy)]
struct ListLine {
    item_line: usize,
    follows_card_link: bool,
}

struct OrderedList {
    start: usize,
    end: usize,
    items: Vec<usize>,
}

#[derive(Default)]
struct Markdown {
    list_lines: HashMap<usize, ListLine>,
    ordered: Vec<OrderedList>,
    item_cards: HashMap<usize, Vec<String>>,
    headings: HashSet<usize>,
    omitted: HashSet<usize>,
}

/// CommonMark의 제목·인용·코드와 목록 항목을 원문 행에 대응시킨다.
fn markdown(text: &str) -> Markdown {
    let starts = std::iter::once(0).chain(text.match_indices('\n').map(|(at, _)| at + 1)).collect::<Vec<_>>();
    let line_at = |offset| starts.partition_point(|&start| start <= offset);
    let source_line = |line| text[starts[line - 1]..].lines().next().unwrap_or_default();
    let mut items: Vec<ListItem> = Vec::new();
    let mut lists = Vec::new();
    let mut excluded = 0;
    let mut markdown = Markdown::default();
    for (event, span) in Parser::new(text).into_offset_iter() {
        match event {
            Event::Start(Tag::BlockQuote(_) | Tag::CodeBlock(_)) => {
                markdown.omitted.extend(line_at(span.start)..=line_at(span.end.saturating_sub(1)));
                excluded += 1;
            }
            Event::Start(Tag::Heading { .. }) => {
                markdown.headings.insert(line_at(span.start));
                excluded += 1;
            }
            Event::End(TagEnd::BlockQuote(_) | TagEnd::CodeBlock | TagEnd::Heading(_)) => excluded -= 1,
            Event::Start(Tag::List(ordinal)) => lists.push(if ordinal.is_some() && excluded == 0 {
                let index = markdown.ordered.len();
                markdown.ordered.push(OrderedList {
                    start: line_at(span.start),
                    end: line_at(span.end.saturating_sub(1)),
                    items: Vec::new(),
                });
                Some(index)
            } else {
                None
            }),
            Event::End(TagEnd::List(_)) => {
                lists.pop();
            }
            Event::Start(Tag::Item) => {
                let line = line_at(span.start);
                let marker = &text[span];
                let width = marker.find([' ', '\t', '\r', '\n']).unwrap_or(marker.len());
                let marker_end = indent_end(source_line(line), 0) + width;
                let indent = indent_end(&marker[width..], marker_end).max(marker_end + 1);
                if let Some(Some(list)) = lists.last()
                    && excluded == 0
                {
                    markdown.ordered[*list].items.push(line);
                }
                items.push(ListItem { line, indent, has_link: false });
            }
            Event::End(TagEnd::Item) => {
                items.pop();
            }
            Event::Start(Tag::Link { dest_url, .. }) if excluded == 0 => {
                if let Some(item) = items.last_mut()
                    && line_at(span.start) == item.line
                {
                    let card_ids = ids(&dest_url);
                    item.has_link |= !card_ids.is_empty() || !ids(&text[span.clone()]).is_empty();
                    let prefix = text[starts[item.line - 1]..span.start].trim().trim_end_matches(['*', '_']).trim_end();
                    if let Some(number) = prefix.strip_suffix(['.', ')'])
                        && !number.is_empty()
                        && number.bytes().all(|b| b.is_ascii_digit())
                        && !card_ids.is_empty()
                    {
                        markdown.item_cards.insert(item.line, card_ids);
                    }
                }
            }
            Event::Text(_) if excluded == 0 => {
                if let Some(item) = items.last() {
                    let line = line_at(span.start);
                    if line == item.line || indent_end(source_line(line), 0) >= item.indent {
                        markdown.list_lines.insert(
                            line,
                            ListLine { item_line: item.line, follows_card_link: line != item.line && item.has_link },
                        );
                    }
                }
            }
            _ => {}
        }
    }
    markdown
}

/// 인용·코드를 제외하고 제목과 문단의 연결을 보존한다.
fn blocks(text: &str, markdown: &Markdown) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    let mut section = 0;
    let mut choices_section = 0;
    let mut question_group = None;
    let mut separated = true;
    for (index, line) in text.lines().enumerate() {
        let t = line.trim_start();
        if t.is_empty()
            || markdown.omitted.contains(&(index + 1))
            || (line.starts_with("    ") || line.starts_with('\t')) && !markdown.list_lines.contains_key(&(index + 1))
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
        let title = markdown.headings.contains(&(index + 1)) || t.starts_with("**") && t.ends_with("**");
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

fn choice_prefix(line: &str) -> Option<usize> {
    static CHOICE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^\s*(?:[-*]\s*)?\*{0,2}[A-D](?:[.)]|\(추천\))?:?\*{0,2}:?\s+").unwrap());
    CHOICE.find(line).map(|m| m.end()).filter(|&end| end < line.len())
}

struct Request {
    rule: &'static str,
    offset: usize,
    scope: Range<usize>,
    fresh_at: Option<usize>,
}

fn requests(text: &str, required: bool, previous_block: Option<&str>) -> Vec<Request> {
    static QUESTION: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)[^\n.!?。;,，]*?(?:[가-힣]+까요|[가-힣]+나요|하시겠|원하시|어느|어떤|무엇|어떻게|해도 될|진행해도)[^\n.!?。;]*[?？]|\b(?:shall i|should i|can i|would you like|do you want|which .{0,100}prefer)\b[^\n]*[?？]").unwrap()
    });
    static ASK: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)(?:골라|선택해|알려|말씀해|정해|답해|확인해)[^\n.!?。;,:：]{0,80}(?:주세요|주십시오)|(?:라고|이라고)(?:만)?\s*해\s*(?:주세요|주십시오)|\b(?:let me know|please choose)\b").unwrap()
    });
    static CONDITIONAL: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)(?:원하시면|원하신다면|싶으시면|싶으면|시려면|시작하려면|뜻이 다르면|다른 뜻|만료되면|재부팅|끝나면|그 뒤에|나중에|흐리면|문제가.*면|카드가 뜨면|카드가 나오면|올라오면|옮길 때|때가 되면|이었다면|시간이 지나면|항목이 있으면|if you want|if you'd like|if needed)").unwrap()
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
    static COMPLETED_RESULT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?:^|[.!?？。\n])[^.!?？。\n]*(?:처리했습니다|처리됐습니다|처리를\s*마쳤습니다|반영했습니다|적용했습니다|완료했습니다|확정했습니다|두기로\s*했습니다|그대로입니다)[.!。]\s*$").unwrap()
    });
    static RESULT_FEEDBACK: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?:^|[,，])\s*(?:(?:체감이\s*어땠는지|체감상\s*빨라졌는지)|[가-힣 ]{1,32}\s*변경도\s*하지\s*않으려던\s*(?:것이면|거라면))\s*[,，]?\s*(?:알려|말씀해)\s*(?:주세요|주십시오)\b").unwrap()
    });
    static NO_INFORMATION_NEEDED: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?:정해\s*주실\s*것|알려\s*주시면\s*좋은\s*것)(?:은|이|도)?\s*없(?:습니다|어요|고|으며|지만|는데|다)",
        )
        .unwrap()
    });
    let mut out = Vec::new();
    // 조건부 제안은 실제 질문 어미와 분리한다. 한 문장의 제안이 다음 문장의 요청을 가리지 않는다.
    let mut offset = 0;
    let mut previous = previous_block.unwrap_or_default();
    let mask = |m: &regex::Captures<'_>| " ".repeat(m[0].len());
    for sentence in text.split_inclusive(['.', '!', '?', '？', '。', ';', '\n']) {
        let mut matches = QUESTION.find_iter(sentence).map(|m| ("question", m.range())).collect::<Vec<_>>();
        // 직전 확정·완료 안내에 붙은 의견 구절만 같은 바이트 길이의 공백으로 바꾼다.
        let actionable = if NAME_FEEDBACK.is_match(sentence) && DECIDED_NAME.is_match(previous) {
            NAME_FEEDBACK.replace_all(sentence, mask)
        } else if RESULT_FEEDBACK.is_match(sentence) && COMPLETED_RESULT.is_match(previous) {
            RESULT_FEEDBACK.replace_all(sentence, mask)
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
        for ask in ASK.find_iter(sentence) {
            matches.push(("request", ask.range()));
        }
        if (sentence.contains("알려 주시면 좋은")
            || sentence.contains("정해 주실 것")
            || required && ["답해 주시면", "알려 주시면", "정해 주시면"].iter().any(|s| sentence.contains(s)))
            && !sentence.contains("나중")
        {
            matches.push(("required_information", 0..sentence.len()));
        }
        matches.sort_by_key(|(_, span)| span.start);
        for (i, (rule, span)) in matches.iter().enumerate() {
            let previous_end = if i == 0 { 0 } else { matches[i - 1].1.end.min(span.start) };
            let end = matches.get(i + 1).map_or(sentence.len(), |(_, next)| next.start.max(span.end));
            // 쉼표 앞 설명은 새 주제 여부에 쓰되, 직전 요청이 끝나면 새 요청을 따로 본다.
            let start = sentence[previous_end..span.start].rfind([',', '，']).map_or(previous_end, |at| {
                let comma = sentence[previous_end + at..].chars().next().unwrap();
                previous_end + at + comma.len_utf8()
            });
            let end = sentence[span.end..end].find([',', '，']).map_or(end, |at| span.end + at);
            let context = &sentence[previous_end..end];
            let verification = sentence[span.clone()].starts_with("확인해")
                && !["할지", "될지", "여부", "맞는지", "괜찮은지", "승인", "결정"].iter().any(|s| context.contains(s));
            let future_permission = context.contains("해도 되면") && text.contains("여쭙겠습니다");
            if *rule != "question" && (verification || CONDITIONAL.is_match(context) || future_permission) {
                continue;
            }
            let fresh_at = fresh_tail(context).map(|tail| offset + start.max(end - tail.len()));
            out.push(Request { rule, offset: offset + span.start, scope: offset + start..offset + end, fresh_at });
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
        let fresh_at = fresh_tail(text).map(|tail| text.len() - tail.len());
        out.push(Request { rule: "choices", offset: 0, scope: 0..text.len(), fresh_at });
    }
    out
}

/// 링크는 카드 참조에서 원문을 쓰고, 문장 감지에서는 URL과 링크 제목을 건너뛴다.
fn request_prose(text: &str) -> String {
    let mut prose = text.to_owned();
    for (event, span) in Parser::new(text).into_offset_iter() {
        if matches!(event, Event::Start(Tag::Link { .. } | Tag::Image { .. })) {
            let blank = text[span.clone()].bytes().map(|b| if b == b'\n' { '\n' } else { ' ' }).collect::<String>();
            prose.replace_range(span, &blank);
        }
    }
    prose
}

fn sentence_start(text: &str, offset: usize) -> usize {
    text[..offset]
        .rfind(['.', '!', '?', '？', '。', ';', '\n'])
        .map_or(0, |at| at + text[at..].chars().next().unwrap().len_utf8())
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

/// 대상이 없는 선택 안내는 같은 주제에서 이미 소개한 카드 하나를 받는다.
fn selection_guidance(text: &str) -> bool {
    static GUIDANCE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(concat!(
            r"(?x)^\s*[*]*\s*(?:(?:그중|이\s*중|여기서)\s+)?",
            r"(?:(?:(?:시안|화면|자료|그림|영상|검수본|내용)(?:을|를)\s+)?",
            r"(?:보고|읽고|(?:읽어\s+)?보시고|살펴보시고|보신\s*(?:뒤|후|다음)|(?:확인|검토)(?:하고|하신\s*(?:뒤|후)|\s*후))\s+)?",
            r"(?:(?:둘|두\s*가지|두\s*안|[A-D](?:\s*/\s*[A-D])+)(?:\s*중(?:에서)?)\s+)?",
            r"(?:(?:하나|한\s*가지)(?:를)?\s+|(?:[A-D]|한\s*안|원하(?:는|시는)\s*(?:안|쪽|선택지))(?:을|를)?\s+)?",
            r"(?:골라|선택해|정해|답해)\s*(?:주세요|주십시오)[.!?？。*]*\s*$"
        ))
        .unwrap()
    });
    GUIDANCE.is_match(text)
}

/// "두 질문 모두"처럼 요청이 받을 카드 수를 밝힌 경우다.
fn named_question_group(text: &str) -> Option<(Range<usize>, usize)> {
    static GROUP: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(concat!(
            r"(?x)(?:(?<before>[0-9]+|한|두|세)\s*(?:질문|카드)",
            r"|(?:질문|카드)\s*(?<after>[0-9]+\s*개?|한\s*개|두\s*개|세\s*개))\s*(?:모두|각각)\b",
        ))
        .unwrap()
    });
    let matched = GROUP.captures_iter(text).last()?;
    let count = card_count(matched.name("before").or_else(|| matched.name("after"))?.as_str());
    (count > 1).then(|| (matched.get(0).unwrap().range(), count))
}

fn lettered_options(sentence: &str) -> bool {
    static OPTIONS: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(concat!(
            r"(?x)^\s*(?:[^,.!?。\n]{1,60}면\s+[A-D]\s*[,，]\s*)+",
            r"[^,.!?。\n]{1,60}면\s+[A-D](?:을|를)?\s+(?:골라|선택해)\s*(?:주세요|주십시오)[.!?？。]*\s*$",
        ))
        .unwrap()
    });
    OPTIONS.is_match(sentence)
}

fn decision_details(text: &str) -> bool {
    static DECISION: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"승인|허가|결정|여부|(?:어느|어떤)\s*안|해도|(?:[할될갈말]|고를|고칠|바꿀|지울|삭제할)\s*(?:지|건지|것인지)")
            .unwrap()
    });
    DECISION.is_match(text)
}

/// 같은 항목의 보충 정보 요청에만 앞 카드 링크를 적용한다.
fn supplementary_information(prefix: &str, request: &str) -> bool {
    static ADDITION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"도\s*(?:함께\s*)?$").unwrap());
    (request.starts_with("알려") || request.starts_with("말씀해"))
        && ADDITION.is_match(prefix)
        && !decision_details(prefix)
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
        Some(card_count(&c["count"]))
    })
}

fn card_count(count: &str) -> usize {
    let count = count.split_whitespace().collect::<String>();
    match count.as_str() {
        "하나" | "한" | "한개" => 1,
        "둘" | "두" | "두개" => 2,
        "셋" | "세" | "세개" => 3,
        _ => count.trim_end_matches('개').parse().unwrap_or(usize::MAX),
    }
}

enum CardLink {
    Mentioned(Vec<String>),
    Guidance { expected: usize, ids: Vec<String> },
    Supplementary(Vec<String>),
    SeparateRequest,
}

impl CardLink {
    fn evidence(self, request: &Request, line: usize, cards: &HashMap<String, CardState>) -> Evidence {
        let resolve = |ids: Vec<String>| -> Option<Vec<_>> {
            ids.into_iter().map(|id| cards.get(&id).map(|&state| (id, state))).collect()
        };
        let separate = matches!(self, Self::SeparateRequest);
        let refs = match self {
            Self::Mentioned(ids) => resolve(ids),
            Self::Guidance { expected, ids } if ids.len() == expected => resolve(ids),
            Self::Supplementary(ids) if matches!(ids.as_slice(), [id] if cards.get(id) == Some(&CardState::Open)) => {
                resolve(ids)
            }
            _ => None,
        };
        Evidence {
            rule: request.rule,
            line,
            refs: refs.unwrap_or_default(),
            fresh: separate || request.fresh_at.is_some(),
        }
    }
}

struct CardLinks<'a> {
    blocks: &'a [Block],
    markdown: &'a Markdown,
    replacements: &'a HashMap<String, String>,
    cards: &'a HashMap<String, CardState>,
    reply_ids: Vec<String>,
}

impl CardLinks<'_> {
    fn ids(&self, text: &str) -> Vec<String> {
        answer_ids(text, self.replacements)
    }

    fn prior_selection_ids(&self, index: usize, before: usize, line: usize) -> Vec<String> {
        let current = &self.blocks[index];
        let item = self.markdown.list_lines.get(&line).map(|list| list.item_line);
        let mut ids = Vec::new();
        for (at, block) in self.blocks[..=index].iter().enumerate().rev() {
            if block.section != current.section || block.question_group != current.question_group {
                break;
            }
            let text = if at == index { &block.text[..before] } else { &block.text };
            if let Some(item_line) = item {
                let own_lines = text
                    .lines()
                    .enumerate()
                    .filter(|(n, _)| {
                        self.markdown.list_lines.get(&(block.line + n)).is_some_and(|m| m.item_line == item_line)
                    })
                    .map(|(_, line)| line);
                extend_unique(&mut ids, own_lines.flat_map(|line| self.ids(line)));
            } else {
                extend_unique(&mut ids, self.ids(text));
            }
        }
        ids
    }

    fn line(&self, index: usize, line: usize) -> Option<&str> {
        self.blocks[..=index].iter().rev().find_map(|b| b.text.lines().nth(line.checked_sub(b.line)?))
    }

    fn named_group_ids(&self, index: usize, line: usize, expected: usize) -> Vec<String> {
        let block = &self.blocks[index];
        let topic_start = self.blocks[..=index]
            .iter()
            .rev()
            .take_while(|b| b.section == block.section && b.question_group == block.question_group)
            .last()
            .map_or(block.line, |b| b.line);
        let Some(list) = self.markdown.ordered.iter().rev().find(|list| list.start >= topic_start && list.end < line)
        else {
            return Vec::new();
        };
        if list.items.len() != expected {
            return Vec::new();
        }
        let mut result = Vec::new();
        for &item in &list.items {
            let Some((id, source)) = self.markdown.item_cards.get(&item).and_then(|cards| {
                let [id] = cards.as_slice() else { return None };
                Some((id, self.line(index, item)?))
            }) else {
                return Vec::new();
            };
            if !self.ids(source).contains(id) || result.contains(id) {
                return Vec::new();
            }
            result.push(id.clone());
        }
        result
    }

    fn link(&self, index: usize, request: &Request, prose: &str, line: usize, has_next: bool) -> CardLink {
        let block = &self.blocks[index];
        let fresh = request.fresh_at.is_some();
        let start = request.fresh_at.unwrap_or(request.scope.start);
        let is_request = request.rule == "request";
        let is_choices = request.rule == "choices";
        let same_section = |other: &Block| {
            if is_choices { other.choices_section == block.choices_section } else { other.section == block.section }
        };

        let mut own = self.ids(&block.text[start..request.scope.end]);
        if own.is_empty()
            && !fresh
            && is_request
            && let Some(end) = option_intro_end(prose, request.offset, request.scope.start)
        {
            let intro = sentence_start(prose, end);
            own = self.ids(&block.text[intro..end]);
        }
        // 기존 카드의 빠른 응답은 명시한 카드의 증거를 없애지 않는다.
        if fresh || own.iter().any(|id| !self.cards.contains_key(id)) {
            return CardLink::Mentioned(own);
        }
        if is_request && let Some(count) = summary_card_count(prose, request.offset) {
            let ids = if self.reply_ids.len() >= count { self.reply_ids.clone() } else { Vec::new() };
            return CardLink::Mentioned(ids);
        }
        let clause = &prose[request.scope.clone()];
        if is_request
            && own.is_empty()
            && let Some((group, expected)) = named_question_group(&prose[..request.offset])
        {
            let after_group = group.end.saturating_sub(request.scope.start);
            if selection_guidance(&clause[after_group..])
                || lettered_options(&prose[sentence_start(prose, request.scope.start)..request.scope.end])
            {
                return CardLink::Guidance { expected, ids: self.named_group_ids(index, line, expected) };
            }
        }
        if is_request && selection_guidance(clause) {
            let ids = if own.is_empty() { self.prior_selection_ids(index, request.scope.start, line) } else { own };
            if !ids.is_empty() {
                return CardLink::Guidance { expected: 1, ids };
            }
        } else if !own.is_empty() {
            return CardLink::Mentioned(own);
        }

        if is_request
            && let Some(list) = self.markdown.list_lines.get(&line).filter(|m| m.follows_card_link)
            && supplementary_information(&prose[request.scope.start..request.offset], &prose[request.offset..])
        {
            return CardLink::Supplementary(
                self.line(index, list.item_line).map_or_else(Vec::new, |line| self.ids(line)),
            );
        }

        let earlier = self.prior_selection_ids(index, request.scope.start, line);
        let known = |id: &String| self.cards.contains_key(id);
        let mut linked = Vec::new();
        // 질문 뒤에 붙인 카드 목록은 연결한다. 이미 등장한 카드로 뒤의 별개 결정을 덮지 않는다.
        if is_choices || earlier.is_empty() && !has_next {
            extend_unique(
                &mut linked,
                self.ids(&block.text[request.scope.end..])
                    .into_iter()
                    .chain(
                        self.blocks[index + 1..].iter().take_while(|b| same_section(b)).flat_map(|b| self.ids(&b.text)),
                    )
                    .filter(known),
            );
            if let Some(group) = block.question_group {
                let mut groups = Vec::new();
                for other in self.blocks.iter().filter(|b| same_section(b)).filter_map(|b| b.question_group) {
                    if !groups.contains(&other) {
                        groups.push(other);
                    }
                }
                if groups.len() > 1 {
                    linked = groups
                        .iter()
                        .position(|g| *g == group)
                        .and_then(|i| linked.get(i))
                        .cloned()
                        .into_iter()
                        .collect();
                }
            }
        }
        if linked.is_empty()
            && (is_choices
                || is_request
                    && !decision_details(clause)
                    && (reply_method_only(clause)
                        || ["직접 답", "카드에", "카드에서", "그 질문", "위 질문", "해당 질문"]
                            .iter()
                            .any(|part| clause.contains(part))))
        {
            extend_unique(
                &mut linked,
                self.ids(&block.text[..request.scope.start])
                    .into_iter()
                    .chain(
                        self.blocks[..index]
                            .iter()
                            .rev()
                            .take_while(|b| same_section(b))
                            .flat_map(|b| self.ids(&b.text)),
                    )
                    .filter(known),
            );
        }
        if linked.is_empty()
            && is_request
            && ["카드를 확인", "아래 질문", "아래 새 카드", "새 카드로 답", "새 번호로 답"]
                .iter()
                .any(|part| clause.contains(part))
        {
            linked = self.reply_ids.clone();
        }
        // 그 턴에서 앞 카드를 실제로 올렸어도 뒤의 별개 승인은 새 카드가 필요하다.
        if linked.is_empty()
            && !earlier.is_empty()
            && (request.rule == "question" || is_request && decision_details(clause))
        {
            return CardLink::SeparateRequest;
        }
        CardLink::Mentioned(linked)
    }
}

fn extend_unique(ids: &mut Vec<String>, new: impl IntoIterator<Item = String>) {
    for id in new {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
}

fn inspect(text: &str, cards: &HashMap<String, CardState>, replacements: &HashMap<String, String>) -> Inspection {
    let markdown = markdown(text);
    let mut blocks = blocks(text, &markdown);
    for index in 0..blocks.len() {
        let (current, later) = blocks.split_at_mut(index + 1);
        let block = &mut current[index];
        let next = later
            .first()
            .filter(|next| next.section == block.section && next.question_group == block.question_group)
            .map_or("", |next| next.text.as_str());
        mask_withdrawn_card(&mut block.text, next, cards);
    }

    let mut links = CardLinks { blocks: &blocks, markdown: &markdown, replacements, cards, reply_ids: Vec::new() };
    let refs = blocks.iter().flat_map(|b| links.ids(&b.text)).filter(|id| cards.contains_key(id));
    let mut reply_ids = Vec::new();
    extend_unique(&mut reply_ids, refs);
    links.reply_ids = reply_ids;

    let required_sections = blocks
        .iter()
        .filter(|b| {
            ["카드는 없음", "카드로 올리지", "선택지는 두 가지", "해 주실 것"]
                .iter()
                .any(|marker| b.text.contains(marker))
        })
        .map(|b| b.section)
        .collect::<HashSet<_>>();
    let mut result = Inspection::default();
    for (index, block) in blocks.iter().enumerate() {
        let previous =
            blocks[..index].last().filter(|prev| prev.section == block.section).map(|prev| prev.text.as_str());
        let prose = request_prose(&block.text);
        let requests = requests(&prose, required_sections.contains(&block.section), previous);
        for (i, request) in requests.iter().enumerate() {
            let line = block.line + block.text[..request.offset].bytes().filter(|b| *b == b'\n').count();
            let has_next = requests[i + 1..].iter().any(|next| next.rule != "choices");
            let link = links.link(index, request, &prose, line, has_next);
            let selection = matches!(link, CardLink::Guidance { .. });
            let evidence = link.evidence(request, line, cards);
            if evidence.refs.is_empty() {
                if result.missing.as_ref().is_none_or(|previous| evidence.fresh && !previous.fresh) {
                    result.missing = Some(evidence);
                }
            } else if evidence
                .refs
                .iter()
                .any(|(_, state)| *state == CardState::Closed || selection && *state == CardState::Answered)
            {
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
