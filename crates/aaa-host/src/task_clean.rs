//! 끝난 thread의 이어진 task 정리(할 일 T3, plan §8.315, t-0469 §4 P2: 시작=진행 중, 끝남=자동 정리).
//!
//! - thread가 끝나면(resolve) host가 끝난 까닭과 버림을 thread 기록(`TaskClean`)에 적고, ticker가 한 번 판정한다. 이 칸 전에 끝난
//!   thread는 까닭이 없어 판정하지 않는다(올린 직후 옛 thread에 이어진 task를 지우지 않는다). 다시 열면 지워 다음 끝남을 판정한다.
//! - 완료는 좁게 본다: PR 병합으로 끝났거나 마지막 보고 첫 줄이 `[완료]`다. 그러면 줄을 지우고 기록에 「자동 정리」로 남겨 되살릴 수
//!   있게 하고 coordinator inbox에 알린다. 하위 task가 남은 task는 지우지 않는다(지우기는 하위 묶음과 함께 간다).
//! - 그 밖(보고 표지 `[결정 필요]`·`[착지]` 등, 보고 없음, 사본 없이 버림)은 줄을 남기고 「확인 필요」다. 표지는 따로 적지 않고 할 일
//!   화면을 읽을 때 이어진 thread가 끝났는지로 정한다(`attention`). 사람이 「끝남으로 지우기」·「다시 맡기기」·「연결만 풀기」로 정한다.
//! - 지운 task의 상위에 하위가 하나도 남지 않으면 그 상위에 이 thread를 이어 「확인 필요」로 올린다(상위는 저절로 지우지 않는다).
//!   상위가 이미 다른 thread와 이어져 있으면 그대로 둔다.
//! - task와 이어지지 않은 thread에는 아무것도 하지 않는다.

use std::path::Path;

use aaa_proto::{ErrorBody, ErrorCode, Role, TaskAnchor, TaskOp, TaskView, ThreadStatus};

use crate::orch::ThreadRec;
use crate::{Host, err, task_edit, tasks};

/// 판정
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// 완료(까닭): 줄을 지운다
    Clean(String),
    /// 확인 필요(까닭): 줄을 남긴다
    Check(String),
}

/// P2 판정표(t-0469 §4): 끝난 까닭 × 보고 표지 × 버림 × 하위 task. 하위가 남은 task는 지우지 않는다(지우기가 하위 묶음과 함께 간다).
/// 병합은 보고 표지보다 앞선다(PR 없는 직접 병합은 `[완료]`로 본다).
pub(crate) fn judge(why: Option<&str>, discard: bool, marker: Option<&str>, children: bool) -> Verdict {
    if children {
        return Verdict::Check("하위 task가 남아 있다".into());
    }
    if why == Some("merged") {
        return Verdict::Clean("PR 병합".into());
    }
    if discard {
        return Verdict::Check("마지막 사본 없이 끝냈다".into());
    }
    match marker {
        Some("[완료]") => Verdict::Clean("보고 [완료]".into()),
        Some(m) => Verdict::Check(format!("보고 표지 {m}")),
        None => Verdict::Check("보고가 없다".into()),
    }
}

/// 이어진 thread가 끝난 task의 「확인 필요」 까닭(할 일 화면 `TaskView.attention`). 열린 thread거나, 완료로 판정했지만 아직 지우기
/// 전(곧 지운다)이면 없음
pub(crate) fn attention(t: &TaskView, rec: &ThreadRec, marker: Option<&str>, children: bool) -> Option<String> {
    if rec.status != ThreadStatus::Resolved || rec.resolve_pending.is_some() {
        return None;
    }
    let id = &rec.id;
    let c = &rec.task_clean;
    let Some(why) = c.why.as_deref() else {
        return Some(format!("{id}가 끝났다. 끝났는지 정한다"));
    };
    let same = |titles: &[String]| titles.iter().any(|x| x.to_lowercase() == t.title.to_lowercase());
    match judge(Some(why), c.discard, marker, children) {
        Verdict::Check(r) => Some(format!("{id} 끝남: {r}")),
        Verdict::Clean(_) if c.judged_ms.is_none() => None,
        Verdict::Clean(_) if same(&c.parents) => Some(format!("하위 task가 모두 끝났다(마지막 {id})")),
        Verdict::Clean(r) if same(&c.cleaned) => Some(format!("{id} 완료({r})로 자동 정리했다가 되살렸다")),
        Verdict::Clean(r) => Some(format!("{id}가 완료({r})로 끝났다")),
    }
}

