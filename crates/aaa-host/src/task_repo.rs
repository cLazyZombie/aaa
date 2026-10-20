//! task 저장소 칸(plan §8.328): 칸에 쓸 값 판정과 `--from-task` 시작의 저장소·머신 고르기.
//!
//! - 칸 값은 `project.toml` repos 경로의 마지막 이름이나 `작업 폴더`다. 경로를 받으면 이름으로 바꿔 적는다.
//! - 띄울 때는 그 이름을 가진 repos 줄(머신마다 경로가 다를 수 있다) 가운데 하나를 고른다. owner·`--machine`이 머신을 정했으면
//!   그 머신이고, 거기에 그 저장소가 없으면 거부한다. 정하지 않았으면 덜 붐비는 머신이다(`machine_order`).

use std::collections::HashMap;
use std::path::Path;

use aaa_proto::{Connection, ErrorBody, ErrorCode, ThreadKind, ThreadStatus};

use crate::orch::{StartArgs, ThreadRec};
use crate::project::Repo;
use crate::tasks::REPO_DIR;
use crate::{Host, err};

fn bad(m: impl Into<String>) -> ErrorBody {
    err(ErrorCode::BadRequest, m)
}

/// 저장소 경로의 마지막 이름(끝 구분자 무시, `\`도 구분자로 본다: Windows 머신 경로)
pub(crate) fn repo_name(path: &str) -> Option<&str> {
    let p = path.trim_end_matches(['/', '\\']);
    let name = p.rsplit(['/', '\\']).next()?;
    Path::new(name).file_name().and_then(|n| n.to_str()).filter(|n| !n.is_empty())
}

/// 칸에 쓸 수 있는 이름(겹치지 않게 project.toml에 적힌 차례)
pub(crate) fn repo_names(repos: &[Repo]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for n in repos.iter().filter_map(|r| repo_name(&r.path)) {
        if crate::tasks::repo_like(n) && !out.iter().any(|x| x == n) {
            out.push(n.to_string());
        }
    }
    out
}

/// 칸 값 판정: `작업 폴더`, 저장소 이름, 또는 repos 경로(이름으로 바꾼다)
pub(crate) fn check_repo(repos: &[Repo], raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    if raw == REPO_DIR {
        return Ok(raw.to_string());
    }
    let name = if raw.contains(['/', '\\']) {
        let r = repos
            .iter()
            .find(|r| r.path.trim_end_matches(['/', '\\']) == raw.trim_end_matches(['/', '\\']))
            .ok_or_else(|| format!("project.toml repos에 없는 경로다: {raw}"))?;
        repo_name(&r.path).ok_or_else(|| format!("저장소 경로에 이름이 없다: {}", r.path))?
    } else {
        raw
    };
    if !crate::tasks::repo_like(name) {
        return Err(format!("task 저장소 칸에 쓸 수 없는 이름이다(영문·숫자·`.`·`_`·`-`): {name}"));
    }
    let names = repo_names(repos);
    if !names.iter().any(|n| n == name) {
        let have = if names.is_empty() { "없음".to_string() } else { names.join(", ") };
        return Err(format!("project.toml repos에 없는 저장소다: {name}(있는 것: {have}, 또는 `{REPO_DIR}`)"));
    }
    Ok(name.to_string())
}

/// 머신 하나의 붐빔
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Busy {
    pub name: String,
    /// 연결돼 있고 점검 이상이 없다
    pub ready: bool,
    /// 모든 프로젝트의 열린(시작 중 포함) thread 수
    pub open: u32,
    pub load_x100: Option<u32>,
}

/// 덜 붐비는 차례(고를 수 있는 머신만): 열린 thread 적은 차례 → 부하 낮은 차례(모르면 뒤) → 이름
pub(crate) fn least_busy_order(mut all: Vec<Busy>) -> Vec<String> {
    all.retain(|b| b.ready);
    all.sort_by(|a, b| {
        (a.open, a.load_x100.unwrap_or(u32::MAX), &a.name).cmp(&(b.open, b.load_x100.unwrap_or(u32::MAX), &b.name))
    });
    all.into_iter().map(|b| b.name).collect()
}

