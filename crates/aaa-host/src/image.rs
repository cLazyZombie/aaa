//! host 소유 이미지 원장. 요청 ID·소유 세션·예약을 보존하고 불명 실행을 반복하지 않는다.
use crate::{Host, account_pool::Allocation, err};
use aaa_common::util;
use aaa_proto::*;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

const PREFIX: &str = "image-job:";
// 구독 소비율의 환산값이 아닌, 기존 small 작업과 같은 초기 배정 정책이다.
// 51% 잔량 - 안전 여유 10%p에서 새 관측 없이도 시안 8건(40%p)을 배정한다.
pub(crate) const RESERVE: u8 = 5;

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Job {
    pub begin: ImageBegin,
    pub machine: String,
    pub session: String,
    pub project: String,
    pub owner: String,
    pub model: String,
    pub effort: String,
    pub view: ImageView,
    pub allocation: Option<Allocation>,
    pub launched: bool,
}
pub(crate) struct Reservation {
    pub account: String,
    pub allocation: Allocation,
}
pub(crate) fn pool_reservations(tx: &Connection) -> anyhow::Result<Vec<Reservation>> {
    let mut st = tx.prepare("SELECT value FROM meta WHERE substr(key,1,10)='image-job:'")?;
    let rows = st.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut out = vec![];
    for text in rows {
        let j: Job = serde_json::from_str(&text)?;
        if let (Some(account), Some(allocation)) = (j.view.account, j.allocation) {
            out.push(Reservation { account, allocation });
        }
    }
    Ok(out)
}
fn bad(text: &str) -> ErrorBody {
    err(ErrorCode::BadRequest, text)
}
fn io_error(_: impl std::fmt::Display) -> ErrorBody {
    bad("이미지 작업 파일·원장을 처리하지 못했다")
}
fn valid_id(id: &str) -> bool {
    id.len() == 26 && id.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}
fn key(id: &str) -> String {
    format!("{PREFIX}{id}")
}
pub(crate) fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
pub(crate) fn descriptor(bytes: &[u8]) -> ImageFile {
    ImageFile { bytes: bytes.len() as u64, sha256: sha(bytes) }
}
pub(crate) fn image_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("jpg")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("webp")
    } else {
        None
    }
}
fn regular(path: &Path) -> Result<std::fs::Metadata, ErrorBody> {
    let m = path.symlink_metadata().map_err(io_error)?;
    if !m.is_file() || m.len() > IMAGE_MAX_BYTES {
        return Err(bad("이미지 파일 형식·크기·symlink를 확인한다"));
    }
    Ok(m)
}