fn anchor(t: &TaskView) -> TaskAnchor {
    TaskAnchor { list: t.list.clone(), title: t.title.clone(), block_sha256: t.block_sha256.clone(), line: t.line }
}

/// 자동 정리 기록의 actor(기록 화면 「host 자동 정리」)
const HOST_ACTOR: &str = "host";

impl Host {
    /// ticker: 끝났고 아직 판정하지 않은 thread마다 이어진 task를 한 번 정리한다. 쓰기가 충돌하면(그사이 그 task가 바뀜) 다음 tick에 다시 본다
    pub(crate) fn task_clean_pass(&self, project: &str, dir: &Path) {
        let due: Vec<ThreadRec> = self
            .db
            .threads(Some(project))
            .unwrap_or_default()
            .iter()
            .filter_map(|j| serde_json::from_str::<ThreadRec>(j).ok())
            .filter(|r| r.status == ThreadStatus::Resolved && r.resolve_pending.is_none() && r.role != Role::Evaluator)
            .filter(|r| r.task_clean.why.is_some() && r.task_clean.judged_ms.is_none())
            .collect();
        for rec in due {
            if let Err(e) = self.task_clean_one(project, dir, &rec) {
                tracing::warn!(project, thread = %rec.id, "할 일 자동 정리를 다음 셈으로 미룬다: {}", e.message);
            }
        }
    }

    fn task_clean_one(&self, project: &str, dir: &Path, rec: &ThreadRec) -> Result<(), ErrorBody> {
        let text = std::fs::read_to_string(dir.join("TASKS.md")).unwrap_or_default();
        let (_, all) = tasks::parse_board(&text);
        let marker = self.report_marker(rec);
        let c = &rec.task_clean;
        let mut clean: Vec<(TaskView, String)> = Vec::new();
        let mut check: Vec<(String, String)> = Vec::new();
        for t in all.iter().filter(|t| t.thread.as_deref() == Some(rec.id.as_str())) {
            let children = all.iter().any(|x| x.parent == Some(t.line));
            match judge(c.why.as_deref(), c.discard, marker, children) {
                Verdict::Clean(r) => clean.push((t.clone(), r)),
                Verdict::Check(r) => check.push((t.title.clone(), r)),
            }
        }
        let mut cleaned = Vec::new();
        let mut parents = Vec::new();
        if !clean.is_empty() {
            let ops: Vec<TaskOp> = clean.iter().map(|(t, _)| TaskOp::Delete { at: anchor(t) }).collect();
            let (conflict, log_ids) = self.task_write(project, dir, &ops, None, HOST_ACTOR, &no_input(), true)?;
            if let Some((_, reason, _)) = conflict {
                return Err(err(ErrorCode::Unavailable, reason));
            }
            for (i, (t, r)) in clean.iter().enumerate() {
                let restore =
                    log_ids.get(i).map(|id| format!(" 되살리기: `aaa task restore {id}`")).unwrap_or_default();
                self.inbox_add(
                    project,
                    "task",
                    &rec.id,
                    "task auto-cleaned",
                    &format!("「{}」 task를 지웠다({} 완료: {r}).{restore}", t.title, rec.id),
                );
                cleaned.push(t.title.clone());
            }
            parents = self.raise_parents(project, dir, rec, &all, &clean);
        }
        // 확인 필요 알림은 지우기가 쓰인 뒤에 낸다: 쓰기가 실패하면 다음 tick이 다시 판정해 알림이 겹친다
        for (title, r) in &check {
            self.inbox_add(
                project,
                "task",
                &rec.id,
                "task needs check",
                &format!(
                    "「{title}」 task 확인 필요({} 끝남: {r}). 줄은 남겼다. 끝났는지·다시 맡길지·연결만 풀지 사용자에게 묻는다",
                    rec.id
                ),
            );
        }
        self.update_rec(project, &rec.id, |r| {
            r.task_clean.judged_ms = Some(aaa_common::util::now_ms());
            r.task_clean.cleaned = cleaned.clone();
            r.task_clean.parents = parents.clone();
        })?;
        Ok(())
    }

