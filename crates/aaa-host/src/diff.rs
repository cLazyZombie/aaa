//! thread 변경 비교(plan §8.305). host는 git을 실행하지 않는다: 그 thread의 머신 agent(diff.v1)가 worktree에서 읽는다.
//! base는 thread를 시작할 때 agent가 정한 기준 ref(`ThreadRec.base`)다. 답은 그대로 웹에 넘긴다(host에 쌓지 않는다).

use std::time::Duration;

use aaa_proto::{
    AgentRequest, AgentResponse, DiffQuery, DiffScope, ErrorBody, ErrorCode, FrontResponse, ResolvePreview, ThreadKind,
    ThreadStatus,
};

use crate::{Host, err};

impl Host {
    pub(crate) async fn thread_diff(
        &self,
        project: &str,
        thread: &str,
        query: DiffQuery,
    ) -> Result<FrontResponse, ErrorBody> {
        let rec = self.load(project, thread)?;
        if rec.repo.is_none() {
            return Err(err(ErrorCode::NotFound, "git 저장소에서 일하는 thread가 아니다"));
        }
        let cwd = rec.cwd.clone().ok_or_else(|| err(ErrorCode::NotFound, "thread 작업 폴더(worktree) 기록이 없다"))?;
        let conn = self.live_conn(&rec.machine_id)?;
        if !self.machine_feature(&rec.machine_id, "diff.v1") {
            return Err(err(
                ErrorCode::Unavailable,
                "그 머신 agent가 변경 파일 읽기를 아직 모른다(agent binary를 바꿔야 한다)",
            ));
        }
        // agent는 비교 전체를 20초에 끊는다. 그 위에 연결 여유를 둔다
        let wait = aaa_common::diff::QUERY_TIMEOUT + Duration::from_secs(10);
        match conn.request_timeout(AgentRequest::ThreadDiff { cwd, base: rec.base.clone(), query }, wait).await? {
            AgentResponse::Diff(diff) => Ok(FrontResponse::ThreadDiff { project: rec.project, thread: rec.id, diff }),
            other => Err(err(ErrorCode::Internal, format!("예상하지 못한 응답: {other:?}"))),
        }
    }
}

/// 끝내기 미리보기의 파일 수. 다 세지 못한 수(새 파일이 상한을 넘어 뺐다, 목록을 잘랐다)나 기준을 찾지 못한 branch 범위(agent가
/// HEAD로 떨어져 미커밋과 같아진다)는 0으로 보이면 안 된다: 모름(까닭)이다
fn diff_count(diff: &aaa_proto::DiffView) -> Result<u32, String> {
    match &diff.files {
        _ if diff.untracked_omitted || diff.truncated => Err("바뀐 파일이 너무 많아 다 세지 못했다".into()),
        _ if diff.scope == DiffScope::Branch && diff.base == "HEAD" => {
            Err("비교할 base(origin/main 등)를 찾지 못했다".into())
        }
        Some(f) => Ok(f.len() as u32),
        None => Err("바뀐 파일이 너무 많아 다 세지 못했다".into()),
    }
}

