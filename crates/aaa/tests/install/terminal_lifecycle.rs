//! ConPTY처럼 터미널 소유자가 출력 쪽을 붙잡아도 실제 자식 종료로 닫고 끝 출력을 수집한다.
use super::*;
use portable_pty::{MasterPty, PtyPair};

struct RetainedOutputWriter {
    master: Box<dyn MasterPty + Send>,
    eof_reader: std::os::unix::net::UnixStream,
    _eof_writer: std::os::unix::net::UnixStream,
}
impl MasterPty for RetainedOutputWriter {
    fn resize(&self, size: PtySize) -> anyhow::Result<()> {
        self.master.resize(size)
    }
    fn get_size(&self) -> anyhow::Result<PtySize> {
        self.master.get_size()
    }
    fn try_clone_reader(&self) -> anyhow::Result<Box<dyn Read + Send>> {
        Ok(Box::new(self.master.try_clone_reader()?.chain(self.eof_reader.try_clone()?)))
    }
    fn take_writer(&self) -> anyhow::Result<Box<dyn Write + Send>> {
        self.master.take_writer()
    }
    fn process_group_leader(&self) -> Option<libc::pid_t> {
        self.master.process_group_leader()
    }
    fn as_raw_fd(&self) -> Option<i32> {
        self.master.as_raw_fd()
    }
    fn tty_name(&self) -> Option<PathBuf> {
        self.master.tty_name()
    }
}

#[test]
fn child_exit_closes_terminal_owner_before_eof_and_preserves_final_output() {
    let pair = native_pty_system().openpty(PtySize { rows: 36, cols: 120, pixel_width: 0, pixel_height: 0 }).unwrap();
    // ConPTY의 출력 pipe처럼 실제 자식 출력 뒤에도 소유자가 writer를 붙잡는다.
    let (eof_reader, eof_writer) = std::os::unix::net::UnixStream::pair().unwrap();
    let pair = PtyPair {
        master: Box::new(RetainedOutputWriter { master: pair.master, eof_reader, _eof_writer: eof_writer }),
        slave: pair.slave,
    };
    let mut command = CommandBuilder::new("/bin/sh");
    command.env_remove("ENV");
    command.env_remove("BASH_ENV");
    command.args([
        "-c",
        r#"printf '정확히 yes: '; IFS= read -r answer; [ "$answer" = yes ] || exit 41; i=0; while [ "$i" -lt 300 ]; do printf 'final-output-%03d\n' "$i"; i=$((i+1)); done; exit 7"#,
    ]);
    let (code, text) = drive_terminal_on(pair, command, &[("정확히 yes", "yes\r")], |_, _| {});
    assert_eq!(code, 7, "{}", redact_private(&text));
    assert_eq!(text.matches("final-output-").count(), 300, "끝 출력도 모두 수집해야 한다");
    assert!(text.contains("final-output-299"));
}
