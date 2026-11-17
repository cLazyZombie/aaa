//! 기획서 ID(`k-NNNN`, t-1957). 정본은 문서 첫 `# ` 제목 아래 숨은 표지 `<!-- aaa-id: k-NNNN -->`이고, 번호는 host 전역 원장
//! (`db::design_ids`)이 낸다. main 트리를 읽을 때마다 표지와 원장을 맞추고(옮기기·이름 바꾸기는 표지가 따라간다), 표지가 없는
//! 문서는 경로로 번호를 잇는다. 표지는 aaa가 저장할 때(웹 「고치기」·`aaa plan save`) 넣고, 이미 있는 문서는 「한 번에 달기」가
//! 기획 branch `design/aaa-ids` 한 commit으로 넣는다(main에는 그 branch의 「main에 반영」 착지로 들어간다).
use std::sync::Arc;

use aaa_proto::{
    AgentRequest, AgentResponse, DESIGN_MAX_TEXT, DesignBranchState, DesignIdMark, DesignTree, DeviceRole, ErrorBody,
    ErrorCode, FrontResponse,
};

use crate::db::{DesignIdDoc, design_id_text};
use crate::orch::internal;
use crate::{Host, err};

/// 트리 문서마다 표지를 읽고 표지를 넣는다(DesignTree.ids·DesignMarkIds)
pub(crate) const IDS_FEATURE: &str = "design_ids.v1";
/// 한 번에 달기의 기획 branch 주제
pub(crate) const IDS_SLUG: &str = "aaa-ids";

impl Host {
    /// main 트리의 문서마다 원장과 맞춘 ID를 싣는다. 옛 agent의 트리(`ids` 거짓)는 표지를 모르므로 번호를 주지 않는다
    pub(crate) fn design_ids_fill(&self, project: &str, repo: &str, tree: &mut DesignTree) {
        if !tree.ids {
            return;
        }
        let markers: Vec<Option<i64>> = tree
            .docs
            .iter()
            .map(|d| {
                // 문서 안 표지(옛 판 agent는 `id`에만 실었다)
                let file = d.id_file.as_deref().or(d.id.as_deref());
                file.and_then(|id| aaa_proto::short_id_number(id, aaa_proto::DESIGN_ID_PREFIX))
            })
            .collect();
        let docs: Vec<DesignIdDoc> = tree
            .docs
            .iter()
            .zip(&markers)
            .map(|(d, marker)| DesignIdDoc { path: &d.path, title: &d.title, marker: *marker })
            .collect();
        match self.db.design_ids_sync(project, repo, &docs) {
            Ok(ids) => {
                for ((d, id), marker) in tree.docs.iter_mut().zip(ids).zip(&markers) {
                    d.id = Some(design_id_text(id.number));
                    d.id_marked = id.marked;
                    d.id_file = marker.map(design_id_text);
                }
            }
            Err(e) => {
                // 번호 없이 트리는 그대로 보인다
                tracing::warn!(project, repo, error = %format!("{e:#}"), "기획서 ID 원장을 맞추지 못했다");
                for d in &mut tree.docs {
                    d.id = None;
                    d.id_marked = false;
                }
                tree.ids = false;
            }
        }
    }

