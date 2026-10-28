# resource_limits의 웹 준비 경계 (t-1244)

## 원인

첫 착지 후보 `a5ac37c6`의 전체 검증에서
`low_soft_limit_is_raised_for_host_and_agent_and_web_bursts_release_fds`가
`resource_limits.rs:71:52`의 `status["web_listen"].as_str().unwrap()`으로 실패했다.
보존한 실험실의 host 시작과 웹 listen 로그 간격은 약 7ms였다.
당시 status JSON 원문은 없어 그 값의 형태를 소급 확정하지 않는다.

제품은 `front::start` 다음에 `web::start`를 호출한다. Front의 일반 status 응답이 성공해도
웹 listener bind와 `web_listen` 공개 전일 수 있다. 시험은 첫 성공 응답을 저장한 뒤
프로젝트를 만들고 그 옛 응답에서 웹 주소를 꺼냈다. 그사이 웹이 준비되어도 저장한 값은 바뀌지 않는다.

## 수리 전 재현과 수리

기존 준비 코드를 같은 의미의 `wait_web_listen` 함수로 옮기고 회귀를 추가했다.
실제 Host·Front를 열고 웹 시작만 oneshot으로 보류한다. CLI의 첫 실제 status가 성공하면서
`web_listen: null`인 것을 단언하고, 그 응답을 읽은 사건으로 웹 시작을 허용한다.
시간 지연이나 제품의 시험 전용 hook은 없다.

```sh
nice -n 15 cargo test --profile e2e --locked -p aaa --test resource_limits \
  web_readiness_waits_past_a_successful_status_without_a_listener -- --exact --nocapture
```

수리 전 준비 코드는 이 실제 응답에서 동일한 `Option::unwrap()`의 `None` 실패를 0.35초에 냈다.
원문과 재현 패치는 t-1244 보고의 `library/resource-web-red.log`·`library/resource-web-red.patch`에 보존한다.

수리판은 일반 status 성공에 이어 `web_listen` 문자열이 나오는 상태까지 기다린다.
실패하면 마지막 status 응답이나 연결 오류를 남긴다. `setup(8192)`와 `setup(64)`가 같은 함수를 쓴다.
기존 `T=15초`, RLIMIT·384연결·fd 여유·실제 EMFILE·Front/HTTP 복구·경고 한 번 단언은 유지한다.
제품 코드·프로토콜·상한은 바꾸지 않으며 고정 sleep·실패 재시도는 없다.

새 회귀가 필요한 이유는 자연 기동이 빠를 때도 성공 status의 null을 반드시 먼저 관측하고,
그 뒤 실제 웹 bind·HTTP 200 응답까지 확인해 준비 경계 누락이 재발하지 않게 하기 위해서다.
회귀의 임베디드 Host는 부모의 OS 한도를 바꾸지 않는다. 실제 제한 자식 host·agent와 FD/EMFILE 계약은
기존 두 E2E가 그대로 검사한다. runtime 종료 뒤 임시 폴더가 정리되도록 수명을 유지한다.

```sh
nice -n 15 cargo test --profile e2e --locked -p aaa --test resource_limits -- --nocapture
```

수리판은 같은 null 선행 응답을 포함한 5개 시험 전체가 1.87초에 통과했다.
재착지의 전체 ci-local·cross·production·공개/개인정보 검사와 원격 확인은 이 부분 검증과 구별하며
사용자 승인에 따라 최신 main에서 새 전용 worktree로 수행한다. 첫 검증 실패 때 원문을 남기고 멈춘다.

## 참고

- [Tokio oneshot](https://docs.rs/tokio/latest/tokio/sync/oneshot/index.html)
  ([MIT](https://github.com/tokio-rs/tokio/blob/master/LICENSE)): 첫 실제 응답 뒤 웹 시작을 해제하는 단발 통지.
- [Playwright 사건 대기](https://playwright.dev/docs/events#waiting-for-event)
  ([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE)): 원인 사건과 준비 관측 순서를 고정하는 원칙.

외부 코드를 복사하지 않고 기존 상태 대기 도구와 Tokio 채널을 사용했다.
