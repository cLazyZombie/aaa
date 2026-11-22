//! `aaa doctor`: 환경 점검.

use super::*;

#[derive(Args)]
pub(super) struct DoctorArgs {
    /// 빠진 Codex 전역 hook을 설치한다(백업 뒤 원자 쓰기)
    #[arg(long)]
    pub(super) fix: bool,
    #[command(subcommand)]
    pub(super) cmd: Option<DoctorCmd>,
}

#[derive(Subcommand)]
pub(super) enum DoctorCmd {
    /// 지금 프로세스 계보에서 측정한다(P0 E1·E2)
    Probe {
        /// 첫 실행 표본 수(새 실행 파일 수)
        #[arg(long, default_value_t = 30)]
        samples: u32,
        /// 실제로 읽어 볼 키체인 generic password의 service 이름
        #[arg(long)]
        keychain_service: Option<String>,
        /// 읽기·쓰기를 확인할 폴더(외장 볼륨)
        #[arg(long)]
        volume_dir: Option<String>,
        /// 결과에 붙일 조건 이름
        #[arg(long)]
        label: Option<String>,
        /// 결과 JSON을 이 파일에 쓴다
        #[arg(long)]
        out: Option<PathBuf>,
        /// warm·처리량을 비교할 기준선 결과 JSON
        #[arg(long)]
        baseline: Option<PathBuf>,
        /// 처리량 측정을 건너뛴다
        #[arg(long)]
        no_throughput: bool,
        /// 전체 디스크 접근(FDA)이 필요한 파일을 읽어 본다
        #[arg(long)]
        fda_check: bool,
        /// 결과를 상태 폴더 probe/results/<label>-<시각>.json에도 쓴다(LaunchAgent 실행용)
        #[arg(long)]
        out_state: bool,
        /// 음성 대조: 책임을 버린(disclaim) 자식으로 다시 실행해 잰다. 느리게 나와야 정상이다.
        #[arg(long)]
        disclaim: bool,
    },
    /// 결과 JSON 여러 개를 한 표로
    Summary { files: Vec<PathBuf> },
    /// 모든 프로젝트 폴더의 aaa 파일(coordinator 안내 AGENTS.md·uploads/)을 host에서 본다
    /// (herdr-projects `doctor`). 안내가 낡았으면 `--fix`가 지금 aaa의 것으로 다시 쓴다
    Files {
        /// 고친다: aaa 표지가 있는 AGENTS.md는 다시 쓰고, 표지 없는 AGENTS.md는 `<이름>.before-aaa`로 옮겨 둔다
        #[arg(long)]
        fix: bool,
    },
    /// 다른 프로세스의 책임 프로세스(E6: codex app-server·빌드 캐시 daemon 등)
    Lineage {
        /// 명령줄에 이 문자열이 들어 있는 프로세스를 찾는다(여러 번)
        #[arg(long = "match", default_values_t = ["app-server".to_string(), "kache".to_string(), "sccache".to_string(), "gradle".to_string(), "bazel".to_string()])]
        patterns: Vec<String>,
    },
}

