//! task 첨부(plan §8.319, 사용자 10-20 「할일에 이미지 첨부할수있게 해」, 18:3x 「md·html 문서도 task에 첨부(링크)」).
//!
//! - 올린 파일은 coordinator 대화 첨부와 같은 프로젝트 `uploads/`에 둔다(새 저장소를 만들지 않는다). 이름은 내용 sha256 앞 16자 +
//!   확장자라 같은 파일을 두 번 붙여도 하나다. 그림은 signature로 확장자를 정하고(PNG·JPEG·GIF·WebP), 문서(md·html)는 웹이 알린
//!   종류를 UTF-8 글인지 보고 받는다.
//! - TASKS.md에는 메모 한 줄로 적는다(사람이 읽는 Markdown, 프로젝트 폴더 기준 상대 경로): 그림 `![그림](uploads/<이름>)`,
//!   문서 `[제목](uploads/<이름>.md)`, 보고서 안 문서 `[제목](library/<보고서>/<경로>)`, 바깥 주소 `[제목](https://…)`. 이 모듈은
//!   줄을 쓰지 않는다: 웹이 `task_edit`로 메모를 고친다(충돌·기록·되살리기가 그대로 간다).
//! - task나 첨부 줄을 지워도 파일은 지우지 않는다: 기록의 되살리기가 같은 줄을 돌려놓으면 그대로 보인다.
//! - `--from-task`로 thread를 띄우면 메모가 가리키는 host 파일(uploads·library)을 그 머신 작업 폴더 `attachments/`에 복사하고
//!   할 일 글의 경로를 `attachments/<이름>`으로 바꾼다(`WorkAttachWrite`, 다른 머신이어도 같다). 이 기능이 없는 agent면 시작을
//!   거부한다: host 경로를 brief에 넣으면 다른 머신에서는 열 수 없다. 바깥 주소는 그대로 둔다.

use std::sync::Arc;

use aaa_proto::{AgentRequest, AgentResponse, B64, DeviceRole, ErrorBody, ErrorCode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::orch::ThreadRec;
use crate::{Host, err};

/// 파일 한 개 상한(대화 첨부와 같다)
pub(crate) const MAX_BYTES: u64 = 16 << 20;
/// agent 기능: 작업 폴더 `attachments/` 쓰기
pub(crate) const FEATURE: &str = "work_attach.v1";

/// thread 작업 폴더로 넘길 파일 하나: 거기 쓸 이름과 host 프로젝트 폴더 안 원본
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TaskFile {
    pub name: String,
    pub src: String,
}

/// uploads·attachments 안 이름(`<16 hex>.<png|jpg|gif|webp|md|html>`)인가
pub(crate) fn valid_name(name: &str) -> bool {
    let Some((stem, ext)) = name.split_once('.') else { return false };
    stem.len() == 16
        && stem.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        && matches!(ext, "png" | "jpg" | "gif" | "webp" | "md" | "html")
}

/// 받는 문서 종류 → 확장자(htm·markdown은 같은 것으로)
pub(crate) fn doc_ext(kind: &str) -> Option<&'static str> {
    match kind {
        "md" | "markdown" => Some("md"),
        "html" | "htm" => Some("html"),
        _ => None,
    }
}

pub(crate) fn mime(name: &str) -> &'static str {
    match name.rsplit('.').next() {
        Some("png") => "image/png",
        Some("jpg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("md") => "text/markdown; charset=utf-8",
        Some("html") => "text/html; charset=utf-8",
        _ => "image/webp",
    }
}

/// `library/<보고서>/<경로>`의 (보고서, 경로). 경로는 md·html 문서만
fn library_ref(target: &str) -> Option<(&str, &str)> {
    let (name, path) = target.strip_prefix("library/")?.split_once('/')?;
    let doc = path.rsplit('.').next().is_some_and(|e| doc_ext(&e.to_ascii_lowercase()).is_some());
    (crate::library::safe_rel(name) && !name.contains('/') && crate::library::safe_rel(path) && doc).then_some((name, path))
}

