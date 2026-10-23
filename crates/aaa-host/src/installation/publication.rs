//! 고정 worker의 실제 kernel peer / 실행 SHA / 서명 / 소비 intent 위에만 한 포트 공개를 연결한다.
use super::*;
use crate::tailscale;
use serde::{Deserialize, Serialize};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Issued {
    plan: InstallPlan,
    authorization: InstallAuthorization,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Nonce {
    authorization_sha256: String,
    installation_id: String,
    consumed: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Execution {
    schema: u32,
    id: String,
    worker_pid: u32,
    account_sha256: String,
    instance: Option<String>,
    plan_sha256: String,
    scope_sha256: String,
    files: Option<String>,
    stage: String,
    last_exit: Option<u8>,
    pending: Option<serde_json::Value>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema: u32,
    installation_id: String,
    plan_sha256: String,
    permission: InstallPermission,
    owned: bool,
    mapping_sha256: Option<String>,
}
fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, ErrorBody> {
    serde_json::from_slice(
        &private_file::read(path, 1024 * 1024)
            .map_err(|source_error| aaa_common::diagnostic::context(source_error, denied()))?,
    )
    .map_err(|source_error| aaa_common::diagnostic::context(source_error, denied()))
}
fn exact_error(error: tailscale::ExactError) -> ErrorBody {
    err(if error.conflict { ErrorCode::Conflict } else { ErrorCode::Unavailable }, error.code)
}
fn ports(plan: &InstallPlan) -> Result<(u16, u16), ErrorBody> {
    if plan.choices.role != Some(InstallRole::Host) || plan.choices.tailscale_serve != Some(InstallServe::On) {
        return Err(denied());
    }
    let web = plan
        .choices
        .web_listen
        .as_deref()
        .unwrap_or("127.0.0.1:7431")
        .parse::<std::net::SocketAddr>()
        .map_err(|source_error| aaa_common::diagnostic::context(source_error, denied()))?;
    if !web.ip().is_loopback() || web.port() == 0 {
        return Err(denied());
    }
    Ok((plan.choices.serve_port.filter(|p| *p != 0).ok_or_else(denied)?, web.port()))
}
impl Host {
    fn install_execution(
        &self,
        id: &str,
        peer: Option<u32>,
        writing: bool,
    ) -> Result<(PathBuf, InstallPlan, InstallPermission), ErrorBody> {
        if !id_ok(id) || writing && peer.is_none() {
            return Err(denied());
        }
        let root = self
            .state
            .parent()
            .ok_or_else(denied)?
            .canonicalize()
            .map_err(|source_error| aaa_common::diagnostic::context(source_error, denied()))?;
        let authority = root.join("install/authorization");
        let issued: Issued = read(&authority.join(format!("{id}.input.json")))?;
        let plan = issued.plan;
        let auth = issued.authorization;
        let c = &auth.claims;
        let now = if writing { util::now_ms() } else { c.issued_at_ms };
        let checked = aaa_config::install::plan(plan.choices.clone(), plan.context.clone())
            .map_err(|source_error| aaa_common::diagnostic::context(source_error, denied()))?;
        let pin = private_file::read(&authority.join("issuer.pin"), 32)
            .map_err(|source_error| aaa_common::diagnostic::context(source_error, denied()))?;
        if checked != plan
            || Path::new(&plan.context.state_dir) != root
            || c.installation_id != id
            || c.plan_sha256 != aaa_config::install::plan_sha256(&plan)
            || c.platform != plan.context.platform
            || c.instance != plan.choices.instance
            || c.target_account != plan.context.target_account
            || c.target_account != private_file::account().ok_or_else(denied)?
            || c.binary_sha256 != plan.context.binary_sha256
            || c.binary_sha256 != self.running_sha256
            || c.issuer != format!("local-ed25519:{}", digest(&pin))
            || c.issued_at_ms > now
            || c.expires_at_ms <= now
            || c.expires_at_ms.checked_sub(c.issued_at_ms).is_none_or(|span| span == 0 || span > 600_000)
            || !id_ok(&c.nonce)
            || !install_crypto::verify_authorization(&pin, c, &auth.signature)
        {
            return Err(denied());
        }
        let nonce: Nonce = read(&authority.join(format!("{}.json", c.nonce)))?;
        if nonce.installation_id != id
            || !nonce.consumed
            || nonce.authorization_sha256
                != digest(
                    &install_crypto::canonical(&auth)
                        .map_err(|source_error| aaa_common::diagnostic::context(source_error, denied()))?,
                )
        {
            return Err(denied());
        }
        let execution: Execution = read(&root.join("install/executions").join(format!("{id}.json")))?;
        if execution.schema != 1
            || execution.id != id
            || execution.worker_pid == 0
            || execution.plan_sha256 != c.plan_sha256
            || execution.scope_sha256
                != digest(
                    &serde_json::to_vec(&c.permissions)
                        .map_err(|source_error| aaa_common::diagnostic::context(source_error, denied()))?,
                )
            || execution.account_sha256 != digest(c.target_account.as_bytes())
            || execution.instance != c.instance
            || execution.stage != "files_applied"
            || execution.files.as_deref().is_none_or(|id| !id_ok(id))
            || execution.last_exit.is_some_and(|exit| !matches!(exit, 0..=6 | 130))
            || execution.pending.as_ref().is_some_and(|value| !value.is_string())
        {
            return Err(denied());
        }
        if writing {
            let pid = peer.ok_or_else(denied)?;
            if execution.worker_pid != pid || execution.last_exit.is_some() {
                return Err(denied());
            }
            let image = aaa_common::process::image(pid).ok_or_else(denied)?;
            let bytes =
                std::fs::read(image).map_err(|source_error| aaa_common::diagnostic::context(source_error, denied()))?;
            if digest(&bytes) != c.binary_sha256 {
                return Err(denied());
            }
        }
        let (port, web) = ports(&plan)?;
        let expected = plan
            .choices
            .web_listen
            .as_deref()
            .unwrap_or("127.0.0.1:7431")
            .parse::<std::net::SocketAddr>()
            .map_err(|source_error| aaa_common::diagnostic::context(source_error, denied()))?;
        let bound = self.web_listen.lock().unwrap().as_deref().and_then(|s| {
            s.parse::<std::net::SocketAddr>()
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()
        });
        if self.web_port() != web || bound != Some(expected) {
            return Err(denied());
        }
        let target = format!("https:{port}/loopback:{web}");
        let mut permissions =
            c.permissions.iter().filter(|p| p.operation == InstallOperation::PublishWeb && p.target == target).cloned();
        let permission = permissions.next().ok_or_else(denied)?;
        if permissions.next().is_some()
            || permission.before_sha256.as_deref().is_none_or(|v| !digest_ok(v))
            || permission.after_sha256.as_deref().is_none_or(|v| !digest_ok(v))
        {
            return Err(denied());
        }
        Ok((authority.join(format!("{id}.serve.json")), plan, permission))
    }
    pub(crate) async fn install_publish(&self, peer: Option<u32>, id: &str) -> Result<FrontResponse, ErrorBody> {
        let _gate = self.publish_gate.lock().await;
        self.no_pending_publication()?;
        let (path, plan, permission) = self.install_execution(id, peer, true)?;
        let (port, web) = ports(&plan)?;
        let native = tailscale::conditional_snapshot(web, port).await.map_err(exact_error)?;
        let actual = native.snapshot.permission().map_err(exact_error)?;
        if actual.operation != permission.operation
            || actual.target != permission.target
            || actual.after_sha256 != permission.after_sha256
        {
            return Err(err(ErrorCode::Conflict, "serve_port_conflict"));
        }
        let previous: Option<Receipt> = match path.symlink_metadata() {
            Ok(_) => Some(read(&path)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(source_error) => {
                aaa_common::diagnostic::observe(&source_error, "오류 뒤 기존 복구·거부 분기를 따른다");
                return Err(denied());
            }
        };
        if let Some(record) = &previous {
            if record.schema != 1
                || record.installation_id != id
                || record.plan_sha256 != aaa_config::install::plan_sha256(&plan)
                || record.permission != permission
            {
                return Err(denied());
            }
            if record.mapping_sha256.as_ref().is_some_and(|hash| {
                native
                    .snapshot
                    .mapping_sha256()
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
                    .as_ref()
                    != Some(hash)
            }) {
                return Err(err(ErrorCode::Conflict, "serve_port_conflict"));
            }
        }
        if actual.before_sha256 != permission.before_sha256 && !(native.snapshot.reused && previous.is_some()) {
            return Err(err(ErrorCode::Conflict, "serve_port_conflict"));
        }
        let reused = native.snapshot.reused;
        let mut record = previous.unwrap_or(Receipt {
            schema: 1,
            installation_id: id.into(),
            plan_sha256: aaa_config::install::plan_sha256(&plan),
            permission,
            owned: false,
            mapping_sha256: None,
        });
        private_file::write(
            &path,
            &serde_json::to_vec(&record)
                .map_err(|source_error| aaa_common::diagnostic::context(source_error, unavailable()))?,
        )
        .map_err(|source_error| aaa_common::diagnostic::context(source_error, unavailable()))?;
        // 실제 compare-and-swap 직전까지 서명/현재 nonce/worker 소유를 다시 검사한다.
        self.install_execution(id, peer, true)?;
        let after = native.publish().await.map_err(exact_error)?;
        record.owned |= !reused;
        record.mapping_sha256 = Some(after.mapping_sha256().map_err(exact_error)?);
        private_file::write(
            &path,
            &serde_json::to_vec(&record)
                .map_err(|source_error| aaa_common::diagnostic::context(source_error, unavailable()))?,
        )
        .map_err(|source_error| aaa_common::diagnostic::context(source_error, unavailable()))?;
        self.db
            .set_meta("publish_origin", &after.url().to_ascii_lowercase())
            .map_err(|source_error| aaa_common::diagnostic::context(source_error, unavailable()))?;
        self.db
            .set_meta("publish_selected_port", &port.to_string())
            .map_err(|source_error| aaa_common::diagnostic::context(source_error, unavailable()))?;
        if record.owned {
            self.db
                .set_meta("publish_port", &port.to_string())
                .map_err(|source_error| aaa_common::diagnostic::context(source_error, unavailable()))?;
        }
        let public_https =
            if self.install_https(&after.url()).await { InstallCheckState::Passed } else { InstallCheckState::Unknown };
        Ok(FrontResponse::InstallPublished {
            publication: InstallPublication {
                serve_port: port,
                target_port: web,
                reused,
                owned: record.owned,
                mapping: InstallCheckState::Passed,
                public_https,
            },
        })
    }
    pub(crate) async fn install_publish_status(&self, id: &str) -> Result<FrontResponse, ErrorBody> {
        let (path, plan, permission) = self.install_execution(id, None, false)?;
        let (port, web) = ports(&plan)?;
        let record: Receipt = read(&path)?;
        let current = tailscale::conditional_snapshot(web, port).await.map_err(exact_error)?.snapshot;
        if record.schema != 1
            || record.installation_id != id
            || record.plan_sha256 != aaa_config::install::plan_sha256(&plan)
            || record.permission != permission
        {
            return Err(err(ErrorCode::Conflict, "serve_port_conflict"));
        }
        if record.mapping_sha256.is_none() {
            return Err(unavailable());
        }
        if !current.reused
            || current.permission().map_err(exact_error)?.after_sha256 != record.permission.after_sha256
            || current
                .mapping_sha256()
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()
                != record.mapping_sha256
        {
            return Err(err(ErrorCode::Conflict, "serve_port_conflict"));
        }
        let public_https = if self.install_https(&current.url()).await {
            InstallCheckState::Passed
        } else {
            InstallCheckState::Unknown
        };
        Ok(FrontResponse::InstallPublished {
            publication: InstallPublication {
                serve_port: port,
                target_port: web,
                reused: true,
                owned: record.owned,
                mapping: InstallCheckState::Passed,
                public_https,
            },
        })
    }
    async fn install_https(&self, url: &str) -> bool {
        let challenge = hex::encode(rand::random::<[u8; 32]>());
        let proof = hex::encode(rand::random::<[u8; 32]>());
        {
            let mut probes = self.install_probes.lock().unwrap();
            probes.retain(|_, (_, expires)| *expires > std::time::Instant::now());
            if probes.len() >= 16 {
                return false;
            }
            probes.insert(
                challenge.clone(),
                (proof.clone(), std::time::Instant::now() + std::time::Duration::from_secs(10)),
            );
        }
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            crate::http::request("GET", &format!("{url}/api/v1/install-probe/{challenge}"), &[], &[], 1024),
        )
        .await;
        self.install_probes.lock().unwrap().remove(&challenge);
        matches!(response, Ok(Ok(response)) if response.status == 200 && response.body == proof.as_bytes())
    }
    /// 공개 nonce는 단 한 번 현재 host의 무작위 proof만 돌려준다. 기기·계정·host ID는 없다.
    pub(crate) fn install_probe(&self, challenge: &str) -> Option<String> {
        if challenge.len() != 64 || !challenge.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let (proof, expires) = self.install_probes.lock().unwrap().remove(challenge)?;
        (expires > std::time::Instant::now()).then_some(proof)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn host(path: &Path) -> std::sync::Arc<Host> {
        let mut cfg = aaa_config::HostConfig::default();
        cfg.notify.web_push = false;
        cfg.web.listen = "127.0.0.1:0".into();
        Host::open(cfg, path.into()).unwrap()
    }

    /// 소비자 경계의 private 입력 fixture다. 파일 엔진 실행/영수증 생산은 별도 CLI owner가 검증한다.
    #[tokio::test]
    async fn scoped_publication_requires_the_running_hosts_owned_listener_and_a_bound_private_job() {
        let directory =
            std::env::temp_dir().canonicalize().unwrap().join(format!("aaa-install-publish-proof-{}", util::new_id()));
        let root = directory.join("state");
        private_file::directory(&root).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let mut cfg = aaa_config::HostConfig::default();
        cfg.notify.web_push = false;
        cfg.web.listen = address.to_string();
        let host = Host::open(cfg, root.join("host")).unwrap();
        let text = |path: &Path| path.to_string_lossy().into_owned();
        let context = InstallPlanContext {
            platform: InstallPlatform {
                os: if cfg!(windows) {
                    InstallOs::Windows
                } else if cfg!(target_os = "macos") {
                    InstallOs::Macos
                } else {
                    InstallOs::Linux
                },
                arch: if cfg!(target_arch = "aarch64") { InstallArch::Aarch64 } else { InstallArch::X86_64 },
            },
            version: "0.1.2".into(),
            binary_sha256: host.running_sha256.clone(),
            bin_path: text(&directory.join(if cfg!(windows) { "bin/aaa.exe" } else { "bin/aaa" })),
            config_dir: text(&directory.join("config")),
            state_dir: text(&root),
            projects_root: text(&directory.join("projects")),
            default_shell: if cfg!(windows) { "cmd.exe".into() } else { "/bin/sh".into() },
            effective_config_sha256: digest(b"fixture configuration"),
            target_account: private_file::account().unwrap(),
        };
        let choices = InstallChoices {
            role: Some(InstallRole::Host),
            name: Some("host-a".into()),
            local_agent: Some(InstallToggle::Disabled),
            project_mode: Some(InstallProjectMode::Skip),
            config_dir: Some(context.config_dir.clone()),
            state_dir: Some(context.state_dir.clone()),
            agent_listen: Some(if address.port() == 17430 {
                "127.0.0.1:17431".into()
            } else {
                "127.0.0.1:17430".into()
            }),
            web: Some(InstallWeb::Local),
            web_listen: Some(address.to_string()),
            tailscale_serve: Some(InstallServe::On),
            serve_port: Some(8443),
            pairing: Some(InstallPairing::Skip),
            service: Some(InstallService::None),
            start: Some(InstallStart::Deferred),
            ..Default::default()
        };
        let plan = aaa_config::install::plan(choices, context).unwrap();
        let external = tailscale::TsOutput {
            status: Some(r#"{"BackendState":"Running","Self":{"DNSName":"host-a.example.ts.net."}}"#.into()),
            serve: Some("{}".into()),
        };
        let permissions = vec![tailscale::exact(Some(&external), address.port(), 8443).unwrap().permission().unwrap()];
        let request = InstallApprovalRequest {
            installation_id: None,
            expected_plan_sha256: aaa_config::install::plan_sha256(&plan),
            expected_scope_sha256: digest(&install_crypto::canonical(&permissions).unwrap()),
            plan,
            permissions,
            agent_identity: None,
        };
        let FrontResponse::InstallApproval { approval } =
            host.install_approve(DeviceRole::Controller, "device:person", request).unwrap()
        else {
            panic!("approval view");
        };
        let package = export(&host.state, &approval.installation_id).unwrap();
        let id = &approval.installation_id;
        let authority = root.join("install/authorization");
        private_file::directory(&root.join("install")).unwrap();
        private_file::directory(&authority).unwrap();
        let pin = private_file::read(&host.state.join("install/host-issuer/issuer.pin"), 32).unwrap();
        private_file::write(&authority.join("issuer.pin"), &pin).unwrap();
        let input = serde_json::json!({"plan":package.plan,"authorization":package.authorization});
        private_file::write(&authority.join(format!("{id}.input.json")), &serde_json::to_vec(&input).unwrap()).unwrap();
        let authorization: InstallAuthorization = serde_json::from_value(input["authorization"].clone()).unwrap();
        let nonce = serde_json::json!({"installation_id":id,"consumed":true,"authorization_sha256":digest(&install_crypto::canonical(&authorization).unwrap())});
        private_file::write(
            &authority.join(format!("{}.json", authorization.claims.nonce)),
            &serde_json::to_vec(&nonce).unwrap(),
        )
        .unwrap();
        private_file::directory(&root.join("install/executions")).unwrap();
        let execution = serde_json::json!({"schema":1,"id":id,"worker_pid":std::process::id(),"account_sha256":digest(authorization.claims.target_account.as_bytes()),"instance":authorization.claims.instance,"plan_sha256":authorization.claims.plan_sha256,"scope_sha256":digest(&serde_json::to_vec(&authorization.claims.permissions).unwrap()),"files":id,"stage":"files_applied","last_exit":null,"pending":null});
        private_file::write(
            &root.join("install/executions").join(format!("{id}.json")),
            &serde_json::to_vec(&execution).unwrap(),
        )
        .unwrap();
        assert!(
            host.install_execution(id, Some(std::process::id()), true).is_err(),
            "cfg의 포트가 같아도 이 host가 listen하지 않으면 다른 앱을 공개할 수 있다"
        );
        crate::web::start_bound(&host, listener).unwrap();
        assert!(host.install_execution(id, Some(std::process::id()), true).is_ok());
        let execution_path = root.join("install/executions").join(format!("{id}.json"));
        for (field, value) in [
            ("worker_pid", serde_json::json!(std::process::id() + 1)),
            ("scope_sha256", serde_json::json!(digest(b"different scope"))),
            ("account_sha256", serde_json::json!(digest(b"different account"))),
            ("stage", serde_json::json!("intent")),
            ("last_exit", serde_json::json!(0)),
        ] {
            let mut changed = execution.clone();
            changed[field] = value;
            private_file::write(&execution_path, &serde_json::to_vec(&changed).unwrap()).unwrap();
            assert!(
                host.install_execution(id, Some(std::process::id()), true).is_err(),
                "다른 worker/scope/account/phase 또는 끝난 실행은 쓰기 peer가 아니다"
            );
        }
        private_file::write(&execution_path, &serde_json::to_vec(&execution).unwrap()).unwrap();
        let nonce_path = authority.join(format!("{}.json", authorization.claims.nonce));
        let mut unconsumed = nonce.clone();
        unconsumed["consumed"] = serde_json::json!(false);
        private_file::write(&nonce_path, &serde_json::to_vec(&unconsumed).unwrap()).unwrap();
        assert!(host.install_execution(id, Some(std::process::id()), true).is_err());
        private_file::write(&nonce_path, &serde_json::to_vec(&nonce).unwrap()).unwrap();
        let mut expired = authorization.clone();
        let now = util::now_ms();
        expired.claims.issued_at_ms = now - 300_000;
        expired.claims.expires_at_ms = now - 1;
        let signer = install_crypto::Signer::open(
            &private_file::read(&host.state.join("install/host-issuer/issuer.pk8"), 4096).unwrap(),
        )
        .unwrap();
        expired.signature = signer.sign_authorization(&expired.claims).unwrap();
        let expired_input = serde_json::json!({"plan":input["plan"],"authorization":expired});
        private_file::write(&authority.join(format!("{id}.input.json")), &serde_json::to_vec(&expired_input).unwrap())
            .unwrap();
        let mut expired_nonce = nonce.clone();
        expired_nonce["authorization_sha256"] =
            serde_json::json!(digest(&install_crypto::canonical(&expired).unwrap()));
        private_file::write(&nonce_path, &serde_json::to_vec(&expired_nonce).unwrap()).unwrap();
        assert!(
            host.install_execution(id, Some(std::process::id()), true).is_err(),
            "정상 서명도 만료 뒤 쓰기를 허용하지 않는다"
        );
        assert!(host.install_execution(id, None, false).is_ok(), "만료한 같은 ID의 readonly 상태는 계속 조회한다");
        private_file::write(&authority.join(format!("{id}.input.json")), &serde_json::to_vec(&input).unwrap()).unwrap();
        private_file::write(&nonce_path, &serde_json::to_vec(&nonce).unwrap()).unwrap();
        assert!(host.install_execution(id, Some(std::process::id()), true).is_ok());
    }
    #[tokio::test]
    async fn public_probe_confirms_this_host_and_cannot_be_a_pairing_or_authentication_token() {
        let root = std::env::temp_dir().join(format!("aaa-install-probe-{}", util::new_id()));
        let first = host(&root.join("first"));
        let other = host(&root.join("other"));
        crate::web::start(&first).await.unwrap();
        crate::web::start(&other).await.unwrap();
        let own = format!("http://127.0.0.1:{}", first.web_port());
        let wrong = format!("http://127.0.0.1:{}", other.web_port());
        assert!(first.install_https(&own).await);
        assert!(!first.install_https(&wrong).await, "같은 판/HTML의 다른 host 응답은 현재 instance 확인이 아니다");
        // 200만으로 성공이라고 하면 다른 프로세스의 HTML/응답을 현재 instance로 오인한다.
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let foreign = tokio::spawn(async move {
            let (mut peer, _) = listener.accept().await.unwrap();
            let mut head = Vec::new();
            while !head.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                peer.read_exact(&mut byte).await.unwrap();
                head.extend(byte);
                assert!(head.len() < 8192);
            }
            peer.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\n\r\nforeign").await.unwrap();
        });
        assert!(!first.install_https(&format!("http://{address}")).await);
        foreign.await.unwrap();
        assert!(first.db.devices().unwrap().is_empty());
        assert!(other.db.devices().unwrap().is_empty());
        assert!(first.install_probe(&"a".repeat(64)).is_none());
        // 일반 W0 pairing 인증 경계에는 어떤 기기도 추가하지 않는다.
    }
    #[tokio::test]
    async fn scoped_publish_rejects_fronts_without_an_os_bound_worker_before_native_io() {
        let root = std::env::temp_dir().join(format!("aaa-install-publish-deny-{}", util::new_id()));
        let host = host(&root.join("state/host"));
        let id = util::new_id();
        for (role, actor) in [
            (DeviceRole::Reader, "worker:untrusted"),
            (DeviceRole::Controller, "device:person"),
            (DeviceRole::Local, "local:cli"),
        ] {
            assert_eq!(
                host.call(role, actor, aaa_proto::FrontRequest::InstallPublish { installation_id: id.clone() })
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::Forbidden
            );
        }
        let mut front = host.new_front("cli".into());
        front.peer_pid = Some(std::process::id());
        assert_eq!(
            host.call_from(
                DeviceRole::Local,
                "local:cli",
                Some(&front),
                aaa_proto::FrontRequest::InstallPublish { installation_id: id }
            )
            .await
            .unwrap_err()
            .code,
            ErrorCode::Forbidden
        );
        assert!(!root.join("state/install").exists(), "거부된 요청은 issuer/nonce/공개 intent를 만들지 않는다");
        assert!(host.db.meta("publish_port").unwrap().is_none());
    }
}