impl Host {
    pub(crate) fn image_dir(&self, id: &str) -> Result<PathBuf, ErrorBody> {
        if !valid_id(id) {
            return Err(bad("이미지 작업 ID는 26자리 영문 대문자·숫자다"));
        }
        let parent = self.state.join("image-jobs");
        for p in [&parent, &parent.join(id)] {
            if p.symlink_metadata()
                .inspect_err(|error| aaa_common::diagnostic::observe(error, "성공 여부만 쓰는 분기의 하위 오류"))
                .is_ok_and(|m| !m.is_dir())
            {
                return Err(bad("이미지 작업 폴더를 확인한다"));
            }
        }
        Ok(parent.join(id))
    }
    fn image_load(&self, id: &str) -> Result<Job, ErrorBody> {
        if !valid_id(id) {
            return Err(bad("이미지 작업 ID를 확인한다"));
        }
        self.db
            .meta(&key(id))
            .map_err(io_error)?
            .ok_or_else(|| err(ErrorCode::NotFound, "이미지 작업이 없다"))
            .and_then(|s| serde_json::from_str(&s).map_err(io_error))
    }
    pub(crate) fn image_save(&self, job: &Job) -> Result<(), ErrorBody> {
        let mut clean = job.clone();
        clean.view.reason = aaa_common::redact::text(&clean.view.reason).chars().take(1024).collect();
        self.db.set_meta(&key(&job.begin.id), &serde_json::to_string(&clean).map_err(io_error)?).map_err(io_error)?;
        self.bump();
        self.accounts_bump();
        Ok(())
    }
    fn image_owner(&self, machine: &str, session: &str) -> Result<(String, String), ErrorBody> {
        let i = self.inner.lock().unwrap();
        i.panes
            .get(session)
            .filter(|p| p.machine_id == machine && p.info.state.is_running())
            .ok_or_else(|| err(ErrorCode::Forbidden, "실행 중인 자기 세션의 이미지 요청만 허용한다"))?;
        drop(i);
        if let Some(r) = self.thread_of_pane(session) {
            if r.role != Role::Worker || r.status != ThreadStatus::Open {
                return Err(err(ErrorCode::Forbidden, "열린 worker thread에서 이미지를 요청한다"));
            }
            return Ok((r.project, format!("thread:{}", r.id)));
        }
        self.coordinator_project_of_actor(&format!("local:image@{session}"))
            .map(|project| (project, "coordinator".into()))
            .ok_or_else(|| err(ErrorCode::Forbidden, "프로젝트의 thread·coordinator에서 이미지를 요청한다"))
    }
    pub(crate) async fn image_request(
        self: &Arc<Self>,
        machine: &str,
        session: &str,
        request: ImageRequest,
    ) -> Result<ImageResponse, ErrorBody> {
        let _account_read = self.account_rename_gate.read().await;
        let (project, owner) = self.image_owner(machine, session)?;
        if matches!(request, ImageRequest::List) {
            let jobs = self
                .db
                .meta_suffixes(PREFIX)
                .map_err(io_error)?
                .iter()
                .map(|id| self.image_load(id))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .filter(|j| {
                    j.project == project
                        && j.view.state != ImageState::Forgotten
                        && (j.owner == owner || owner == "coordinator")
                })
                .map(|j| {
                    let mut view = j.view;
                    view.reason = view.reason.chars().take(200).collect();
                    view
                })
                .collect();
            return Ok(ImageResponse::Jobs { jobs });
        }
        let id = request.id().ok_or_else(|| bad("이미지 작업 ID가 없다"))?.to_string();
        let dir = self.image_dir(&id)?;
        let guard = self.thread_op_lock("image", &id).lock_owned().await;
        if let ImageRequest::Begin(mut begin) = request {
            let _begin_guard = self.thread_op_lock("image", "begin").lock_owned().await;
            begin.prompt = aaa_common::redact::text(&begin.prompt);
            if begin.prompt.trim().is_empty()
                || begin.prompt.len() > 32000
                || begin.inputs.len() > IMAGE_MAX_COUNT
                || begin.inputs.iter().any(|f| {
                    f.bytes == 0
                        || f.bytes > IMAGE_MAX_BYTES
                        || f.sha256.len() != 64
                        || !f.sha256.bytes().all(|b| b.is_ascii_hexdigit())
                })
                || begin.inputs.iter().map(|f| f.bytes).sum::<u64>() > IMAGE_MAX_BYTES
            {
                return Err(bad("이미지 입력은 본문 32KB·참고 5개·합계 50MiB 이하다"));
            }
            if self.db.meta(&key(&id)).map_err(io_error)?.is_some() {
                let old = self.image_load(&id)?;
                if old.owner != owner || old.project != project || old.begin != begin {
                    return Err(err(ErrorCode::Conflict, "다른 요청에 쓴 이미지 작업 ID다"));
                }
                return Ok(ImageResponse::Job(old.view));
            }
            let ids = self.db.meta_suffixes(PREFIX).map_err(io_error)?;
            if ids
                .iter()
                .filter_map(|id| {
                    self.image_load(id)
                        .inspect_err(|error| {
                            aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                        })
                        .ok()
                })
                .filter(|j| j.view.state != ImageState::Forgotten)
                .count()
                >= 256
            {
                return Err(bad("host 이미지 원장 한도(256건)에 도달했다"));
            }
            let own = ids
                .iter()
                .filter_map(|id| {
                    self.image_load(id)
                        .inspect_err(|error| {
                            aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                        })
                        .ok()
                })
                .filter(|j| {
                    j.owner == owner
                        && j.project == project
                        && matches!(
                            j.view.state,
                            ImageState::Draft | ImageState::Queued | ImageState::Running | ImageState::Waiting
                        )
                })
                .count();
            if own >= 4 {
                return Err(bad("세션당 미완료 이미지 작업은 4건까지다"));
            }
            self.check_profile_allowed(&project, &begin.profile)?;
            let profile = self.configured_thread_profile(Some(&project), &begin.profile)?;
            if profile.harness != "codex" || profile.model.is_empty() {
                return Err(bad("이미지 실행에는 모델을 명시한 Codex profile이 필요하다"));
            }
            self.image_account_allowed(&project, begin.account.as_deref())?;
            util::ensure_private_dir(dir.parent().unwrap()).map_err(io_error)?;
            util::ensure_private_dir(&dir).map_err(io_error)?;
            let view = ImageView {
                id: id.clone(),
                state: ImageState::Draft,
                account: None,
                reason: "참고 이미지 전송 뒤 시작한다".into(),
                outputs: vec![],
                total_tokens: None,
            };
            self.image_save(&Job {
                begin,
                machine: machine.into(),
                session: session.into(),
                project,
                owner,
                model: profile.model,
                effort: profile.effort,
                view: view.clone(),
                allocation: None,
                launched: false,
            })?;
            return Ok(ImageResponse::Job(view));
        }
        let mut job = self.image_load(&id)?;
        let project_read = owner == "coordinator"
            && matches!(request, ImageRequest::Status { .. } | ImageRequest::Read { .. } | ImageRequest::Forget { .. });
        if (job.owner != owner && !project_read) || job.project != project {
            return Err(err(ErrorCode::Forbidden, "자기 이미지 작업만 접근한다"));
        }
        match request {
            ImageRequest::Begin(_) | ImageRequest::List => unreachable!(),
            ImageRequest::Forget { .. } => {
                if matches!(job.view.state, ImageState::Queued | ImageState::Running | ImageState::Unknown) {
                    return Err(bad("실행 중이거나 결과 불명인 이미지 작업은 보존한다"));
                }
                for (prefix, count) in [("input", job.begin.inputs.len()), ("output", job.view.outputs.len())] {
                    for index in 0..count {
                        match std::fs::remove_file(dir.join(format!("{prefix}-{index}"))) {
                            Ok(()) => {}
                            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                            Err(e) => return Err(io_error(e)),
                        }
                    }
                }
                job.view.state = ImageState::Forgotten;
                job.view.reason = "이미지 파일 정리 완료 · 중복 방지 영수증과 사용량 예약은 보존".into();
                job.view.outputs.clear();
                self.image_save(&job)?;
            }
            ImageRequest::Input { index, offset, data, .. } => {
                if job.view.state != ImageState::Draft {
                    return Err(bad("시작 전 이미지 입력만 바꿀 수 있다"));
                }
                let input = job.begin.inputs.get(index as usize).ok_or_else(|| bad("참고 이미지 번호를 확인한다"))?;
                if data.0.is_empty()
                    || data.0.len() > IMAGE_CHUNK
                    || offset.saturating_add(data.0.len() as u64) > input.bytes
                {
                    return Err(bad("이미지 전송 조각의 위치·크기를 확인한다"));
                }
                let path = dir.join(format!("input-{index}"));
                if !path.exists() {
                    util::write_private(&path, &[]).map_err(io_error)?;
                }
                let size = regular(&path)?.len();
                let mut f = std::fs::OpenOptions::new().read(true).write(true).open(path).map_err(io_error)?;
                if offset == size {
                    f.seek(SeekFrom::End(0)).map_err(io_error)?;
                    f.write_all(&data.0).map_err(io_error)?;
                    f.sync_data().map_err(io_error)?;
                } else if offset + data.0.len() as u64 <= size {
                    f.seek(SeekFrom::Start(offset)).map_err(io_error)?;
                    let mut old = vec![0; data.0.len()];
                    f.read_exact(&mut old).map_err(io_error)?;
                    if old != data.0 {
                        return Err(err(ErrorCode::Conflict, "같은 위치에 다른 이미지 조각을 보냈다"));
                    }
                } else {
                    return Err(bad("참고 이미지를 처음부터 순서대로 보낸다"));
                }
            }
            ImageRequest::Start { .. } => {
                if matches!(job.view.state, ImageState::Draft | ImageState::Waiting) {
                    let _dispatch = self.thread_op_lock("image", "dispatch").lock_owned().await;
                    let active = self
                        .db
                        .meta_suffixes(PREFIX)
                        .map_err(io_error)?
                        .iter()
                        .filter_map(|id| {
                            self.image_load(id)
                                .inspect_err(|error| {
                                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                                })
                                .ok()
                        })
                        .filter(|j| matches!(j.view.state, ImageState::Queued | ImageState::Running))
                        .count();
                    if active >= 4 {
                        return Err(err(
                            ErrorCode::Unavailable,
                            "host 이미지 동시 실행 한도(4건) · 같은 ID로 나중에 시작한다",
                        ));
                    }
                    job.machine = machine.into();
                    job.session = session.into();
                    self.image_account_allowed(&project, job.begin.account.as_deref())?;
                    for (index, expected) in job.begin.inputs.iter().enumerate() {
                        let path = dir.join(format!("input-{index}"));
                        regular(&path)?;
                        let bytes = std::fs::read(&path).map_err(io_error)?;
                        if &descriptor(&bytes) != expected || image_extension(&bytes).is_none() {
                            return Err(bad("참고 이미지의 길이·SHA·파일 형식을 확인한다"));
                        }
                    }
                    job.view.state = ImageState::Queued;
                    job.view.reason = "host 계정 배정 중".into();
                    self.image_save(&job)?;
                    let h = self.clone();
                    let id = id.clone();
                    tokio::spawn(async move {
                        h.image_run(&id).await;
                    });
                }
            }
            ImageRequest::Status { wait, .. } => {
                let mut changes = self.changed.subscribe();
                drop(guard);
                if wait && matches!(job.view.state, ImageState::Queued | ImageState::Running) {
                    // 사건 또는 응답 상한으로 끝난다. 다시 보내도 생성은 하지 않는다.
                    aaa_common::diagnostic::ignored_nested(
                        tokio::time::timeout(Duration::from_secs(1), changes.changed()).await,
                        "이미지 상태 사건 또는 상한 뒤 기존 기록 조회",
                    );
                    job = self.image_load(&id)?;
                }
                return Ok(ImageResponse::Job(job.view));
            }
            ImageRequest::Read { index, offset, .. } => {
                if job.view.state != ImageState::Succeeded {
                    return Err(bad("끝난 이미지 결과만 읽을 수 있다"));
                }
                let expected =
                    job.view.outputs.get(index as usize).ok_or_else(|| bad("출력 이미지 번호를 확인한다"))?;
                if offset > expected.bytes {
                    return Err(bad("출력 이미지 위치를 확인한다"));
                }
                let path = dir.join(format!("output-{index}"));
                if regular(&path)?.len() != expected.bytes {
                    return Err(bad("출력 이미지 길이가 바뀌었다"));
                }
                let mut f = std::fs::File::open(path).map_err(io_error)?;
                f.seek(SeekFrom::Start(offset)).map_err(io_error)?;
                let mut data = vec![0; IMAGE_CHUNK.min((expected.bytes - offset) as usize)];
                f.read_exact(&mut data).map_err(io_error)?;
                return Ok(ImageResponse::Chunk { eof: offset + data.len() as u64 == expected.bytes, data: B64(data) });
            }
        }
        Ok(ImageResponse::Job(job.view))
    }
    fn image_account_allowed(&self, _project: &str, requested: Option<&str>) -> Result<(), ErrorBody> {
        let pool = self.account_pool()?;
        if self.pool_harness_enabled("codex")? {
            if requested.is_some_and(|a| !pool.members.iter().any(|m| m.account == a)) {
                return Err(err(ErrorCode::Forbidden, "이 프로젝트에 허용한 host 계정 풀에서 고른다"));
            }
        } else {
            let default = self
                .default_account("codex")?
                .ok_or_else(|| bad("host의 Codex 기본 계정이나 프로젝트 계정 풀이 필요하다"))?;
            if requested.is_some_and(|a| a != default) {
                return Err(err(ErrorCode::Forbidden, "host의 Codex 기본 계정만 허용한다"));
            }
        }
        Ok(())
    }

