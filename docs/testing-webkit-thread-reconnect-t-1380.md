# macOS WebKit 재연결 목록 수집 조사(t-1380)

## 결론과 남은 근거

원인은 아직 확정하지 않았다. 제품 수리로 표시하지 않으며, 이 변경은 다음 CI에서
요청·헤더·본문과 페이지 실행 지연을 구분할 진단 후보다.
기존 20초 상한, 두 완전 목록, 2,505건의 개수·차례·고유값·실제 DOM 검사를 유지한다.

대상은 [main f44d7821의 macOS WebKit 실패](https://github.com/cLazyZombie/aaa/actions/runs/41212474873/job/124570168313)다.
실패한 CI를 재실행하지 않았고 로컬 통과를 원본 실패의 수리 증거로 쓰지 않는다.

## 원본에서 확인한 것

실패 artifact의 WebKit JSONL과 job 전체 로그를 대조했다.
아래 시각은 WebKit trace의 첫 사건 기준이며 문서 식별값은 표시하지 않는다.

| 시각(ms) | 사건 |
| --- | --- |
| 4,743 | 초기 문서의 26쪽 전체 본문 수집 완료 |
| 5,532 | reload 문서 시작 |
| 8,790 | reload 목록의 마지막 응답 보류 |
| 8,798 | 첫 events 연결 close 1006 |
| 9,327 | 두 번째 연결의 첫 요청 보류, 옛 마지막 응답 해제 |
| 9,479 | 옛 완전 목록의 마지막 DOM 행 확인 |
| 9,480 | 새 첫 요청 해제, 같은 문서의 완전 목록 두 개 대기 시작 |
| 9,482 | reload 문서의 첫 전체 본문 수집 완료 |
| 15,157 / 22,579 | 추가 events close 1006 |
| 16,702 / 24,465 | 세 번째·네 번째 events 연결 open |
| 30,481 | 기존 대기 상한에서 실패 |

실패 직전까지 요청 105개·본문 수집 102개다. 초기 문서와 reload 문서에서 각각
한 목록만 완성됐으며, reload 문서의 첫 요청은 세 번 더 시작됐다.
그 세 읽기는 실패 때까지 전체 목록의 끝에 도달하지 않았다.
남은 본문 세 개의 요청 커서는 각각 t-11698·t-12098·t-11198 다음이다.
관측 오류는 다음 원문이며 body-error·request-failed·route-failure 사건은 없었다.

```text
20000ms 안에 재연결 HTTP 두 완전 목록의 본문 수집이 되지 않았다(페이지가 답하지 않음)
```

이 오류의 괄호는 공통 `inTime` 문구다. 이것만으로 페이지가 실제 얼었다고 판단하지 않는다.
job 전체 로그에는 host의 `상태 사건 연결이 3초 동안 답하지 않아 끊는다` 세 줄이 있다.
CI는 3 CPU·worker 2개이며 E12 전후 load1은 15.63·25.57이다.
부하·1006·host 무응답 기록만으로 환경이나 제품의 원인을 확정하지 않는다.

원본은 헤더·requestfinished·route.fetch 시작/끝을 기록하지 않았다.
브라우저가 읽은 JSON과 Node의 `response.body()` 수집 시각도 구별하지 않았고
해당 WebKit 페이지의 heartbeat·crash·pageerror 계측도 없다.
따라서 남은 요청이 서버 응답 대기, 브라우저 실행 지연, protocol 본문 회수 중 어디에서
밀렸는지 소급 확정할 수 없다. 다른 Chromium 화면의 공통 실패 진단은 그 근거가 아니다.

## 추가한 계측

- 같은 Document와 요청 번호로 브라우저 fetch 시작·헤더·실제 앱 JSON 읽기·오류를 잇는다.
  원래 Response와 JSON 반환값·오류를 그대로 돌려주며 clone이나 추가 본문 읽기는 하지 않는다.
- Node에서는 response 헤더·requestfinished·기존 protocol 본문 수집을 각각 기록한다.
  route.fetch와 fulfill의 시작/끝도 같은 요청 번호로 잇는다.
- 성공 판정과 무관한 1초 heartbeat에 브라우저 시각과 간격을 남긴다.
  Node 수신 시각과 대조해 페이지 실행 정지와 관측자 전달 지연을 구분한다.
  pagehide에서 timer를 지우며, heartbeat를 기다려 시험을 통과시키지 않는다.
- page close·crash·pageerror와 시작 CPU/load, 대기 시작/실패 시 load·미완료 요청의
  헤더 유무·본문 회수 상태를 남긴다. 쿠키·전체 헤더·요청 URL·인증값은 추가하지 않는다.
- 본문 오류·실제 문서 교체 취소·최초 오류 보존과 route/context 정리 순서를 유지한다.

새 유닛 두 개가 필요한 이유는 계측 때문에 본문이 추가 소비되거나 원래 오류가 바뀌는 것과,
페이지 종료 뒤 진단 timer가 남는 것을 막기 위해서다. 실제 시간을 자지 않고 직접 사건을 전달한다.

## 개발 검사

- 기존 코드 로컬 대조: 웹 유닛 673개·타입·번들·E12 전체·공개 검사 통과.
  Chromium 목록 3.491초·WebKit 목록 4.504초다. 원본 CI의 재현·수리 증거는 아니다.
- 요청 경계 계측을 넣은 E12와 CI에서 함께 돌던 E23의 병렬 대조: 두 spec 모두 통과.
  E12 WebKit 목록 4.521초다. 이 개발 실행은 미커밋 변경으로 출처 요약을 만들지 못했다.
- heartbeat·실패 snapshot까지 넣은 진단 후보: 웹 유닛 673개·타입·번들·E12 전체·공개 검사 통과.
  Chromium 목록 3.519초·WebKit 목록 4.386초다.
  WebKit 요청 78개의 route 전송·헤더·다운로드·앱 JSON·protocol 본문 수집이 각각 대응했고,
  초기 문서 한 목록과 reload 문서 두 목록을 모두 확인했다.
- 마지막 유닛 두 개를 포함한 웹 유닛 최종 675개가 1.734초에 모두 통과했다.
- E12 전체 단계는 기존 보고서 준비 등을 포함한다. 위 목록 단계 수치를
  전체 spec이 5초 이내라는 주장으로 쓰지 않는다.
- 전체 gate·OS cross·main 병합/push는 수행하지 않았다. `web/e2e/lib` 변경은
  착지 때 `docs/verification.md`의 전체 범위다.

## 참고 구현

- [Playwright Response](https://playwright.dev/docs/api/class-response)와
  고정 1.64.1 패키지의 WebKit `Network.getResponseBody` 경계
  ([Apache-2.0](https://github.com/microsoft/playwright/blob/main/LICENSE)):
  헤더 사건, 다운로드 완료, 별도 protocol 본문 호출을 나눠 관측한다.
- [Puppeteer CdpHTTPResponse](https://github.com/puppeteer/puppeteer/blob/main/packages/puppeteer-core/src/cdp/HTTPResponse.ts)
  ([Apache-2.0](https://github.com/puppeteer/puppeteer/blob/main/LICENSE)):
  body 완료 신호 뒤 별도 protocol 호출을 실행하고 원래 실패를 유지하는 경계를 대조했다.

코드·새 의존성은 가져오지 않았다. 다음 CI 근거로 지연 위치를 좁힌 뒤 원인을 재현하고 수리한다.
