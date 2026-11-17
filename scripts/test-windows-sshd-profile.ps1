# OS 자료 경계만 대체한다. 운영 계정·서비스·파일은 변경하지 않는다.
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-sshd-test-profile.ps1')
$user = [pscustomobject]@{ Name = 'aaa-ci-0123456789'; SID = @{ Value = 'S-1-5-21-111-222-333-1001' } }
$script:reads = 0
$script:removed = @()
$script:profile = [pscustomobject]@{ SID = $user.SID.Value; LocalPath = 'C:\Users\aaa-ci-0123456789'; Special = $false; Loaded = $false }
function Get-CimInstance($ClassName, $Filter, $ErrorAction) {
    if ($ClassName -ne 'Win32_UserProfile' -or $Filter -ne "SID='$($user.SID.Value)'") { throw '잘못된 OS 조회' }
    $script:reads++
    return $script:profile
}
function Remove-CimInstance($InputObject, $ErrorAction) { $script:removed += $InputObject }
Remove-WindowsSshTestProfile $user
if ($script:removed.Count -ne 1 -or ![object]::ReferenceEquals($script:removed[0], $script:profile)) {
    throw '확인한 한 프로필만 삭제해야 한다'
}
foreach ($field in @('SID', 'LocalPath', 'Special', 'Loaded')) {
    $original = $script:profile.$field
    $script:profile.$field = switch ($field) {
        'SID' { 'S-1-5-21-111-222-333-1002' }
        'LocalPath' { 'C:\Users\unrelated-user' }
        default { $true }
    }
    $rejected = $false
    try { Remove-WindowsSshTestProfile $user } catch { $rejected = $true }
    $script:profile.$field = $original
    if (!$rejected -or $script:removed.Count -ne 1) { throw "다른 소유·경로·특수·사용 중 프로필을 거부해야 한다: $field" }
}
$user.Name = 'unrelated-user'
$before = $script:reads
$rejected = $false
try { Remove-WindowsSshTestProfile $user } catch { $rejected = $true }
if (!$rejected -or $script:reads -ne $before -or $script:removed.Count -ne 1) { throw '일회 계정이 아니면 OS 조회 전에 거부해야 한다' }
Write-Host 'M4 profile cleanup: 정확한 계정·SID·프로필 하나만 삭제, 다른 소유·경로·특수·사용 중 프로필 거부'