    /// 지운 task의 상위에 하위가 남지 않았으면 이 thread를 이어 「확인 필요」로 올린다. 올린 상위 제목
    fn raise_parents(
        &self,
        project: &str,
        dir: &Path,
        rec: &ThreadRec,
        before: &[TaskView],
        clean: &[(TaskView, String)],
    ) -> Vec<String> {
        let text = std::fs::read_to_string(dir.join("TASKS.md")).unwrap_or_default();
        let (_, now) = tasks::parse_board(&text);
        let mut raised = Vec::new();
        for (t, _) in clean {
            let Some(old) = t.parent.and_then(|l| before.iter().find(|p| p.line == l)) else { continue };
            // 상위 블록은 하위를 품지 않아 지우기 뒤에도 hash가 같다
            let Some(p) = now.iter().find(|p| p.block_sha256 == old.block_sha256 && p.title == old.title) else {
                continue;
            };
            if p.thread.is_some() || now.iter().any(|x| x.parent == Some(p.line)) || raised.contains(&p.title) {
                continue;
            }
            let op = TaskOp::Update {
                at: anchor(p),
                title: None,
                owner: None,
                person: false,
                notes: None,
                thread: Some(rec.id.clone()),
                done: None,
            };
            // 잇지 못하면(그사이 상위가 바뀌었다) 다시 하지 않지만 신호는 남긴다: 줄 대신 inbox로 묻게 한다
            let summary = match self.task_write(project, dir, &[op], None, HOST_ACTOR, &no_input(), true) {
                Ok((None, _)) => {
                    raised.push(p.title.clone());
                    format!(
                        "상위 「{}」 task의 하위가 모두 끝났다(마지막 {}). 상위에 {}를 이었다. 끝났는지 사용자에게 묻는다",
                        p.title, rec.id, rec.id
                    )
                }
                Ok((Some((_, why, _)), _)) | Err(ErrorBody { message: why, .. }) => {
                    tracing::warn!(project, task = %p.title, "상위 task를 올리지 못했다: {why}");
                    format!(
                        "상위 「{}」 task의 하위가 모두 끝났다(마지막 {}). 상위에 잇지 못했다({why}). 끝났는지 사용자에게 묻는다",
                        p.title, rec.id
                    )
                }
            };
            self.inbox_add(project, "task", &rec.id, "task needs check", &summary);
        }
        raised
    }

    /// 할 일 화면·`context`의 「확인 필요」: 이어진 thread가 끝난 task마다 까닭을 단다
    pub(crate) fn fill_attention(&self, project: &str, list: &mut [TaskView]) {
        let parents: std::collections::HashSet<u32> = list.iter().filter_map(|t| t.parent).collect();
        for t in list.iter_mut() {
            let Some(id) = t.thread.clone() else { continue };
            let Ok(rec) = self.load(project, &id) else { continue };
            let marker = self.report_marker(&rec);
            t.attention = attention(t, &rec, marker, parents.contains(&t.line));
        }
    }
}

