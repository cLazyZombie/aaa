# 백업 잠금 owner 종료와 fork 사본(t-1263, q-1664)

## 첫 착지 실패

미push 통합 `22e30c54`의 첫 전체 ci-local에서
`backup::tests::wal_backup_restores_auth_history_and_rotates_only_its_files`가 실패했다.
첫 백업은 성공했지만 바로 다음 `run(..., DAY_MS * 2)`가 `backup/tests.rs:62:54`에서
`백업 중복 실행 잠금` / `설치 자원을 다른 실행기가 사용 중입니다`로 실패했다.
host 유닛은 760 passed·1 failed·2 ignored였다. 원문과 시작/실패 uptime을 보고 library에 보존하고
새 검증·push를 중단했다. 진행 중이던 자기 검증 프로세스만 PID를 확인해 종료했다.

## 수리 전 단일 재현

원래 통합 실행 파일을 재빌드하지 않고 해당 시험 하나만 실행했다.
시험 프로세스에만 적용한 macOS dyld interpose fixture는 실제 `flock` 반환을 바꾸지 않는다.
첫 백업 잠금 획득 뒤 fork 자식의 FD 상속을 pipe 사건으로 확인하고, 둘째 잠금 획득이 끝나야
pipe로 자식을 해제해 `waitpid`로 종료를 수집한다. 잠금은 자식의 exec 전 창에 남는다.
자식의 alarm은 해제 사건이 오지 않을 때만 실패시키는 안전 상한이며 정상 종료 조건이 아니다.

```text
fork-window: child-ready ... cloexec=1
fork-window: second-flock result=-1 errno=35 child-alive=1
backup/tests.rs:62:54
백업 중복 실행 잠금
설치 자원을 다른 실행기가 사용 중입니다
original-exit=101
```

수리 전 단일 시험은 1.27초에 같은 실패 줄·오류를 재현했다.
시험 실험실은 고유 경로이며 원래 `run`의 첫 owner는 반환한 뒤다. Unix flock은 open file description에
연결되므로 부모가 `File`을 닫아도 fork/dup 사본이 남으면 잠금이 남는다.
CLOEXEC는 exec 시 닫을 뿐 fork와 exec 사이 창을 막지 않는다.
첫 전체 실패에서 어느 자식의 fork 창이 겹쳤는지는 당시 로그에 없어 소급 특정하지 않는다.

원본과 후보의 수리 전 `backup.rs` Git blob은 모두
`9e513962525b0f38890abb5eab99aaecb23edecf`다.
첫 재현 도구의 signal.h 누락·없는 dylib 로드 오류는 제품 재현 결과와 구별해 보고에 남겼다.
fixture를 고친 뒤 얻은 위 RED만 원인 증거로 센다.

## 수리와 필요한 회귀

백업 owner의 수명이 끝날 때 Unix에서 명시적으로 `File::unlock`하는 `BackupLock`을 둔다.
성공·오류·unwind에서 같은 Drop 경계를 쓰며 해제 오류는 로그에 남긴다.
Windows는 기존 비상속 쓰기 handle 예약을 close로 해제한다. 대기·재시도·spawn 전체 직렬화는 추가하지 않는다.
기존 install CLI `Lock`·agent `CollectionLock`의 같은 수명 원칙을 따른다.

새 Unix 유닛
`backup::tests::backup_owner_releases_lock_even_while_a_fork_or_dup_copy_remains_open`은
실제 `try_clone`으로 같은 open file description의 사본을 남긴다.
owner가 살아 있을 때 경쟁 획득 거부 → owner Drop → 사본이 살아 있는 동안 다음 획득 성공 →
새 owner 중 경쟁 획득 거부를 순서대로 확인한다. 시간 경과·별도 프로세스 없이 소유권 사건으로 끝난다.
필요성: FD 사본이 백업 완료 뒤에도 남는 회귀를 결정적으로 막으면서 실제 동시 실행 거부를 지킨다.

기존 WAL·인증/예약 이력 복원·다른 파일/부분 파일 보존·회전/시계 역행 검사는 수정하지 않았다.
수리 후 같은 fork fixture에서 둘째 flock은 자식 생존 중 result=0·errno=0이며 원래 시험은
0.81초에 통과했다. 새 소유권 회귀는 0.00초에 통과했다. 두 단일 시험 모두 5초 이하다.

## t-1266과의 겹침과 남은 검증

t-1266의 자주 깨지는 시험 정리 중 위 WAL 백업 시험은 이 수리와 겹친다.
시험을 삭제·축소하거나 기대값·timeout을 바꾸지 않고 실제 백업 잠금 수명을 고쳤다.
다른 backup/설치 잠금·다른 flaky 시험 정리로 범위를 넓히지 않았다.
재현 C/dylib는 보고 library의 일회성 증거이고 제품 hook·실행 의존성은 추가하지 않는다.

가벼운 단일 시험·fmt·diff·공개 검사만 수행한다. coordinator가 t-1261 독립 계측 종료를 알리기 전에는
전체 ci-local·cross를 새로 실행하지 않는다. 전체 검증 통과로 기록하지 않으며, 재착지는 그때의 최신 원격
main을 새로 통합해 수행한다. t-1262가 먼저 착지할 수 있다. 수리는 진행 목록 6개 변경과 별도 커밋이다.

## 참고

[Rust File::lock/unlock](https://doc.rust-lang.org/std/fs/struct.File.html#method.lock)과
[Rust Unix 파일 구현](https://github.com/rust-lang/rust/blob/1.90.0/library/std/src/sys/fs/unix.rs)
([MIT/Apache-2.0](https://github.com/rust-lang/rust/blob/1.90.0/COPYRIGHT))의 명시적 해제와
[flock 문서](https://man.freebsd.org/cgi/man.cgi?query=flock&sektion=2&format=html)의
공유 open file description 수명을 참고했다. 외부 코드는 가져오지 않았다.
