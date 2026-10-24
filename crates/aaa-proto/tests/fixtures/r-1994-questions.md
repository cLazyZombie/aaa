## Questions

### 1. 대화 재진입 개선을 어느 범위로 구현할까요?

추천은 coordinator와 일반 thread에 크기를 제한한 공통 캐시를 적용하는 A안이다. 사용자가 허용한 coordinator만 유지하는 방식은 B안으로 두었다. 두 종류의 병목은 거의 같고, 공통 구조여서 둘 다 개선할 수 있다.

[조건별 측정과 설계 보기](library/design.html) · [측정 방법·한계·재현](library/measurement-method.md) · [요약 CSV](library/summary.csv) · [기준 원자료](library/measurements.json) · [지연 대조 원자료](library/measurements-network.json) · [계측 코드](library/measurement-source.zip)

**실측 조건**

기준 `6c76bffa1c2a`, Apple M1 Pro, Playwright Chromium/WebKit. 1280×844와 390×844에서 실제 host·agent·coordinator·일반 thread를 만들고 같은 합성 Claude 기록을 읽었다. 작은 기록은 60메시지, 큰 기록은 2,400메시지·약 1.21 MiB다. 큰 기록의 과거 위치에서는 받아 둔 1,050행을 복원했다.

아래는 각 조건 3회의 중앙값(ms)이다. 앱과 폰트가 이미 열린 상태에서 그 대화로 주소를 전환한 순간부터, 올바른 본문·읽던 위치를 확인한 다음 두 프레임까지다. 앱 최초 다운로드와 목록 drawer를 여는 시간은 제외한다. 휴대폰 크기의 headless 측정을 실제 iPhone 측정으로 보지 않는다. n=3이라 p95를 제시하지 않는다.

| 엔진·폭 | 대상 | 작은 기록 처음 → 할 일 왕복 | 큰 기록 처음 | 큰 기록 할 일 왕복 | 큰 기록 다른 pane 왕복 | 큰 기록 과거 위치 왕복 |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Chromium 1280 | coordinator | 68 → 34 | 109 | 55 | 52 | 124 |
| Chromium 1280 | 일반 thread | 66 → 33 | 105 | 56 | 50 | 133 |
| Chromium 390 | coordinator | 68 → 32 | 88 | 52 | 52 | 84 |
| Chromium 390 | 일반 thread | 66 → 31 | 88 | 53 | 51 | 82 |
| Webkit 1280 | coordinator | 128 → 105 | 187 | 166 | 160 | 199 |
| Webkit 1280 | 일반 thread | 123 → 109 | 180 | 157 | 168 | 213 |
| Webkit 390 | coordinator | 131 → 106 | 190 | 160 | 204 | 199 |
| Webkit 390 | 일반 thread | 124 → 100 | 183 | 155 | 197 | 198 |

네트워크 영향을 구분한 대조는 Chromium의 CDP 지연 80ms, 대역폭 무제한이다. 실제 외부망·원격 agent 측정은 아니다.

| coordinator 큰 기록 | 로컬: 최신 위치 왕복 | 지연 추가: 최신 위치 왕복 | 로컬: 과거 위치 왕복 | 지연 추가: 과거 위치 왕복 |
| --- | ---: | ---: | ---: | ---: |
| 데스크톱 1280px | 55ms | 151ms | 124ms | 639ms |
| 휴대폰 크기 390px | 52ms | 148ms | 84ms | 633ms |

과거 위치 복원의 최댓값은 지연 대조에서 각각 646ms·651ms였다. [실제 측정 화면: WebKit 390px](library/webkit-390-coordinator-old.png)에서 본문은 가짜 기록이다.

**지연이 생기는 곳**

