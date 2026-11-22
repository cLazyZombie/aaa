//! `aaa events`: 흐름 사건 읽기와 따라가기.

use super::*;

/// 흐름 사건: cursor 다음을 읽고, `follow`면 새 사건을 계속 보인다
pub(super) fn events_cmd(json: bool, project: Option<String>, after: u64, follow: bool) -> Result<ExitCode> {
    let rt = runtime()?;
    rt.block_on(async {
        let mut c = aaa_common::front::FrontClient::connect(&aaa_config::paths::state_dir()).await?;
        let first = c.call(FrontRequest::Events { project: project.clone(), after, limit: None, tail: false }).await?;
        let events = match first {
            Ok(FrontResponse::Events { events }) => events,
            Ok(_) => anyhow::bail!("예상하지 못한 응답"),
            Err(e) => return front_print(json, Err(e)),
        };
        let show = |e: &aaa_proto::FlowEvent| -> Result<()> {
            if json {
                println!("{}", serde_json::to_string(e)?);
            } else {
                print!("{}", render_event(e));
            }
            Ok(())
        };
        if !follow {
            // 한 번 읽기: 다른 명령처럼 JSON 객체 하나(따라 읽기는 사건마다 한 줄)
            if json {
                println!("{}", serde_json::to_string(&serde_json::json!({ "events": events }))?);
            } else {
                events.iter().for_each(|e| print!("{}", render_event(e)));
            }
            return Ok(ExitCode::SUCCESS);
        }
        // 프로젝트마다 마지막으로 보인 seq. 처음 읽기에 없던 프로젝트는 cursor(`after`)부터다
        let mut seen: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
        for e in &events {
            show(e)?;
            seen.insert(e.project.clone(), e.seq);
        }
        while let Some(m) = c.next().await? {
            let aaa_proto::FrontMsg::Event(aaa_proto::FrontEvent::Flow(e)) = m else { continue };
            if project.as_ref().is_some_and(|p| *p != e.project) {
                continue;
            }
            let last = seen.get(&e.project).copied().unwrap_or(after);
            match follow_step(last, e.seq) {
                FollowStep::Skip => {}
                FollowStep::Show => {
                    show(&e)?;
                    seen.insert(e.project.clone(), e.seq);
                }
                // 사이가 비었다(host가 놓친 사건을 보내지 못했다): 그 프로젝트를 cursor부터 다시 읽어 차례대로 보인다
                FollowStep::Refill { after: from } => {
                    let mut last = from;
                    while last < e.seq {
                        let req = FrontRequest::Events {
                            project: Some(e.project.clone()),
                            after: last,
                            limit: None,
                            tail: false,
                        };
                        let Ok(FrontResponse::Events { events: mut got }) = c.call(req).await? else { break };
                        got.sort_by_key(|g| g.seq);
                        let before = last;
                        for g in &got {
                            if g.seq > last {
                                show(g)?;
                                last = g.seq;
                            }
                        }
                        if last == before {
                            break;
                        }
                    }
                    // 다시 읽어도 그 사건에 닿지 못했다(읽기 실패): 받은 것은 보인다
                    if last < e.seq {
                        show(&e)?;
                        last = e.seq;
                    }
                    seen.insert(e.project.clone(), last);
                }
            }
        }
        Ok(ExitCode::SUCCESS)
    })
}

/// 따라 읽기에서 받은 사건(그 프로젝트에서 마지막으로 보인 seq가 `last`)을 어떻게 하나. seq는 프로젝트마다 1부터 빈칸
/// 없이 오른다: 바로 다음이면 보이고, 이미 본 것은 버리고, 사이가 비면 `last`부터 다시 읽는다(빈칸을 버리지 않는다)
#[derive(Debug, PartialEq)]
pub(super) enum FollowStep {
    Show,
    Skip,
    Refill { after: u64 },
}

pub(super) fn follow_step(last: u64, seq: u64) -> FollowStep {
    if seq <= last {
        FollowStep::Skip
    } else if seq == last + 1 {
        FollowStep::Show
    } else {
        FollowStep::Refill { after: last }
    }
}

pub(super) fn render_event(e: &aaa_proto::FlowEvent) -> String {
    format!(
        "{}#{}\t{}\t{}\t{}\t{}{}\n",
        e.project,
        e.seq,
        e.kind,
        e.thread.clone().unwrap_or_default(),
        e.machine.clone().unwrap_or_default(),
        e.detail,
        e.evidence.as_ref().map(|p| format!("\t({p})")).unwrap_or_default()
    )
}

pub(super) fn render_question(s: &mut String, q: &aaa_proto::QuestionView) {
    use std::fmt::Write as _;
    let at = match (&q.project, &q.thread) {
        (Some(p), Some(t)) => format!("{p}/{t}"),
        _ => q.pane.clone(),
    };
    let state = match q.state {
        aaa_proto::QuestionState::Open => "열림",
        aaa_proto::QuestionState::Answered => "답함",
        aaa_proto::QuestionState::Gone => "사라짐",
    };
    let _ = writeln!(
        s,
        "{}\t{}\t{}@{}\t{state}{}\t{}\t답: {}",
        q.short_id.as_deref().unwrap_or(&q.id),
        q.question_kind,
        at,
        q.machine,
        q.answer.as_ref().map(|a| format!("({a})")).unwrap_or_default(),
        q.text,
        q.options.join("·")
    );
}
