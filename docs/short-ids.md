# 짧은 번호

표시는 최소 폭이며 최대 자릿수가 아니다. t·d·r·q·a·i는 최소 네 자리, p는 기존 최소 여덟 자리다. CLI 입력과 앱 링크는 앞의 0 개수에 상관없이 같은 양의 정수를 찾는다. 정본·DB·파일명은 그대로 둔다.

| 종류 | 생성·조회 | 순서와 경계 |
| --- | --- | --- |
| t thread | DB 숫자 최댓값+1, 요청의 thread 선택자 정규화 | DB의 정수 CAST, CI 병합 소유자, 같은 시각 자동화 thread 선택 |
| d 할 일 | 프로젝트별 number↔ULID 원장 | 목록 위치·예약 position·보관 ULID·기록 정수 id 유지, 제목보다 번호 우선 |
| r 보고 판 | 프로젝트별 seq 원장 | 판은 seq DESC, 본문·manifest 파일은 정본 번호, 목록/가져오기/정리의 같은 시각 번호는 숫자순 |
| q 질문 | 프로젝트별 seq·질문 출처 원장 | 번호 별칭도 같은 답·draft·자동 답 시도 기록 사용, 프로젝트 생략 중복 거부 |
| a 자동화 | 프로젝트별 number↔이름 원장 | 기존 이름·파일·규칙 순서는 유지, 번호 입력은 기존 이름으로 해석한 뒤 권한 판정 |
| p post | 전역 SQLite 정수 seq, 32자리 발신 ID와 별도 | 표시 폭 8 유지, post 조회 별칭, 목록 seq·rowid 정렬 유지 |
| i inbox | 프로젝트별 number↔기존 저장 ID 원장 | list·context·웹·알림은 별칭을 표시하고 done은 두 ID 모두 받음. 완료·prune 뒤에도 번호 예약 유지 |
| m·옛 q·기타 | 무작위 ULID 부분·ULID·해시 | 순번으로 바꾸지 않음. inbox는 시각 우선·같은 시각 대상 번호순, 답한 기록은 시각·삽입순 |

순번 parser는 ASCII 숫자 전체와 SQLite i64 범위를 확인한다. 0·부호·부분 일치·범위 초과를 받지 않는다. 웹은 숫자로 변환하지 않고 0을 제거해 큰 번호를 반올림하지 않는다. prefix가 다른 ID를 서로 바꾸지 않는다. 프로젝트가 명시된 주소는 범위를 유지하고, 맨 번호가 여러 프로젝트에 있으면 임의로 하나를 고르지 않는다. 실제 대상이 없는 맨 번호는 링크하지 않는다. 주소·경로·파일 이름·코드 블록·이미 있는 링크는 일반 텍스트 자동 링크에서 제외한다.

이관한 보고서의 이름·기존 자동화 이름·옛 확인창의 무작위 q ID가 숫자 별칭과 겹치면 모호한 요청을 거부한다. 자동화 새 이름·본문·인증/발신 ID는 정규화하지 않는다. 기존 원장과 파일을 유지한다. inbox의 `id`는 모든 API에서 기존 저장 키이며 질문 목록·웹 종료 요청·파일 연결에도 같은 값을 쓴다. 선택적 `short_id`만 표시용 별칭이며 CLI 일반 출력·웹·알림은 그 값을 보이고, 구형 host처럼 없으면 id를 보인다. JSON은 저장 id와 short_id를 함께 내보낸다. DB JSON·투영 파일의 ID와 파일명은 기존 값이다. 별칭 원장 `inbox_ids`는 기존 항목을 시각·저장 ID 순으로 한 번 채우고 새 저장과 원자적으로 추가한다. `i-1`·`i-0001`·`i-00001`은 같은 프로젝트의 같은 항목이다. 다른 프로젝트를 검색하지 않고, 옛 저장 ID와 번호가 서로 다른 항목을 가리키면 거부한다. 프로젝트 rename·내보내기·삭제에 원장도 함께 옮긴다.

시험 도구의 instance 이름·Windows 시험 폴더도 최소 네 자리 이상을 받으며 끝 네 글자를 잘라 대조하지 않는다. E2E의 thread 번호 추출도 같은 최소 폭을 쓴다. 제품의 Windows 전용 경로 처리·서비스 동작은 바꾸지 않는다.

참고는 구성과 경계 판단에만 썼으며 코드·의존성을 가져오지 않았다.

- [herdr의 ID 해석](https://github.com/herdrdev/herdr/blob/master/src/app/ids.rs), [Apache-2.0](https://github.com/herdrdev/herdr/blob/master/LICENSE): 표시 ID·숫자 별칭을 실제 대상과 대조한다.
- [Ovis/옛 herdr-web-ui의 깊은 주소](https://github.com/devswha/ovis/blob/main/src/lib/deepLink.ts), [MIT](https://github.com/devswha/ovis/blob/main/LICENSE): ID와 머신/주소의 범위를 유지하고 번호 재사용으로 다른 대상을 고르는 일을 막는다.
- [herdr-projects의 task 조회](https://github.com/eliasstravik/herdr-projects/blob/main/crates/core/src/store/tasks.rs), [MIT](https://github.com/eliasstravik/herdr-projects/blob/main/LICENSE): 내부 ID·표시 번호·저장소 키를 구별한다.
- [GitHub 자동 참조](https://docs.github.com/en/get-started/writing-on-github/working-with-advanced-formatting/autolinked-references-and-urls)와 [Jira 키 형식](https://confluence.atlassian.com/adminjiraserver/changing-the-project-key-format-938847081.html): 번호와 프로젝트/저장소 범위를 함께 대조하는 설명을 참고했다. 문서의 형식 설명만 참고하며 외부 서비스의 별칭 동작을 aaa 계약으로 가정하지 않는다.