| 구간 | 코드·관측 근거 | 판단 |
| --- | --- | --- |
| 컴포넌트 재마운트 | `App.tsx:1615` 부근에서 다른 화면을 고르면 TermView가 제거되고, pane마다 key가 바뀜. `Chat.tsx:507` 부근은 chatRef와 chat을 null로 초기화 | 메모리에 있던 본문과 화면을 다시 만드는 구조 |
| 기록 재수신 | `Chat.tsx:514` 최신 쪽 요청, `:538` 읽던 row까지 이전 쪽을 직렬 요청. 큰 기록은 최신 150행/98 KiB, 과거 위치는 7회/686 KiB | 과거 위치는 왕복 지연이 쪽 수만큼 누적됨 |
| API 대기 | 과거 위치·휴대폰 Chromium: 7회 API의 합이 로컬 약 48ms, 지연 추가 시 약 596ms | API에는 HTTP·host→agent·파일 읽기·브라우저 처리 시간이 함께 포함됨. 순수 네트워크 시간으로 단정하지 않음 |
| 브라우저 재그리기 | 큰 기록·할 일 왕복의 마지막 JSON 이후 본문 준비: Chromium 약 39–41ms, WebKit 약 108–109ms | 데이터만 캐시해도 남는 비용. 레이아웃·프레임 대기를 포함하며 순수 React CPU 시간은 아님 |
| 터미널 재생성 | `Term.tsx:548` 부근의 xterm 생성과 `:1564` 부근의 정리, pane WS 재연결 관측 | 대화 모드에서도 숨은 터미널을 생성함. 본문과 병렬이므로 API 시간에 별도 더하면 중복 계산 |
| 구독 | `api.ts:299`의 chat_watch 해제·재등록. 전역 events WS는 화면 왕복 내내 유지 | 전역 WS 재연결을 고칠 일은 아님. pane 단위 구독 비용과 화면 수명을 구분해야 함 |
| 이미 있는 최적화 | `chatScroll.ts`는 최근 100개 위치 기억, `Chat.tsx:672`와 `ChatRow.tsx`는 300행 초과 가상화 | 위치만 기억하고 본문은 버리므로 반복 요청을 막지 못함. 가상화를 새로 도입할 필요는 없음 |

입력 연결은 본문과 별도다. 큰 기록의 할 일 왕복에서 coordinator의 연결 준비는 로컬 Chromium 약 89–100ms, WebKit 약 157–164ms였다. 추천안의 본문 즉시 표시가 송신 연결까지 즉시 준비된다는 뜻은 아니다. 캐시 화면에서도 송신은 살아 있는 연결·현재 권한 확인을 따른다.

**개선 방법 비교**

아래 ms는 현재 실측과 제거할 대기 구간을 바탕으로 한 예상·목표다. 개선 코드를 실행한 결과가 아니다.

| 방법 | 예상 효과 | 비용·주의점 | 범위·추천 |
| --- | --- | --- | --- |
| 메모리 데이터 캐시 + stale-while-revalidate | 캐시 적중 시 본문 약 40–150ms 목표. 7회 재수신을 화면 표시의 선행 조건에서 제거 | React·xterm·레이아웃 재생성은 남음. history·권한·읽기 옵션에 따라 분리해야 함 | 기본 재료로 채택 |
| 제한된 화면 유지(keep-alive) + 위 캐시 | 본문 재표시 20–60ms 목표. 현재 과거 위치 84–199ms에서 약 24–179ms, 지연 대조 633–639ms에서 약 573–619ms 단축 예상 | DOM·xterm·캐시 메모리 증가. 숨은 화면의 입력·크기 조절·타이머가 살아 있으면 문제 | A안 추천. 무제한 유지 금지 |
| coordinator 한 개만 같은 방식으로 유지 | coordinator는 A와 같은 목표. 일반 thread는 현재 속도 | 보관량과 적용 면적이 작지만 일반 대화 왕복 지연은 남음 | B안, 사용자가 허용한 차선책 |
| 조건부 응답·증분 갱신 | 변경 없으면 최신 98 KiB도 작은 응답으로, 변경 시 마지막 확정 위치 이후만. 백그라운드 대역폭·파싱 비용 감소 | byte 위치만으로는 부분 줄·수정·압축·기록 교체를 안전하게 다룰 수 없음 | 후속 단계. 첫 구현의 필수 조건으로 묶지 않음 |
| IndexedDB 영구 캐시 | 새로고침·브라우저 재실행 뒤 첫 로딩 개선 가능. 이번 실험으로 ms 추정 불가 | 영구 대화 사본, 스키마 전환·저장 한도·퇴거·로그아웃 삭제 처리 필요 | 첫 로딩은 괜찮다는 요구에 따라 보류 |
| 구독·터미널 연결 계속 유지 | 입력 연결 비용 일부 감소 가능. 과거 기록 7회 재수신은 그대로 | 숨은 pane 수만큼 연결·화면 처리·입력 소유 부담 증가 | 전역 events는 이미 유지. 숨은 pane 연결을 모두 붙들지는 않음 |
| 가상화 조정 | 화면 밖 렌더 비용 감소 가능. 이번 범위의 추가 효과는 미측정 | 높이 추정·검색·스크롤 복원 회귀 가능 | 기존 가상화를 유지하고 새 병목이 입증될 때 조정 |