impl Host {
    /// 끝내기 전 확인(관리 GUI W4, plan §8.333): 커밋하지 않은 파일·base 대비 바뀐 파일·사본 상태. 기록·inbox를 고치지 않는다:
    /// PR은 기록된 상태만 본다(`merged_head`는 PR을 다시 찾아 기록을 고친다). 읽지 못한 것은 모름이고, 모르면 남기기가 기본이다.
    pub(crate) async fn thread_resolve_preview(
        &self,
        project: &str,
        thread: &str,
    ) -> Result<ResolvePreview, ErrorBody> {
        let rec = self.load(project, thread)?;
        let worktree = rec.kind == ThreadKind::Worktree && rec.cwd.is_some();
        let merged = rec.pr_state.as_deref().is_some_and(|s| s.eq_ignore_ascii_case("merged"));
        let count = |r: Result<FrontResponse, ErrorBody>| -> Result<u32, String> {
            match r {
                Ok(FrontResponse::ThreadDiff { diff, .. }) => diff_count(&diff),
                Ok(other) => Err(format!("예상하지 못한 응답: {other:?}")),
                Err(e) => Err(e.message),
            }
        };
        let (mut uncommitted, mut changed, mut unknown) = (None, None, None);
        if rec.repo.is_some() && rec.cwd.is_some() {
            let ask = |scope| self.thread_diff(project, thread, DiffQuery { scope, ..Default::default() });
            let (u, c) = tokio::join!(ask(DiffScope::Uncommitted), ask(DiffScope::Branch));
            match (count(u), count(c)) {
                (Ok(u), Ok(c)) => (uncommitted, changed) = (Some(u), Some(c)),
                (Err(e), _) | (_, Err(e)) => unknown = Some(e),
            }
        } else if worktree {
            unknown = Some("git 저장소 기록이 없다".into());
        }
        let mut risks = Vec::new();
        if rec.status == ThreadStatus::Starting {
            risks.push("준비 중이라 지금은 끝낼 수 없다".into());
        }
        if let Some(n) = uncommitted.filter(|n| *n > 0) {
            risks.push(format!("커밋하지 않은 파일 {n}개: worktree를 지우면 잃는다"));
        }
        if !merged && changed.is_some_and(|c| c > uncommitted.unwrap_or(0)) {
            risks.push(format!(
                "base({}) 대비 병합되지 않은 변경이 있다: branch는 남지만 worktree 밖에는 사본이 없을 수 있다",
                rec.base.as_deref().unwrap_or("기준")
            ));
        }
        if rec.copy.as_deref() == Some("partial") {
            risks.push(format!("사본에 빠진 것이 있다: {}", rec.copy_notes.join("; ")));
        }
        if let Some(why) = unknown.as_ref().filter(|_| worktree) {
            risks.push(format!("worktree 상태를 모른다: {why}"));
        }
        Ok(ResolvePreview {
            project: rec.project.clone(),
            thread: rec.id.clone(),
            thread_kind: rec.kind,
            worktree,
            branch: rec.branch.clone(),
            merged,
            copy: rec.copy.clone(),
            copy_notes: rec.copy_notes.clone(),
            uncommitted,
            changed,
            unknown,
            keep_default: worktree && !risks.is_empty(),
            risks,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aaa_proto::{DiffFile, DiffStatus, DiffView};

    /// 다 세지 못했거나 base를 못 찾은 비교는 0개가 아니라 모름이다(그래야 위험으로 보고 남기기가 기본이 된다)
    #[test]
    fn preview_counts_are_unknown_when_the_diff_is_partial_or_baseless() {
        let file = DiffFile {
            path: "a".into(),
            old_path: None,
            status: DiffStatus::Modified,
            additions: Some(1),
            deletions: Some(0),
        };
        let view = |scope, base: &str| DiffView {
            scope,
            base: base.into(),
            files: Some(vec![file.clone()]),
            ..Default::default()
        };
        assert_eq!(diff_count(&view(DiffScope::Uncommitted, "HEAD")), Ok(1));
        assert_eq!(diff_count(&view(DiffScope::Branch, "origin/main")), Ok(1));
        assert!(diff_count(&view(DiffScope::Branch, "HEAD")).unwrap_err().contains("base"));
        let omitted =
            DiffView { untracked_omitted: true, files: Some(Vec::new()), ..view(DiffScope::Uncommitted, "HEAD") };
        assert!(diff_count(&omitted).is_err(), "새 파일을 뺀 0개는 모름이다");
        let cut = DiffView { truncated: true, ..view(DiffScope::Uncommitted, "HEAD") };
        assert!(diff_count(&cut).is_err());
        assert!(diff_count(&DiffView { files: None, ..view(DiffScope::Uncommitted, "HEAD") }).is_err());
    }
}
