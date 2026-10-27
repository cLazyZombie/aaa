//! 기존 증분 Reader에서 완료 답만 고른다. 도구 결과·중간 답·sidechain은 완료 답이 아니다.
use aaa_proto::TurnReply;
use serde_json::Value;

#[derive(Default)]
pub(crate) struct Reader {
    started: Option<u64>,
    text: String,
    pub completed: Vec<TurnReply>,
}

impl Reader {
    pub fn line(&mut self, source: crate::plan::Source, line: &str) {
        // 일반 도구 출력·usage를 또 JSON으로 읽지 않는다.
        if ![
            "\"user\"",
            "\"assistant\"",
            "task_started",
            "task_complete",
            "turn_started",
            "turn_complete",
            "turn_duration",
            "turn_aborted",
        ]
        .iter()
        .any(|s| line.contains(s))
        {
            return;
        }
        let Ok(v) = serde_json::from_str::<Value>(line).inspect_err(|error| {
            tracing::debug!(%error, "완료 답 기록 JSON 파싱 실패");
        }) else {
            return;
        };
        let Some(at) = v["timestamp"].as_str().and_then(aaa_common::util::parse_iso) else { return };
        match source {
            crate::plan::Source::Codex => {
                if v["type"] != "event_msg" {
                    return;
                }
                let p = &v["payload"];
                match p["type"].as_str().unwrap_or_default() {
                    "task_started" | "turn_started" => self.start(at),
                    "task_complete" | "turn_complete" => {
                        self.text = p["last_agent_message"].as_str().unwrap_or_default().chars().take(16_000).collect();
                        self.finish(at);
                    }
                    "turn_aborted" => {
                        self.started = None;
                        self.text.clear();
                    }
                    _ => {}
                }
            }
            crate::plan::Source::Claude => {
                if v["isSidechain"] == true {
                    return;
                }
                let content = v.pointer("/message/content").and_then(Value::as_array);
                match v["type"].as_str().unwrap_or_default() {
                    "user"
                        if v["isMeta"] != true
                            && !content.is_some_and(|c| c.iter().any(|b| b["type"] == "tool_result")) =>
                    {
                        let text =
                            v.pointer("/message/content").and_then(Value::as_str).map(str::to_owned).unwrap_or_else(
                                || {
                                    content
                                        .into_iter()
                                        .flatten()
                                        .filter_map(|b| b["text"].as_str())
                                        .collect::<Vec<_>>()
                                        .join("\n")
                                },
                            );
                        if crate::chat::claude_human(&v, &text) {
                            self.start(at);
                        }
                    }
                    "assistant" => {
                        let Some(c) = content else { return };
                        if c.iter().any(|b| b["type"] == "tool_use") {
                            self.text.clear();
                            return;
                        }
                        let text = c
                            .iter()
                            .filter(|b| b["type"] == "text")
                            .filter_map(|b| b["text"].as_str())
                            .collect::<Vec<_>>()
                            .join("\n");
                        if !text.is_empty() {
                            self.text = text.chars().take(16_000).collect();
                        }
                    }
                    // end_turn 뒤 Stop hook이 계속 일할 수도 있다. harness의 턴 길이 사건까지 기다린다.
                    "system" if v["subtype"] == "turn_duration" => self.finish(at),
                    _ => {}
                }
            }
        }
    }

    fn start(&mut self, at: u64) {
        self.started = Some(at);
        self.text.clear();
    }

    fn finish(&mut self, at: u64) {
        if let Some(started_ms) = self.started.take().filter(|s| *s <= at)
            && !self.text.is_empty()
        {
            // 처음 읽는 긴 기록에서도 메모리 상한을 지킨다. 최근 완료 턴만 전송한다.
            if self.completed.len() == 32 {
                self.completed.remove(0);
            }
            self.completed.push(TurnReply { started_ms, finished_ms: at, text: std::mem::take(&mut self.text) });
        }
        self.text.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::Source::*;
    use serde_json::json;

    #[test]
    fn both_harnesses_only_publish_completed_main_turns() {
        let mut r = Reader::default();
        let mut line = |source, mut v: Value| {
            v["timestamp"] = json!("2026-10-27T01:00:00Z");
            r.line(source, &v.to_string());
        };
        line(Codex, json!({"type":"event_msg","payload":{"type":"task_started"}}));
        line(Codex, json!({"type":"response_item","payload":{"role":"assistant","text":"중간 질문?"}}));
        line(Codex, json!({"type":"event_msg","payload":{"type":"task_complete","last_agent_message":"진행할까요?"}}));
        line(Claude, json!({"type":"user","message":{"content":"시작"}}));
        line(
            Claude,
            json!({"type":"assistant","isSidechain":true,"message":{"stop_reason":"end_turn","content":[{"type":"text","text":"하위 질문?"}]}}),
        );
        line(
            Claude,
            json!({"type":"assistant","message":{"content":[{"type":"text","text":"도구 전 질문?"},{"type":"tool_use"}]}}),
        );
        line(Claude, json!({"type":"user","message":{"content":[{"type":"tool_result","content":"결과 질문?"}]}}));
        line(
            Claude,
            json!({"type":"assistant","message":{"stop_reason":null,"content":[{"type":"text","text":"선택해 주세요."}]}}),
        );
        line(Claude, json!({"type":"system","subtype":"turn_duration"}));
        line(Claude, json!({"type":"system","subtype":"turn_duration"}));
        assert_eq!(r.completed.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(), ["진행할까요?", "선택해 주세요."]);
    }
}
