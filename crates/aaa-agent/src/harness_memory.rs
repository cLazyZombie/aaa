//! 세션마다 마지막 harness 상태를 기억하고 host로 보낸다.

use super::*;

/// 다시 알릴 harness 사건의 칸. 칸마다 마지막 하나를 남긴다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Slot {
    /// 진행 보고(`aaa report`)
    Report,
    /// harness 세션 ID·기록 위치(SessionStart hook, Codex notify)
    Session,
    /// 상태(화면 감지, 끝남). 세션 ID 사건이 이것을 덮으면 다시 붙은 host가 상태를 잃는다.
    State,
    /// 계획·배경 작업 수(plan.rs)
    Plan,
    /// Codex가 OSC 제목에 알린 처리 속도
    CodexSpeed,
    /// 마지막 완료 답. 연결 복구 때 같은 사건을 재전송하며 host가 중복을 거른다.
    Reply,
    /// 완료 오류와 다음 turn 시작 관측. 상태 사건과 따로 재전송한다.
    ProviderTurn,
}

impl Slot {
    /// 다시 알리지 않는 사건(입력 제출 같은 한때 신호)은 None
    pub(super) fn of(e: &aaa_proto::HarnessEvent) -> Option<Slot> {
        match e.kind.as_str() {
            _ if e.harness == "report" => Some(Slot::Report),
            "session_start" | "session" => Some(Slot::Session),
            "plan" => Some(Slot::Plan),
            "codex_speed" => Some(Slot::CodexSpeed),
            "turn_reply" => Some(Slot::Reply),
            "provider_turn" => Some(Slot::ProviderTurn),
            "prompt_submit" | "agy_receipt" => None,
            _ => Some(Slot::State),
        }
    }
}

/// 정리 사건(`compacting`, plan §8.309)을 모르는 host(`compacting.v1`을 알리지 않음)에는 작업 중으로 보낸다: 옛 host는 모르는 kind를
/// 버려 정리 내내 앞 상태(쉼)로 남고, 받음 대기(`ACCEPTED`)도 끝내 받지 못한다. 기억하는 사건(`remember_harness`)은 그대로 둔다
pub(super) fn for_host(mut ev: aaa_proto::HarnessEvent, host_compacting: bool) -> aaa_proto::HarnessEvent {
    if ev.kind == "compacting" && !host_compacting {
        ev.kind = "working".into();
    }
    ev
}

impl Agent {
    /// harness 사건을 기억하고(다시 알릴 것이면) host에 보낸다
    pub(super) async fn harness_event(self: &Arc<Self>, mut ev: aaa_proto::HarnessEvent) {
        let _delivery = self.harness_delivery.lock().await;
        if matches!(ev.kind.as_str(), "session_start" | "session") {
            let pane_started = self.sessions.lock().unwrap().get(&ev.session).map(|e| e.info.started_at_ms);
            let generation =
                self.plan_since.lock().unwrap().get(&ev.session).copied().or(pane_started).unwrap_or(ev.at_ms);
            ev.provider_turn = ev.harness_session.as_ref().map(|id| aaa_proto::ProviderTurn {
                generation_ms: generation,
                harness_session: id.clone(),
                started_ms: generation,
                at_ms: ev.at_ms,
                finished_ms: None,
                error: None,
            });
        }
        // 이 pane이 시작한 harness 세션과 다른 세션(중첩 Codex thread 등)의 사건은 버린다(herdr-projects `foreign_session`)
        if ev.harness != "report" {
            let mut started = self.harness_started.lock().unwrap();
            if ev.kind == "session_start" {
                if ev.harness == "agy" && started.contains_key(&ev.session) {
                    return;
                }
                if let Some(id) = &ev.harness_session {
                    started.insert(ev.session.clone(), id.clone());
                }
            } else if hooks::foreign_session(started.get(&ev.session).map(String::as_str), &ev) {
                return;
            }
        }
        let ev = if Slot::of(&ev).is_some() {
            let me = self.clone();
            let kept = ev.clone();
            tokio::task::spawn_blocking(move || me.remember_harness(kept)).await.unwrap_or(ev)
        } else {
            ev
        };
        let ev = for_host(ev, self.host_compacting.load(std::sync::atomic::Ordering::Relaxed));
        self.send_host(&LinkMsg::Event(AgentEvent::Harness(ev))).await;
    }