impl Host {
    /// 머신마다 붐빔과 host 로컬 머신 이름
    fn busy(&self) -> (Vec<Busy>, Option<String>) {
        let mut open: HashMap<String, u32> = HashMap::new();
        for r in
            self.db.threads(None).unwrap_or_default().iter().filter_map(|j| serde_json::from_str::<ThreadRec>(j).ok())
        {
            if matches!(r.status, ThreadStatus::Open | ThreadStatus::Starting) {
                *open.entry(r.machine_id).or_default() += 1;
            }
        }
        let views = self.machine_views();
        let local = views.iter().find(|m| m.local).map(|m| m.name.clone());
        let all = views
            .into_iter()
            .map(|m| Busy {
                ready: m.connection == Connection::Connected && m.problems.is_empty(),
                open: open.get(&m.id).copied().unwrap_or(0),
                load_x100: m.health.as_ref().and_then(|h| h.load_x100),
                name: m.name,
            })
            .collect();
        (all, local)
    }

    /// 덜 붐비는 차례의 머신 이름과 host 로컬 머신 이름(「바로 시작」 미리보기)
    pub(crate) fn machine_order(&self) -> (Vec<String>, Option<String>) {
        let (all, local) = self.busy();
        (least_busy_order(all), local)
    }

    /// 칸에 쓸 값(`task_edit`의 저장소 판정)
    pub(crate) fn task_repo_ok(&self, project: &str, raw: &str) -> Result<String, String> {
        check_repo(&self.settings(project).repos, raw)
    }

