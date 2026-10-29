//! 끝난 thread의 이어진 task 정리(할 일 T3, plan §8.316, t-0469 §4 P2: 시작=진행 중, 끝남=자동 정리).
//!
//! - thread가 끝나면(resolve) host가 끝난 까닭과 버림을 thread 기록(`TaskClean`)에 적고, ticker가 한 번 판정한다. 이 칸 전에 끝난
//!   thread는 까닭이 없어 판정하지 않는다(올린 직후 옛 thread에 이어진 task를 지우지 않는다). 다시 열면 지워 다음 끝남을 판정한다.
//! - 완료는 좁게 본다: origin/main 조상을 확인했거나 마지막 보고 첫 줄이 `[완료]`다. 그러면 줄을 지우고 기록에 「자동 정리」로 남겨 되살릴 수
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
/// 병합은 보고 표지보다 앞선다. 직접 병합은 agent가 origin/main 조상을 확인한다.
pub(crate) fn judge(why: Option<&str>, discard: bool, marker: Option<&str>, children: bool) -> Verdict {
    if children {
        return Verdict::Check("하위 task가 남아 있다".into());
    }
    if discard {
        return Verdict::Check("마지막 사본 없이 끝냈다".into());
    }
    if why == Some("main_merged") {
        return Verdict::Clean("origin/main 병합".into());
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
    if crate::followups::pending(rec) {
        return Some(format!("{} 후속 남음: thread에서 후속 할 일 연결이나 버림·정리 완료 사유를 기록한다", rec.id));
    }
    let id = &rec.id;
    let c = &rec.task_clean;
    let Some(why) = c.why.as_deref() else {
        return Some(format!("{id}가 끝났다. 끝났는지 정한다"));
    };
    let same = |titles: &[String]| titles.iter().any(|x| x.to_lowercase() == t.title.to_lowercase());
    let why = if c.merged_main.is_some() { "main_merged" } else { why };
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
        if let Err(e) = self.task_done_clean(project, dir) {
            tracing::warn!(project, "완료 체크 보관을 다음 셈으로 미룬다: {}", e.message);
            return;
        }
        let due: Vec<ThreadRec> = self
            .db
            .threads(Some(project))
            .unwrap_or_default()
            .iter()
            .filter_map(|j| {
                serde_json::from_str::<ThreadRec>(j)
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            })
            .filter(|r| r.status == ThreadStatus::Resolved && r.resolve_pending.is_none() && r.role != Role::Evaluator)
            .filter(|r| {
                (r.task_clean.why.is_some() && r.task_clean.judged_ms.is_none())
                    || (r.automation.is_some() && crate::followups::pending(r))
            })
            .collect();
        for rec in due {
            let rec = if rec.automation.is_some() && crate::followups::pending(&rec) {
                match self.update_rec(project, &rec.id, crate::followups::handoff_automation) {
                    Ok(rec) => rec,
                    Err(e) => {
                        tracing::warn!(project, thread = %rec.id, "자동화 후속 인계를 기록하지 못했다: {}", e.message);
                        continue;
                    }
                }
            } else {
                rec
            };
            self.followup_notice(&rec);
            if rec.task_clean.why.is_none() {
                continue;
            }
            if let Err(e) = self.task_clean_one(project, dir, &rec) {
                tracing::warn!(project, thread = %rec.id, "할 일 자동 정리를 다음 셈으로 미룬다: {}", e.message);
            }
        }
    }

    /// 파일의 완료 체크도 완료 근거다. 하위를 먼저 보관하고 미완료 하위가 있는 상위는 남긴다.
    fn task_done_clean(&self, project: &str, dir: &Path) -> Result<(), ErrorBody> {
        // 파일 제거 뒤 DB 확정 전에 멈춘 영수증부터 회수한다.
        self.archive_recover(project)?;
        self.task_identify(project)?;
        loop {
            let text = std::fs::read_to_string(dir.join("TASKS.md"))
                .map_err(|e| err(ErrorCode::Internal, format!("TASKS.md: {e}")))?;
            let all = tasks::parse(&text);
            let mut ops = Vec::new();
            for task in all.iter().filter(|t| t.done && !all.iter().any(|c| c.parent == Some(t.line))) {
                if let Some(id) = task.thread.as_deref() {
                    match self.load(project, id) {
                        Ok(rec) if crate::followups::pending(&rec) => continue,
                        Ok(_) => {}
                        Err(e) if e.code == ErrorCode::NotFound => {}
                        Err(e) => return Err(e),
                    }
                }
                ops.push(TaskOp::Delete { at: anchor(task) });
            }
            if ops.is_empty() {
                return Ok(());
            }
            // 판정 뒤 직접 편집이 끼어들면 완료 체크와 하위 유무부터 다시 판단한다.
            let (conflict, _) =
                self.task_write(project, dir, &ops, Some(&tasks::sha256(&text)), HOST_ACTOR, &no_input(), true)?;
            if let Some((_, reason, _)) = conflict {
                return Err(err(ErrorCode::Unavailable, reason));
            }
        }
    }

    /// 현재 branch 끝만 main에 포함됐을 때 정리 근거로 쓴다. 보고의 SHA는 삭제 근거가 아니다.
    pub(crate) async fn main_branch_head(&self, rec: &ThreadRec) -> Option<String> {
        if !self.machine_feature(&rec.machine_id, "git_merged.v1") {
            return None;
        }
        let repo = rec.repo.as_deref()?;
        let branch = rec.branch.as_ref().filter(|b| !b.is_empty())?;
        let conn = self.live_conn(&rec.machine_id).ok()?;
        let query = aaa_proto::GitQuery::MergedIntoMain { branch: Some(branch.clone()), commits: vec![] };
        match conn
            .request_timeout(
                aaa_proto::AgentRequest::GitQuery { cwd: repo.into(), query },
                aaa_common::git_watch::QUERY_TIMEOUT + std::time::Duration::from_secs(5),
            )
            .await
        {
            Ok(aaa_proto::AgentResponse::Git(answer)) => {
                answer.merged_commit.filter(|sha| aaa_common::git_watch::valid_sha(sha))
            }
            Ok(other) => {
                tracing::warn!(thread = %rec.id, "main 포함 조회 응답이 아니다: {other:?}");
                None
            }
            Err(e) => {
                tracing::warn!(thread = %rec.id, "main 포함 조회 실패: {}", e.message);
                None
            }
        }
    }

    pub(crate) async fn task_main_merge(&self, rec: &ThreadRec) -> Option<String> {
        if rec.task_clean.discard
            || self.report_marker(rec) == Some("[완료]")
            || !self.machine_feature(&rec.machine_id, "git_merged.v1")
        {
            return None;
        }
        // 보고 없음·중단은 branch가 우연히 main에 포함돼 있어도 완료로 바꾸지 않는다.
        let report = self.verified_report(rec)?;
        let repo = rec.repo.as_deref()?;
        let dir = self
            .project_dir(&rec.project)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()?;
        let tasks = std::fs::read_to_string(dir.join("TASKS.md"))
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()?;
        if !tasks::parse(&tasks).iter().any(|t| t.thread.as_deref() == Some(rec.id.as_str())) {
            return None;
        }
        let conn = self
            .live_conn(&rec.machine_id)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()?;
        let query = aaa_proto::GitQuery::MergedIntoMain { branch: rec.branch.clone(), commits: merge_commits(&report) };
        let response = conn
            .request_timeout(
                aaa_proto::AgentRequest::GitQuery { cwd: repo.into(), query },
                aaa_common::git_watch::QUERY_TIMEOUT + std::time::Duration::from_secs(5),
            )
            .await;
        match response {
            Ok(aaa_proto::AgentResponse::Git(answer)) => {
                answer.merged_commit.filter(|sha| aaa_common::git_watch::valid_sha(sha))
            }
            Ok(other) => {
                tracing::warn!(thread = %rec.id, "main 포함 조회 응답이 아니다: {other:?}");
                None
            }
            Err(e) => {
                tracing::warn!(thread = %rec.id, "main 포함 조회 실패: {}", e.message);
                None
            }
        }
    }

    fn task_clean_one(&self, project: &str, dir: &Path, rec: &ThreadRec) -> Result<(), ErrorBody> {
        self.task_identify(project)?;
        let text = std::fs::read_to_string(dir.join("TASKS.md"))
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 파일을 읽지 못해 기본값을 쓴다"))
            .unwrap_or_default();
        let (_, all) = tasks::parse_board(&text);
        let marker = self.report_marker(rec);
        let c = &rec.task_clean;
        let mut clean: Vec<(TaskView, String)> = Vec::new();
        let mut check: Vec<(String, String)> = Vec::new();
        for t in all.iter().filter(|t| t.thread.as_deref() == Some(rec.id.as_str())) {
            let children = all.iter().any(|x| x.parent == Some(t.line));
            if crate::followups::pending(rec) {
                check.push((
                    t.title.clone(),
                    "후속 남음: thread에서 후속 할 일 연결이나 버림·정리 완료 사유를 기록한다".into(),
                ));
                continue;
            }
            match judge(
                if c.merged_main.is_some() { Some("main_merged") } else { c.why.as_deref() },
                c.discard,
                marker,
                children,
            ) {
                Verdict::Clean(r) => clean.push((t.clone(), r)),
                Verdict::Check(r) => check.push((t.title.clone(), r)),
            }
        }
        let mut cleaned = Vec::new();
        let mut parents = Vec::new();
        if !clean.is_empty() {
            let ops: Vec<TaskOp> = clean.iter().map(|(t, _)| TaskOp::Delete { at: anchor(t) }).collect();
            let (conflict, _log_ids) = self.task_write(project, dir, &ops, None, HOST_ACTOR, &no_input(), true)?;
            if let Some((_, reason, _)) = conflict {
                return Err(err(ErrorCode::Unavailable, reason));
            }
            for (t, r) in &clean {
                let restore =
                    format!(" 되살리기: `aaa task archive --restore {}`", t.id.as_deref().unwrap_or_default());
                self.inbox_add(
                    project,
                    "task",
                    &rec.id,
                    "task auto-cleaned",
                    &format!("「{}」 task를 완료 보관했다({} 완료: {r}).{restore}", t.title, rec.id),
                );
                cleaned.push(t.title.clone());
            }
            parents = self.raise_parents(project, dir, rec, &all, &clean);
        }
        // 확인 필요 알림은 지우기가 쓰인 뒤에 낸다: 쓰기가 실패하면 다음 tick이 다시 판정해 알림이 겹친다
        self.followup_notice(rec);
        for (title, r) in check.iter().filter(|_| !crate::followups::pending(rec)) {
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
        // task_write의 파일 사건은 판정 저장보다 먼저 나간다. 그 사건으로 읽은 board는 올린 상위의
        // attention이 아직 없을 수 있다. 파일 hash가 같아도 DB 판정 확정 뒤 다시 읽게 한다.
        if !clean.is_empty() || !check.is_empty() {
            let text = std::fs::read_to_string(dir.join("TASKS.md"))
                .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 파일을 읽지 못해 기본값을 쓴다"))
                .unwrap_or_default();
            aaa_common::diagnostic::ignored(
                self.task_events.send((project.to_string(), tasks::sha256(&text))),
                "수신자가 끝난 뒤에도 현재 작업은 마친다",
            );
        }
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
        let text = std::fs::read_to_string(dir.join("TASKS.md"))
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 파일을 읽지 못해 기본값을 쓴다"))
            .unwrap_or_default();
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
                profile: None,
                machine: None,
                notes: None,
                thread: Some(rec.id.clone()),
                done: None,
                repo: None,
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
            if t.done && parents.contains(&t.line) {
                t.attention = Some("완료 체크했지만 하위 task가 남아 있다".into());
                continue;
            }
            let Some(id) = t.thread.clone() else { continue };
            let Ok(rec) = self.load(project, &id).inspect_err(|error| {
                aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다")
            }) else {
                continue;
            };
            let marker = self.report_marker(&rec);
            t.attention = attention(t, &rec, marker, parents.contains(&t.line));
        }
    }
}

