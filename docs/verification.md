# 개발 검사와 착지 검증

개발 중에는 바꾼 계약을 지키는 관련 owner를 확인하고, 착지 창에서 최종 통합 SHA의 gate를 한 번 실행한다.
개발 후보마다 전체 ci-local·양 OS cross·Windows production을 반복하지 않는다. 공통 backend·protocol·의존성 변경,
충돌 해소로 영향이 넓어졌으면 관련 owner를 더 확인하고 그 이유를 기록한다. 이미 발생한 실패는 원인과 수리 증거를 남긴다.

`scripts/ci-local.sh`의 인자 없는 실행은 기존 전체 gate다. 단계 명령·병렬도·worker·시한·단언을 바꾸지 않는다.
`scripts/verify.py`는 변경 범위와 단계를 기록하며 이 진입점을 호출한다. Python 3.11 이상을 사용한다.

## 개발

```sh
# 웹 타입·번들·웹 유닛 전체 + 관련 E2E + 공개 검사
nice -n 15 python3 scripts/verify.py develop --base origin/main --spec 23,109
# 관련 Rust crate와 strict clippy. 필요하면 --package를 더하거나 --spec을 함께 쓴다.
nice -n 15 python3 scripts/verify.py develop --base origin/main --package aaa-host
# 직접 좁힐 때도 실제 owner를 명시한다.
nice -n 15 cargo test --profile verify --locked -p aaa-host --lib backup::tests::
nice -n 15 bash scripts/ci-local.sh --only web,e2e,public --spec 23,109
```

`--spec`은 개발용 `--only …e2e…`에서만 받는다. 실행 binary의 별도 사본·번들 최신성 검사를 그대로 거치고,
선택한 spec의 모든 기기 조합을 실행한다. 선택은 로그의 `e2e-selection.txt`에 남긴다. 부분 통과를 전체 통과로 쓰지 않는다.
웹 개발에서 `--spec`을 생략하면 웹 E2E 전체다. Rust 개발은 관련 crate를 사람이 지정한다. 자동으로 임의의 owner를 추정하지 않는다.

## 착지

착지 시작 시점의 main SHA를 기준으로 보존한다. 후보들 통합·충돌 해소를 끝내고 커밋한 뒤 같은 기준으로 실행한다.
`origin/main`이 움직였다면 이미 보존한 SHA를 쓴다. 범위를 좁히려고 기준을 새 HEAD로 바꾸지 않는다.

```sh
nice -n 15 python3 scripts/verify.py landing --base <착지-시작-main-SHA> --dry-run
nice -n 15 python3 scripts/verify.py landing --base <착지-시작-main-SHA>
```

| 최종 변경 | 로컬 착지 gate | 보존하는 계약 |
| --- | --- | --- |
| `web/src/`, `web/public/`, `web/test/`, `web/e2e/specs/`와 Markdown 문서만 | `ci-local --only web,e2e,public` | 타입·번들·웹 유닛 전체, 현재 자산을 내장한 host·agent, Chromium·WebKit E2E 전체 |
| `docs/`의 Markdown만 | `ci-local --only public` | 공개 저장소 검사 |
| 그 밖, 혼합 변경, 변경 없음 | 인자 없는 ci-local 전체 | 기존 Rust/Python·의존 경계·설치/SSH·백업·웹 전체 |

Git의 기준 SHA부터 현재 작업 파일까지 직접 비교한다. rename은 삭제·추가 양쪽 경로를 보며 미추적 파일도 포함한다.
착지는 clean HEAD에서만 시작하고 실행 뒤 HEAD·작업 파일이 바뀌면 실패한다. `--spec`·`--package`·`AAA_E2E_ONLY`는 착지에서 거부한다.
실행 계획·기준/최종 SHA·변경 파일·종료 코드·경과는 `target/verification/<단계>-<시각>/`에 보존한다.

웹 전용 gate는 Rust 시험 사전 컴파일·Rust 전수·무관한 Linux/Windows cross를 반복하지 않는다. 대신 실제 내장 자산 빌드는 유지한다.
웹 package/lockfile·번들러·patch·proto·harness·공유 fixture·CI 스크립트가 바뀌면 전체로 돌아간다. full gate는 기존대로 양 OS strict cross와
Windows 변경의 실제 native owner를 확인한다. cross 성공을 Windows 실행 성공으로 기록하지 않는다.