    /// 화면 감지가 알린 상태(pump에서)
    pub(super) async fn detected(self: &Arc<Self>, session: &str, harness: detect::Harness, r: detect::Report) {
        tracing::info!(%session, kind = r.kind, rule = ?r.rule, "harness 상태(화면 감지)");
        self.harness_event(aaa_proto::HarnessEvent {
            session: session.to_string(),
            harness: harness.label().to_string(),
            kind: r.kind.to_string(),
            harness_session: None,
            message: r.message,
            question_kind: r.question_kind.map(str::to_string),
            percent: None,
            at_ms: util::now_ms(),
            transcript: None,
            plan: None,
            background: None,
            context: None,
            model_observation: None,
            codex_speed: None,
            provider_turn: None,
            turn_reply: None,
        })
        .await;
    }

    pub(super) async fn detect_codex_speed(self: &Arc<Self>, session: &str, title: &str) {
        let at_ms = util::now_ms();
        let Some(speed) = self.observe_codex_title(session, title, at_ms) else { return };
        self.harness_event(aaa_proto::HarnessEvent {
            session: session.into(),
            harness: "codex".into(),
            kind: "codex_speed".into(),
            at_ms,
            harness_session: None,
            message: None,
            question_kind: None,
            percent: None,
            transcript: None,
            plan: None,
            background: None,
            context: None,
            model_observation: None,
            codex_speed: Some(speed),
            turn_reply: None,
            provider_turn: None,
        })
        .await;
    }

    /// 다시 알릴 사건을 바꾸고 세션 폴더에 적는다. 같은 칸의 앞 사건을 바꾸되, 이번 사건에 없는 기록 위치·
    /// harness 세션 ID는 앞 사건 것을 이어받는다. 돌려준 사건을 host에 보낸다: agent가 다시 떠 같은 상태를 다시
    /// 감지했으면 앞 사건(그 상태가 된 때)을 그대로 보낸다.
    pub(super) fn remember_harness(&self, mut ev: aaa_proto::HarnessEvent) -> aaa_proto::HarnessEvent {
        let Some(slot) = Slot::of(&ev) else { return ev };
        let mut map = self.harness_last.lock().unwrap();
        let list = map.entry(ev.session.clone()).or_default();
        let send = ev.clone();
        // 셸 복귀 뒤 host가 다시 붙어도 옛 대화·계획을 되살리지 않는다.
        if ev.kind == "released" {
            list.clear();
            self.harness_started.lock().unwrap().remove(&ev.session);
        }
        if let Some(i) = list.iter().position(|e| Slot::of(e) == Some(slot)) {
            let same_state = |o: &aaa_proto::HarnessEvent| {
                slot == Slot::State && (&o.harness, &o.kind, &o.message) == (&ev.harness, &ev.kind, &ev.message)
            };
            if same_state(&list[i]) {
                return list[i].clone();
            }
            let old = list.remove(i);
            if old.harness == ev.harness && slot != Slot::Session {
                ev.transcript = ev.transcript.or(old.transcript);
                ev.harness_session = ev.harness_session.or(old.harness_session);
            }
        }
        list.push(ev.clone());
        list.sort_by_key(|e| e.at_ms);
        // 파일 쓰기도 lock 안에서 해 순서가 뒤바뀌지 않게 한다
        if let Err(e) = util::write_json(&SessionPaths::new(&self.state, &ev.session).harness(), list) {
            tracing::warn!(session = %ev.session, "harness.json을 쓰지 못했다: {e}");
        }
        send
    }

    /// 세션별 마지막 harness 사건(시각 차례). hello에 싣거나 옛 host에 다시 보낸다.
    pub fn harness_events(&self) -> Vec<aaa_proto::HarnessEvent> {
        let mut last: Vec<_> = self.harness_last.lock().unwrap().values().flatten().cloned().collect();
        last.sort_by_key(|e| e.at_ms);
        last
    }
}
