//! 머신 등록 창과 확인 코드 승인.

use super::*;

impl Host {
    // ───────────── enrollment ─────────────

    pub fn enroll_open(&self) -> bool {
        self.enroll.lock().unwrap().open_until.is_some_and(|t| t > Instant::now())
    }

    /// 등록 요청을 대기열에 넣는다. 승인되면 receiver가 끝난다.
    pub(crate) fn enroll_request_bound(
        &self,
        name: &str,
        machine_id: &str,
        fingerprint: &str,
        sas: &str,
        installation: Option<aaa_proto::install::InstallEnrollmentBinding>,
    ) -> Result<oneshot::Receiver<()>, ErrorBody> {
        if installation.as_ref().is_some_and(|b| {
            b.installation_id.len() != 26
                || !b.installation_id.bytes().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
                || b.plan_sha256.len() != 64
                || !b.plan_sha256.bytes().all(|c| c.is_ascii_hexdigit())
        }) {
            return Err(err(ErrorCode::BadRequest, "설치 등록 ID·계획 hash를 확인하세요"));
        }
        if !self.enroll_open() {
            return Err(err(
                ErrorCode::EnrollClosed,
                "host에서 `aaa machine enroll --open 10m`으로 등록 창을 먼저 연다",
            ));
        }
        if !util::machine_name_ok(name) {
            return Err(err(ErrorCode::BadRequest, MACHINE_NAME_RULE));
        }
        if let Ok(Some(m)) = self.db.machine_by_name(name)
            && m.id != machine_id
        {
            return Err(err(ErrorCode::NameTaken, format!("이름 {name}은 다른 머신이 쓴다")));
        }
        let mut e = self.enroll.lock().unwrap();
        if e.pending.len() >= 8 && !e.pending.contains_key(name) {
            return Err(err(ErrorCode::Unavailable, "대기 중인 등록이 너무 많다"));
        }
        let (tx, rx) = oneshot::channel();
        e.pending.insert(
            name.to_string(),
            PendingRt {
                installation,
                machine_id: machine_id.to_string(),
                fingerprint: fingerprint.to_string(),
                sas: sas.to_string(),
                since_ms: util::now_ms(),
                attempts: 0,
                accept: Some(tx),
            },
        );
        drop(e);
        self.enroll_bump();
        Ok(rx)
    }

    pub fn enroll_cancel(&self, name: &str, fingerprint: &str) {
        let mut e = self.enroll.lock().unwrap();
        if e.pending.get(name).is_some_and(|p| p.fingerprint == fingerprint) {
            e.pending.remove(name);
            drop(e);
            self.enroll_bump();
        }
    }

    /// DB가 확정한 등록만 runtime 목록에 반영한다. 재전달이 살아 있는 연결을 지우지 않는다.
    pub(super) fn machine_registered(&self, id: &str, name: &str) {
        let mut inner = self.inner.lock().unwrap();
        let machine = inner.machines.entry(id.into()).or_insert_with(|| MachineRt {
            name: name.into(),
            local: false,
            os: None,
            boot_id: None,
            dev_mode: false,
            conn: None,
            last_seen_ms: None,
            health: None,
            health_sent: None,
            features: vec![],
            identity: None,
            agent_pid: None,
            colors: None,
            build: Default::default(),
        });
        machine.name = name.into();
    }

    pub(super) fn approve(&self, name: &str, code: &str, actor: &str) -> Result<FrontResponse, ErrorBody> {
        let mut e = self.enroll.lock().unwrap();
        let Some(p) = e.pending.get_mut(name) else {
            return Err(err(ErrorCode::NotFound, format!("대기 중인 등록이 없다: {name}")));
        };
        let code: String = code.chars().filter(|c| c.is_ascii_digit()).collect();
        if code != p.sas {
            p.attempts += 1;
            if p.attempts >= 3 {
                e.pending.remove(name); // sender drop → agent 쪽은 거부로 끝난다
                self.db.audit(actor, "enroll_reject", name);
                self.enroll_bump();
                return Err(err(ErrorCode::EnrollCodeMismatch, "확인 코드가 3번 틀려 이 등록 요청을 버렸다"));
            }
            return Err(err(ErrorCode::EnrollCodeMismatch, "확인 코드가 다르다(agent 화면의 코드를 입력한다)"));
        }
        let p = e.pending.remove(name).unwrap();
        if let Some(binding) = &p.installation {
            self.db
                .install_enroll(
                    &binding.installation_id,
                    &binding.plan_sha256,
                    &util::new_id(),
                    &p.fingerprint,
                    &p.machine_id,
                    name,
                )
                .map_err(|source_error| {
                    aaa_common::diagnostic::context(
                        source_error,
                        err(ErrorCode::Forbidden, "설치 등록의 기존 신원·폐기 상태가 다릅니다"),
                    )
                })?;
        } else {
            self.db
                .upsert_machine(&p.machine_id, name, Some(&p.fingerprint), false)
                .map_err(|e| err(ErrorCode::Internal, e.to_string()))?;
        }
        self.machine_registered(&p.machine_id, name);
        if let Some(tx) = p.accept {
            aaa_common::diagnostic::ignored(tx.send(()), "수신자가 끝난 뒤에도 현재 작업은 마친다");
        }
        self.db.audit(actor, "enroll_approve", name);
        drop(e);
        self.enroll_bump();
        self.bump();
        Ok(FrontResponse::Done)
    }
}
