# 빈 alt 그림·영상 회귀 검증(t-1351)

## 원인과 변경

`web/src/herdr/markdown.ts`의 marker는 링크 제목을 한 글자 이상 요구했고, 빠른 사전 검사도 같은 조건이었다.
두 조건을 함께 고쳐 `![](library/…)`를 기존 image node로 전달한다. 영상 여부와 경로·권한 판정은 기존 renderer와
카드·보고별 resolver가 그대로 맡는다. 빈 일반 링크는 원문 text로 보존하고, 미디어를 표시할 수 없는 곳의 빈 alt도
`![](주소)` 대체 글을 남긴다. 새 의존성·API·미디어 요청 정책 변경은 없다.

문서와 `aaa guide questions`는 설명이 있는 `![시안](…)`·`![시안 영상](…)`을 권장하고 빈 alt도 지원한다고 명시한다.

## 참고 구현과 라이선스

- [herdr-web-ui 고정 판 markdown.ts](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/src/lib/markdown.ts)
  ([MIT](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/LICENSE)):
  marker와 label 사전 검사 모두 같은 제한이 있다. 기존 선형 스캔과 image·fallback 구조를 유지하며 제한을 보완했다.
- [herdr v0.9.5](https://github.com/herdrdev/herdr/tree/v0.9.5)
  ([Apache-2.0](https://github.com/herdrdev/herdr/blob/v0.9.5/LICENSE)):
  저장소 파일 목록과 README를 확인했다. 터미널 runtime이고 보고용 웹 Markdown 파서는 없어 이 변경의 직접 구현은
  herdr-web-ui를 따른다. 외부 코드를 추가로 복사하지 않았다.

## 꼭 필요한 회귀 시험

- `web/test/markdown.test.mjs`: 빈 alt 그림·MP4·WebM, 기존 alt와 상대 주소·fragment·인코딩, 인접 미디어를 검사한다.
  빈 링크가 사라지거나 미디어로 바뀌지 않는지, 표시할 수 없는 그림의 대체 글·기존 링크·강조·code·깨진 문법을 보존하는지 확인한다.
  원본 파서에 새 시험을 적용하면 빈 alt·빈 일반 링크·강조 안 빈 alt의 세 시험이 실패한다.
- `web/e2e/specs/115-empty-alt-media.mjs`: 실제 host·agent가 수집한 보고·library를 사용한다.
  Chromium 데스크톱·WebKit 휴대폰에서 질문 카드와 보고 본문 각각의 그림 decode, 빈 alt 기본 영상 이름,
  기존 alt, 두 영상의 실제 재생, 최초 누름 전 src 없음, 같은 주소 해석, 빈 링크 원문과 선택 보존을 검사한다.
  그림 decode와 영상 timeupdate 사건으로 끝내며 정한 시간만큼 자지 않는다.
- 기존 E2E 53·69는 질문 원문·선택지·thread/coordinator 공통 카드·답 사본 계약의 회귀를 확인한다.

## 실행 결과

- 웹 타입 검사·production 번들·웹 유닛 전체: 667개 통과.
- 기존 질문 E2E 53·69: 두 spec 전체 통과. 해당 실행의 115 실패는 아래 시험 작성 오류로 구분한다.
- 최종 E2E 115: 준비 807ms, Chromium 카드·보고 1845ms, WebKit 카드·보고 3148ms. 새 개별 단계는 모두 5초 이내다.
- `cargo test --profile verify --locked -p aaa-host --lib guide::tests::`: 4개 통과.
- `cargo clippy --profile verify --locked -p aaa-host --lib -- -D warnings`: 통과.
- `cargo fmt --all --check`: 통과.
- 공개 저장소 검사: 통과. 후보 커밋 메타데이터 결과는 보고 첨부에 남긴다.

### 첫 실행에서 확인한 시험 작성 오류

제품 코드는 첫 웹 검사 뒤 추가로 바꾸지 않았다. 새 E2E 작성 과정에서 다음 원인을 고쳤으며
시한 연장·자동 재시도·제품 단언 완화는 하지 않았다.

1. 첫 실행은 카드 주소에도 보고서의 `/v/<cap>/<파일>` 꼴을 가정했다. 카드 자료는 `library/`를 포함한다.
2. 다음 실행은 CLI의 `questions.files_base`를 비교했다. `web.rs`의 `attachment_views`·`attachment_base`는
   웹 응답에서 API 경로를 기기별 `/v/<cap>/`로 바꾼다. 최종 시험은 같은 기기의 실제 `thread_show` 웹 응답과
   실제 `library_open` 응답에서 기준 주소를 얻어 파일 경로를 정확히 비교한다. 기존 alt와 빈 alt의 주소도 같아야 한다.
3. 기본 fake는 `library/index.html`을 만든다. 보고 화면은 이 경우 HTML iframe을 우선하므로 Markdown 제목 대기가
   실패했다. 독립 시험 fixture에서 그 파일을 지우고 새 보고를 수집하여 실제 Markdown 보고를 연다.

초기 개발 명령 실행 중 시험 파일과 문서를 고쳐 workspace 측정 요약의 출처 검사가 거부된 기록도 남겼다.
개별 검사 결과와 실패 원인을 구분하며 이를 착지 gate 통과로 쓰지 않는다.

최종 spec 수정 뒤 파서·E2E 규칙·고정 sleep 검사 17개도 통과했다.
착지 창의 최종 통합 SHA gate는 별도다. `guide.rs` 수정이 포함되므로 Git 범위 판정은 full이며,
개발에서는 관련 host owner와 웹 검증을 수행했다. 이 작업은 병합·push·배포하지 않는다.
