//! 완료된 native turn의 오류만 읽는다. 재시도 중 오류·도구 결과·본문의 오류 단어는 근거가 아니다.
use aaa_proto::{ProviderFailure, ProviderFailureKind as K, ProviderTurn};
use serde_json::Value;

#[derive(Default)]
pub(crate) struct Reader {
    codex_session: Option<String>,
    turn_id: Option<String>,
    pub latest: Option<ProviderTurn>,
}

fn failure(code: Option<&str>, status: Option<u64>) -> ProviderFailure {
    // 코드가 있으면 그 의미를 우선한다. 429만으로 구독 고갈, 403만으로 로그인 만료라고 단정하지 않는다.
    let kind = match code {
        Some("server_overloaded" | "overloaded" | "overloaded_error" | "model_cap") => K::Overloaded,
        Some("unauthorized" | "authentication_failed") => K::Authentication,
        Some("usage_limit_exceeded" | "billing_error") => K::Allowance,
        Some("model_not_found") => K::Model,
        Some("rate_limit" | "rate_limit_exceeded") => K::RateLimit,
        None
        | Some("http_connection_failed" | "response_stream_connection_failed" | "response_stream_disconnected") => {
            match status {
                Some(529) => K::Overloaded,
                Some(401) => K::Authentication,
                Some(429) => K::RateLimit,
                _ => K::Unknown,
            }
        }
        _ => K::Unknown,
    };
    ProviderFailure {
        kind,
        code: (kind != K::Unknown).then(|| code.map(str::to_owned)).flatten(),
        http_status: status.filter(|s| (400..=599).contains(s)).map(|s| s as u16),
    }
}

impl Reader {
    pub fn line(&mut self, source: crate::plan::Source, line: &str) {
        if ![
            "session_meta",
            "task_started",
            "turn_started",
            "task_complete",
            "turn_complete",
            "turn_aborted",
            "\"user\"",
            "\"assistant\"",
            "turn_duration",
        ]
        .iter()
        .any(|s| line.contains(s))
        {
            return;
        }
        let Ok(v) = serde_json::from_str::<Value>(line).inspect_err(|error| {
            tracing::debug!(%error, "공급자 오류 기록 JSON 파싱 실패");
        }) else {
            return;
        };
        let Some(at) = v["timestamp"].as_str().and_then(aaa_common::util::parse_iso) else { return };
        match source {
            crate::plan::Source::Codex => {
                let p = &v["payload"];
                if v["type"] == "session_meta" {
                    self.codex_session =
                        p["id"].as_str().filter(|s| aaa_common::util::harness_session_ok(s)).map(str::to_owned);
                    self.latest = None;
                    self.turn_id = None;
                    return;
                }
                if v["type"] != "event_msg" {
                    return;
                }
                let Some(session) = self.codex_session.clone() else { return };
                match p["type"].as_str().unwrap_or_default() {
                    "task_started" | "turn_started" => {
                        if self.start(session, at) {
                            self.turn_id = p["turn_id"].as_str().map(str::to_owned);
                        }
                    }
                    "task_complete" | "turn_complete" | "turn_aborted" => {
                        // 같은 파일에서도 이전 turn의 늦은 완료는 현재 turn을 끝내지 않는다.
                        if self.turn_id.as_deref() != p["turn_id"].as_str() {
                            return;
                        }
                        let error = p.get("error").filter(|e| !e.is_null()).map(|e| {
                            let info = &e["codex_error_info"];
                            let (code, status) = match info {
                                Value::String(s) => (Some(s.as_str()), None),
                                Value::Object(m) if m.len() == 1 => m
                                    .iter()
                                    .next()
                                    .map(|(code, detail)| (Some(code.as_str()), detail["http_status_code"].as_u64()))
                                    .unwrap_or((None, None)),
                                _ => (None, None),
                            };
                            failure(code, status)
                        });
                        self.finish(&session, at, error);
                    }
                    _ => {}
                }
            }
            crate::plan::Source::Claude => {
                if v["isSidechain"] == true
                    || v["agentId"].is_string()
                    || v["agent_id"].is_string()
                    || v["parent_tool_use_id"].is_string()
                {
                    return;
                }
                let Some(session) = v["sessionId"].as_str().filter(|s| aaa_common::util::harness_session_ok(s)) else {
                    return;
                };
                match v["type"].as_str().unwrap_or_default() {
                    "user" => {
                        let c = &v["message"]["content"];
                        if c.as_array().is_some_and(|c| c.iter().any(|b| b["type"] == "tool_result")) {
                            return;
                        }
                        let text = c.as_str().map(str::to_owned).unwrap_or_else(|| {
                            c.as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(|b| b["text"].as_str())
                                .collect::<Vec<_>>()
                                .join("\n")
                        });
                        if crate::chat::claude_human(&v, &text) {
                            self.start(session.into(), at);
                        }
                    }
                    "assistant"
                        if v["aborted"] != true && (v["isApiErrorMessage"] == true || v["error"].is_string()) =>
                    {
                        self.finish(session, at, Some(failure(v["error"].as_str(), v["apiErrorStatus"].as_u64())));
                    }
                    "system" if v["subtype"] == "turn_duration" => {
                        // 오류 응답이 이미 끝낸 turn을 성공으로 덮지 않는다.
                        self.finish(session, at, None);
                    }
                    _ => {}
                }
            }
        }
    }

    fn start(&mut self, session: String, at: u64) -> bool {
        if self.latest.as_ref().is_some_and(|t| t.at_ms > at) {
            return false;
        }
        self.latest = Some(ProviderTurn {
            generation_ms: 0,
            harness_session: session,
            started_ms: at,
            at_ms: at,
            finished_ms: None,
            error: None,
        });
        true
    }

