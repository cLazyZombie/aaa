//! 기획 문서 ID와 main 트리를 맞춘다.
use crate::{
    Host,
    db::{DesignIdDoc, design_id_text},
    orch::internal,
};
use aaa_proto::{DesignTree, ErrorBody, FrontResponse};
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
    /// 지금 main 트리에 있는 문서의 ID(원장에서, agent를 거치지 않는다)
    pub(crate) fn design_ids_list(&self, project: &str) -> Result<FrontResponse, ErrorBody> {
        self.project_dir(project)?;
        let ids = self.db.design_ids_present(project).map_err(|e| internal(format!("{e:#}")))?;
        Ok(FrontResponse::DesignIds { ids })
    }
}