    async fn image_run(self: Arc<Self>, id: &str) {
        let _account_read = self.account_rename_gate.read().await;
        let guard = self.thread_op_lock("image", id).lock_owned().await;
        let Ok(mut job) = self
            .image_load(id)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
        else {
            return;
        };
        if job.view.state != ImageState::Queued {
            return;
        }
        drop(guard);
        let result = self.image_prepare(&mut job).await;
        if let Err(e) = result {
            job.view.state = if e.code == ErrorCode::Unavailable { ImageState::Waiting } else { ImageState::Failed };
            job.view.reason = e.message;
            aaa_common::diagnostic::ignored(self.image_save(&job), "보조 작업 실패 뒤 기존 처리를 계속한다");
            return;
        }
        // 송신 전 영속 표지. host가 중단되면 다음 host는 Unknown으로 남기며 다시 생성하지 않는다.
        job.launched = true;
        job.view.state = ImageState::Running;
        job.view.reason = "host Codex에서 이미지 생성 중".into();
        if self
            .image_save(&job)
            .as_ref()
            .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
            .is_err()
        {
            return;
        }
        // 실행 표지를 확정한 뒤에는 계정별 running 검사로 이름 변경을 거부한다. 다른 계정 변경은 막지 않는다.
        drop(_account_read);
        let result = crate::image_rpc::generate(&self, &job).await;
        match result {
            Ok((outputs, tokens)) => {
                job.view.state = ImageState::Succeeded;
                job.view.outputs = outputs;
                job.view.total_tokens = tokens;
                job.view.reason = "이미지 생성 완료".into();
            }
            Err(failure) => {
                job.view.state = if failure.uncertain { ImageState::Unknown } else { ImageState::Failed };
                job.view.reason = failure.reason;
                job.launched = failure.sent;
                if !failure.sent
                    && let Some(a) = &mut job.allocation
                {
                    a.released = true;
                }
            }
        }
        if job.view.state != ImageState::Unknown
            && let Some(a) = &mut job.allocation
        {
            a.ended_at_ms = Some(util::now_ms());
        }
        aaa_common::diagnostic::ignored(self.image_save(&job), "보조 작업 실패 뒤 기존 처리를 계속한다");
        self.db.audit("host", "image_generation", &format!("{} {} {:?}", job.project, id, job.view.state));
    }