    fn finish(&mut self, session: &str, at: u64, error: Option<ProviderFailure>) {
        let Some(t) =
            self.latest.as_mut().filter(|t| t.harness_session == session && t.finished_ms.is_none() && t.at_ms <= at)
        else {
            return;
        };
        t.at_ms = at;
        t.finished_ms = Some(at);
        t.error = error;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::Source::{Claude, Codex};
    use serde_json::json;
    const ID: &str = "00000000-0000-4000-8000-000000000001";
    const OTHER: &str = "00000000-0000-4000-8000-000000000002";
    fn feed(r: &mut Reader, source: crate::plan::Source, mut v: Value, second: u8) {
        v["timestamp"] = json!(format!("2026-10-31T00:00:{second:02}Z"));
        r.line(source, &v.to_string());
    }

    #[test]
    fn completed_native_codes_distinguish_causes_without_reading_sensitive_text() {
        for (claude, codex, kind) in [
            ("overloaded", "server_overloaded", K::Overloaded),
            ("authentication_failed", "unauthorized", K::Authentication),
            ("billing_error", "usage_limit_exceeded", K::Allowance),
            ("model_not_found", "model_not_found", K::Model),
            ("rate_limit", "rate_limit_exceeded", K::RateLimit),
            ("future_secret", "future_secret", K::Unknown),
        ] {
            let mut r = Reader::default();
            feed(&mut r, Claude, json!({"type":"user","sessionId":ID,"message":{"content":"시작"}}), 1);
            feed(
                &mut r,
                Claude,
                json!({"type":"assistant","sessionId":ID,"isApiErrorMessage":true,"error":claude,"message":{"content":[{"type":"text","text":"민감한 원문"}]}}),
                2,
            );
            feed(&mut r, Claude, json!({"type":"system","sessionId":ID,"subtype":"turn_duration"}), 3);
            assert_eq!(r.latest.as_ref().unwrap().error.as_ref().unwrap().kind, kind);
            assert!(!serde_json::to_string(&r.latest).unwrap().contains("민감한"));
            feed(&mut r, Codex, json!({"type":"session_meta","payload":{"id":ID}}), 0);
            feed(&mut r, Codex, json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"turn-a"}}), 1);
            feed(&mut r, Codex, json!({"type":"event_msg","payload":{"type":"error","codex_error_info":codex}}), 2);
            assert_eq!(r.latest.as_ref().unwrap().error, None);
            feed(
                &mut r,
                Codex,
                json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-a","error":{"codex_error_info":codex,"message":"민감한 원문"}}}),
                3,
            );
            let error = r.latest.as_ref().unwrap().error.as_ref().unwrap();
            assert_eq!(error.kind, kind);
            if kind == K::Unknown {
                assert_eq!(error.code, None);
            }
        }
        assert_eq!(failure(Some("http_connection_failed"), Some(401)).kind, K::Authentication);
        assert_eq!(failure(Some("future_secret"), Some(529)).kind, K::Unknown);
        assert_eq!(failure(None, Some(429)).kind, K::RateLimit);
        assert_eq!(failure(None, Some(403)).kind, K::Unknown);
        assert_eq!(failure(Some("model_not_found"), Some(529)).kind, K::Model);
    }

    #[test]
    fn other_sessions_sidechains_tools_old_turns_and_success_do_not_restore_an_error() {
        let mut r = Reader::default();
        feed(&mut r, Claude, json!({"type":"user","sessionId":ID,"message":{"content":"시작"}}), 1);
        for extra in [
            json!({"sessionId":OTHER}),
            json!({"sessionId":ID,"isSidechain":true}),
            json!({"sessionId":ID,"agent_id":"sub"}),
        ] {
            let mut v = json!({"type":"assistant","error":"overloaded"});
            v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            feed(&mut r, Claude, v, 2);
        }
        feed(
            &mut r,
            Claude,
            json!({"type":"user","sessionId":ID,"message":{"content":[{"type":"tool_result","is_error":true}]}}),
            3,
        );
        assert_eq!(r.latest.as_ref().unwrap().error, None);
        feed(&mut r, Claude, json!({"type":"assistant","sessionId":ID,"error":"overloaded"}), 4);
        assert!(r.latest.as_ref().unwrap().error.is_some());
        feed(&mut r, Claude, json!({"type":"user","sessionId":ID,"message":{"content":"다음"}}), 5);
        feed(&mut r, Claude, json!({"type":"assistant","sessionId":ID,"error":"overloaded"}), 4);
        assert_eq!(r.latest.as_ref().unwrap().error, None);
        feed(&mut r, Claude, json!({"type":"system","sessionId":ID,"subtype":"turn_duration"}), 6);
        assert_eq!(r.latest.as_ref().unwrap().error, None);
        feed(&mut r, Codex, json!({"type":"session_meta","payload":{"id":ID}}), 0);
        feed(&mut r, Codex, json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"new"}}), 7);
        feed(
            &mut r,
            Codex,
            json!({"type":"event_msg","payload":{"type":"task_complete","turn_id":"old","error":{"codex_error_info":"server_overloaded"}}}),
            8,
        );
        assert_eq!(r.latest.as_ref().unwrap().finished_ms, None);
        feed(&mut r, Codex, json!({"type":"event_msg","payload":{"type":"turn_aborted","turn_id":"new"}}), 9);
        assert_eq!(r.latest.as_ref().unwrap().error, None);
    }
}
