//! profile 고르기와 실행 인자 풀기.

use super::*;

impl Host {
    pub(crate) fn profile_config(&self) -> Result<aaa_config::HostConfig, ErrorBody> {
        match self.config_dir.get() {
            Some(dir) => aaa_config::load_host(dir).map(|l| l.config).map_err(|e| bad(e.render())),
            None => Ok(self.cfg.clone()),
        }
    }

    pub(crate) async fn profiles(&self, machine: Option<&str>) -> Result<FrontResponse, ErrorBody> {
        let cfg = self.profile_config()?;
        let mid = match self.resolve_machine(machine) {
            Ok(mid) => Some(mid),
            Err(_) if machine.is_none() => None,
            Err(e) => return Err(e),
        };
        // agent에 묻는 것은 설치·로그인한 기본 harness뿐이다. 옛 agent의 사용자 정의는 쓰지 않는다.
        let detected = if let Some(mid) = &mid
            && self.machine_feature(mid, "host_profiles.v1")
            && let Ok(conn) = self.live_conn(mid)
        {
            match conn.request(AgentRequest::Harnesses).await? {
                AgentResponse::Profiles { profiles } => profiles,
                other => return Err(internal(format!("예상하지 못한 응답: {other:?}"))),
            }
        } else {
            Vec::new()
        };
        let mut profiles: Vec<aaa_proto::ProfileView> = cfg
            .profiles
            .iter()
            .map(|(name, p)| aaa_proto::ProfileView {
                name: name.clone(),
                harness: p.harness.clone(),
                model: (!p.model.is_empty()).then(|| p.model.clone()),
                effort: (!p.effort.is_empty()).then(|| p.effort.clone()),
                args: p.args.clone(),
                description: p.description.clone(),
                builtin: false,
            })
            .collect();
        profiles.extend(detected.into_iter().filter(|p| p.builtin && !cfg.profiles.contains_key(&p.name)));
        Ok(FrontResponse::Profiles {
            machine: mid.map(|mid| self.machine_name(&mid)).unwrap_or_else(|| "host".into()),
            profiles,
        })
    }

    pub(crate) fn profile_resolve(&self, name: Option<&str>) -> Result<FrontResponse, ErrorBody> {
        let cfg = self.profile_config()?;
        let name = name.unwrap_or_else(|| {
            if cfg.defaults.thread_profile.is_empty() { "claude" } else { &cfg.defaults.thread_profile }
        });
        let builtin = !cfg.profiles.contains_key(name);
        let p = cfg
            .profiles
            .get(name)
            .cloned()
            .or_else(|| aaa_config::profile_edit::builtin(name))
            .ok_or_else(|| bad(format!("host.toml에 profile `{name}`가 없다: `aaa profile list`로 본다")))?;
        Ok(FrontResponse::ProfileResolved {
            profile: aaa_proto::ProfileView {
                name: name.to_string(),
                harness: p.harness.clone(),
                model: (!p.model.is_empty()).then(|| p.model.clone()),
                effort: (!p.effort.is_empty()).then(|| p.effort.clone()),
                args: aaa_config::profile_edit::launch_flags(&p),
                description: p.description,
                builtin,
            },
        })
    }

    /// host에서 이름·기본값·safety를 풀어 전달한다. 옛 agent에는 정의를 잃은 채 이름만 보내지 않는다.
    pub(crate) fn resolved_profile(
        &self,
        project: Option<&str>,
        mid: &str,
        name: &str,
    ) -> Result<aaa_proto::ResolvedProfile, ErrorBody> {
        self.require_host_profiles(mid)?;
        let profile = self.configured_thread_profile(project, name)?;
        if profile.harness == "agy" {
            if project.is_none() {
                return Err(bad("agy는 자동 thread profile로 사용한다"));
            }
            if !self.machine_feature(mid, aaa_proto::AGY_WORKER_FEATURE) {
                return Err(bad("그 머신 agent는 agy 자동 thread를 지원하지 않는다"));
            }
            self.agy_subscription(mid)?;
        }
        Ok(profile)
    }

    pub(super) fn agy_subscription(&self, mid: &str) -> Result<String, ErrorBody> {
        let i = self.inner.lock().unwrap();
        let m = i.machines.get(mid).filter(|m| m.conn.is_some()).ok_or_else(|| bad("agy 실행 머신이 끊겼다"))?;
        let mut subscriptions = m.features.iter().filter_map(|f| f.strip_prefix(aaa_proto::AGY_SUBSCRIPTION_PREFIX));
        let alias = subscriptions.next().filter(|alias| aaa_config::profile_edit::validate_name(alias).is_ok());
        match alias {
            Some(alias) if subscriptions.next().is_none() => Ok(alias.into()),
            _ => Err(bad("이 머신의 agy 개인 구독 별칭을 확인하지 못했다")),
        }
    }

    /// 승인 범위는 host 설정만 본다. 연결·agent 판은 실제 시작 후보를 고를 때 확인한다.
    pub(crate) fn configured_thread_profile(
        &self,
        project: Option<&str>,
        name: &str,
    ) -> Result<aaa_proto::ResolvedProfile, ErrorBody> {
        // 새 pane은 thread safety를 덧붙이지 않던 기존 계약을 유지한다.
        let safety = project.map(|project| {
            let s = self.safety(project);
            (self.default_thread_profile(project), s.thread_agent_args, s.yolo)
        });
        self.profile_definition(name, safety)
    }

