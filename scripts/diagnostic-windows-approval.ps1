param(
    [Parameter(Mandatory=$true)][string]$TestBinary,
    [Parameter(Mandatory=$true)][string]$Output,
    [ValidateRange(1,32)][int]$Waves = 4,
    [ValidateRange(1,8)][int]$Parallel = 8
)
$ErrorActionPreference = 'Stop'
[System.Diagnostics.Process]::GetCurrentProcess().PriorityClass = 'BelowNormal'
$test = (Resolve-Path -LiteralPath $TestBinary).Path
if (Test-Path -LiteralPath $Output) { throw '진단 출력 폴더가 이미 있습니다. 기존 원문을 덮지 않습니다.' }
$root = (New-Item -ItemType Directory -Path $Output).FullName
New-Item -ItemType Directory -Path "$root\tmp" | Out-Null
$env:AAA_PTY_DIAGNOSTIC_DIR = "$root\trace"
$env:AAA_PTY_NATIVE_STACK = '1'
$env:RUST_TEST_THREADS = '1'
$env:RUST_BACKTRACE = '0'
# 이 진단은 한 test binary process마다 --exact 한 건만 실행한다.
# 다른 panic/backtrace와 DbgHelp를 함께 사용하지 않는다.
$proofInfo = New-Object System.Diagnostics.ProcessStartInfo
$proofInfo.FileName = $test
$proofInfo.Arguments = 'authorization::diagnostic_conpty_retains_keys_before_the_first_cooked_read --exact --ignored --nocapture'
$proofInfo.UseShellExecute = $false
$proofInfo.CreateNoWindow = $true
$proofInfo.RedirectStandardOutput = $true
$proofInfo.RedirectStandardError = $true
$proofInfo.StandardOutputEncoding = New-Object System.Text.UTF8Encoding
$proofInfo.StandardErrorEncoding = New-Object System.Text.UTF8Encoding
$proof = New-Object System.Diagnostics.Process
$proof.StartInfo = $proofInfo
if (!$proof.Start()) { throw 'native proof child start failed' }
$proofOut = $proof.StandardOutput.ReadToEndAsync()
$proofErr = $proof.StandardError.ReadToEndAsync()
$proof.WaitForExit()
$proofOut.GetAwaiter().GetResult() | Set-Content -Encoding UTF8 "$root\proof.stdout"
$proofErr.GetAwaiter().GetResult() | Set-Content -Encoding UTF8 "$root\proof.stderr"
if ($proof.ExitCode -ne 0) { throw "native capture proof failed: $($proof.ExitCode)" }
$proof.Dispose()
$results = @()
for ($wave = 0; $wave -lt $Waves; $wave++) {
    $running = @()
    for ($slot = 0; $slot -lt $Parallel; $slot++) {
        $info = New-Object System.Diagnostics.ProcessStartInfo
        $info.FileName = $test
        $info.Arguments = 'authorization::external_tty_reviews_the_same_real_plan_and_issues_private_bound_authorization --exact --nocapture'
        $info.WorkingDirectory = Split-Path -Parent $test
        $info.UseShellExecute = $false
        $info.CreateNoWindow = $true
        $info.RedirectStandardOutput = $true
        $info.RedirectStandardError = $true
        $info.StandardOutputEncoding = New-Object System.Text.UTF8Encoding
        $info.StandardErrorEncoding = New-Object System.Text.UTF8Encoding
        $info.EnvironmentVariables['TEMP'] = "$root\tmp"
        $info.EnvironmentVariables['TMP'] = "$root\tmp"
        $process = New-Object System.Diagnostics.Process
        $process.StartInfo = $info
        if (!$process.Start()) { throw 'owned diagnostic child start failed' }
        $running += [pscustomobject]@{
            Process = $process
            Stdout = $process.StandardOutput.ReadToEndAsync()
            Stderr = $process.StandardError.ReadToEndAsync()
            Started = [DateTime]::UtcNow
            Slot = $slot
        }
    }
    $failed = $false
    foreach ($owned in $running) {
        $owned.Process.WaitForExit()
        $out = $owned.Stdout.GetAwaiter().GetResult()
        $err = $owned.Stderr.GetAwaiter().GetResult()
        $code = $owned.Process.ExitCode
        $elapsed = ([DateTime]::UtcNow - $owned.Started).TotalSeconds
        $id = "$wave-$($owned.Slot)"
        $out | Set-Content -Encoding UTF8 "$root\trial-$id.stdout"
        $err | Set-Content -Encoding UTF8 "$root\trial-$id.stderr"
        $results += [pscustomobject]@{ Wave=$wave; Slot=$owned.Slot; Exit=$code; Wall=$elapsed }
        $owned.Process.Dispose()
        if ($code -ne 0) { $failed = $true }
    }
    $results | ConvertTo-Json | Set-Content -Encoding UTF8 "$root\trials.json"
    Write-Output "wave $wave complete: failed=$failed"
    if ($failed) { exit 1 }
}