pub(super) fn doctor_cmd(a: DoctorArgs, json: bool) -> Result<ExitCode> {
    anyhow::ensure!(!a.fix || a.cmd.is_none(), "doctor --fix는 하위 명령 없이 쓴다");
    match a.cmd {
        None => {
            let home = crate::integration::codex_home();
            let exe = std::env::current_exe()?;
            let backups = if a.fix { crate::integration::configure(home.clone(), &exe, false)? } else { vec![] };
            let check = crate::integration::healthy(home, &exe);
            let (ok, detail) = match check {
                Ok(true) => (true, "Codex 전역 hook 설치됨(실행하려면 Codex에서 신뢰)".to_string()),
                Ok(false) => (
                    false,
                    "Codex 전역 hook이 빠졌거나 낡았다: `aaa doctor --fix` 또는 `aaa integration install codex`"
                        .to_string(),
                ),
                Err(e) => (false, format!("Codex 전역 hook 점검 실패: {e:#}")),
            };
            let r = doctor::probe(&doctor::ProbeOptions { samples: 3, throughput: false, ..Default::default() });
            let usage = front_call(FrontRequest::LibraryUsage)
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()
                .and_then(Result::ok);
            if json {
                let mut value = serde_json::to_value(&r)?;
                value["library"] = serde_json::to_value(&usage)?;
                value["codex_hooks"] = serde_json::json!({"ok":ok,"detail":detail,"backups":backups});
                println!("{}", serde_json::to_string_pretty(&value)?);
            } else {
                print_report(&r, false)?;
                if let Some(usage) = usage {
                    print!("{}", render_front(&usage));
                } else {
                    println!("보고서 용량: host에 연결할 수 없어 확인하지 못했다");
                }
                println!("[{}] {detail}", if ok { "ok  " } else { "warn" });
                for path in backups {
                    println!("백업: {}", path.display());
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Some(DoctorCmd::Probe { disclaim: true, .. }) => {
            #[cfg(target_os = "macos")]
            {
                let args: Vec<String> = std::env::args().filter(|a| a != "--disclaim").collect();
                let code = doctor::macos::rerun_disclaimed(&args)?;
                Ok(ExitCode::from(code as u8))
            }
            #[cfg(not(target_os = "macos"))]
            anyhow::bail!("--disclaim은 macOS 전용이다")
        }
        Some(DoctorCmd::Probe {
            samples,
            keychain_service,
            volume_dir,
            label,
            out,
            baseline,
            no_throughput,
            fda_check,
            out_state,
            disclaim: false,
        }) => {
            let mut r = doctor::probe(&doctor::ProbeOptions {
                samples,
                keychain_service,
                volume_dir,
                label,
                throughput: !no_throughput,
                activity_held: false,
                fda_check,
            });
            if let Some(b) = baseline {
                let base: doctor::ProbeReport =
                    aaa_common::util::read_json(&b).with_context(|| format!("기준선 {}", b.display()))?;
                let worse = doctor::compare_throughput(&base, &r);
                if !worse.is_empty() {
                    r.reasons.extend(worse.into_iter().map(|w| format!("처리량 gate 실패: {w}")));
                    r.pass = false;
                }
            }
            let mut outs: Vec<PathBuf> = out.into_iter().collect();
            if out_state {
                let name = format!("{}-{}.json", r.label.clone().unwrap_or("probe".into()), r.created_at_ms);
                outs.push(aaa_config::paths::state_dir().join("probe/results").join(name));
            }
            for out in &outs {
                if let Some(dir) = out.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                std::fs::write(out, serde_json::to_vec_pretty(&r)?)?;
            }
            print_report(&r, json)?;
            Ok(if r.pass { ExitCode::SUCCESS } else { ExitCode::from(2) })
        }
        Some(DoctorCmd::Summary { files }) => {
            let mut rows = Vec::new();
            for f in files {
                let r: doctor::ProbeReport =
                    aaa_common::util::read_json(&f).with_context(|| format!("{}", f.display()))?;
                rows.push((f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), r));
            }
            print!("{}", doctor::summary_table(&rows));
            Ok(ExitCode::SUCCESS)
        }
        Some(DoctorCmd::Files { fix }) => {
            let res = front_call(FrontRequest::ProjectFiles { fix })?;
            // 고치지 못한 프로젝트가 있으면 실패로 끝낸다(참고 `doctor`의 FAIL). 고칠 것만 있으면 warn이고 성공이다
            let failed =
                matches!(&res, Ok(FrontResponse::ProjectFiles { checks }) if checks.iter().any(|c| c.error.is_some()));
            let code = front_print(json, res)?;
            Ok(if failed { ExitCode::FAILURE } else { code })
        }
        Some(DoctorCmd::Lineage { patterns }) => {
            #[cfg(target_os = "macos")]
            {
                let rows = doctor::macos::daemons(&patterns);
                if json {
                    let v: Vec<_> = rows
                        .iter()
                        .map(|(pid, cmd, rp, path)| serde_json::json!({"pid":pid,"command":cmd,"responsible_pid":rp,"responsible_path":path}))
                        .collect();
                    println!("{}", serde_json::to_string_pretty(&v)?);
                } else if rows.is_empty() {
                    println!("해당 프로세스 없음: {patterns:?}");
                } else {
                    for (pid, cmd, rp, path) in rows {
                        println!("{pid}\t책임 {}\t{}\n\t{cmd}", rp.unwrap_or(-1), path.unwrap_or_default());
                    }
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = (patterns, json);
                println!("macOS에서만 의미가 있다");
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

pub(super) fn print_report(r: &doctor::ProbeReport, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(r)?);
    } else {
        print!("{}", doctor::render(r));
    }
    Ok(())
}
