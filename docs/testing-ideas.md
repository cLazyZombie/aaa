# 아이디어 기능 검증(t-1260)

기획 t-1255 r-4673(결정 28개)의 일곱 부분을 실제 host·agent·PTY와 브라우저로 확인한 기록이다. 정한 시간을 자지 않고 host 기록·PTY 출력·DOM 표지로 끝낸다.

## 새 시험과 까닭

| 시험 | 꼭 필요한 까닭 |
| --- | --- |
| host `ideas::tests::revisions_conflicts_and_user_edits` | 판 계약(지금 판으로만 쓰고, thread 겹침은 두 판·사람 겹침은 `merge/`)과 고친 요약·n-ID 원장 |
| host `ideas::tests::handoff_locks_and_copy_reopens` | 넘기기(초안마다 할 일·메모 첫 줄 링크)·잠금·복사와 thread 시작 때 넘길 노트·그림 |
| host `ideas::tests::summary_counts_lines_per_section` | 「(노트 고침: …)」 요약 꼴(결정 16) |
| host `ideas::tests::ids_and_media_names` | 폴더·그림 이름 거름(폴더 밖·바깥 경로를 읽지 않는다) |
| host `idea_thread::tests::only_the_ideas_own_thread_saves_from_a_session` | 보기 권한 thread 세션이 자기 노트만 저장하는 권한 경계 |
| host `idea_thread::tests::rests_only_idle_idea_threads_after_the_window` | 조용히 내리는 때(결정 20)의 경계 |
| 웹 `test/ideas.test.mjs` | 휴대폰 페이지 쓸기(결정 24)·고친 노트 머리·thread가 받는 인용 꼴·그림 주소 거름 |
| 웹 E2E 121 `121-ideas.mjs` | 넓은 창 흐름(고치기·그림·의견·말하기·thread 저장)과 실행 한도 밖·보류 뒤 잇기·넘기기·첨부·복사, WebKit 390 쓸기 |

## 결과(10-28, 로컬 Mac 하나)

- `cargo test -p aaa-host -p aaa-config -p aaa-proto -p aaa-agent --lib`: 통과(host 766, agent 186).
- `cargo test -p aaa --test m13 task_board_reads_tasks_and_follows_file_edits`·`--test deps`: 통과.
- `npm test`(web): 653개 통과. `npm run typecheck` 통과.
- 웹 E2E 121: 5.9초 통과. 「나중」을 걷은 43·76·83과 의견 모음 120·보류 85·앱 틀 04: 통과.

- 전체 `scripts/ci-local.sh`(df3b2140): web·fmt·build·clippy·test·public 통과, e2e는 121개 중 5개 실패. 활동 막대 단추 차례 기대값(26·57·62)은 아이디어 단추를 「답할 것」 위에 두고 기대값을 고쳤고, 46의 「나중」 단계는 뺐다. 103(대화 보관 계측)은 부하 평균 29~80에서 「페이지가 답하지 않음」으로 실패했고 같은 실행 파일로 따로 돌려 통과했다. 고친 뒤 26·46·57·62·103·121을 다시 돌려 모두 통과. `--only linux-clippy` 통과.

## E2E에서 찾아 고친 것

- 보류한 아이디어 thread를 잇기(같은 대화 restart)가 실행 한도에 막혔다(`thread_restart_claim`이 자동화만 뺐다). 아이디어 thread도 뺀다. 한도 1과 일반 thread 하나를 둔 E2E 121이 그 경로를 지킨다.
- 다른 화면에 다녀와 다시 열 때 잇기를 하지 않았다(본문을 숨긴 채 두어 한 번만 깨웠다). 숨으면 다시 깨운다.
- 노트 절 이름: Markdown 렌더러는 머리를 `data-markdown-heading`으로 그린다(h2가 아니다).
- 목록 줄 제목이 높이 0으로 접혔다(전역 단추 높이). 줄 높이를 풀었다.
