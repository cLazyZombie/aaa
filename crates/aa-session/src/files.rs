//! agent 상태 폴더 안의 세션 파일 배치. 세션 하나 = 폴더 하나.
//!
//! `<agent state>/sessions/<id>/spec.json`  agent가 쓴다(실행 spec·token·boot ID)
//! `<agent state>/sessions/<id>/meta.json`  session host가 시작하며 쓴다(pid)
//! `<agent state>/sessions/<id>/exit.json`  session host가 종료하며 쓴다(exit code·마지막 offset)
//! `<agent state>/sessions/<id>/final.replay`·`final.json`  종료 때 마지막 화면
//! `<agent state>/run/<id>.sock`            session IPC(짧은 경로: UDS 길이 제한)

use std::path::{Path, PathBuf};

use aa_proto::SpawnSpec;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct SessionPaths {
    pub dir: PathBuf,
    pub sock: PathBuf,
}

impl SessionPaths {
    pub fn new(agent_state: &Path, id: &str) -> Self {
        SessionPaths {
            dir: agent_state.join("sessions").join(id),
            sock: agent_state.join("run").join(format!("{id}.sock")),
        }
    }
    pub fn from_dir(dir: &Path) -> Self {
        let id = dir.file_name().unwrap().to_string_lossy().to_string();
        let agent_state = dir.parent().and_then(|p| p.parent()).unwrap_or(Path::new("."));
        Self::new(agent_state, &id)
    }
    pub fn id(&self) -> String {
        self.dir.file_name().unwrap().to_string_lossy().to_string()
    }
    pub fn spec(&self) -> PathBuf {
        self.dir.join("spec.json")
    }
    pub fn meta(&self) -> PathBuf {
        self.dir.join("meta.json")
    }
    pub fn exit(&self) -> PathBuf {
        self.dir.join("exit.json")
    }
    pub fn final_replay(&self) -> PathBuf {
        self.dir.join("final.replay")
    }
    pub fn final_screen(&self) -> PathBuf {
        self.dir.join("final.json")
    }
    pub fn log(&self) -> PathBuf {
        self.dir.join("session-host.log")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecFile {
    pub id: String,
    pub token: String,
    pub boot_id: String,
    pub created_at_ms: u64,
    pub spec: SpawnSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaFile {
    pub host_pid: u32,
    pub child_pid: Option<u32>,
    pub started_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExitFile {
    pub code: Option<i32>,
    pub ended_at_ms: u64,
    pub end_offset: u64,
    pub cols: u16,
    pub rows: u16,
}
