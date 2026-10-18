//! 사람만 하는 일의 문(plan §8.19·§8.162): routine 명령 승인, 감독자 설치, profile·safety 바꾸기.
//!
//! - aaa 세션 안(`AAA_SESSION_ID`·`AAA_HOOK_SOCK`)이면 거부한다: coordinator·thread와 그 도구가 대신하지 못하게.
//! - 표준 입력이 터미널이 아니면 거부한다: agent의 셸 도구는 터미널 없이 명령을 돌린다(herdr-projects `require_person`).
//! - 남는 한계: aaa 밖에서 터미널을 흉내 내는 수단(PTY를 여는 다른 프로그램)까지 막지는 못한다.

use std::io::{BufRead as _, IsTerminal as _, Write as _};

use anyhow::{Result, bail};

/// 사람이 aaa 세션 밖 터미널에서 직접 실행했는가. `what`은 거부 말에 들어간다(무엇을 하려 했나).
pub fn require(what: &str) -> Result<()> {
    if std::env::var_os("AAA_SESSION_ID").is_some() || std::env::var_os("AAA_HOOK_SOCK").is_some() {
        bail!("{what}: aaa 세션 안에서는 하지 않는다. 사람이 host 머신의 일반 터미널(aaa pane 밖)에서 한다");
    }
    if !std::io::stdin().is_terminal() {
        bail!("{what}: 사람이 터미널에서 실행해야 한다(표준 입력이 터미널이 아니다)");
    }
    Ok(())
}

/// 물음을 보이고 한 줄을 읽는다(앞뒤 빈칸 없이)
pub fn ask(prompt: &str) -> Result<String> {
    print!("{prompt}");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

/// `[y/N]` 물음: y·yes만 예다
pub fn confirm(prompt: &str) -> Result<bool> {
    Ok(matches!(ask(&format!("{prompt} [y/N] "))?.as_str(), "y" | "Y" | "yes"))
}