    /// `--from-task` 시작에 task 저장소 칸을 옮긴다(`apply_from_task` 뒤). `kind_given`·`a.repo`·`a.machine`은 요청이 정한 값이다.
    /// 칸이 비면 요청의 `--repo`를 그 칸처럼 쓰고, 그것도 없으면 작업 폴더로 띄운다(요청이 `--kind`를 정하지 않았으면 경고).
    pub(crate) fn apply_task_repo(
        &self,
        a: &mut StartArgs,
        field: Option<&str>,
        kind_given: Option<ThreadKind>,
    ) -> Result<Option<String>, ErrorBody> {
        let repos = self.settings(&a.project).repos;
        let given = a.repo.clone();
        let want = match (field, given.as_deref()) {
            (Some(f), Some(g)) => {
                let g_name = check_repo(&repos, g).map_err(bad)?;
                if g_name != f {
                    return Err(bad(format!("task 저장소 칸과 --repo가 다르다: {f} / {g}")));
                }
                Some(f.to_string())
            }
            (Some(f), None) => Some(f.to_string()),
            // 칸이 빈 task에 project.toml에 없는 경로를 주면 지금처럼 그 경로로 띄운다(일반 `thread start --repo`와 같다)
            (None, Some(g)) => match check_repo(&repos, g) {
                Ok(name) => Some(name),
                Err(_) if g.contains(['/', '\\']) => return Ok(None),
                Err(e) => return Err(bad(e)),
            },
            (None, None) => None,
        };
        let (order, local) = self.machine_order();
        let mut warning = None;
        match want.as_deref() {
            None | Some(REPO_DIR) => {
                if kind_given == Some(ThreadKind::Worktree) {
                    return Err(bad("task가 작업 폴더인데 --kind worktree다"));
                }
                if want.is_none() && kind_given.is_none() {
                    warning = Some("task에 저장소 칸이 없어 작업 폴더로 띄웠다".to_string());
                }
                a.kind = ThreadKind::Dir;
                a.repo = None;
                if a.machine.is_none() {
                    a.machine = order.into_iter().next();
                }
            }
            Some(name) => {
                if kind_given == Some(ThreadKind::Dir) {
                    return Err(bad(format!("task 저장소가 {name}인데 --kind dir다")));
                }
                // 그 이름의 repos 줄(머신 이름, 경로). 요청이 경로를 줬으면 그 경로만(끝 구분자 무시, `check_repo`와 같다)
                let trim = |p: &str| p.trim_end_matches(['/', '\\']).to_string();
                let named: Vec<&Repo> = repos
                    .iter()
                    .filter(|r| repo_name(&r.path) == Some(name))
                    .filter(|r| given.as_deref().is_none_or(|g| !g.contains(['/', '\\']) || trim(&r.path) == trim(g)))
                    .collect();
                if named.is_empty() {
                    return Err(bad(format!("project.toml repos에 {name} 저장소가 없다")));
                }
                // 머신을 적지 않은 줄은 host 로컬 머신이다: 로컬 agent가 없으면 그 줄은 띄울 곳이 없다
                let mut cands: Vec<(String, String)> = named
                    .iter()
                    .filter_map(|r| Some((r.machine.clone().or_else(|| local.clone())?, r.path.clone())))
                    .collect();
                if cands.is_empty() {
                    return Err(err(
                        ErrorCode::NotFound,
                        format!("{name} 저장소가 host 로컬 머신에만 있는데 로컬 agent가 없다"),
                    ));
                }
                if let Some(m) = &a.machine {
                    cands.retain(|(cm, _)| cm == m);
                    if cands.is_empty() {
                        return Err(bad(format!("{m}에 {name} 저장소가 없다(project.toml repos)")));
                    }
                }
                let (machine, path) = if cands.len() == 1 {
                    cands.remove(0)
                } else {
                    let pick = order.iter().find_map(|o| cands.iter().position(|(cm, _)| cm == o));
                    match pick {
                        Some(i) => cands.remove(i),
                        None => {
                            let mut names: Vec<_> = cands.iter().map(|(m, _)| m.as_str()).collect();
                            names.dedup();
                            return Err(err(
                                ErrorCode::Unavailable,
                                format!(
                                    "{name} 저장소가 있는 머신({})이 모두 끊겼거나 점검 이상이다",
                                    names.join(", ")
                                ),
                            ));
                        }
                    }
                };
                // 고른 머신에 같은 이름 저장소가 또 있으면 말없이 고르지 않는다
                let same: Vec<&str> = cands.iter().filter(|(m, _)| *m == machine).map(|(_, p)| p.as_str()).collect();
                if !same.is_empty() {
                    return Err(bad(format!(
                        "{machine}에 {name} 저장소가 둘 이상이다({path}, {}). --repo로 경로를 준다",
                        same.join(", ")
                    )));
                }
                a.kind = ThreadKind::Worktree;
                a.repo = Some(path);
                a.machine = Some(machine);
            }
        }
        Ok(warning)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(path: &str, machine: Option<&str>) -> Repo {
        Repo { path: path.into(), machine: machine.map(Into::into) }
    }

    #[test]
    fn names_and_values() {
        let repos = [
            repo("/work/aaa", None),
            repo("/srv/b/aaa/", Some("mac-b")),
            repo(r"C:\src\rc", Some("win-a")),
            repo("/work/한글", None),
        ];
        assert_eq!(repo_names(&repos), ["aaa", "rc"], "겹침·칸에 못 쓰는 이름은 뺀다");
        assert_eq!(check_repo(&repos, "aaa").unwrap(), "aaa");
        assert_eq!(check_repo(&repos, "/srv/b/aaa").unwrap(), "aaa", "경로는 이름으로");
        assert_eq!(check_repo(&repos, r"C:\src\rc").unwrap(), "rc");
        assert_eq!(check_repo(&repos, REPO_DIR).unwrap(), REPO_DIR);
        assert!(check_repo(&repos, "bbb").unwrap_err().contains("있는 것: aaa, rc"));
        assert!(check_repo(&repos, "/work/bbb").unwrap_err().contains("없는 경로"));
        assert!(check_repo(&repos, "/work/한글").unwrap_err().contains("쓸 수 없는 이름"));
    }

    #[test]
    fn least_busy_skips_unready_then_open_load_name() {
        let b = |name: &str, ready, open, load| Busy { name: name.into(), ready, open, load_x100: load };
        let order = least_busy_order(vec![
            b("mac-a", true, 2, Some(10)),
            b("mac-b", true, 1, None),
            b("mac-c", true, 1, Some(300)),
            b("mac-d", false, 0, Some(0)),
            b("mac-e", true, 1, Some(300)),
        ]);
        assert_eq!(order, ["mac-c", "mac-e", "mac-b", "mac-a"]);
    }
}