    /// coordinator(`project open`, plan §8.180): safety의 coordinator 몫(참고 `Safety::launch_args`의 Role::Coordinator) —
    /// 프로젝트 기본 coordinator profile과 같은 harness의 기본 profile에 `coordinator_agent_args`, yolo면 권한 건너뛰기
    pub(crate) fn resolved_coordinator_profile(
        &self,
        project: &str,
        mid: &str,
        name: &str,
    ) -> Result<aaa_proto::ResolvedProfile, ErrorBody> {
        let s = self.safety(project);
        self.resolve_launch(
            mid,
            name,
            Some((self.default_coordinator_profile(project), s.coordinator_agent_args, s.yolo)),
        )
    }

    /// `safety`: (그 역할의 프로젝트 기본 profile, 기본 profile에 붙일 인자, yolo)
    pub(super) fn resolve_launch(
        &self,
        mid: &str,
        name: &str,
        safety: Option<(String, Vec<String>, bool)>,
    ) -> Result<aaa_proto::ResolvedProfile, ErrorBody> {
        self.require_host_profiles(mid)?;
        let profile = self.profile_definition(name, safety)?;
        if profile.harness == "agy" {
            return Err(bad("agy는 자동 thread profile로 사용한다"));
        }
        Ok(profile)
    }

    pub(super) fn require_host_profiles(&self, mid: &str) -> Result<(), ErrorBody> {
        if !self.machine_feature(mid, "host_profiles.v1") {
            return Err(bad(
                "그 머신 agent는 host profile을 받지 못한다(host_profiles.v1 없음). 연결은 유지된다. 그 머신 aaa agent를 갱신한 뒤 시작한다",
            ));
        }
        Ok(())
    }

    pub(super) fn profile_definition(
        &self,
        name: &str,
        safety: Option<(String, Vec<String>, bool)>,
    ) -> Result<aaa_proto::ResolvedProfile, ErrorBody> {
        let cfg = self.profile_config()?;
        let builtin = !cfg.profiles.contains_key(name);
        let mut p =
            cfg.profiles.get(name).cloned().or_else(|| aaa_config::profile_edit::builtin(name)).ok_or_else(|| {
                bad(format!(
                    "host.toml에 profile `{name}`가 없다: `aaa profile list`로 보고 사람이 `aaa profile add`로 더한다"
                ))
            })?;
        if let Some((default, agent_args, yolo)) = safety {
            if p.harness == "agy" {
                aaa_config::profile_edit::validate_agy_policy(&p, yolo).map_err(bad)?;
            }
            let legacy = cfg
                .profiles
                .get(&default)
                .cloned()
                .or_else(|| aaa_config::profile_edit::builtin(&default))
                .map(|p| p.harness)
                .unwrap_or_else(|| "claude".into());
            let mut args = aaa_config::safety::legacy_args(builtin, &p.harness, &agent_args, &legacy);
            args.extend(p.args);
            p.args = aaa_config::safety::apply_yolo(yolo, &p.harness, &args);
        }
        Ok(aaa_proto::ResolvedProfile {
            harness: p.harness,
            model: p.model,
            effort: p.effort,
            args: p.args,
            env: p.env,
            command: p.command,
        })
    }

    /// thread가 그 프로젝트에서 이 profile을 써도 되는가(safety `thread_profiles`, 참고 `check_allowed`). 목록은 사람만 바꾼다.
    pub(crate) fn check_profile_allowed(&self, project: &str, name: &str) -> Result<(), ErrorBody> {
        let safety = self.safety(project);
        if safety.profile_allowed(name) {
            return Ok(());
        }
        let list = safety.thread_profiles.unwrap_or_default();
        Err(err(
            ErrorCode::Forbidden,
            format!(
                "profile `{name}`는 `{project}`의 thread에 허용되지 않았다. 허용: {}. 이 목록은 사람만 바꾼다(`aaa profile allow threads … --project {project}`)",
                if list.is_empty() { "없음".to_string() } else { list.join(", ") }
            ),
        ))
    }

    /// `thread start`에 profile이 없을 때(참고 `resolve`·`new_project_default`): project.toml `thread_profile` → host.toml
    /// `[defaults] thread_profile` → 기본 profile `claude`. host.toml은 쓸 때마다 다시 읽는다.
    pub(crate) fn default_thread_profile(&self, project: &str) -> String {
        let own = self.settings(project).thread_profile;
        if !own.is_empty() {
            return own;
        }
        let global = match self.config_dir.get() {
            Some(dir) => aaa_config::safety::default_thread_profile(dir),
            None => self.cfg.defaults.thread_profile.clone(),
        };
        if global.is_empty() { "claude".into() } else { global }
    }

    /// `project open`에 profile이 없을 때(참고 `resolve`의 Role::Coordinator): project.toml `coordinator_profile` → host.toml
    /// `[defaults] coordinator_profile` → 기본 profile `claude`
    pub(crate) fn default_coordinator_profile(&self, project: &str) -> String {
        let own = self.settings(project).coordinator_profile;
        if !own.is_empty() {
            return own;
        }
        let global = match self.config_dir.get() {
            Some(dir) => aaa_config::safety::default_profile(dir, "coordinator_profile"),
            None => self.cfg.defaults.coordinator_profile.clone(),
        };
        if global.is_empty() { "claude".into() } else { global }
    }
}
