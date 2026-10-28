# macOS CI test 단계의 반복 재컴파일(t-1221)

## 원 artifact에서 확인한 시간

두 실패의 전체 `test.log`, `python.log`, `rust/state.json`, `events.jsonl`,
`summary.json`, `build.jsonl`, `build.stderr.log`, 모든 binary·doctest 로그를 확보했다.
원문은 thread library에 보존하며 공개 문서에는 실제 경로·PID를 옮기지 않는다.

| 원 실행 | 사전 컴파일 | binary 실행 | doctest 시작 뒤 중단 |
| --- | ---: | ---: | ---: |
| [a57ab031](https://github.com/cLazyZombie/aaa/actions/runs/40610453963/job/122560442945) | 228.59초 | 602.91초 | 79.17초 |
| [3ee2b329](https://github.com/cLazyZombie/aaa/actions/runs/40521069216/job/122247984754) | 295.77초 | 563.09초 | 51.75초 |

- 두 `state.json` 모두 실제 CPU 예산3·worker6·libtest thread8이다. 처음 m13 singleton3개는
  각각1자리를 예약해 함께 실행되고, 나머지 m13 묶음과 일반 binary는 각각3자리를 예약해 직렬로 실행된다.
  a57ab031의 m13 전체는389.19초다. 일반 binary는 실제 실행 전 최대602.86초를 기다렸다.
  이 직렬화는 worker 숫자를 올려도 없어지지 않는다.
- a57ab031의 binary 시작 시 load average(1분)는20.76~40.56이었다. load만으로 CPU 사용률이나
  다른 작업의 부하를 판정하지 않는다. 원 process snapshot·CPU 사용률은 없다.
- a57ab031은 build 단계의 전체 no-run4분35초 뒤, clippy를 거쳐 test 준비에서 같은 workspace를
  다시3분48초 컴파일했다. `build.jsonl`에서도 aaa-common 및 의존 crate의 `fresh=false`를 확인했다.
  완료한31개 binary/shard는 모두 exit0이고 Python70개는 성공(2개 skip)이다.
- a57ab031 doctest 원문은 의존 라이브러리 빌드1분11초 → 여러 crate의0개 doctest 정상 완료 →
  마지막 `aaa_session` rustdoc 시작이다. 이전 실행은 의존 라이브러리 컴파일 중 끊겼다.
  둘 다15분 test 단계 예산 초과다. 개별 doctest 실행 hang이나 Cargo lock 대기라는 원문은 없다.
  마지막 rustdoc 자식의 상태·종료 코드는 없어 그 종료를 소급 확정하지 않는다.

## 재현과 수리

`aaa-common/build.rs`가 `git rev-parse --git-path packed-refs`의 결과를 존재 확인 없이
`rerun-if-changed`로 등록했다. Git은 파일이 없어도 그 경로를 반환한다. CI처럼
`git init` 후 loose ref로 checkout한 저장소에는 packed-refs가 없을 수 있다.
Cargo는 이 경우 매 호출에서 dirty로 판단해 aaa-common과 그 의존 crate를 다시 컴파일한다.

같은 build script를 사용하는 의존성 없는 실제 Cargo package에서, 변경 없이 두 번 검사하면
수리 전 두 번째도 `fresh=false`다. verbose 원인은 `packed-refs ... is missing`이다.
Windows 재현은 두 번째0.35초 재컴파일에서 수리 뒤0.01초 Fresh로 바뀌었다.
원 CI는 fingerprint 원인과 Git 파일 목록을 수집하지 않았으므로, 이 결함을 당시 시간 초과의
유일 원인이라고 단정하지 않는다. 위 반복 컴파일·직렬 실행과 독립 재현을 구분한다.

- packed-refs는 존재할 때만 감시한다. loose ref가 pack될 때의 삭제는 기존 ref 감시로 감지한다.
- packed branch에 새 loose ref가 생기는 경우는 가장 가까운 기존 ref 폴더를 감시한다.
  다음 빌드부터 실제 파일을 감시한다. linked worktree의 공용 ref 경로도 Git의 반환값을 쓴다.
- HEAD·index 감시, BUILD_SHA와 추적 Rust 소스 목록은 유지한다. detached HEAD의 이동과
  index에 새 소스를 더하는 사건도 실제 Cargo 검증으로 확인한다.
- CPU 예약의 singleton 예외·전체 binary·전체 workspace doctest·실패 전파·원문 수집은 유지한다.
  CPU 예산보다 큰 thread 수를 무작정 겹치지 않는다. timeout·재시도·기대값·시험 목록은 바꾸지 않는다.

## 필요한 회귀와 검증

`scripts/test_build_watch.py`의 두 시험은 실제 Git과 실제 Cargo를 쓰는 작은 독립 package다.
상위 ci-local의 `CARGO_TARGET_DIR`를 물려받지 않고 각 fixture의 target을 사용한다.
처음 통합 시 이 격리가 없어 생성물 조회가 실패한 원문도 보존했으며, 경로를 격리해 수리했다.

- `test_fresh_checkout_reuses_build_but_tracks_index_and_commit`: 새 checkout의 두 번째 빌드를
  재사용하면서, 새 추적 소스·새 commit의 경로 목록과 SHA는 갱신해야 하므로 필요하다.
  원 build script에서는 두 번째 Fresh 단언이 실패하고 수리 뒤 통과한다.
- `test_packed_linked_worktree_tracks_new_loose_ref_and_detached_head`: 없는 파일을 단순히
  무시하면 packed branch의 다음 commit을 놓칠 수 있어 필요하다. 실제 linked worktree에서
  packed ref → 새 loose ref → detached HEAD 이동과 매 경계 다음의 Fresh를 확인한다.

Windows의 두 시험은 합계5.68초, Mac은 합계2.52초였다. 상위 target 환경까지 둔 격리 검증은
Mac 합계3.99초로 통과했다. 각 시험은5초 이하다. 기존 runner·CI 셸 회귀도 함께 확인한다.
Mac 첫 전체 Rust 실행은32개 job(binary31개·doctest1개) 모두 성공했다. Python 첫 통합의
위 fixture 오류를 수정하고 깨끗한 후보4a256795에서 다음 검증을 완료했다.

```sh
RUST_TEST_THREADS=8 nice -n 15 bash scripts/ci-local.sh --only fmt,clippy,test,public --logs <logs>
```

- fmt2초·clippy23초·test296초·public3초, 전체301초로 모두 통과했다.
- Python72개 성공(2개 skip), Rust32개 job 모두 exit0, 개별 시험1470개 성공·7개 ignored다.
  전체 workspace doctest는6.36초에 정상 종료했다.
- 최종 집계 뒤 no-run은3.44초, 이어진 동일 no-run은0.14초·전체 artifact Fresh였다.
  두 호출 모두 성공했다. 작은 재현의 실패/성공과 전체 원문·상태·사건 기록을 library에 보존한다.
- 이 Mac은10 CPU다. 원 CI의3 CPU·동시 부하·캐시와 다르므로 이 시간을 직접적인 전후 속도
  비교나 GitHub 회복 증거로 쓰지 않는다. 원 CI에 없는 fingerprint·Git 파일 상태는 미확정으로 남긴다.

전체 ci-local·cross·병합·push는 coordinator의 착지 창에서 별도로 수행한다. 로컬 Mac은
호스티드3 CPU runner와 다르므로 로컬 통과 시간을 GitHub 회복 시간으로 해석하지 않는다.

## 오픈소스 참고

- [Cargo build script 변경 감지](https://doc.rust-lang.org/cargo/reference/build-scripts.html#change-detection),
  [Cargo 시험·doctest 범위](https://doc.rust-lang.org/cargo/commands/cargo-test.html)
  ([MIT/Apache-2.0](https://github.com/rust-lang/cargo/blob/master/LICENSE-MIT)):
  mtime 감시와 별도 rustdoc 호출의 의미를 따르며 Cargo 자체의 캐시 판정을 사용한다.
- [nextest의 threads-required](https://nexte.st/docs/configuration/threads-required/)
  ([MIT/Apache-2.0](https://github.com/nextest-rs/nextest/blob/main/LICENSE-MIT)):
  CPU 예약과 실제 동시 실행을 구분한다. 외부 runner 도입·과예약으로 우회하지 않는다.
- [pytest-xdist의 실행 분배](https://pytest-xdist.readthedocs.io/en/stable/distribution.html)
  ([MIT](https://github.com/pytest-dev/pytest-xdist/blob/master/LICENSE)):
  worker 수와 묶음 크기가 서로 다른 제어임을 대조했다.

설계 원칙과 문서만 참고했으며 외부 코드·실행 의존성은 추가하지 않았다.