    async fn image_prepare(self: &Arc<Self>, job: &mut Job) -> Result<(), ErrorBody> {
        let (project, owner) = self.image_owner(&job.machine, &job.session)?;
        if project != job.project || owner != job.owner {
            return Err(err(ErrorCode::Forbidden, "이미지 작업의 thread 소속이 바뀌었다"));
        }
        self.check_profile_allowed(&job.project, &job.begin.profile)?;
        let profile = self.configured_thread_profile(Some(&job.project), &job.begin.profile)?;
        if profile.harness != "codex" || profile.model != job.model || profile.effort != job.effort {
            return Err(err(ErrorCode::Conflict, "이미지 profile이 바뀌었다. 새 작업 ID로 요청한다"));
        }
        self.image_account_allowed(&job.project, job.begin.account.as_deref())?;
        let auto = self.pool_harness_enabled("codex")?;
        let selection = if auto {
            let profile = ResolvedProfile {
                harness: "codex".into(),
                model: job.model.clone(),
                effort: job.effort.clone(),
                args: vec![],
                command: vec![],
                env: Default::default(),
            };
            let mut input = self
                .account_selection_input(&job.project, &profile, None, AccountTaskSize::Small)
                .await?
                .ok_or_else(|| bad("계정 풀 입력이 없다"))?;
            if let Some(account) = &job.begin.account {
                input.only(account);
            }
            Some(input)
        } else {
            None
        };
        let default = if auto { None } else { self.default_account("codex")? };
        let default_group =
            self.account_pool()?.members.into_iter().find(|m| Some(&m.account) == default.as_ref()).map(|m| m.group);
        self.db
            .image_transaction(|tx| {
                let (account, allocation) = if let Some(input) = &selection {
                    input.select_with_percent(tx, util::now_ms(), RESERVE)?
                } else {
                    let account = default.clone().ok_or_else(|| anyhow::anyhow!("Codex 기본 계정이 없다"))?;
                    let generation = self.accounts.generation(&account).map_err(|source_error| {
                        aaa_common::diagnostic::context(source_error, anyhow::anyhow!("계정 인증을 읽지 못했다"))
                    })?;
                    let group = default_group.clone().unwrap_or_else(|| account.clone());
                    (
                        account,
                        Allocation {
                            generation,
                            ended_at_ms: None,
                            released: false,
                            reserve_percent: Some(RESERVE),
                            selection: AccountSelection {
                                size: AccountTaskSize::Small,
                                group,
                                unobserved: false,
                                reason: "host Codex 기본 계정 · 이미지 small 5%p 예약".into(),
                                selected_at_ms: util::now_ms(),
                            },
                        },
                    )
                };
                let credential = self.accounts.credential(&account).map_err(|source_error| {
                    aaa_common::diagnostic::context(source_error, anyhow::anyhow!("계정 인증을 읽지 못했다"))
                })?;
                anyhow::ensure!(
                    credential.harness == "codex"
                        && credential.auth_generation.as_deref() == Some(&allocation.generation),
                    "계정 인증 세대·harness가 바뀌었다"
                );
                job.view.account = Some(account);
                job.allocation = Some(allocation);
                tx.execute("UPDATE meta SET value=?2 WHERE key=?1", [key(&job.begin.id), serde_json::to_string(job)?])?;
                Ok(())
            })
            .map_err(|e| {
                if e.to_string().starts_with("start_wait:") {
                    err(ErrorCode::Unavailable, e.to_string())
                } else {
                    io_error(e)
                }
            })
    }

