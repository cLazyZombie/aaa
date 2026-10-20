//! thread 변경 비교(plan §8.305). host는 git을 실행하지 않는다: 그 thread의 머신 agent(diff.v1)가 worktree에서 읽는다.
//! base는 thread를 시작할 때 agent가 정한 기준 ref(`ThreadRec.base`)다. 답은 그대로 웹에 넘긴다(host에 쌓지 않는다).

use std::time::Duration;

use aaa_proto::{AgentRequest, AgentResponse, DiffQuery, ErrorBody, ErrorCode, FrontResponse};

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