/// 메모 글이 가리키는 host 파일의 경로들(`](uploads/…)`·`](library/…)`, 처음 나온 차례, 겹치지 않게)
pub(crate) fn refs(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (at, _) in text.match_indices("](") {
        let rest = &text[at + 2..];
        let Some(end) = rest.find(')') else { continue };
        let target = &rest[..end];
        let ok = target.strip_prefix("uploads/").is_some_and(valid_name) || library_ref(target).is_some();
        if ok && !out.iter().any(|n| n == target) {
            out.push(target.to_string());
        }
    }
    out
}

impl Host {
    /// 웹이 붙인 파일을 프로젝트 `uploads/`에 쓰고 메모에 적을 상대 경로(`uploads/<이름>`)를 돌려준다. `kind`는 문서 종류(md·html),
    /// 없으면 그림이다
    pub(crate) fn task_file_save(
        &self,
        role: DeviceRole,
        actor: &str,
        project: &str,
        kind: Option<&str>,
        bytes: &[u8],
    ) -> Result<String, ErrorBody> {
        if !role.can_control() {
            return Err(err(ErrorCode::Forbidden, "보기 권한으로는 첨부할 수 없다"));
        }
        let dir = self.project_dir(project)?.join("uploads");
        if bytes.is_empty() || bytes.len() as u64 > MAX_BYTES {
            return Err(err(ErrorCode::BadRequest, "파일은 1바이트부터 16MiB까지 첨부한다"));
        }
        let ext = match kind {
            None => crate::attach::sniff(bytes)
                .ok_or_else(|| err(ErrorCode::BadRequest, "그림은 PNG·JPEG·GIF·WebP만 첨부한다"))?,
            Some(k) => {
                let ext = doc_ext(k).ok_or_else(|| err(ErrorCode::BadRequest, "문서는 md·html만 첨부한다"))?;
                if std::str::from_utf8(bytes).is_err() || bytes.contains(&0) {
                    return Err(err(ErrorCode::BadRequest, "문서가 UTF-8 글이 아니다"));
                }
                ext
            }
        };
        let name = format!("{}.{ext}", &hex::encode(Sha256::digest(bytes))[..16]);
        let path = dir.join(&name);
        // 같은 내용은 이미 있다(이름이 내용 hash다). symlink 등 일반 파일이 아닌 것은 덮지 않고 거부한다
        match path.symlink_metadata() {
            Ok(m) if m.is_file() => {}
            Ok(_) => return Err(err(ErrorCode::Internal, format!("uploads/{name}가 일반 파일이 아니다"))),
            Err(_) => std::fs::create_dir_all(&dir)
                .and_then(|_| aaa_common::util::write_private(&path, bytes))
                .map_err(|e| err(ErrorCode::Internal, e.to_string()))?,
        }
        if actor.strip_prefix("device:").is_some_and(|d| self.is_revoked(d)) {
            return Err(err(ErrorCode::Unauthorized, "기기가 폐기됐다"));
        }
        self.db.audit(actor, "task_file", &format!("{project} {name} {} bytes", bytes.len()));
        Ok(format!("uploads/{name}"))
    }

    /// 화면이 그릴·열 파일(보기 권한도 읽는다). 이름 꼴만 받고 uploads 밖·symlink는 읽지 않는다
    pub(crate) fn task_file_read(&self, project: &str, name: &str) -> Result<Vec<u8>, ErrorBody> {
        if !valid_name(name) {
            return Err(err(ErrorCode::BadRequest, "첨부 이름 꼴이 아니다"));
        }
        let path = self.project_dir(project)?.join("uploads").join(name);
        match path.symlink_metadata() {
            Ok(m) if m.is_file() && m.len() <= MAX_BYTES => {
                std::fs::read(&path).map_err(|e| err(ErrorCode::Internal, e.to_string()))
            }
            Ok(_) => Err(err(ErrorCode::BadRequest, "일반 파일이 아니거나 너무 크다")),
            Err(_) => Err(err(ErrorCode::NotFound, format!("uploads/{name}가 없다"))),
        }
    }