/// 자동 정리는 owner를 바꾸거나 되살리지 않는다
fn no_input() -> task_edit::Ctx<'static> {
    fn owner_ok(_: &str, _: bool) -> Result<(), String> {
        Ok(())
    }
    fn removed(_: i64) -> Result<task_edit::Removed, String> {
        Err("자동 정리는 되살리지 않는다".into())
    }
    task_edit::Ctx { owner_ok: &owner_ok, removed: &removed }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// P2 판정표(t-0469 §6.3): 까닭 × 보고 표지 × 버림 × 하위 task
    #[test]
    fn judge_table() {
        let clean = |r: &str| Verdict::Clean(r.into());
        let check = |r: &str| Verdict::Check(r.into());
        for (why, discard, marker, children, want) in [
            // PR 병합은 보고 표지와 무관하게 완료(병합은 사본을 버리고 끝내지 않는다)
            (Some("merged"), false, None, false, clean("PR 병합")),
            (Some("merged"), false, Some("[착지]"), false, clean("PR 병합")),
            (Some("merged"), true, None, false, clean("PR 병합")),
            // 직접 병합 착지: 보고 [완료]
            (Some("manual"), false, Some("[완료]"), false, clean("보고 [완료]")),
            (Some("auto"), false, Some("[완료]"), false, clean("보고 [완료]")),
            // 사람 대기·진행 표지, 보고 없음, 버림은 확인 필요
            (Some("manual"), false, Some("[결정 필요]"), false, check("보고 표지 [결정 필요]")),
            (Some("manual"), false, Some("[검수 대기]"), false, check("보고 표지 [검수 대기]")),
            (Some("manual"), false, Some("[확인 필요]"), false, check("보고 표지 [확인 필요]")),
            (Some("manual"), false, Some("[착지]"), false, check("보고 표지 [착지]")),
            (Some("auto"), false, Some("[진행 중]"), false, check("보고 표지 [진행 중]")),
            (Some("manual"), false, None, false, check("보고가 없다")),
            (Some("manual"), true, Some("[완료]"), false, check("마지막 사본 없이 끝냈다")),
            // 하위 task가 남은 상위는 병합·[완료]여도 지우지 않는다
            (Some("merged"), false, Some("[완료]"), true, check("하위 task가 남아 있다")),
            (Some("manual"), false, Some("[완료]"), true, check("하위 task가 남아 있다")),
        ] {
            assert_eq!(judge(why, discard, marker, children), want, "{why:?} {discard} {marker:?} {children}");
        }
    }

    fn rec(why: Option<&str>, judged: bool) -> ThreadRec {
        let mut r = ThreadRec::starting("p", "t", "m", "claude", Role::Worker, aaa_proto::ThreadKind::Dir, 1);
        r.id = "t-0007".into();
        r.status = ThreadStatus::Resolved;
        r.task_clean.why = why.map(str::to_string);
        r.task_clean.judged_ms = judged.then_some(2);
        r
    }

    fn task(title: &str) -> TaskView {
        tasks::parse(&format!("## Backlog\n- [ ] {title} · t-0007\n")).remove(0)
    }

    /// 「확인 필요」 까닭: 열린 thread·판정 전 완료는 없음, 옛 thread는 판정 없이 확인 필요, 되살린 task와 올린 상위는 그 까닭
    #[test]
    fn attention_reasons() {
        let t = task("일");
        let mut open = rec(Some("manual"), false);
        open.status = ThreadStatus::Open;
        assert_eq!(attention(&t, &open, Some("[결정 필요]"), false), None);
        assert_eq!(attention(&t, &rec(Some("merged"), false), None, false), None, "곧 지운다");
        assert_eq!(
            attention(&t, &rec(None, false), Some("[완료]"), false).unwrap(),
            "t-0007가 끝났다. 끝났는지 정한다"
        );
        assert_eq!(
            attention(&t, &rec(Some("manual"), true), Some("[착지]"), false).unwrap(),
            "t-0007 끝남: 보고 표지 [착지]"
        );
        let mut restored = rec(Some("manual"), true);
        restored.task_clean.cleaned = vec!["일".into()];
        assert!(attention(&t, &restored, Some("[완료]"), false).unwrap().contains("되살렸다"));
        let mut parent = rec(Some("merged"), true);
        parent.task_clean.parents = vec!["일".into()];
        assert_eq!(attention(&t, &parent, None, false).unwrap(), "하위 task가 모두 끝났다(마지막 t-0007)");
    }
}
