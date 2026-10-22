#Requires -Version 5.1
<#
Bootstrap for a pinned official release. The published irm | iex entry pins version/SHA internally.
Service changes and installation approval belong to the native installer. See docs/windows-bootstrap.md.
InstallArgs is a string array (use & from PowerShell, not powershell -File).
#>
[CmdletBinding()]
param(
    [string]$Version,
    [string]$ExpectedBinarySha256,
    [AllowEmptyString()][string[]]$InstallArgs = @()
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Assert-AaaPath {
    param([string]$Path, [bool]$Directory)
    $cursor = $Path
    $leaf = $true
    while ($cursor) {
        # GetAttributes also observes reparse points rather than following them.
        try { $attributes = [IO.File]::GetAttributes($cursor) }
        catch [IO.FileNotFoundException] { $attributes = $null }
        catch [IO.DirectoryNotFoundException] { $attributes = $null }
        if ($null -ne $attributes) {
            if (($attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'bootstrap_unsafe_path' }
            $isDirectory = ($attributes -band [IO.FileAttributes]::Directory) -ne 0
            if ($isDirectory -ne ($Directory -or -not $leaf)) { throw 'bootstrap_unsafe_path' }
        }
        $leaf = $false
        $parent = [IO.Path]::GetDirectoryName($cursor)
        if ($parent -eq $cursor) { break }
        $cursor = $parent
    }
}

function ConvertTo-AaaCommandLine {
    param([AllowEmptyString()][string[]]$Arguments)
    # CRT/CommandLineToArgvW quoting, independent of PowerShell 5.1/7 native modes.
    $quoted = foreach ($argument in $Arguments) {
        if ($null -eq $argument -or $argument.Contains([char]0)) { throw 'bootstrap_invalid_argument' }
        $builder = New-Object Text.StringBuilder
        [void]$builder.Append('"')
        $slashes = 0
        foreach ($character in $argument.ToCharArray()) {
            if ($character -eq '\') { $slashes++; continue }
            if ($character -eq '"') {
                [void]$builder.Append(('\' * (2 * $slashes + 1)))
            } else {
                [void]$builder.Append(('\' * $slashes))
            }
            [void]$builder.Append($character)
            $slashes = 0
        }
        [void]$builder.Append(('\' * (2 * $slashes)))
        [void]$builder.Append('"')
        $builder.ToString()
    }
    return ($quoted -join ' ')
}

function Invoke-AaaNative {
    param([string]$Binary, [AllowEmptyString()][string[]]$Arguments, [switch]$Capture)
    $info = New-Object Diagnostics.ProcessStartInfo
    $info.FileName = $Binary
    $info.Arguments = ConvertTo-AaaCommandLine $Arguments
    $info.UseShellExecute = $false
    $info.CreateNoWindow = [bool]$Capture
    # Actual install inherits the external console and all three standard handles.
    # Only the version probe is captured; it cannot consume approval input.
    $info.RedirectStandardOutput = [bool]$Capture
    $info.RedirectStandardError = [bool]$Capture
    $process = New-Object Diagnostics.Process
    $process.StartInfo = $info
    try {
        if (-not $process.Start()) { throw 'bootstrap_start_failed' }
        if ($Capture) {
            $stdout = $process.StandardOutput.ReadToEndAsync()
            $stderr = $process.StandardError.ReadToEndAsync()
        }
        $process.WaitForExit()
        if ($Capture) {
            return @{ Code = $process.ExitCode; Output = $stdout.Result; Error = $stderr.Result }
        }
        return $process.ExitCode
    } finally { $process.Dispose() }
}

function Assert-AaaExecutable {
    param([string]$Path, [string]$ExpectedHash, [string]$ExpectedVersion)
    Assert-AaaPath $Path $false
    if ((Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash -ine $ExpectedHash) { throw 'bootstrap_checksum_mismatch' }
    $stream = [IO.File]::OpenRead($Path)
    $reader = New-Object IO.BinaryReader($stream)
    try {
        if ($stream.Length -lt 64 -or $reader.ReadUInt16() -ne 0x5a4d) { throw 'bootstrap_invalid_executable' }
        $stream.Position = 0x3c
        $offset = $reader.ReadUInt32()
        if ($offset -lt 64 -or $offset + 26 -gt $stream.Length) { throw 'bootstrap_invalid_executable' }
        $stream.Position = $offset
        if ($reader.ReadUInt32() -ne 0x4550 -or $reader.ReadUInt16() -ne 0x8664) { throw 'bootstrap_wrong_arch' }
        $stream.Position = $offset + 24
        if ($reader.ReadUInt16() -ne 0x20b) { throw 'bootstrap_wrong_arch' }
    } finally { $reader.Dispose() }
    $probe = Invoke-AaaNative $Path @('--version') -Capture
    if ($probe.Code -ne 0 -or $probe.Output.TrimEnd("`r", "`n") -cne "aaa $ExpectedVersion" -or $probe.Error.Length -ne 0) {
        throw 'bootstrap_version_mismatch'
    }
}

function Invoke-AaaBootstrap {
    param([string]$ReleaseVersion, [AllowEmptyString()][string[]]$Arguments = @(), [string]$ExpectedHash)
    $temporary = $null
    $stage = $null
    try {
        if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { throw 'bootstrap_windows_required' }
        $architecture = $env:PROCESSOR_ARCHITECTURE
        if ($env:PROCESSOR_ARCHITEW6432) { $architecture = $env:PROCESSOR_ARCHITEW6432 }
        if ($architecture -ine 'AMD64') { throw 'bootstrap_unsupported_arch' }
        if ($ReleaseVersion -cnotmatch '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$') { throw 'bootstrap_invalid_version' }
        if ($ExpectedHash -and $ExpectedHash -cnotmatch '^[a-fA-F0-9]{64}$') { throw 'bootstrap_invalid_checksum' }
        # Validate argv before placing anything, including embedded NULs.
        [void](ConvertTo-AaaCommandLine $Arguments)
        $profile = $env:USERPROFILE
        if (-not $profile -or $profile -notmatch '^[A-Za-z]:[\\/]') { throw 'bootstrap_unsafe_path' }
        $profile = [IO.Path]::GetFullPath($profile)
        Assert-AaaPath $profile $true
        if (-not [IO.Directory]::Exists($profile)) { throw 'bootstrap_unsafe_path' }
        $destination = [IO.Path]::Combine($profile, '.local', 'bin', 'aaa.exe')
        Assert-AaaPath $destination $false
        $base = "https://github.com/cLazyZombie/aaa/releases/download/v$ReleaseVersion"
        $assetUrl = "$base/aaa-windows-x86_64.exe"
        $temporary = [IO.Path]::Combine([IO.Path]::GetTempPath(), ('aaa-bootstrap-' + [Guid]::NewGuid().ToString('N')))
        if ([IO.Directory]::Exists($temporary) -or [IO.File]::Exists($temporary)) { throw 'bootstrap_unsafe_path' }
        [void][IO.Directory]::CreateDirectory($temporary)
        $manifestPath = [IO.Path]::Combine($temporary, 'manifest.json')
        Invoke-WebRequest -UseBasicParsing -Uri "$base/manifest.json" -OutFile $manifestPath | Out-Null
        $manifest = [IO.File]::ReadAllText($manifestPath) | ConvertFrom-Json
        if ($manifest.version -cne $ReleaseVersion) { throw 'bootstrap_manifest_mismatch' }
        $asset = $manifest.assets.'windows-x86_64'
        if ($asset.url -cne $assetUrl -or $asset.sha256 -cnotmatch '^[a-fA-F0-9]{64}$') { throw 'bootstrap_manifest_mismatch' }
        if ($ExpectedHash -and $asset.sha256 -ine $ExpectedHash) { throw 'bootstrap_checksum_mismatch' }
        $separateExecutor = $false
        # The one-line entry may coexist with an older aaa. Stage the verified new executor
        # separately; the Rust plan/approval owns all decisions about existing installations.
        if ($ExpectedHash -and [IO.File]::Exists($destination) -and
            (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash -ine $ExpectedHash) {
            $destination = [IO.Path]::Combine($profile, '.local', 'share', 'aaa', 'installers', $ExpectedHash.Substring(0, 16).ToLowerInvariant(), 'aaa.exe')
            Assert-AaaPath $destination $false
            $separateExecutor = $true
            [Console]::WriteLine('Existing aaa is preserved. Reviewing the installation with a separately verified executor.')
        }
        # Existing, different binaries are never overwritten (including running ones).
        if ([IO.File]::Exists($destination)) {
            Assert-AaaExecutable $destination $asset.sha256 $ReleaseVersion
        } else {
            $download = [IO.Path]::Combine($temporary, 'aaa.exe')
            Invoke-WebRequest -UseBasicParsing -Uri $assetUrl -OutFile $download | Out-Null
            Assert-AaaExecutable $download $asset.sha256 $ReleaseVersion
            Assert-AaaPath $destination $false
            $parent = [IO.Path]::GetDirectoryName($destination)
            [void][IO.Directory]::CreateDirectory($parent)
            Assert-AaaPath $parent $true
            $stage = [IO.Path]::Combine($parent, ('.aaa-bootstrap-' + [Guid]::NewGuid().ToString('N') + '.exe'))
            [IO.File]::Copy($download, $stage, $false)
            Assert-AaaExecutable $stage $asset.sha256 $ReleaseVersion
            # Same-volume, non-overwriting atomic rename. A concurrent winner stays.
            [IO.File]::Move($stage, $destination)
            $stage = $null
        }
        $installArguments = @('install') + $Arguments
        if ($separateExecutor -and -not @($Arguments | Where-Object { $_ -ceq '--bin-path' -or $_.StartsWith('--bin-path=') }).Count) {
            # The reviewed plan must use this executor, not overwrite/reject the older default binary.
            $installArguments += @('--bin-path', $destination)
        }
        return (Invoke-AaaNative $destination $installArguments)
    } catch {
        $code = $_.Exception.Message
        if ($code -cnotmatch '^bootstrap_[a-z_]+$') { $code = 'bootstrap_failed' }
        [Console]::Error.WriteLine($code)
        return 2
    } finally {
        if ($stage -and [IO.File]::Exists($stage)) { [IO.File]::Delete($stage) }
        if ($temporary -and [IO.Directory]::Exists($temporary)) {
            foreach ($name in @('manifest.json', 'aaa.exe')) {
                $file = [IO.Path]::Combine($temporary, $name)
                if ([IO.File]::Exists($file)) { [IO.File]::Delete($file) }
            }
            [IO.Directory]::Delete($temporary)
        }
    }
}

# Dot-sourcing provides the same entry for a deployment script without exiting its shell.
if ($MyInvocation.InvocationName -ne '.') {
    $code = Invoke-AaaBootstrap -ReleaseVersion $Version -Arguments $InstallArgs -ExpectedHash $ExpectedBinarySha256
    # Invoke-Expression must return to the user's console; a saved script preserves its process exit code.
    if ($MyInvocation.MyCommand -is [Management.Automation.ExternalScriptInfo]) { exit $code }
    $global:LASTEXITCODE = $code
}