    /// `--from-task`의 첨부: 메모가 가리키는 host 파일을 고르고 할 일 글의 경로를 `attachments/<이름>`으로 바꾼다. 없는 파일
    /// (보고서 사본이 agent에만 있는 것 포함)은 넘기지 않고 경고 한 줄로 알린다
    pub(crate) fn task_attachments(&self, project: &str, task: &mut String) -> (Vec<TaskFile>, Vec<String>) {
        let Ok(dir) = self.project_dir(project) else { return (vec![], vec![]) };
        let mut found: Vec<TaskFile> = Vec::new();
        let mut missing = Vec::new();
        for target in refs(task) {
            let file = if let Some(name) = target.strip_prefix("uploads/") {
                dir.join("uploads")
                    .join(name)
                    .symlink_metadata()
                    .is_ok_and(|m| m.is_file() && m.len() <= MAX_BYTES)
                    .then(|| TaskFile { name: name.to_string(), src: target.clone() })
            } else {
                // 보고서 안 문서: 내용 hash로 이름을 붙인다(보고서마다 같은 이름 plan.md가 겹치지 않는다)
                library_ref(&target).and_then(|(report, path)| {
                    let bytes = self.report_doc_bytes(project, report, path, MAX_BYTES).ok()?;
                    let ext = doc_ext(&path.rsplit('.').next()?.to_ascii_lowercase())?;
                    Some(TaskFile { name: format!("{}.{ext}", &hex::encode(Sha256::digest(&bytes))[..16]), src: target.clone() })
                })
            };
            match file {
                Some(f) => {
                    *task = task.replace(&format!("]({target})"), &format!("](attachments/{})", f.name));
                    if !found.iter().any(|x| x.name == f.name) {
                        found.push(f);
                    }
                }
                None => missing.push(target),
            }
        }
        let warnings = (!missing.is_empty())
            .then(|| format!("task 첨부 {}개가 host에 없어 넘기지 않았다: {}", missing.len(), missing.join(", ")))
            .into_iter()
            .collect();
        (found, warnings)
    }

