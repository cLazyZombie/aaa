//! PTY EOF와 독립된 OS 자식 종료 사건. main driver가 wait하여 상태를 수집한다.
use super::{TerminalEvent, mpsc};

#[path = "../support/child_exit.rs"]
mod child_exit;
use child_exit::ChildExit as Prepared;

pub(super) fn watch(
    child: &mut dyn portable_pty::Child,
    tx: mpsc::Sender<TerminalEvent>,
) -> std::thread::JoinHandle<()> {
    let prepared = Prepared::new(child);
    std::thread::spawn(move || {
        let _ = tx.send(TerminalEvent::ChildExited(prepared.and_then(Prepared::wait)));
    })
}

#[cfg(unix)]
#[test]
fn owner_reaping_after_subscription_preserves_the_child_exit_event() {
    use std::process::{Command, Stdio};
    let mut child: Box<dyn portable_pty::Child + Send + Sync> = Box::new(
        Command::new("/bin/sh")
            .args(["-c", "IFS= read -r gate"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let prepared = Prepared::new(child.as_mut()).unwrap();
    child.kill().unwrap();
    let status = child.wait().unwrap();
    assert!(!status.success(), "실제 소유 자식을 종료해야 한다: {status:?}");
    let (tx, rx) = mpsc::channel();
    let waiter = std::thread::spawn(move || tx.send(prepared.wait()).unwrap());
    let event = rx.recv_timeout(std::time::Duration::from_secs(60)).expect("reap 뒤 실제 종료 사건");
    assert!(event.is_ok(), "owner가 먼저 wait해도 종료 통지를 잃지 않는다: {event:?}");
    waiter.join().unwrap();
}
