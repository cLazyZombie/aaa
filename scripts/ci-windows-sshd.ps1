# Windows GitHub Actions 일회성 runner 전용. 운영 설정·계정·기존 sshd 서비스는 바꾸지 않는다.
# SYSTEM의 System32/OpenSSH/sshd.exe를 임시 서비스로 띄우고 일회 계정 SCM 설치를 본다.
# 가짜 계정 암호는 부모 메모리 → 시험 자식 anonymous stdin으로만 전달한다. argv/env/파일/로그 금지.
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:OS -ne 'Windows_NT') { throw '일회성 Windows CI 전용이다' }
$sshd = Join-Path $env:SystemRoot 'System32\OpenSSH\sshd.exe'
if (-not (Test-Path -LiteralPath $sshd)) { throw '시스템 sshd.exe가 없다: M4 실제 owner를 수행할 수 없다' }
if (Get-Service -Name 'aaa-agent' -ErrorAction SilentlyContinue) { throw '기존 aaa-agent 서비스가 있어 시험하지 않는다' }
$tag = [Guid]::NewGuid().ToString('N').Substring(0, 10)
$lab = Join-Path $env:RUNNER_TEMP ('aaa-ssh-' + $tag)
$user = 'aaa-ci-' + $tag
$service = 'aaa-ssh-' + $tag
[void][IO.Directory]::CreateDirectory($lab)
# 복잡도 조건을 충족하는 실행마다 다른 가짜 암호. 값을 출력하거나 GitHub env/output으로 넘기지 않는다.
$password = 'Aa1!' + [Guid]::NewGuid().ToString('N') + [Guid]::NewGuid().ToString('N')
$secure = ConvertTo-SecureString -String $password -AsPlainText -Force
$createdUser = $false
$createdService = $false
try {
    [void](New-LocalUser -Name $user -Password $secure -PasswordNeverExpires -AccountNeverExpires)
    $createdUser = $true
    $admins = Get-LocalGroup -SID 'S-1-5-32-544'
    Add-LocalGroupMember -Group $admins -Member $user
    foreach ($key in @('hostkey', 'userkey')) {
        # ProcessStartInfo는 PS5.1도 빈 argv를 보존한다.
        $keygen = New-Object Diagnostics.Process
        $keygen.StartInfo.FileName = (Get-Command ssh-keygen.exe).Source
        $keygen.StartInfo.Arguments = '-q -t ed25519 -N "" -f "' + (Join-Path $lab $key) + '"'
        $keygen.StartInfo.UseShellExecute = $false
        [void]$keygen.Start(); $keygen.WaitForExit()
        if ($keygen.ExitCode -ne 0) { throw '실험실 SSH 키 생성 실패' }
    }
    Copy-Item -LiteralPath (Join-Path $lab 'userkey.pub') -Destination (Join-Path $lab 'authorized_keys')
    $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
    $listener.Start(); $port = $listener.LocalEndpoint.Port; $listener.Stop()
    $path = $lab.Replace('\', '/')
    $config = @"
ListenAddress 127.0.0.1:$port
HostKey "$path/hostkey"
AuthorizedKeysFile "$path/authorized_keys"
AllowUsers $user
PasswordAuthentication no
KbdInteractiveAuthentication no
StrictModes no
"@
    [IO.File]::WriteAllText((Join-Path $lab 'sshd_config'), $config, [Text.UTF8Encoding]::new($false))
    [IO.File]::WriteAllText((Join-Path $lab 'ssh_config'), @"
Host ci
  HostName 127.0.0.1
  Port $port
  User $user
  IdentityFile "$path/userkey"
  IdentitiesOnly yes
"@, [Text.UTF8Encoding]::new($false))
    # host private key는 SYSTEM/Administrators만 읽는다. 일회 사용자에는 공개 인증 키/폴더 읽기만 준다.
    & icacls.exe $lab /inheritance:r /grant:r '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-544:(OI)(CI)F' | Out-Null
    if ($LASTEXITCODE -ne 0) { throw '실험실 ACL 준비 실패' }
    & icacls.exe (Join-Path $lab 'hostkey') /setowner '*S-1-5-18' | Out-Null
    if ($LASTEXITCODE -ne 0) { throw '시스템 host key 소유권 준비 실패' }
    & $sshd -t -f (Join-Path $lab 'sshd_config')
    if ($LASTEXITCODE -ne 0) { throw '시스템 sshd 설정 검사 실패' }
    [void](New-Service -Name $service -BinaryPathName ('"' + $sshd + '" -f "' + (Join-Path $lab 'sshd_config') + '"') -StartupType Manual)
    $createdService = $true
    Start-Service -Name $service
    (Get-Service -Name $service).WaitForStatus('Running', [TimeSpan]::FromSeconds(30))
    Write-Host 'M4 SYSTEM OpenSSH 실제 owner 시작'
    $test = New-Object Diagnostics.Process
    $test.StartInfo.FileName = (Get-Command cargo.exe).Source
    $test.StartInfo.Arguments = 'test --locked -p aaa-host --lib machine_setup::tests_windows::system_sshd_installs_a_current_user_scm_agent_and_registers_it -- --ignored --exact --nocapture'
    $test.StartInfo.UseShellExecute = $false
    $test.StartInfo.RedirectStandardInput = $true
    $test.StartInfo.RedirectStandardOutput = $true
    $test.StartInfo.RedirectStandardError = $true
    $test.StartInfo.EnvironmentVariables['AAA_WINDOWS_SSH_LAB'] = $lab
    $test.StartInfo.EnvironmentVariables['AAA_WINDOWS_SSH_BINARY'] = (Resolve-Path 'target/debug/aaa.exe').Path
    foreach ($name in @('AAA_SESSION_ID', 'AAA_HOOK_SOCK')) { $test.StartInfo.EnvironmentVariables.Remove($name) }
    [void]$test.Start()
    $out = $test.StandardOutput.ReadToEndAsync()
    $err = $test.StandardError.ReadToEndAsync()
    $test.StandardInput.Write($password)
    $test.StandardInput.Close()
    if (-not $test.WaitForExit(900000)) { $test.Kill(); $test.WaitForExit(); throw 'M4 실제 owner 시한 초과' }
    # 결함이 있어도 가짜 암호를 CI 로그/대화로 내보내지 않는다.
    $stdout = $out.GetAwaiter().GetResult()
    $stderr = $err.GetAwaiter().GetResult()
    if ($stdout.Contains($password) -or $stderr.Contains($password)) { throw 'M4 owner 출력에 보호 입력 누출' }
    [Console]::Out.Write($stdout)
    [Console]::Error.Write($stderr)
    if ($test.ExitCode -ne 0) { throw 'M4 SYSTEM sshd·SCM owner 실패' }
} finally {
    # 이 시험 전에는 없었고 일회 계정 소유인 서비스만 정리한다.
    $agent = Get-CimInstance Win32_Service -Filter "Name='aaa-agent'" -ErrorAction SilentlyContinue
    if ($agent -and $agent.StartName.EndsWith('\' + $user, [StringComparison]::OrdinalIgnoreCase)) {
        Stop-Service -Name 'aaa-agent' -ErrorAction SilentlyContinue
        & sc.exe delete aaa-agent | Out-Null
    }
    if ($createdService) { Stop-Service -Name $service -ErrorAction SilentlyContinue; & sc.exe delete $service | Out-Null }
    if ($createdUser) { Remove-LocalUser -Name $user }
    $password = $null
    $secure.Dispose()
}
