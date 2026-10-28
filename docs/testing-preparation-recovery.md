# 중단 기동 회수와 명시 restart의 시험 경쟁(t-1251)

## 첫 착지 실패

최신 main `3e9ab73f`와 advisor 후보를 통합한 미push `25daa096`의 첫 전체 ci-local에서
`preparation::interrupted_placement_and_resolve_resume_from_agent_records`가 실패했다.
reader의 `출력이 끝날 때까지 "open"이 없었다`는 성공 JSON이 오지 않았다는 이차 증상이다.
보존한 실험실의 `restart.log`에는 실제 원인이 되는 응답이 남았다.

```text
{"error":{"code":"bad_request","message":"t-0001는 준비·정리 작업 중이다. thread show로 상태를 본다: tokio::sync::mutex::TryLockError [...]"}}
```

새 host의 agent 연결 공개 직후 `recover_launches`는 중단된 Starting 기록을 회수한다.
회수와 명시 restart는 같은 `claim_thread`를 사용한다. 원래 시험은 연결 성공만 기다리고
바로 restart를 보내므로 ticker가 claim을 소유한 동안 요청하면 정상적인 작업 중 거부를 받는다.
단독 원본 시험은 3.83초에 통과했다. 이 통과로 전체 gate의 실패를 없애지 않는다.

## 수리 전 재현

일회성 host 시험에서 실제 Starting 기록과 Front listener를 만들고 `rec_lock`으로 DB 갱신
진입을 보류한 상태에서 원래 `recover_launches`를 별도 thread에 실행했다.
실제 thread operation mutex가 점유된 상태를 관측한 뒤 원래 실행 파일의 restart CLI를 한 번
보냈다. 제품 코드·거부 정책은 바꾸지 않고 원문의 순서를 고정한 것이다.

실제 CLI는 exit1과 같은 `준비·정리 작업 중` 오류를 반환했다. 그 표준 출력에 원래 m13의
reader 구현을 그대로 적용하면 같은 `출력이 끝날 때까지 "open"이 없었다`로 0.38초에 실패했다.
고정 sleep·CPU 부하·시간 경과로 순서를 만들지 않았다. 일회성 재현 시험은 패치와 로그를
보고 library에 보존한 뒤 제거했으며 제품 hook이나 공개 시험용 API를 추가하지 않았다.

## 수리

기존 m13 시험에서 agent 연결 이후 `World::wait_show`로 해당 thread의 `failed` 상태를
기다린다. 이 helper는 먼저 Front 사건을 구독하고 현재 상태를 한 번 읽으며, 아직 조건이
아니면 해당 thread 상태 사건을 기다린다.

상태 발행은 회수 claim 반환보다 앞설 수 있어 그 상태 뒤 `Node::ticks(1)`로 다음 ticker
완료까지 확인한다. 실제 `ticker tick`은 요청 뒤 시작한 tick이 끝나야 응답하므로 이미 돌던
회수의 claim이 반환됐음을 확인한다. 회수된 Failed 기록은 다음 recovery pass가 다시 claim하지
않는다. 이후 원래 명시 restart를 한 번만 보내고 Git gate를 해제한다.

제품 동작·원래 성공 기대값·Git gate·중복 자리 방지·agent root 변경·정리 영수증·resolve 사건
한 번 단언을 유지한다. 대기 상한 T도 그대로이며 고정 sleep·재시도·시험 삭제는 없다.
새 상시 시험은 추가하지 않고 기존 회귀의 선행 조건을 고쳤다.

## 검증과 재착지

`cargo test --profile e2e -p aaa --test m13 preparation:: -- --nocapture`의 기존 세 시험이
9.90초에 통과했다. 고정 sleep 검사 8개·fmt·diff·공개 검사도 통과했다.

원본 로그·실험실·제어 재현 패치와 실패 로그, 수리 뒤 preparation 시험의 원문은 t-1251
보고 library에 둔다. 부분 검증 뒤 수리를 advisor 변경과 별도 커밋으로 남긴다.
최신 원격 main의 새 Mac 전용 worktree에서 no-ff 통합부터 전체 ci-local·공개·Linux/Windows
strict cross·Windows production·개인정보 검사·ff push·원격 대조·설치기 전달을 다시 수행한다.
새 착지 회차의 첫 검증 실패도 원문을 보존하고 멈춘다.