    pub(crate) fn image_recover(&self) -> anyhow::Result<()> {
        for id in self.db.meta_suffixes(PREFIX)? {
            let mut job = match self.image_load(&id) {
                Ok(job) => job,
                Err(source_error) => {
                    aaa_common::diagnostic::observe(&source_error, "오류 뒤 기존 복구·거부 분기를 따른다");
                    tracing::warn!("이미지 원장 한 건을 읽지 못했다. 해당 ID 재생성은 거부한다");
                    continue;
                }
            };
            if matches!(job.view.state, ImageState::Queued | ImageState::Running) {
                job.view.state = if job.launched { ImageState::Unknown } else { ImageState::Waiting };
                job.view.reason = "host 재시작 · 생성 자동 반복 없음".into();
                if !job.launched
                    && let Some(a) = &mut job.allocation
                {
                    a.released = true;
                }
                self.image_save(&job).map_err(|e| anyhow::anyhow!(e.message))?;
            }
            // 이전 helper를 추측해 종료하지 않는다. 이 원장 소유의 잔여 인증 파일만 회수한다.
            let home = self.image_dir(&id).map_err(|e| anyhow::anyhow!(e.message))?.join("rpc-home");
            if home
                .symlink_metadata()
                .inspect_err(|error| aaa_common::diagnostic::observe(error, "성공 여부만 쓰는 분기의 하위 오류"))
                .is_ok_and(|m| m.is_dir())
            {
                match std::fs::remove_file(home.join("auth.json")) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(source_error) => {
                        aaa_common::diagnostic::observe(&source_error, "오류 뒤 기존 복구·거부 분기를 따른다");
                        tracing::warn!("이미지 helper의 잔여 인증 파일을 정리하지 못했다");
                    }
                }
            }
        }
        Ok(())
    }
    pub(crate) fn image_reconcile(&self, report: &AccountQuotaReport) -> Result<(), ErrorBody> {
        for id in self.db.meta_suffixes(PREFIX).map_err(io_error)? {
            let j = self.image_load(&id)?;
            if let Some(a) = &j.allocation
                && !a.released
                && let Some(ended) = a.ended_at_ms
                && report.accounts.iter().find(|q| Some(&q.account) == j.view.account.as_ref()).is_some_and(|q| {
                    crate::account_pool::fresh_windows(q, Some(&j.model), util::now_ms())
                        .inspect_err(|error| {
                            aaa_common::diagnostic::observe(error, "성공 여부만 쓰는 분기의 하위 오류")
                        })
                        .is_ok_and(|ws| ws.iter().all(|w| w.window.observed_at_ms > ended))
                })
            {
                self.db
                    .update_meta(&key(&id), |text| {
                        let mut current: Job =
                            serde_json::from_str(&text.ok_or_else(|| anyhow::anyhow!("이미지 원장이 없다"))?)?;
                        if let Some(current) = &mut current.allocation
                            && current.generation == a.generation
                            && current.ended_at_ms == a.ended_at_ms
                        {
                            current.released = true;
                        }
                        Ok(serde_json::to_string(&current)?)
                    })
                    .map_err(io_error)?;
                self.accounts_bump();
            }
        }
        Ok(())
    }
}
