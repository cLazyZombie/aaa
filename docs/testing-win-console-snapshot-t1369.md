# Windows 승인 PTY 스냅샷의 CI 경로 비교 수리(t-1369)

## 원인과 확보한 증거

대상은 main `5c1960fd08c1751d09a447deb25916d9115c2010`의
[CI 실행](https://github.com/cLazyZombie/aaa/actions/runs/41207246262)이다.
전체 job 로그와 `rust-tests-windows-2025-41207246262-1` artifact의 `test.log`를 확보했다.
둘 다 다음과 같은 정제된 하위 오류를 담고 있다.

```text
exit=Some(1), phase=child_image_mismatch, type=RuntimeException, os_code=Some(0)
```

helper 실행과 종료는 관측됐고 부모 PID 검사도 통과했다. 실행 파일 대조에서 거부돼
C# helper 컴파일·DbgHelp 스택 수집·reader thread 식별·성공 스냅샷 파싱에는 도달하지 않았다.
이 실패를 권한·DbgHelp·시간 초과나 간헐 실패로 분류하지 않는다.

당시 helper는 임시 `frames.json`과 `helper.log`를 수집 뒤 삭제한다.
CI artifact에는 `test.log`만 있어 별도 helper 원본은 회수할 수 없다.
당시 두 경로 원문은 기록되지 않았으므로 CI의 실제 문자열 자체를 확인했다고 주장하지 않는다.
원시 스택·개인 경로·소스 묶음·원시 로그는 이 문서나 보고 library에 싣지 않는다.

`scripts/ci-local.sh`는 Git Bash의 `pwd`에 `/target`을 붙여 `CARGO_TARGET_DIR`로 내보낸다.
Windows 실기에서 같은 소스의 기본 Cargo 호출은 원래 시험이 통과했지만,
같은 target 폴더를 가리키는 `C:/aaa-test/t-1369/repo/target`을 지정하면 원래 시험이
위와 같은 단계·종료 코드·예외 종류·OS 코드로 실패했다.
별도 소유 자식 대조에서도 역슬래시 경로는 성공하고 슬래시 경로는 실패했다.
경로 표기가 CI와 실기 기본 실행의 차이를 설명하며, 로컬 성공만으로 간헐이라 판단하지 않았다.

## 변경

- helper가 두 실행 파일 경로를 `Path.GetFullPath`로 정규화한 뒤 기존 대소문자 무시 비교를 한다.
  부모 PID 대조와 전체 경로 대조를 유지한다. 파일 이름만 같은 다른 경로는 계속 거부한다.
- CIM에서 실행 파일 경로를 얻지 못하면 `child_image_unavailable`, 정규화 예외는
  `normalize_child_image`로 구분한다. 권한 부족을 경로 불일치로 뭉뚱그리지 않는다.
- Rust의 실패 반환값에 helper 종료 코드·단계·예외 종류·OS 코드를 보존한다.
  진단 파일이 없거나 잘못됐어도 종료 코드는 남긴다. 경로·임의 예외 문구는 옮기지 않는다.
- 제품의 콘솔 입력 관측 API와 입력 판정은 바꾸지 않았다. 같은 CI 경로 환경에서
  observer 준비·read 호출·reader 스택 수집·read 반환·승인 완료를 확인했다.
  이 수리는 이전 60초 승인 정체의 원인을 밝혔다는 뜻은 아니다.

## 개발 검증

실제 Windows에서 슬래시 `CARGO_TARGET_DIR`를 유지한 채 다음을 확인했다.

- `cargo test --profile verify --locked -p aaa --test install authorization:: -- --nocapture`에
  해당하는 승인 owner 12개 통과. 설치 target의 나머지 24개를 실행한 결과는 아니다.
- `cargo clippy --profile verify --locked -p aaa --test install -- -D warnings` 통과.
- `cargo fmt --all -- --check`, `git diff --check`, `scripts/check-public.sh` 통과.

처음 SSH 아래에서 실행한 승인 묶음은 11개 통과·1개 실패했다.
`ssh_environment_and_helper_hash_flags_do_not_bootstrap_a_new_trust_root`는 실제 SSH 조상이
없는 조건을 전제하지만 실기 접근은 실제 시스템 sshd를 통한다. 제품은 검증 SSH 분기를 지나
잘못된 hash를 거부해 4를 반환했고 시험은 SSH 미검증의 3을 기대했다.
`VerifiedSsh::current`와 hash 검사 순서로 원인을 확인했다.
같은 계정의 WMI 비대화형 프로세스에서 같은 시험 실행 파일을 실행하자 12개 모두 통과했다.
시험의 기대값·제품 권한 검사·시한을 바꾸거나 시험을 제외하지 않았다.
보조 실행기 준비 중 초기 생성 flag 조합에 WMI가 21(잘못된 인자)을 반환해,
문서화된 Detached_Process 값으로 고쳤다. 서비스·예약 작업·시스템 설정은 만들거나 바꾸지 않았다.

개별 프로세스 실행 경과는 다음과 같다. 시작 비용도 포함하며 고정 sleep·재시도·시한 확대는 없다.

| 시험 | 경과 |
| --- | ---: |
| 기존 실제 reader 스택·개인값 제외 owner | 1.547초 |
| Cargo 경로 구분자·`.` 표기 회귀 | 1.538초 |
| 다른 경로의 같은 이름 실행 파일 거부·하위 오류 보존 | 1.228초 |
| 오류 진단의 개인 경로·임의 문구 정제 | 0.019초 |

새 시험이 필요한 이유는 각각 실기의 기본 Cargo 경로로도 CI의 표기 차이를 재현하고,
정규화 후에도 실행 파일 경계를 유지하며, 하위 오류 보존이 개인값 노출로 이어지지 않게 하기 위해서다.
기존 실제 reader 스택 owner는 그대로 유지했다.

원격 시험 전후 서비스·예약 작업·Run 키·사용자 PATH·방화벽 기준 목록은 같았다.
작업 폴더 실행 파일의 생존 프로세스는 0개임을 확인하고 전용 시험 폴더를 정리했다.
로컬 개발 검증이며 새 GitHub 실행·최종 통합 gate·production 빌드·병합·push는 별도 착지 창에서 한다.

## 참고

- [.NET 경로 정규화 설명](https://learn.microsoft.com/dotnet/standard/io/file-path-formats)과
  [.NET runtime Path.Windows.cs](https://github.com/dotnet/runtime/blob/main/src/libraries/System.Private.CoreLib/src/System/IO/Path.Windows.cs)
  ([MIT](https://github.com/dotnet/runtime/blob/main/LICENSE.TXT)): 구분자와 상대 구성요소의 정규화 기준.
- [PowerShell Process.cs](https://github.com/PowerShell/PowerShell/blob/master/src/Microsoft.PowerShell.Commands.Management/commands/management/Process.cs)
  ([MIT](https://github.com/PowerShell/PowerShell/blob/master/LICENSE.txt)): 프로세스 조회 경로의 대안을 비교했다.
  기존 CIM 조회는 유지하고 경로 비교만 고쳤으며 외부 코드를 복사하거나 의존성을 추가하지 않았다.
- [Win32_Process](https://learn.microsoft.com/en-us/windows/win32/cimwin32prov/win32-process):
  ExecutablePath의 권한 조건. 빈 경로를 별도 실패로 남긴다.
- [Win32_ProcessStartup](https://learn.microsoft.com/en-us/windows/win32/cimwin32prov/win32-processstartup):
  비대화형 시험 실행기의 생성 flag 기준.
