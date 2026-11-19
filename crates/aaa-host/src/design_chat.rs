//! 이전 대화 기록을 읽어 기획 이력에 표시한다.
use crate::{Host, orch::internal};
use aaa_proto::ErrorBody;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
/// 기획 branch 하나의 대화
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Chat {
    pub thread: Option<String>,
    /// 아직 보내지 못한 말(차례대로)
    pub pending: Vec<String>,
    /// thread를 띄우지 못한 까닭(다음 말에 지운다)
    pub error: Option<String>,
    /// 마지막으로 말한 때(쉼을 정할 때 마지막 저장과 함께 본다)
    pub said_ms: u64,
    /// 버린 대화: 띄우는 중이던 배경 일이 끝나면 이것을 보고 그 thread를 끝낸다. 다시 말하면 지운다.
    /// 같은 주제의 기획 branch를 새로 열면 기록째 지운다(`design_chat_forget`)
    pub retired: bool,
    /// 대화 thread profile(새 대화 창의 모델, 없으면 프로젝트 기본)
    pub profile: Option<String>,
    /// 대화에 끼우는 host 사건(오래된 차례, 상한 `MAX_EVENTS`)
    pub events: Vec<aaa_proto::DesignEvent>,
    /// 사람이 고쳤고 아직 알리지 않은 문서(다음 말 앞에 「(문서 고침: …)」으로 붙는다)
    pub edited: Vec<String>,
    /// 마지막으로 보낸 말의 앞부분(그 뒤 사건을 그 말의 답 뒤에 끼운다)
    pub last_said: Option<String>,
}

impl Host {
    fn design_chat_path(&self, project: &str, repo: &str, slug: &str) -> Result<PathBuf, ErrorBody> {
        // 기획 branch 기록과 같은 이름 검사(저장소 이름·주제)를 거친다
        self.design_branch_load(project, repo, slug)?;
        Ok(self.state.join("design-chats").join(project).join(repo).join(format!("{slug}.json")))
    }

    pub(crate) fn design_chat_load(&self, project: &str, repo: &str, slug: &str) -> Result<Chat, ErrorBody> {
        let path = self.design_chat_path(project, repo, slug)?;
        match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| internal(format!("{}: {e}", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Chat::default()),
            Err(e) => Err(internal(format!("{}: {e}", path.display()))),
        }
    }
}
