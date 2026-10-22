# Invoked by cargo test --test windows_bootstrap on Windows; no real network/SCM.
param([string]$Bootstrap, [string]$Fixture, [string]$Aaa, [string]$AaaVersion, [string]$Lab)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. $Bootstrap
function Assert([bool]$Condition, [string]$Message) { if (-not $Condition) { throw $Message } }
$script:base = 'https://github.com/cLazyZombie/aaa/releases/download/v1.2.3'
$script:payload = $Fixture
$script:manifestVersion = '1.2.3'
$script:url = "$script:base/aaa-windows-x86_64.exe"
$script:digest = (Get-FileHash -LiteralPath $Fixture -Algorithm SHA256).Hash
$script:downloads = 0
# The external HTTPS boundary alone is replaced; files/PE/process/argv are real.
function Invoke-WebRequest {
    param([switch]$UseBasicParsing, [string]$Uri, [string]$OutFile)
    if ($Uri -ceq "$script:base/manifest.json") {
        $asset = @{ url = $script:url; sha256 = $script:digest }
        $manifest = @{ version = $script:manifestVersion; assets = @{ 'windows-x86_64' = $asset } }
        [IO.File]::WriteAllText($OutFile, ($manifest | ConvertTo-Json -Depth 5))
    } elseif ($Uri -ceq "$script:base/aaa-windows-x86_64.exe") {
        $script:downloads++
        [IO.File]::Copy($script:payload, $OutFile, $false)
    } else { throw 'unexpected network request' }
}
function New-Profile([string]$Name) {
    $profilePath = Join-Path $Lab $Name
    [void][IO.Directory]::CreateDirectory($profilePath)
    $env:USERPROFILE = $profilePath
    $env:AAA_CONFIG_DIR = Join-Path $profilePath 'config'
    $env:AAA_STATE_DIR = Join-Path $profilePath 'state'
    $env:AAA_INSTANCE = 'bootstrap-test'
    return (Join-Path $profilePath '.local\bin\aaa.exe')
}
$env:AAA_BOOTSTRAP_VERSION = '1.2.3'
$env:AAA_BOOTSTRAP_ARGV = Join-Path $Lab 'argv.txt'
$target = New-Profile ('user space ' + [char]0xd55c + [char]0xae00)
$unicode = ([char]0xd55c).ToString() + [char]0xae00
$arguments = @('--read-stdin', '', 'two words', 'quote"middle', 'C:\ends with space\', '\\"', '&|<>^%$();', $unicode, "a`nb")
Assert ((Invoke-AaaBootstrap '1.2.3' $arguments) -eq 23) 'child exit must be preserved'
$expected = (@('install') + $arguments | ForEach-Object {
    ([BitConverter]::ToString([Text.Encoding]::UTF8.GetBytes($_))).Replace('-', '').ToLowerInvariant() + "`n"
}) -join ''
Assert ([IO.File]::ReadAllText($env:AAA_BOOTSTRAP_ARGV) -ceq $expected) 'native argv changed'
Assert ((Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash -ceq $script:digest) 'verified placement'
[IO.File]::SetLastWriteTimeUtc($target, [DateTime]'2001-01-01T00:00:00Z')
$mtime = [IO.File]::GetLastWriteTimeUtc($target)
$count = $script:downloads
Assert ((Invoke-AaaBootstrap '1.2.3' @('unchanged')) -eq 23) 'identical reuse'
Assert ([IO.File]::GetLastWriteTimeUtc($target) -eq $mtime) 'same binary mtime changed'
Assert ($script:downloads -eq $count) 'same binary downloaded again'

# Negative control at the real entry: naive joining must corrupt native argv.
$quoteOwner = (Get-Item Function:\ConvertTo-AaaCommandLine).ScriptBlock
try {
    Set-Item Function:\ConvertTo-AaaCommandLine { param([string[]]$Arguments) return ($Arguments -join ' ') }
    $mutantTarget = New-Profile 'mutant-argv'
    Assert ((Invoke-AaaBootstrap '1.2.3' @('two words', '', 'quote"middle')) -eq 23) 'mutation did not reach native argv'
    $mutantOutput = [IO.File]::ReadAllText($env:AAA_BOOTSTRAP_ARGV)
    $intact = "696e7374616c6c`n74776f20776f726473`n`n71756f7465226d6964646c65`n"
    Assert ($mutantOutput -cne $intact) 'native owner missed naive quoting regression'
} finally { Set-Item Function:\ConvertTo-AaaCommandLine $quoteOwner }

# Rejections retain target bytes/mtime and unrelated files, never execute payload.
foreach ($case in @('version', 'url', 'hash', 'pe', 'machine', 'binary-version', 'existing', 'parent-file', 'arch', 'nul')) {
    $target = New-Profile ('reject-' + $case)
    $other = Join-Path $env:USERPROFILE 'other-instance.txt'
    [IO.File]::WriteAllText($other, 'keep this instance')
    $script:manifestVersion = '1.2.3'
    $script:url = "$script:base/aaa-windows-x86_64.exe"
    $script:payload = $Fixture
    $script:digest = (Get-FileHash -LiteralPath $Fixture -Algorithm SHA256).Hash
    $env:AAA_BOOTSTRAP_VERSION = '1.2.3'
    $argv = @('must-not-run')
    $arch = $env:PROCESSOR_ARCHITECTURE
    $wow = $env:PROCESSOR_ARCHITEW6432
    switch ($case) {
        'version' { $script:manifestVersion = '1.2.4' }
        'url' { $script:url = 'https://example.invalid/evil.exe' }
        'hash' { $script:digest = '0' * 64 }
        'pe' {
            $script:payload = Join-Path $Lab 'not-pe.exe'
            [IO.File]::WriteAllText($script:payload, 'not executable')
            $script:digest = (Get-FileHash -LiteralPath $script:payload -Algorithm SHA256).Hash
        }
        'machine' {
            $script:payload = Join-Path $Lab 'wrong-machine.exe'
            $bytes = [IO.File]::ReadAllBytes($Fixture)
            $offset = [BitConverter]::ToInt32($bytes, 0x3c)
            $bytes[$offset + 4] = 0x64; $bytes[$offset + 5] = 0xaa
            [IO.File]::WriteAllBytes($script:payload, $bytes)
            $script:digest = (Get-FileHash -LiteralPath $script:payload -Algorithm SHA256).Hash
        }
        'binary-version' { $env:AAA_BOOTSTRAP_VERSION = '1.2.4' }
        'existing' {
            [void][IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($target))
            [IO.File]::WriteAllText($target, 'unrelated existing exe')
            [IO.File]::SetLastWriteTimeUtc($target, $mtime)
        }
        'parent-file' { [IO.File]::WriteAllText((Join-Path $env:USERPROFILE '.local'), 'not directory') }
        'arch' { $env:PROCESSOR_ARCHITECTURE = 'ARM64'; $env:PROCESSOR_ARCHITEW6432 = '' }
        'nul' { $argv = @('bad' + [char]0 + 'argument') }
    }
    [IO.File]::Delete($env:AAA_BOOTSTRAP_ARGV)
    try { $result = Invoke-AaaBootstrap '1.2.3' $argv }
    finally { $env:PROCESSOR_ARCHITECTURE = $arch; $env:PROCESSOR_ARCHITEW6432 = $wow }
    Assert ($result -eq 2) "rejection did not fail: $case"
    Assert (-not [IO.File]::Exists($env:AAA_BOOTSTRAP_ARGV)) "rejected payload executed: $case"
    Assert ([IO.File]::ReadAllText($other) -ceq 'keep this instance') "other instance touched: $case"
    if ($case -eq 'existing') {
        Assert ([IO.File]::ReadAllText($target) -ceq 'unrelated existing exe') 'existing file overwritten'
        Assert ([IO.File]::GetLastWriteTimeUtc($target) -eq $mtime) 'existing mtime touched'
    } else { Assert (-not [IO.File]::Exists($target)) "rejected binary placed: $case" }
}

# Junctions must be rejected without writing into their destination.
$target = New-Profile 'junction'
$outside = Join-Path $Lab 'outside'
[void][IO.Directory]::CreateDirectory($outside)
New-Item -ItemType Junction -Path (Join-Path $env:USERPROFILE '.local') -Target $outside | Out-Null
try {
    Assert ((Invoke-AaaBootstrap '1.2.3' @()) -eq 2) 'junction accepted'
    Assert (@([IO.Directory]::GetFileSystemEntries($outside)).Count -eq 0) 'junction target changed'
} finally { [IO.Directory]::Delete((Join-Path $env:USERPROFILE '.local')) }

# Actual aaa, not a fixture, still owns dry-run and denies --yes as authority.
$target = New-Profile 'real-aaa'
$script:base = "https://github.com/cLazyZombie/aaa/releases/download/v$AaaVersion"
$script:manifestVersion = $AaaVersion
$script:url = "$script:base/aaa-windows-x86_64.exe"
$script:payload = $Aaa
$script:digest = (Get-FileHash -LiteralPath $Aaa -Algorithm SHA256).Hash
$common = @('--role', 'agent', '--name', 'windows-a', '--host', 'example.ts.net:7430', '--network', 'direct',
    '--enroll', 'skip', '--work-root', (Join-Path $env:USERPROFILE 'work'), '--work-storage', 'internal',
    '--harness', 'none', '--service', 'none', '--start', 'deferred', '--non-interactive', '--json')
$result = Invoke-AaaBootstrap $AaaVersion ($common + @('--dry-run'))
Assert ($result -eq 0) 'real aaa dry-run failed'
Assert (-not [IO.Directory]::Exists($env:AAA_STATE_DIR)) 'dry-run wrote state'
$result = Invoke-AaaBootstrap $AaaVersion ($common + @('--yes'))
Assert ($result -eq 3) '--yes granted authority'
Assert (-not [IO.Directory]::Exists($env:AAA_STATE_DIR)) 'unauthorized install wrote state'
Write-Output 'bootstrap-owner-ok'
