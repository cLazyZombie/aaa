//! Windows OpenSSH(cmd/PowerShell 5.1 포함): 고정 명령만 UTF-16LE로 싣고 binary·비밀 stdin은 .NET 원시 stream으로 넘긴다.
use base64::Engine as _;

pub(super) fn command(body: &str) -> String {
    let bytes: Vec<_> = body.encode_utf16().flat_map(u16::to_le_bytes).collect();
    format!(
        "powershell -NoProfile -NonInteractive -EncodedCommand {}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

/// .NET Framework ConsoleStream은 OpenSSH의 overlapped stdin pipe에서 멈춘다.
/// PowerShell SSHProcessMediator처럼 표준 핸들을 FileStream으로 연다:
/// https://github.com/PowerShell/PowerShell/issues/14478#issuecomment-1064764004
/// 프로세스 소유 표준 핸들은 닫지 않는다. 입력 내용은 Add-Type 소스에 들어가지 않는다.
const STDIN: &str = r#"
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class AaaSetupNative {
    [DllImport("kernel32.dll", SetLastError=true)]
    public static extern IntPtr GetStdHandle(int kind);
}
'@
$handle = [Microsoft.Win32.SafeHandles.SafeFileHandle]::new([AaaSetupNative]::GetStdHandle(-10), $false)
if ($handle.IsInvalid) { throw 'stdin handle unavailable' }
$stdinStream = [IO.FileStream]::new($handle, [IO.FileAccess]::Read)
"#;

fn command_with_stdin(body: &str) -> String {
    command(&format!("$ErrorActionPreference='Stop'\n{STDIN}\n{body}"))
}

/// host에는 임시 폴더의 무작위 이름만 돌려준다. 경로·사용자 이름을 shell 글로 보간하지 않는다.
pub(super) fn safe_dir(dir: &str) -> bool {
    dir.strip_prefix("aaa-setup.").is_some_and(|s| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit()))
}

pub(super) fn upload() -> String {
    command_with_stdin(
        r#"
$ErrorActionPreference = 'Stop'
$name = 'aaa-setup.' + [Guid]::NewGuid().ToString('N')
$dir = Join-Path ([IO.Path]::GetTempPath()) $name
[void][IO.Directory]::CreateDirectory($dir)
$exe = Join-Path $dir 'aaa.exe'
$file = [IO.File]::Create($exe)
try { $stdinStream.CopyTo($file) } finally { $file.Dispose(); $stdinStream.Dispose() }
Write-Output 'AAA-UP-1'
Write-Output $name
Write-Output ((Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLowerInvariant())
& $exe --version
Write-Output 'AAA-UP-END'
"#,
    )
}

pub(super) fn cleanup(dir: &str) -> Option<String> {
    safe_dir(dir).then(|| command(&format!(
        "$ErrorActionPreference='Stop'; $dir=Join-Path ([IO.Path]::GetTempPath()) '{dir}'; Remove-Item -LiteralPath $dir -Recurse -Force"
    )))
}

/// PowerShell의 text pipeline을 거치지 않는다. stdin EOF까지 원시 bytes를 넘긴 뒤 child exit를 그대로 낸다.
pub(super) fn install(dir: &str, args: &str) -> Option<String> {
    install_at(dir, args, "aaa")
}

#[cfg(test)]
pub(super) fn install_instance(dir: &str, args: &str, instance: &str) -> Option<String> {
    aaa_config::paths::check_instance(instance)
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
        .ok()?;
    install_at(dir, &format!("--instance {instance} {args}"), &format!("aaa-{instance}"))
}

fn install_at(dir: &str, args: &str, state_name: &str) -> Option<String> {
    safe_dir(dir).then(|| {
        command_with_stdin(&format!(
            r#"
$ErrorActionPreference = 'Stop'
$exe = Join-Path (Join-Path ([IO.Path]::GetTempPath()) '{dir}') 'aaa.exe'
$base = $env:LOCALAPPDATA
if ([string]::IsNullOrEmpty($base)) {{ $base = $env:USERPROFILE }}
$state = Join-Path $base '{state_name}\state'
$p = New-Object Diagnostics.Process
$p.StartInfo.FileName = $exe
# install-probe/bootstrap은 명시적 상태 경로가 필수다. 모든 단계가 detect의 기본 자리와 같아야 한다.
$p.StartInfo.Arguments = '--state-dir "' + $state + '" {args}'
$p.StartInfo.UseShellExecute = $false
$p.StartInfo.CreateNoWindow = $true
$p.StartInfo.RedirectStandardInput = $true
[void]$p.Start()
try {{ $stdinStream.CopyTo($p.StandardInput.BaseStream) }} finally {{ $p.StandardInput.Close(); $stdinStream.Dispose() }}
$p.WaitForExit()
exit $p.ExitCode
"#
        ))
    })
}

pub(super) fn path(path: &str) -> bool {
    let p = path.replace('\\', "/");
    let bytes = p.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && &bytes[1..3] == b":/"
        && p.len() <= 4096
        && !p.chars().any(char::is_control)
        && !p[2..].contains(':')
        && !p.contains(['"', '<', '>', '|', '*', '?'])
        && !p.split('/').any(|c| c == ".." || c == ".")
}
