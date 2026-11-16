# 끝나는 문서의 터미널 압축 Worker(t-1894 착지 gate)

## 실패와 원인

t-1894 2-1 착지 gate(전체 ci-local)에서 웹 E2E 23 「종료 시작의 완료 응답: WebKit 실제 reload에서 projects 뒤 routines를
시작하지 않는다」가 페이지 오류 하나로 실패했다. 기대는 「실제 reload의 처리되지 않은 오류」 없음이었다.

```
Cannot load blob:http://127.0.0.1:<port>/<uuid> due to access control checks.
```

- 그때 머신 1분 부하는 20.3이었다(10코어, E2E 4 worker와 Rust 시험이 함께 돎). 부하 없이 같은 spec을 4개 동시로 8번
  되풀이했을 때는 8/8 통과해 재현되지 않았다.
- 오류 stack(`vk → yk → hk`)을 그 빌드 bundle에서 찾으면 Vite 인라인 Worker 포장(blob 주소로 `new Worker`) →
  `createTerminalWorker` → `TerminalReceiver` 생성자다. 터미널 압축 Worker를 blob으로 띄운 자리다.
- 이 단계는 `beforeunload`에서 붙잡아 둔 `projects` 응답을 풀어 준다. 부하가 높으면 끝나는 문서가 그 뒤에도 시작 일을 이어 가다
  터미널에 붙으며 압축 Worker를 새로 만든다. WebKit에서 떠나기 시작한 뒤(`beforeunload` 안, 그 뒤 task·microtask) blob Worker를
  만들면 매번(5/5) 같은 페이지 오류가 났다(실험 spec, 커밋하지 않음). 반대로 Worker를 만든 바로 뒤 reload하는 것은 오류가 없었다(0/5).
- 착지 후보의 웹 bundle은 main과 바이트로 같았다(생성 형식 파일만 다르다: main의 형식 파일로 빌드한 JS·CSS 328개 SHA-256이 같음).
  main에 있던 결함이다. t-0584(「페이지 종료 뒤 예약 조회 재개」)가 fetch 후속은 막았지만 터미널 연결은 막지 않았다.

## 수리

- `apiRequest.ts`의 「페이지 종료됨」(`beforeunload`·`pagehide`에서 켜고 BFCache `pageshow`에서 끈다)을 `pageClosing()`으로 내보낸다.
- `Term.tsx` `connect()`는 떠나는 중이면 압축 Worker·WebSocket을 만들지 않고 `data-retry="page-closed"` 표지만 남긴다.
  `pageshow`의 다시 붙기(`onWake`)는 이렇게 미룬 연결도 잇는다.

## 시험

- 웹 E2E 23 「종료 문서의 터미널: WebKit에서 떠나기 시작한 뒤 끊긴 터미널은 압축 Worker·연결을 새로 만들지 않고 pageshow 뒤
  다시 붙는다」: WebKit에서 터미널 WebSocket을 `routeWebSocket`으로 잡고, 가짜 `beforeunload` 뒤 그 연결을 닫는다. 다시 붙기 타이머가
  돈 뒤의 판단(미룬 표지나 새 연결 중 먼저 온 것)을 기다려 압축 Worker 수·연결 수가 그대로인지 본다. 가짜 `pageshow` 뒤 연결과
  Worker가 하나씩 늘고 페이지 오류가 없는지 본다. 고정 sleep은 없다.
- RED: `connect()`의 확인을 끈 판에서 「끝나는 문서는 압축 Worker를 새로 만들지 않는다: 2 !== 1」로 실패했다. GREEN: 수리한 판에서
  `ci-local --only web,e2e,public --spec 23,97,07,13`(터미널 다시 연결·압축·스크롤 기록 포함)이 통과했다.
- 실제 reload 경합(부하 의존)은 결정적으로 만들지 못해, 그 메커니즘(떠난 뒤 blob Worker 생성 → 페이지 오류)을 실험으로 확인하고
  제품이 떠난 뒤 Worker를 만들지 않는 계약을 위 단계로 지킨다.
