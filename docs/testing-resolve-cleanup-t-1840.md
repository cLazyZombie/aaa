# resolve의 정리 전용 후속 자동 기록(t-1840)

## 범위와 판정

보고 수집은 `[완료]`의 `## Next` 각 줄을 원본 판·순번의 SHA-256 ID로 후속 원장에 저장한다.
보고를 바꾸어도 이전 미처리 후속을 지우지 않는다. resolve 뒤 미처리 후속은 완료 보관을 막고
`task-followup`의 `task needs check`와 할 일의 확인 필요 표시를 만든다. 명시적 후속 처리 뒤에는
다음 task 정리에서 `task auto-cleaned`를 남긴다.

보고 지침 `brief_text.rs`가 제시하는 정리 문구는 `정리: worktree와 branch를 지운다`다.
`정리:`는 자유 형식 문장 앞부분일 뿐 작업 종류를 보장하는 구조 필드가 아니다. 별도 finish의
정리 문구 생성 경로는 없었다. 따라서 이 한 문구와 일치하는 미처리 후속만 자동 처리한다.
문장 확장·다른 정리 대상·다른 후속이 하나라도 섞이면 명시적으로 처리해야 한다.

resolve는 기존 agent 정리 응답의 worktree 삭제·branch 삭제가 모두 참이고,
현재 branch 끝의 원격 main 포함 SHA를 받은 경우에만 자동 기록한다. keep-worktree,
불완전 사본 보존, 사본 없이 끝냄, 다른 머신의 앞 자리는 자동 처리하지 않는다.
대상 worktree·branch·main 포함 SHA와 actor `host`·처리 시각을 후속 원장에 보존한다.
이 저장은 `resolve_pending` 해제와 함께 수행한다. 정리 중의 알림은 보류하고 실제
종료 직후 아직 후속이 남았을 때만 확인 필요를 알린다.

## 회귀 owner

- `aaa-host`의 `orch::report_tests::resolve_records_cleanup_only_followup_before_exposing_resolved_task`:
  agent 요청 경계에서 정리 중 알림·task 판정을 실행해 카드가 없음을 확인한다. 정리 후 기록의
  대상·SHA·actor·시각, 즉시 task 표시, 완료 보관, 닫힌 inbox까지 포함한 확인 알림 부재,
  host를 다시 열었을 때 원장 보존을 확인한다.
- `resolve_preserves_cleanup_when_other_followups_are_pending`: 정리와 배포 혼합,
  다른 정리 대상이 포함된 문구는 자동 처리하지 않는다.
- `resolve_preserves_cleanup_followup_without_complete_deletion_evidence`: dirty worktree,
  branch 미삭제, main 근거 없음, 명시적 worktree 보존, 다른 머신의 앞 자리에서 후속과 task를 보존한다.
- `aaa` 통합 owner `m13::task_main_merge::resolve_cleanup_only_next_archives_task_without_needs_check_notice`:
  실제 CLI·host·agent·로컬 bare origin을 실행해 commit·main 병합·push 뒤 worktree와 branch 삭제,
  삭제한 branch 끝 SHA 기록, task 보관과 확인 필요 알림 부재를 확인한다.

시험에 sleep·재시도·시한 연장·기대값 완화는 추가하지 않았다.

## 검증

- 새 host 회귀 세 시험: 통과(총 0.62초).
- 최초 컴파일에서 시험 fixture의 GitAnswer에 잘못 넣은 필드 네 개로 실패했다.
  LeftPlace 초기화 보정이 GitAnswer에도 적용된 편집 오류였으며 그 초기화만 복구했다.
- 최초 전체 ci-local은 종료 코드 1이다. 웹 빌드·웹 유닛 720개·fmt·공개 검사는 통과했다.
  Rust 347개 실행 단위 중 두 개가 실패했다. 새 통합 fixture의 저장소 workspace 등록 누락을
  기존 owner와 같은 등록으로 고쳤고, coordinator 핵심 안내가 16KiB를 넘은 것은 새 중복 문장을
  빼서 고쳤다. 상세 동작 안내는 threads 주제에 유지했다. 상한·기대값은 바꾸지 않았다.
- 수정 뒤 실제 CLI 통합 시험은 통과했다(1.47초). `ci-local --only test`로 Rust 전체를 다시
  실행해 347개 실행 단위와 Python 검사가 모두 통과했다(107초). 재컴파일·수정 없는 재시도로
  실패를 덮지 않았다.
- 최초 브라우저 E2E는 42개 spec·53개 작업 중 52개 작업이 통과했고 `133-account-rename`만
  휴대폰 WebKit에서 첫 이름 변경 뒤 입력란을 찾지 못해 실패했다. 실패 화면에는 원래 계정 행과
  이름 변경 단추가 있고 편집 form이 없었다. 이 변경은 frontend·계정 이름 변경 경로를 건드리지 않는다.
  같은 시작 SHA의 깨끗한 별도 worktree에서 spec 133을 실행하면 통과했고, 후보에서도 클릭의
  capture/bubble·form 생성 사건을 기록한 진단 실행은 통과했다. 원인은 재현되지 않았으므로
  기존 결함·부하 탓이라고 단정하지 않으며 최초 전체 E2E를 통과로 바꾸지 않는다.
- 관련 `aaa-host`·`aaa` strict Clippy(`--all-targets -- -D warnings`): 통과.
- 최종 안내 참조를 보완한 뒤 guide 관련 네 시험·fmt·공개 검사: 통과.
- 착지 창의 최종 통합 SHA gate·배포는 별도 절차다.

## 로컬 증거 위치

원시 기록은 library에 복사하지 않고 작업 폴더에 보존한다.

- `target/ci-local/t-1840/`: 최초 전체 검증과 E2E 실패 화면.
- `target/ci-local/t-1840-rust-fixed/`: 수정 뒤 Rust·Python 전체 성공.
- `target/baseline-133-evidence/`: 시작 main SHA에서 spec 133 단독 대조 성공.
- `target/diag-133.log`: 후보의 입력 사건 관측을 추가한 진단 결과.

검증은 모두 `aaa verify -- <명령>`으로 실행했다. 미커밋 상태의 후보 실행은 검증 원장 자동
집계 대상이 아니므로 이 문서에 직접 결과를 적는다. 기준 대조용 worktree는 증거를 옮긴 뒤 지웠다.

## 참고한 공개 구현

- [Git worktree](https://git-scm.com/docs/git-worktree): dirty worktree의 기본 삭제 거부를 유지한다.
  [Git 라이선스](https://github.com/git/git/blob/master/COPYING)는 GPL-2.0이며 코드를 가져오지 않았다.
- [GitHub CLI merge](https://cli.github.com/manual/gh_pr_merge): 병합·branch 정리가 한 작업의 후속일 때
  해당 실행의 결과로 처리한다는 흐름을 참고했다.
  [라이선스](https://github.com/cli/cli/blob/trunk/LICENSE)는 MIT이며 코드를 가져오지 않았다.

외부 구현을 새로 도입하지 않고 기존 agent 삭제 계약과 host 후속 원장을 사용한다.