/// 명시한 병합/착지 줄의 SHA만 후보로 쓴다. 본문 속 빌드·검증 SHA와 예제 코드는 근거가 아니다.
fn merge_commits(report: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut fence = None;
    for line in report.lines() {
        let line = line.trim();
        if line.starts_with("```") || line.starts_with("~~~") {
            let mark = &line[..3];
            if fence == Some(mark) {
                fence = None;
            } else if fence.is_none() {
                fence = Some(mark);
            }
            continue;
        }
        if fence.is_some() {
            continue;
        }
        let line = line.trim_start_matches("- ").trim_start_matches("* ").replace("**", "");
        let Some((label, value)) = line.split_once(':') else { continue };
        if !matches!(label.trim(), "병합 커밋" | "착지 커밋" | "병합" | "착지" | "Merge commit" | "merge commit")
        {
            continue;
        }
        for token in value.split(|c: char| !c.is_ascii_hexdigit()) {
            if (7..=64).contains(&token.len()) && !result.iter().any(|s| s == token) {
                result.push(token.to_string());
                if result.len() == 8 {
                    return result;
                }
            }
        }
    }
    result
}

/// 자동 정리는 owner·저장소를 바꾸거나 되살리지 않는다
fn no_input() -> task_edit::Ctx<'static> {
    fn owner_ok(_: &str, _: bool) -> Result<(), String> {
        Ok(())
    }
    fn repo_ok(_: &str) -> Result<String, String> {
        Err("자동 정리는 저장소 칸을 바꾸지 않는다".into())
    }
    fn removed(_: i64) -> Result<task_edit::Removed, String> {
        Err("자동 정리는 되살리지 않는다".into())
    }
    task_edit::Ctx { owner_ok: &owner_ok, profiles: &[], repo_ok: &repo_ok, removed: &removed }
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
            (Some("main_merged"), false, Some("[착지]"), false, clean("origin/main 병합")),
            (Some("main_merged"), true, Some("[착지]"), false, check("마지막 사본 없이 끝냈다")),
            (Some("main_merged"), false, Some("[착지]"), true, check("하위 task가 남아 있다")),
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
            (Some("main_merged"), false, Some("[완료]"), true, check("하위 task가 남아 있다")),
            (Some("manual"), false, Some("[완료]"), true, check("하위 task가 남아 있다")),
        ] {
            assert_eq!(judge(why, discard, marker, children), want, "{why:?} {discard} {marker:?} {children}");
        }
    }

    #[test]
    fn only_explicit_merge_commit_lines_are_candidates() {
        let report = "[착지] 병합했다\n## Report\n- **병합 커밋**: `abcdef123`\n착지: 1234567\n검증: 7654321\n```text\n병합: 9876543\n```\n~~~\n착지: 1111111\n~~~\n병합 커밋: `abcdef123`\n";
        assert_eq!(merge_commits(report), ["abcdef123", "1234567"]);
        assert!(merge_commits("병합 커밋: --help main^ ").is_empty());
        // thread brief가 시키는 착지 결과 줄(첫 단락 끝)을 host가 병합 근거로 읽는다
        let guide = crate::brief_text::THREAD_GUIDE;
        let line = guide.split("`착지: ").nth(1).and_then(|rest| rest.split('`').next()).unwrap();
        assert_eq!(merge_commits(&format!("[완료] 결론\n\n원인과 수리.\n착지: {line}\n")), ["1a2b3c4d"]);
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

    /// 파일 사건 직후 읽은 화면은 아직 미판정이다. DB 판정 저장 뒤에도 다시 읽을 사건이 있어야 한다.
    #[test]
    fn parent_attention_is_published_after_the_verdict_is_saved() {
        let root = std::env::temp_dir().join(format!("aaa-clean-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = root.join("projects").display().to_string();
        cfg.notify.web_push = false;
        let h = Host::open(cfg, root.join("host")).unwrap();
        h.project_create("demo", Default::default(), "local:cli").unwrap();
        let dir = h.project_dir("demo").unwrap();
        std::fs::write(dir.join("TASKS.md"), "## Backlog\n- [ ] 상위\n  - [ ] 하위 · t-0001\n").unwrap();
        h.task_identify("demo").unwrap();
        let id =
            h.db.thread_insert_next("demo", |id| {
                let mut r = rec(Some("main_merged"), false);
                r.project = "demo".into();
                r.id = id.into();
                serde_json::to_string(&r).unwrap()
            })
            .unwrap();
        h.update_rec("demo", &id, |r| r.status = ThreadStatus::Resolved).unwrap();
        let r = h.load("demo", &id).unwrap();
        let mut events = h.task_events.subscribe();
        // 실제 update_rec 잠금에서 마지막 DB 저장만 보류한다. 파일 쓰기·사건·board 읽기는 실제 경로다.
        let gate = h.rec_lock.lock().unwrap();
        let worker_host = h.clone();
        let worker_dir = dir.clone();
        let worker = std::thread::spawn(move || worker_host.task_clean_one("demo", &worker_dir, &r));
        let rt = tokio::runtime::Runtime::new().unwrap();
        let before = rt
            .block_on(async {
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    loop {
                        let (_, sha256) = events.recv().await.unwrap();
                        let b = h.task_board("demo").unwrap();
                        if b.sha256 == sha256 && b.tasks.len() == 1 && b.tasks[0].thread.as_deref() == Some(id.as_str())
                        {
                            break b;
                        }
                    }
                })
                .await
            })
            .unwrap();
        assert!(before.tasks[0].attention.is_none(), "파일 사건이 DB 판정보다 먼저 왔다");
        drop(gate);
        worker.join().unwrap().unwrap();
        let final_event = events.try_recv().expect("판정 저장 뒤 tasks 사건이 빠졌다");
        let after = h.task_board("demo").unwrap();
        assert_eq!(final_event, ("demo".into(), after.sha256.clone()));
        assert_eq!(before.sha256, after.sha256, "파일은 그대로이고 판정만 바뀐다");
        assert_eq!(after.tasks[0].attention.as_deref(), Some("하위 task가 모두 끝났다(마지막 t-0001)"));
        drop(h);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// 「확인 필요」 까닭: 열린 thread·판정 전 완료는 없음, 옛 thread는 판정 없이 확인 필요, 되살린 task와 올린 상위는 그 까닭
    #[test]
    fn attention_reasons() {
        let t = task("일");
        let mut open = rec(Some("manual"), false);
        open.status = ThreadStatus::Open;
        assert_eq!(attention(&t, &open, Some("[결정 필요]"), false), None);
        assert_eq!(attention(&t, &rec(Some("main_merged"), false), None, false), None, "곧 지운다");
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
        let mut parent = rec(Some("main_merged"), true);
        parent.task_clean.parents = vec!["일".into()];
        assert_eq!(attention(&t, &parent, None, false).unwrap(), "하위 task가 모두 끝났다(마지막 t-0007)");
    }
}
