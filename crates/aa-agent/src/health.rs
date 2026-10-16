//! 머신 건강(§8.10). 주기적으로 디스크·부하·전원·toolchain을 재서 host에 보낸다.
//!
//! - 이상(problems)은 새 세션을 거부할 이유다: 작업 root 디스크 여유 부족, toolchain 명령 실패.
//! - 주의(warnings)는 보이기만 한다: 배터리 전원.
//! - 시계는 host가 보고의 `at_ms`와 자기 시계를 비교해 본다.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use aa_config::HealthSection;
use aa_proto::{AgentEvent, HealthProblem, LinkMsg, MachineHealth, ToolCheck};

use crate::Agent;

const TOOL_TIMEOUT: Duration = Duration::from_secs(10);

/// (여유, 전체) 바이트
#[cfg(unix)]
fn disk(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: c는 NUL로 끝나는 경로이고 st는 쓸 수 있는 statvfs다
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return None;
    }
    let unit = st.f_frsize as u64;
    Some((st.f_bavail as u64 * unit, st.f_blocks as u64 * unit))
}

#[cfg(not(unix))]
fn disk(_: &Path) -> Option<(u64, u64)> {
    None
}

#[cfg(unix)]
fn load_x100() -> Option<u32> {
    let mut l = [0f64; 3];
    // SAFETY: 길이 3 배열에 1개만 쓴다
    (unsafe { libc::getloadavg(l.as_mut_ptr(), 1) } == 1).then(|| (l[0] * 100.0).round() as u32)
}

#[cfg(not(unix))]
fn load_x100() -> Option<u32> {
    None
}

/// 전원 연결 여부. 배터리가 없는 Mac은 `pmset`이 AC Power를 알린다.
async fn on_ac() -> Option<bool> {
    if cfg!(target_os = "macos") {
        let out = run(&["pmset".into(), "-g".into(), "batt".into()]).await.ok()?;
        if out.contains("'AC Power'") {
            Some(true)
        } else if out.contains("'Battery Power'") {
            Some(false)
        } else {
            None
        }
    } else if cfg!(target_os = "linux") {
        let mut any = false;
        for e in std::fs::read_dir("/sys/class/power_supply").ok()?.flatten() {
            let p = e.path();
            if std::fs::read_to_string(p.join("type")).is_ok_and(|t| t.trim() == "Mains") {
                any = true;
                if std::fs::read_to_string(p.join("online")).is_ok_and(|o| o.trim() == "1") {
                    return Some(true);
                }
            }
        }
        any.then_some(false)
    } else {
        None
    }
}

/// 명령 하나(시간 상한). 성공하면 stdout+stderr를 돌려준다.
async fn run(argv: &[String]) -> Result<String, String> {
    let mut c = tokio::process::Command::new(&argv[0]);
    c.args(&argv[1..]).stdin(std::process::Stdio::null()).kill_on_drop(true);
    let out = tokio::time::timeout(TOOL_TIMEOUT, c.output())
        .await
        .map_err(|_| format!("{}초 안에 끝나지 않았다", TOOL_TIMEOUT.as_secs()))?
        .map_err(|e| format!("실행하지 못했다: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    if out.status.success() { Ok(text) } else { Err(format!("{}: {}", out.status, first_line(&text))) }
}

fn first_line(s: &str) -> String {
    s.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").chars().take(200).collect()
}

pub async fn collect(cfg: &HealthSection, work_root: &Path) -> MachineHealth {
    // 작업 root가 아직 없으면 그 위 폴더(같은 디스크)를 본다
    let probe = work_root.ancestors().find(|p| p.exists()).unwrap_or(Path::new("/"));
    let d = disk(probe);
    let mut problems = Vec::new();
    let mut warnings = Vec::new();
    if let Some((free, _)) = d
        && cfg.min_free_gb > 0
        && free < cfg.min_free_gb.saturating_mul(1 << 30)
    {
        problems.push(HealthProblem {
            code: "disk".into(),
            text: format!(
                "디스크 여유 {:.1}GB < {}GB({})",
                free as f64 / (1u64 << 30) as f64,
                cfg.min_free_gb,
                probe.display()
            ),
        });
    }
    let mut toolchain = Vec::new();
    for (name, argv) in &cfg.toolchain {
        let check = match run(argv).await {
            Ok(out) => ToolCheck { name: name.clone(), ok: true, version: first_line(&out) },
            Err(e) => {
                problems.push(HealthProblem {
                    code: format!("toolchain:{name}"),
                    text: format!("toolchain {name} 실패: {e}"),
                });
                ToolCheck { name: name.clone(), ok: false, version: e }
            }
        };
        toolchain.push(check);
    }
    let ac = on_ac().await;
    if ac == Some(false) {
        warnings.push("배터리 전원".into());
    }
    MachineHealth {
        at_ms: aa_common::util::now_ms(),
        disk_free_bytes: d.map(|x| x.0),
        disk_total_bytes: d.map(|x| x.1),
        load_x100: load_x100(),
        cpus: std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(1),
        on_ac: ac,
        toolchain,
        problems,
        warnings,
    }
}

impl Agent {
    /// 주기 점검. 결과는 기억해 두었다가 host가 다시 붙을 때도 보낸다.
    pub fn start_health(self: &Arc<Self>, cfg: HealthSection) {
        let me = self.clone();
        tokio::spawn(async move {
            loop {
                let root = me.threads_cfg.lock().unwrap().work_root.clone();
                let h = collect(&cfg, &root).await;
                *me.health.lock().unwrap() = Some(h.clone());
                me.send_health(h).await;
                tokio::time::sleep(Duration::from_secs(cfg.every_s)).await;
            }
        });
    }

    pub(crate) async fn send_health(&self, h: MachineHealth) {
        if self.host_health.load(std::sync::atomic::Ordering::Relaxed) {
            self.send_host(&LinkMsg::Event(AgentEvent::Health(h))).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[tokio::test]
    async fn problems_from_disk_and_toolchain() {
        let mut cfg = HealthSection { every_s: 60, min_free_gb: 0, toolchain: Default::default() };
        cfg.toolchain.insert("ok".into(), vec!["/bin/sh".into(), "-c".into(), "echo 'tool 1.2'".into()]);
        let h = collect(&cfg, Path::new("/nonexistent-aa/work")).await;
        assert!(h.problems.is_empty(), "{:?}", h.problems);
        assert_eq!(h.toolchain[0].version, "tool 1.2");
        assert!(h.disk_free_bytes.is_some() && h.load_x100.is_some());
        cfg.min_free_gb = 1 << 30; // 1EB
        cfg.toolchain.insert("bad".into(), vec!["/bin/sh".into(), "-c".into(), "echo nope >&2; exit 3".into()]);
        let h = collect(&cfg, Path::new("/")).await;
        let all = h.problems.iter().map(|p| format!("{}={}", p.code, p.text)).collect::<Vec<_>>().join(" | ");
        assert!(all.contains("toolchain:bad=toolchain bad 실패") && all.contains("nope"), "{all}");
        assert!(all.contains("disk=디스크 여유"), "{all}");
    }
}
