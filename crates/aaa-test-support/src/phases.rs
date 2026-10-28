//! 선택 계측. 준비·계약·관측·정리를 시험 전체 시간 안에 두고 사건 시각만 기록한다.
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// `AAA_TEST_PHASES`가 없으면 파일을 만들지 않는다. 이름에는 정적 업무 표지만 넣는다.
/// 실험실보다 먼저 선언하고 마지막에 cleanup 표지를 두어 Drop 비용까지 포함한다.
pub struct Phases {
    file: Option<File>,
    case: &'static str,
    started: Instant,
    previous: Instant,
    category: &'static str,
    label: &'static str,
}

impl Phases {
    pub fn new(case: &'static str) -> Self {
        Self::in_dir(case, std::env::var_os("AAA_TEST_PHASES").as_deref().map(Path::new))
    }

    fn in_dir(case: &'static str, dir: Option<&Path>) -> Self {
        let now = Instant::now();
        let mut phases = Self { file: None, case, started: now, previous: now, category: "prepare", label: "start" };
        if let Some(dir) = dir {
            let result = (|| {
                std::fs::create_dir_all(dir)?;
                let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
                File::options().write(true).create_new(true).open(dir.join(format!(
                    "rust-{}-{}-{sequence}.jsonl",
                    std::process::id(),
                    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos()
                )))
            })();
            match result {
                Ok(file) => phases.file = Some(file),
                Err(error) => eprintln!("시험 단계 계측 시작 실패: {error}"),
            }
        }
        phases.write("start", now);
        phases
    }

    pub fn mark(&mut self, category: &'static str, label: &'static str) {
        if self.file.is_none() {
            return;
        }
        let now = Instant::now();
        self.write("phase", now);
        self.previous = now;
        self.category = category;
        self.label = label;
    }

    fn write(&mut self, event: &str, now: Instant) {
        let Some(file) = &mut self.file else { return };
        let row = serde_json::json!({
            "event": event, "case": self.case, "category": self.category, "label": self.label,
            "at_ms": SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis(),
            "elapsed_ms": now.duration_since(self.started).as_secs_f64() * 1000.0,
            "duration_ms": now.duration_since(self.previous).as_secs_f64() * 1000.0,
            "pid": std::process::id(), "panicking": std::thread::panicking()
        });
        // 진단 실패가 원래 시험의 panic을 덮거나 Drop에서 두 번째 panic을 내지 않는다.
        if let Err(error) = writeln!(file, "{row}") {
            eprintln!("시험 단계 계측 쓰기 실패: {error}");
            self.file = None;
        }
    }
}

impl Drop for Phases {
    fn drop(&mut self) {
        self.write("end", Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unwinding_keeps_prior_phases_and_marks_incomplete_cleanup() {
        let root = std::env::temp_dir().join(format!("aaa-phases-{}-{}", std::process::id(), rand::random::<u64>()));
        let error = std::panic::catch_unwind(|| {
            let mut phases = Phases::in_dir("fixture", Some(&root));
            phases.mark("contract", "actual-event");
            phases.mark("cleanup", "owned-resources");
            std::panic::panic_any(17_u32);
        });
        assert_eq!(*error.unwrap_err().downcast::<u32>().unwrap(), 17);
        let files: Vec<_> = std::fs::read_dir(&root).unwrap().map(Result::unwrap).collect();
        assert_eq!(files.len(), 1);
        let rows: Vec<serde_json::Value> = std::fs::read_to_string(files[0].path())
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(
            rows.iter().map(|row| row["event"].as_str().unwrap()).collect::<Vec<_>>(),
            ["start", "phase", "phase", "end"]
        );
        assert_eq!(rows[1]["category"], "prepare");
        assert_eq!(rows[2]["category"], "contract");
        assert_eq!(rows[3]["category"], "cleanup");
        assert_eq!(rows[3]["panicking"], true);
        assert!(rows.windows(2).all(|pair| pair[0]["elapsed_ms"].as_f64() <= pair[1]["elapsed_ms"].as_f64()));
        std::fs::remove_dir_all(root).unwrap();
    }
}