빠른 백업/원격 경로 회귀는 기존 Rust 전체와 GitHub OS 행렬에 남는다. iPhone 첫 응답 경합, history/PTY, 대량 처리 계약은
웹 전용 착지에서도 웹 E2E 전체로 확인한다. GitHub `ci.yml`의 OS별 전체 검사와 브라우저 행렬은 유지한다. 따라서 OS 의미 차이의
주기적인 전체 실행을 없애지 않는다. full gate의 병렬도 조정은 별도 작업에서 다룬다.

## Windows production은 최종 SHA에서 한 번

최종 통합 SHA가 정해진 뒤 salt를 포함한 실제 release를 별도 Cargo 호출로 한 번 만든다. 전체 ci-local·시험에는 salt를 넣지 않는다.
이 빌드는 production 전용 cfg와 dev 의존성 기능이 합쳐지지 않은 실제 실행 파일을 확인하는 단계다.

```sh
nice -n 15 python3 scripts/windows-production.py build --commit <최종-SHA> \
  --artifact target/windows-production/<최종-SHA> --llvm-bin <LLVM-bin-폴더>
```

깨끗한 현재 worktree의 HEAD가 요청 SHA와 같아야 한다. 웹 자산을 빌드하고 Windows에서는 `cargo build`, 그 밖에서는 `cargo xwin build`를 쓴다.
target은 이 worktree의 `target/`만 쓴다. 출력 폴더가 이미 있으면 덮어쓰거나 다시 빌드하지 않는다. 컴파일·기호 검사 실패도 재시도하지 않는다.
검사한 `aaa.exe`, `aaa.pdb`와 상대 파일 이름만 쓴 `provenance.json`을 함께 보존한다. manifest에는 commit/tree/salt/profile,
각 파일 SHA-256, PE/PDB GUID·age, 줄 표 유무, 빌드 경과가 있다. 빌드 경로·머신 이름은 넣지 않는다.

Windows 실행 검사는 그 폴더를 시험 머신의 작업 전용 폴더로 복사한 뒤 수행한다. `native`는 Python 3.9 이상에서 동작한다.

```powershell
py -3 windows-production.py native --commit <최종-SHA> --artifact C:\aaa-test\<작업-ID>\artifact
```

exe와 PDB의 SHA를 복사 전후에 대조하고, 실제 `--version`이 salt 포함 판과 같은지와 격리한 config/state의 `config check`를 확인한다.
성공하면 같은 파일 해시에 결속한 `native-check.json`을 만든다. 서비스·사용자 설정·시스템 정책은 바꾸지 않는다. 이 짧은 실행 확인은
Windows 코드 변경에 필요한 실제 SCM/ConPTY/ACL 등의 owner를 대신하지 않는다. 작업 전용 자원은 검사 뒤 지운다.

native 영수증을 원래 산출물 폴더로 회수하고 다음 명령을 실행한다. `verify`에는 Cargo·npm·재포장·설치·전송이 없다.

```sh
python3 scripts/windows-production.py verify --commit <최종-SHA> \
  --artifact target/windows-production/<최종-SHA> --llvm-bin <LLVM-bin-폴더>
```

PE의 CodeView GUID/age와 실제 PDB를 대조하고 PDB debug·줄 정보, 두 파일 SHA, native 영수증의 SHA·판·성공을 모두 확인한다.
이후 공개 검사와 승인된 push를 마치고 **같은 폴더의 exe/PDB**를 전달한다. 전달 때 다시 빌드하거나 exe를 수정하지 않는다.
전달 대상에서도 같은 SHA를 대조한다. 최종 SHA가 달라지면 옛 후보를 재사용하지 않고 새 SHA의 한 번 빌드·검사를 수행한다.

## 참고한 공개 구현

- [dorny/paths-filter](https://github.com/dorny/paths-filter) ([MIT](https://github.com/dorny/paths-filter/blob/master/LICENSE)):
  변경 경로에 따라 job 범위를 나누는 접근을 참고했다. 이 저장소는 로컬 Git의 미커밋·rename 원본까지 포함하며 알 수 없는 경로는 전체로 둔다.
- [cargo-nextest archive](https://github.com/nextest-rs/nextest/blob/main/site/src/docs/ci-features/archiving.md)
  ([MIT 또는 Apache-2.0](https://github.com/nextest-rs/nextest)):
  빌드와 실행 소비를 나누고 고정 산출물을 재사용하는 접근을 참고했다. runner를 교체하거나 코드를 가져오지는 않았다.
- [Cargo test](https://doc.rust-lang.org/cargo/commands/cargo-test.html): 관련 package/target 선택과 전체 workspace 실행의 구분을 따른다.

외부 코드·새 런타임 의존성은 추가하지 않았다. LLVM은 기존 PE/PDB 진단 도구를 호출한다.
