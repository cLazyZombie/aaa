# 성공한 일회성 M4 계정의 프로필만 삭제한다. SID는 이번 New-LocalUser 결과에서 받는다.
function Remove-WindowsSshTestProfile($CreatedUser) {
    $sid = [string]$CreatedUser.SID.Value
    if ($CreatedUser.Name -notmatch '^aaa-ci-[a-f0-9]{10}$' -or $sid -notmatch '^S-1-5-21-\d+-\d+-\d+-\d+$') {
        throw 'M4 profile cleanup: 이번 실험실 계정이 아니다'
    }
    $profiles = @(Get-CimInstance -ClassName Win32_UserProfile -Filter "SID='$sid'" -ErrorAction Stop)
    if ($profiles.Count -ne 1) { throw 'M4 profile cleanup: 성공한 계정의 프로필을 하나로 확인하지 못했다' }
    $profile = $profiles[0]
    $leaf = ($profile.LocalPath -split '[\\/]')[-1]
    if ($profile.SID -ne $sid -or $profile.Special -or $profile.Loaded -or $leaf -ne $CreatedUser.Name) {
        throw 'M4 profile cleanup: 계정 소유·일반 프로필·로그오프 상태를 확인하지 못했다'
    }
    Remove-CimInstance -InputObject $profile -ErrorAction Stop
}