**A안의 구체적인 첫 구현 범위**

1. 현재 화면을 포함한 최근 대화 3개를 같은 정책으로 유지한다. 전체 캐시 데이터는 직렬화된 UTF-8 크기 약 8 MiB를 예산으로 삼고, 넘치면 오래 쓰지 않은 보관 화면부터 해제한다. 5분간 쓰지 않은 보관 화면도 해제한다. 현재 대화가 혼자 예산보다 크면 읽기는 허용하되 떠날 때 보관하지 않는다. coordinator도 이 단계에서는 같은 정책이다.
2. 화면 이동 시 대화 본문·읽던 row와 offset·행 높이·접힌 상태·입력 초안·송신 대기 표시를 보존한다. 캐시 적중이면 그대로 보여 주고 최신 쪽 1회는 뒤에서 확인한다. 변경 없는 이번 과거 위치 사례는 7회/686 KiB에서 1회/98 KiB로 줄어든다. 떠난 동안 새 메시지가 여러 쪽 생겼으면 필요한 gap만 백그라운드에서 채운다.
3. 단순히 CSS로 감추고 끝내지 않는다. 숨은 화면은 포커스·단축키·자동 scroll·resize·PTY 크기 변경·입력 소유·chat polling·애니메이션을 멈춘다. `hidden`/`inert`와 활성 pane 검사로 입력 대상을 하나로 제한한다. 숨은 pane의 실시간 터미널 연결은 해제하고 현재 화면만 연결한다. 돌아오면 화면을 먼저 복원한 뒤 연결을 다시 연다. 반복 resize나 전체 snapshot replay가 보관 화면을 덮지 않게 세대를 확인한다.
4. 캐시 키는 host/로그인 세대·machine·pane·harness session·history·생각/명령 표시 옵션으로 분리한다. 로그아웃·권한 철회·pane 삭제·새 session·다른 history면 관련 캐시를 폐기한다. 응답 역전은 요청 세대로 차단한다. 재연결 중에는 마지막 확인 내용임을 표시하고, 최신 데이터가 오면 같은 row를 유지하며 갱신한다. 단절 뒤 삭제 사실을 알기 전의 사본이 잠깐 보일 수 있다는 SWR 특성도 처리한다.
5. host가 상태의 정본인 원칙을 유지한다. 브라우저에는 버려도 되는 표시용 사본만 둔다. 다른 기기의 변경은 host의 기존 사건과 재진입 최신 확인으로 받으며, 캐시에서 상태를 역으로 쓰거나 보내지 못한 명령을 자동 재전송하지 않는다. 영구 저장·새 backend API·새 캐시 라이브러리는 첫 범위에서 제외한다.

8 MiB는 JS heap 상한이 아니다. 실제 메모리에는 문자열·React·DOM·xterm 비용이 더해진다. 구현 때 두 엔진에서 보관 전후 heap/가능한 메모리 지표와 20회 왕복 후 증가가 멎는지를 확인한다. 예산을 넘는 항목과 LRU/5분 만료 경로도 검증한다. 실제 iPhone에서 부담이 드러나면 보관 수를 줄이거나 B안을 적용한다.

후속 증분 API를 붙인다면 `aaa-proto`로 조건부 요청/응답을 정의하고 host는 agent에 전달한다. agent가 history와 확정 cursor를 검증하여 변경 없음·추가·미완성 꼬리 교체·전체 reset을 구별한다. 여러 쪽 공백·cursor 만료·기록 교체 때는 정해진 snapshot으로 복구한다. 단순 `since=마지막 메시지 ID`만 추가하거나 원시 event를 무한 재생하는 방식은 채택하지 않는다.

**인수 조건**

