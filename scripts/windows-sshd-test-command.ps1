# 자원 생성 없이 M4의 빌드 입력과 시험 자식을 준비한다. CI와 비특권 회귀가 같은 경계를 쓴다.
function New-WindowsSshTestProcess([string]$RepositoryRoot) {
    $root = (Resolve-Path -LiteralPath $RepositoryRoot).Path
    $binary = Join-Path $root 'target\e2e\aaa.exe'
    if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) {
        throw "M4 stage=preflight binary_profile=e2e test_profile=verify binary=$binary exit=not-started: 실행 파일이 없다. ci-local build를 먼저 실행한다"
    }
    $test = New-Object Diagnostics.Process
    $test.StartInfo.FileName = (Get-Command cargo.exe -ErrorAction Stop).Source
    $test.StartInfo.WorkingDirectory = $root
    $test.StartInfo.Arguments = 'test --profile verify --locked -p aaa-host --lib machine_setup::tests_windows::system_sshd_installs_a_current_user_scm_agent_and_registers_it -- --ignored --exact --nocapture'
    $test.StartInfo.UseShellExecute = $false
    $test.StartInfo.RedirectStandardInput = $true
    $test.StartInfo.RedirectStandardOutput = $true
    $test.StartInfo.RedirectStandardError = $true
    # ci-local과 같은 worktree target을 쓴다. 외부 CARGO_TARGET_DIR의 옛 산출물을 읽거나 갱신하지 않는다.
    $test.StartInfo.EnvironmentVariables['CARGO_TARGET_DIR'] = Join-Path $root 'target'
    $test.StartInfo.EnvironmentVariables['AAA_WINDOWS_SSH_BINARY'] = (Resolve-Path -LiteralPath $binary).Path
    foreach ($name in @('AAA_SESSION_ID', 'AAA_HOOK_SOCK')) { $test.StartInfo.EnvironmentVariables.Remove($name) }
    return $test
}