    /// 세션을 띄우기 전에 task 첨부를 그 머신 작업 폴더 `attachments/`에 쓴다(시작·다시 시작 공통)
    pub(crate) async fn task_attach_transfer(self: &Arc<Self>, rec: &ThreadRec) -> Result<(), ErrorBody> {
        if rec.attachments.is_empty() {
            return Ok(());
        }
        if !self.machine_feature(&rec.machine_id, FEATURE) {
            return Err(err(
                ErrorCode::Unavailable,
                format!("{} agent가 task 첨부를 받지 못한다({FEATURE} 없음). 그 머신 aaa를 갱신한다", self.machine_name(&rec.machine_id)),
            ));
        }
        let work_dir = rec.work_dir.clone().ok_or_else(|| err(ErrorCode::Internal, "작업 폴더 기록이 없다"))?;
        let dir = self.project_dir(&rec.project)?;
        let conn = self.live_conn(&rec.machine_id)?;
        for f in &rec.attachments {
            let read = if let Some((report, path)) = library_ref(&f.src) {
                self.report_doc_bytes(&rec.project, report, path, MAX_BYTES)
            } else if f.src.strip_prefix("uploads/").is_some_and(valid_name) {
                std::fs::read(dir.join(&f.src)).map_err(|e| err(ErrorCode::NotFound, e.to_string()))
            } else {
                Err(err(ErrorCode::BadRequest, "첨부 원본 꼴이 아니다"))
            };
            let bytes = read.map_err(|e| err(ErrorCode::NotFound, format!("task 첨부 {}를 읽지 못했다: {}", f.src, e.message)))?;
            if bytes.is_empty() || bytes.len() as u64 > MAX_BYTES {
                return Err(err(ErrorCode::BadRequest, format!("task 첨부 {}가 1바이트~16MiB가 아니다", f.src)));
            }
            let want = hex::encode(Sha256::digest(&bytes));
            let mut saved = false;
            for (i, chunk) in bytes.chunks(aaa_proto::ATTACH_CHUNK).enumerate() {
                let offset = i * aaa_proto::ATTACH_CHUNK;
                let response = conn
                    .request(AgentRequest::WorkAttachWrite {
                        work_dir: work_dir.clone(),
                        att: f.name.clone(),
                        offset: offset as u64,
                        data: B64(chunk.to_vec()),
                        eof: offset + chunk.len() == bytes.len(),
                    })
                    .await?;
                if let AgentResponse::Attached { sha256, .. } = response {
                    if sha256 != want {
                        return Err(err(ErrorCode::Internal, format!("넘긴 task 첨부 {}의 sha256이 다르다", f.name)));
                    }
                    saved = true;
                }
            }
            if !saved {
                return Err(err(ErrorCode::Internal, format!("task 첨부 {} 저장 응답이 없다", f.name)));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refs_cover_uploads_and_report_docs() {
        let a = "0123456789abcdef.png";
        let b = "fedcba9876543210.md";
        let text = format!(
            "고칠 화면\n![그림](uploads/{a})\n[계획](uploads/{b}) 둘째\n![다시](uploads/{a})\n[계획서](library/t-0482/plan.md)\n\
             [보고서](library/automation-design/index.html)\n[밖](uploads/../x.png)\n[링크](uploads/{a}.txt)\n\
             [숨김](library/t-0001/.git/x.md)\n[위로](library/t-0001/../x.md)\n[그림 파일](library/t-0001/shot.png)\n[바깥](https://example.com/a.md)"
        );
        assert_eq!(
            refs(&text),
            [format!("uploads/{a}"), format!("uploads/{b}"), "library/t-0482/plan.md".into(), "library/automation-design/index.html".into()]
        );
        for bad in ["0123456789ABCDEF.png", "0123456789abcdef.exe", "x.png", "../0123456789abcdef.png", "0123456789abcdef.htm"] {
            assert!(!valid_name(bad), "{bad}");
        }
        assert_eq!((doc_ext("htm"), doc_ext("markdown"), doc_ext("pdf")), (Some("html"), Some("md"), None));
    }

    /// 첨부를 받지 못하는 옛 agent(`work_attach.v1` 없음)에는 thread 기록을 만들기 전에 거부한다: 넘기지 못한 host 경로를
    /// brief에 남기지 않는다. 받는 agent로 넘기는 길은 m13 `task_attachments_reach_a_remote_agent`가 본다
    #[tokio::test]
    async fn old_agent_is_refused_before_a_record() {
        let state = std::env::temp_dir().join(format!("aaa-task-attach-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.notify.web_push = false;
        cfg.projects.root = state.join("projects").display().to_string();
        let h = Host::open(cfg, state.clone()).unwrap();
        h.project_create("demo", Default::default(), "test").unwrap();
        h.agent_connected(
            &aaa_proto::AgentHello {
                proto: aaa_proto::AGENT_LINK,
                machine_id: "m".into(),
                machine_name: "mac-a".into(),
                boot_id: "boot".into(),
                os: "test".into(),
                arch: "test".into(),
                software: "test".into(),
                features: vec![],
                dev_mode: false,
                selfcheck: None,
                sessions: vec![],
                harness: vec![],
            },
            false,
            crate::AgentConn::new(aaa_common::conn::spawn_writer(tokio::io::sink(), aaa_proto::MAX_FRAME), 1),
        );
        let start = crate::orch::StartArgs {
            holds: vec![],
            project: "demo".into(),
            title: "첨부 일".into(),
            machine: Some("mac-a".into()),
            profile: String::new(),
            kind: aaa_proto::ThreadKind::Dir,
            repo: None,
            base: None,
            task: "첨부 일\n![그림](attachments/0123456789abcdef.png)".into(),
            role: aaa_proto::Role::Worker,
            attachments: vec![TaskFile { name: "0123456789abcdef.png".into(), src: "uploads/0123456789abcdef.png".into() }],
        };
        let e = h.thread_start(start, "test").await.unwrap_err();
        assert_eq!(e.code, ErrorCode::Unavailable, "{e:?}");
        assert!(e.message.contains("mac-a") && e.message.contains(FEATURE), "{}", e.message);
        assert!(h.db.threads(Some("demo")).unwrap().is_empty(), "거부한 시작이 thread 기록을 남겼다");
        let _ = std::fs::remove_dir_all(&state);
    }
}