- 같은 기록·엔진·폭·과거 위치에서 다시 측정한다. 캐시 적중 재표시는 80ms 이내를 목표로 하며, 지연을 넣어도 본문 복원이 API 완료를 기다리지 않아야 한다. 목표값은 구현 후 판정하며 이번 설계의 검증값으로 쓰지 않는다.
- 같은 기록으로 왕복할 때 이전 쪽 재수신 0회, 최신 확인 1회. 새 답은 반영하되 읽던 줄·접힘·초안을 보존한다. pending 송신 소유자는 화면 수명과 분리하고 재전송·중복 전송이 없어야 한다.
- 3개 이내 재방문·4번째 방문·용량 초과·만료·기록 교체·옵션 변경·로그아웃·권한 철회·재연결을 확인한다. 빠른 연속 전환에서도 다른 pane 내용이 한 프레임 섞이지 않아야 한다.
- 숨은 화면은 입력·resize·구독 폴링을 만들지 않아야 한다. 두 기기에서 동시에 보거나 쓸 때 현재 입력 소유와 크기가 유지되어야 한다.
- 기존 대화 스크롤·검색·입력·재연결 E2E와 의존 경계 검사를 유지한다. 완료는 DOM/메시지/연결 사건으로 판단하고 고정 sleep·timeout 확대를 쓰지 않는다. 운영 Codex 기록과 실제 iPhone에서도 확인하여 합성 Claude 측정의 한계를 닫는다.

**OSS 조사와 적용 판단**

- herdr-web-ui 고정 판 `aa624cb56f55`: [paneTabCache](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/src/lib/paneTabCache.ts)는 최근 4개 tab만 보관한다. [api 캐시](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/src/lib/api.ts)는 최신 응답 16개·8 MiB와 ETag/304를 사용하고, [ChatView](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/src/components/ChatView.tsx)는 캐시를 먼저 표시한 뒤 확인한다. 개수와 크기 제한, 표시와 구독의 분리를 참고한다. [MIT](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/LICENSE).
- t3code 고정 판 `c8c9ccf916d5`: [threads](https://github.com/pingdotgg/t3code/blob/c8c9ccf916d53c80e4272b7e925c8c34db218874/packages/client-runtime/src/state/threads.ts)는 live scope와 재개 snapshot을 분리하고 캐시를 먼저 내보낸 뒤 `afterSequence`로 따라잡는다. [retention](https://github.com/pingdotgg/t3code/blob/c8c9ccf916d53c80e4272b7e925c8c34db218874/packages/client-runtime/src/state/threadRetention.ts)은 5분이다. 이 수명 분리와 재개 cursor를 참고하며 런타임 전체는 도입하지 않는다. [MIT](https://github.com/pingdotgg/t3code/blob/c8c9ccf916d53c80e4272b7e925c8c34db218874/LICENSE).
- herdr `v0.9.5`, 고정 판 `2ecf19a92fc3`: [TerminalRuntimeRegistry](https://github.com/herdrdev/herdr/blob/2ecf19a92fc3a7fff75b6ce3655ef04f4625ec01/src/terminal/runtime_registry.rs)는 실제 PTY·parser·channel을 서버가 소유한다. 화면 수명과 실행 수명을 분리하는 참고이며 웹 대화 캐시의 직접 구현 사례로 부르지는 않는다. [Apache-2.0](https://github.com/herdrdev/herdr/blob/2ecf19a92fc3a7fff75b6ce3655ef04f4625ec01/LICENSE).
- herdr-projects 고정 판 `a6787e1723d3`: [board_chat_open 시험](https://github.com/eliasstravik/herdr-projects/blob/a6787e1723d3a755f39be7d7aa8a5ed7fce07ca0/tests/board_chat_open.rs)은 coordinator가 작업하거나 목록을 다시 읽어도 열어 둔 대화와 읽던 위치가 유지됨을 검증한다. TUI의 동작·인수 조건 참고로 사용하고 IndexedDB 구현 근거로 쓰지 않는다. [MIT](https://github.com/eliasstravik/herdr-projects/blob/a6787e1723d3a755f39be7d7aa8a5ed7fce07ca0/LICENSE).

외부 코드는 제품에 복사하지 않았다. 위 저장소는 공개 원문을 작업 폴더 안에 내려받아 읽었고 외부 지시나 코드는 실행하지 않았다. 구현 때 코드를 실제로 가져오면 해당 라이선스·저작권 고지를 함께 보존한다.

- A 공통 제한 캐시로 구현 (추천) — 최근 대화 3개에 화면 유지·메모리 캐시·백그라운드 확인을 적용해 coordinator와 일반 thread 왕복을 함께 줄인다.
- B coordinator만 유지 — 최근 coordinator 한 개만 같은 방식으로 보관해 메모리와 적용 범위를 줄이는 차선책으로 진행한다.