    /// 저장할 글에 표지가 없으면 넣는다(Markdown 문서만, `vendor/` 아래·상한을 넘으면 그대로). 번호는 그 branch의 옛 글에 있던
    /// 표지(표지 줄을 지우고 저장했다), 없으면 그 경로의 아직 표지로 적히지 않은 원장 번호, 없으면 새 번호다
    pub(crate) async fn design_text_with_id(
        &self,
        project: &str,
        repo: &str,
        slug: &str,
        path: &str,
        existing: bool,
        text: String,
    ) -> String {
        let markdown = std::path::Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("md"));
        if !markdown || aaa_proto::design_id_unmarked_path(path) || aaa_proto::design_text_id(&text).is_some() {
            return text;
        }
        let before = if existing {
            match self.design_read(project, repo, path, Some(slug)).await {
                Ok(FrontResponse::DesignDoc { doc }) => aaa_proto::design_text_id(&doc.text),
                _ => None,
            }
        } else {
            None
        };
        let id = match before {
            Some(id) => id,
            None => match self.db.design_id_for_path(project, repo, path) {
                Ok(n) => design_id_text(n),
                Err(e) => {
                    tracing::warn!(project, repo, path, error = %format!("{e:#}"), "기획서 ID 번호를 내지 못해 표지 없이 저장한다");
                    return text;
                }
            },
        };
        let out = aaa_proto::design_put_id(&text, &id);
        if out.len() as u64 > DESIGN_MAX_TEXT { text } else { out }
    }

    /// 지금 main 트리에 있는 문서의 ID(원장에서, agent를 거치지 않는다)
    pub(crate) fn design_ids_list(&self, project: &str) -> Result<FrontResponse, ErrorBody> {
        self.project_dir(project)?;
        let ids = self.db.design_ids_present(project).map_err(|e| internal(format!("{e:#}")))?;
        Ok(FrontResponse::DesignIds { ids })
    }

    /// 한 번에 달기(사람만): main 트리를 새로 읽어 표지가 없는 문서를 고르고, 기획 branch `design/aaa-ids`를 열어(쉬던 것은 다시)
    /// 그 문서들에 표지를 한 commit으로 넣는다. 다시 불러도 이미 단 문서는 건너뛴다
    pub(crate) async fn design_mark_ids(
        self: &Arc<Self>,
        role: DeviceRole,
        actor: &str,
        project: &str,
        repo: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        let by = self.design_writer(role, actor, project, None)?;
        let (target, mid) = self.design_target(project, repo)?;
        if !self.machine_feature(&mid, IDS_FEATURE) {
            return Err(err(
                ErrorCode::Unavailable,
                "그 머신 agent가 기획서 ID를 아직 모른다(agent binary를 바꿔야 한다)",
            ));
        }
        let folder = target.folder.clone().unwrap_or_default();
        let view = self.design_repo(project, target.clone()).await;
        let tree = match (view.tree, view.error) {
            (Some(tree), None) if tree.ids => tree,
            (_, Some(e)) => return Err(err(ErrorCode::Unavailable, format!("기획 트리를 새로 읽지 못했다: {e}"))),
            _ => return Err(err(ErrorCode::Unavailable, "기획 트리의 ID를 읽지 못했다")),
        };
        // 표지가 없는 문서만(복사·다른 저장소 표지는 고쳐 쓰지 않으므로 고르지 않는다)
        let ids: Vec<DesignIdMark> = tree
            .docs
            .iter()
            .filter(|d| d.id_file.is_none() && !aaa_proto::design_id_unmarked_path(&d.path))
            .filter_map(|d| d.id.clone().map(|id| DesignIdMark { path: d.path.clone(), id }))
            .collect();
        if ids.is_empty() {
            return Ok(FrontResponse::DesignIdsMarked { branch: IDS_SLUG.into(), marked: 0, skipped: 0, commit: None });
        }
        Box::pin(self.design_branch_open_with(
            role,
            actor,
            project,
            &target.name,
            IDS_SLUG,
            Some("기획서 ID 표지".into()),
            None,
            true,
        ))
        .await?;
        let lock = self.design_branch_lock(project, &target.name, IDS_SLUG);
        let _one = lock.lock().await;
        let mut rec = self
            .design_branch_load(project, &target.name, IDS_SLUG)?
            .ok_or_else(|| err(ErrorCode::NotFound, format!("기획 branch가 없다: design/{IDS_SLUG}")))?;
        if rec.state == DesignBranchState::Rest {
            return Err(err(ErrorCode::Conflict, format!("쉬는 기획 branch다: design/{IDS_SLUG}(다시 연 뒤 단다)")));
        }
        // 반영 중·반영 예약된 branch에 더 쓰지 않는다(main에 들어간 뒤 다시 단다)
        if let Some(id) = self.design_applying(project, &rec) {
            return Err(err(
                ErrorCode::Conflict,
                format!("design/{IDS_SLUG}를 main에 반영 중이다: {id}(들어간 뒤 다시 단다)"),
            ));
        }
        if rec.apply_wait.is_some() {
            return Err(err(ErrorCode::Conflict, format!("design/{IDS_SLUG}의 반영을 예약했다(들어간 뒤 다시 단다)")));
        }
        let conn = self.design_write_conn(&mid)?;
        let count = ids.len();
        let req = AgentRequest::DesignMarkIds {
            project: project.to_string(),
            repo: target.path.clone(),
            folder,
            slug: IDS_SLUG.into(),
            ids,
            message: format!("기획: 기획서 ID 표지 {count}개 ({by})"),
        };
        let AgentResponse::DesignIdsMarked { commit, marked } =
            conn.request_timeout(req, crate::design_branch::WRITE_WAIT).await?
        else {
            return Err(err(ErrorCode::Internal, "예상하지 못한 응답"));
        };
        if let Some(commit) = &commit {
            self.design_chat_event(
                project,
                &target.name,
                IDS_SLUG,
                "edit",
                &format!("기획서 ID 표지 {}개", marked.len()),
                Some(commit.clone()),
            )
            .await;
            rec.updated_ms = aaa_common::util::now_ms();
            rec.head = Some(commit.clone());
            rec.own_head = Some(commit.clone());
            rec.push_error = None;
            rec.push_failures = 0;
            rec.rest_error = None;
            self.design_branch_store(project, &rec)?;
            self.design_push_later(project, &target.name, IDS_SLUG);
        }
        self.db.audit(
            actor,
            "design_mark_ids",
            &format!("{project} {}/{IDS_SLUG} {} {}", target.name, marked.len(), commit.as_deref().unwrap_or("-")),
        );
        // 고른 문서 중 이 branch에 없거나(그 사이 main에서 옮김·더함, 옛 branch) 이미 단 문서
        let skipped = (count - marked.len()) as u32;
        Ok(FrontResponse::DesignIdsMarked { branch: IDS_SLUG.into(), marked: marked.len() as u32, skipped, commit })
    }
}
