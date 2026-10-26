# 비특권 실행: 서비스·사용자·SSH·설치를 건드리지 않고 실제 자식의 argv/env/파일 입력을 검사한다.
param([string]$LabRoot = [IO.Path]::GetTempPath())
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-sshd-test-command.ps1')
$lab = Join-Path $LabRoot ('aaa-test-m4-command-' + [Guid]::NewGuid().ToString('N'))
$repo = Join-Path $lab 'repo with spaces'
$savedPath = $env:PATH
try {
    [void][IO.Directory]::CreateDirectory((Join-Path $repo 'target\e2e'))
    [void][IO.Directory]::CreateDirectory((Join-Path $lab 'tools'))
    # cargo 경계만 native PE로 대체한다. Process.Start와 Windows의 argv/env 전달은 실제로 실행한다.
    $source = Join-Path $lab 'cargo.rs'
    [IO.File]::WriteAllText($source, @'
use std::{env, fs, path::PathBuf};
fn main() {
    let args: Vec<_> = env::args().skip(1).collect();
    assert_eq!(args, ["test", "--profile", "verify", "--locked", "-p", "aaa-host", "--lib",
        "machine_setup::tests_windows::system_sshd_installs_a_current_user_scm_agent_and_registers_it",
        "--", "--ignored", "--exact", "--nocapture"]);
    let root = env::current_dir().unwrap();
    let binary = PathBuf::from(env::var_os("AAA_WINDOWS_SSH_BINARY").unwrap());
    assert!(binary.is_absolute());
    assert_eq!(binary, root.join("target/e2e/aaa.exe"));
    assert_eq!(fs::read(binary).unwrap(), b"fresh e2e bytes");
    assert!(!root.join("target/debug").exists());
    assert_eq!(PathBuf::from(env::var_os("CARGO_TARGET_DIR").unwrap()), root.join("target"));
    assert!(env::var_os("AAA_SESSION_ID").is_none());
    assert!(env::var_os("AAA_HOOK_SOCK").is_none());
    println!("M4 command ok: profile=verify binary=target/e2e/aaa.exe debug_absent=true");
}
'@)
    & rustc.exe --edition 2021 --crate-name m4_command $source -o (Join-Path $lab 'tools\cargo.exe')
    if ($LASTEXITCODE -ne 0) { throw "M4 command fixture build: exit=$LASTEXITCODE" }
    $env:PATH = (Join-Path $lab 'tools') + ';' + $savedPath
    $binary = Join-Path $repo 'target\e2e\aaa.exe'
    [IO.File]::WriteAllText($binary, 'fresh e2e bytes')
    $test = New-WindowsSshTestProcess $repo
    try {
        [void]$test.Start()
        $stdout = $test.StandardOutput.ReadToEndAsync()
        $stderr = $test.StandardError.ReadToEndAsync()
        $test.StandardInput.Close()
        if (-not $test.WaitForExit(30000)) { $test.Kill(); $test.WaitForExit(); throw 'M4 command fixture timeout' }
        $output = $stdout.GetAwaiter().GetResult()
        $errors = $stderr.GetAwaiter().GetResult()
        if ($test.ExitCode -ne 0) { throw "M4 command fixture: exit=$($test.ExitCode) $errors" }
        if ($output.Trim() -ne 'M4 command ok: profile=verify binary=target/e2e/aaa.exe debug_absent=true') {
            throw "M4 command fixture output: $output"
        }
        Write-Host $output.Trim()
    } finally { $test.Dispose() }
    # 옛 debug 캐시가 남아도 e2e가 없으면 자원 생성에 앞서 멈춰야 한다.
    Remove-Item -LiteralPath $binary
    [void][IO.Directory]::CreateDirectory((Join-Path $repo 'target\debug'))
    [IO.File]::WriteAllText((Join-Path $repo 'target\debug\aaa.exe'), 'stale debug bytes')
    $rejected = $false
    try { $unexpected = New-WindowsSshTestProcess $repo; $unexpected.Dispose() } catch {
        if ($_.Exception.Message -notlike '*stage=preflight*binary_profile=e2e*test_profile=verify*exit=not-started*') { throw }
        $rejected = $true
    }
    if (-not $rejected) { throw 'M4 command accepted a stale debug cache without e2e binary' }
    Write-Host 'M4 command ok: missing e2e rejected before resources despite stale debug cache'
} finally {
    $env:PATH = $savedPath
    if (Test-Path -LiteralPath $lab) { Remove-Item -LiteralPath $lab -Recurse -Force }
}
