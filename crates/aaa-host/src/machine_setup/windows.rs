//! Windows OpenSSH(cmd/PowerShell 5.1 포함): 고정 명령만 UTF-16LE로 싣고 binary·비밀 stdin은 .NET 원시 stream으로 넘긴다.
use base64::Engine as _;

pub(super) fn command(body: &str) -> String {
    let bytes: Vec<_> = body.encode_utf16().flat_map(u16::to_le_bytes).collect();
    format!(
        "powershell -NoProfile -NonInteractive -EncodedCommand {}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

/// host에는 임시 폴더의 무작위 이름만 돌려준다. 경로·사용자 이름을 shell 글로 보간하지 않는다.
pub(super) fn safe_dir(dir: &str) -> bool {
    dir.strip_prefix("aaa-setup.").is_some_and(|s| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit()))
}

pub(super) fn upload() -> String {
    command(
        r#"
$ErrorActionPreference = 'Stop'
$name = 'aaa-setup.' + [Guid]::NewGuid().ToString('N')
$dir = Join-Path ([IO.Path]::GetTempPath()) $name
[void][IO.Directory]::CreateDirectory($dir)
$exe = Join-Path $dir 'aaa.exe'
$file = [IO.File]::Create($exe)
try { [Console]::OpenStandardInput().CopyTo($file) } finally { $file.Dispose() }
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
    safe_dir(dir).then(|| {
        command(&format!(
            r#"
$ErrorActionPreference = 'Stop'
$exe = Join-Path (Join-Path ([IO.Path]::GetTempPath()) '{dir}') 'aaa.exe'
$p = New-Object Diagnostics.Process
$p.StartInfo.FileName = $exe
$p.StartInfo.Arguments = '{args}'
$p.StartInfo.UseShellExecute = $false
$p.StartInfo.CreateNoWindow = $true
$p.StartInfo.RedirectStandardInput = $true
[void]$p.Start()
try {{ [Console]::OpenStandardInput().CopyTo($p.StandardInput.BaseStream) }} finally {{ $p.StandardInput.Close() }}
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
