//! v1/옛 routine과 v2가 같은 실행 타입으로 들어오는 경계. 범위·기본값은 보존한다.
use super::*;
use aaa_proto::{
    AutomationAction as A, AutomationCondition as C, AutomationDefinition as D, AutomationEventSource as S,
    AutomationObserve as O, AutomationTrigger as T,
};

fn convert<T: serde::de::DeserializeOwned>(v: impl Serialize) -> Result<T, String> {
    serde_json::from_value(serde_json::to_value(v).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

pub(crate) fn is_v2(text: &str) -> bool {
    toml::from_str::<toml::Table>(text).is_ok_and(|t| t.contains_key("schema"))
}
pub(crate) fn read(file: &str, text: &str) -> Result<File, String> {
    if is_v2(text) {
        from_v2(decode::<D>(file, text)?).map_err(|e| field_error(file, text, "schema", &e))
    } else {
        decode(file, text)
    }
}
pub(crate) fn form(value: serde_json::Value) -> Result<File, String> {
    if value.get("schema").is_some() {
        from_v2(serde_json::from_value(value).map_err(|e| e.to_string())?)
    } else {
        serde_path_to_error::deserialize::<_, File>(value).map_err(|e| format!("폼 {}: {}", e.path(), e.inner()))
    }
}
pub(crate) fn from_v2(d: D) -> Result<File, String> {
    if d.schema != 2 {
        return Err("지원하지 않는 자동화 schema: 2를 쓴다".into());
    }
    let c = &d.condition;
    let mut remaining = c.clone();
    remaining.min_commits = None;
    let on = match d.trigger {
        T::Schedule { every, daily, catch_up } => {
            remaining.min_open_threads = None;
            let schedule =
                Scheduled { every, daily, catch_up: convert(catch_up)?, min_open_threads: c.min_open_threads };
            if let Some(O::Command(cmd)) = d.observe {
                if !c.output_changed {
                    return Err("observe.command에는 condition.output_changed=true가 필요하다".into());
                }
                remaining.output_changed = false;
                Condition::Command(Command {
                    every: schedule.every,
                    daily: schedule.daily,
                    catch_up: schedule.catch_up,
                    min_open_threads: schedule.min_open_threads,
                    command: cmd.command,
                    machine: cmd.machine,
                })
            } else {
                Condition::Schedule(schedule)
            }
        }
        T::Event { source, repo, machine } => {
            if d.observe.is_some() {
                return Err("명령 조회는 일정 트리거에만 쓴다".into());
            }
            remaining.events.clear();
            match source {
                S::Thread => {
                    if repo.is_some() || machine.is_some() {
                        return Err("thread 사건은 condition.repos로 거른다".into());
                    }
                    remaining.repos.clear();
                    remaining.include_automation = false;
                    Condition::Thread(Threads {
                        events: c.events.clone(),
                        repos: c.repos.clone(),
                        include_automation: c.include_automation,
                    })
                }
                S::Pr => {
                    if repo.is_some() || machine.is_some() {
                        return Err("PR 사건의 저장소·머신은 소유 thread가 정한다".into());
                    }
                    Condition::Pr(Pr { events: c.events.clone() })
                }
                S::Ci => {
                    remaining.branches.clear();
                    remaining.workflows.clear();
                    Condition::Ci(Ci {
                        repo: repo.ok_or("CI trigger.repo가 필요하다")?,
                        branches: c.branches.clone(),
                        workflows: c.workflows.clone(),
                        events: c.events.clone(),
                        machine,
                    })
                }
                S::Git => {
                    remaining.branches.clear();
                    Condition::Git(Git {
                        repo: repo.ok_or("Git trigger.repo가 필요하다")?,
                        branches: c.branches.clone(),
                        events: c.events.clone(),
                        machine,
                    })
                }
            }
        }
    };
    if remaining != C::default() {
        return Err("트리거에 적용할 수 없는 condition 필드가 있다".into());
    }
    let action = match d.action {
        A::Notice(a) => Action::Inbox(convert(a)?),
        A::Message(a) => Action::Message(convert(a)?),
        A::Thread(a) => Action::Start(convert(a)?),
        A::Command(a) => {
            if matches!(on, Condition::Command(_)) {
                return Err("한 규칙에 명령 조회와 명령 동작을 함께 쓰지 않는다".into());
            }
            Action::Command(a)
        }
        A::Proposal => Action::Deploy,
    };
    let mut limits: Limits = convert(d.policy)?;
    limits.min_commits = c.min_commits;
    Ok(File { title: d.title, enabled: d.enabled, on, action, limits })
}

pub(crate) fn to_v2(f: &File) -> D {
    let mut c = C { min_commits: f.limits.min_commits, ..Default::default() };
    let mut observe = None;
    let trigger = match &f.on {
        Condition::Schedule(s) => {
            c.min_open_threads = s.min_open_threads;
            T::Schedule {
                every: s.every.clone(),
                daily: s.daily.clone(),
                catch_up: convert(s.catch_up).expect("catchup"),
            }
        }
        Condition::Command(s) => {
            c.min_open_threads = s.min_open_threads;
            observe = Some(O::Command(aaa_proto::AutomationCommand {
                command: s.command.clone(),
                machine: s.machine.clone(),
            }));
            c.output_changed = true;
            T::Schedule {
                every: s.every.clone(),
                daily: s.daily.clone(),
                catch_up: convert(s.catch_up).expect("catchup"),
            }
        }
        Condition::Thread(t) => {
            c.events = t.events.clone();
            c.repos = t.repos.clone();
            c.include_automation = t.include_automation;
            T::Event { source: S::Thread, repo: None, machine: None }
        }
        Condition::Pr(t) => {
            c.events = t.events.clone();
            T::Event { source: S::Pr, repo: None, machine: None }
        }
        Condition::Ci(t) => {
            c.events = t.events.clone();
            c.branches = t.branches.clone();
            c.workflows = t.workflows.clone();
            T::Event { source: S::Ci, repo: Some(t.repo.clone()), machine: t.machine.clone() }
        }
        Condition::Git(t) => {
            c.events = t.events.clone();
            c.branches = t.branches.clone();
            T::Event { source: S::Git, repo: Some(t.repo.clone()), machine: t.machine.clone() }
        }
    };
    let action = match &f.action {
        Action::Inbox(a) => A::Notice(convert(a).expect("notice")),
        Action::Message(a) => A::Message(convert(a).expect("message")),
        Action::Start(a) => A::Thread(convert(a).expect("thread")),
        Action::Command(a) => A::Command(a.clone()),
        Action::Deploy => A::Proposal,
    };
    let mut policy = serde_json::to_value(&f.limits).expect("policy");
    policy.as_object_mut().unwrap().remove("min_commits");
    D {
        schema: 2,
        title: f.title.clone(),
        enabled: f.enabled,
        trigger,
        observe,
        condition: c,
        action,
        policy: convert(policy).expect("policy"),
    }
}
pub(crate) fn encode(f: &File) -> Result<String, String> {
    Ok(format!("#:schema ./automation.schema.json\n{}", toml::to_string_pretty(&to_v2(f)).map_err(|e| e.to_string())?))
}
