# 기획 대화 그림 붙이기·반영 표지(t-1950)

## 원인

- 기획 대화 입력창은 그림을 일부러 막았다(`imagesOff`, t-1894 2-3). 대화 thread는 `aaa plan save`로 Markdown만 저장해 그림을 branch에 넣을 수 없었고, 길은 문서 「고치기 → 그림」뿐이었다.
- 머리의 「반영 대기」는 보는 문서가 아니라 고른 대화의 반영 thread를 가리켰고 thread ID가 보이지 않았다. 이미 main에 들어간(0 앞) 대화도 반영 thread가 끝나면 「main에 반영」 단추가 다시 나와 빈 반영 창을 열었다.
- 사건의 짝 말(옮기기 말처럼 앞 쪽에 있어 불러오지 않은 말)을 못 찾으면 맨 끝에 놓아, 몇 시간 전 사건이 방금 일처럼 보였다.

## 고친 것

- 입력창 그림은 `POST /api/v1/projects/<p>/design-files/<저장소>/<주제>?dir=…&quiet=true`로 그 branch에 commit한다(「그림」과 같은 저장, `quiet`이면 「사람이 고친 문서」에 넣지 않는다). 초안의 `@경로`는 보낼 때 「(그림: 경로)」 줄이 된다. 대화 thread brief는 worktree의 그 파일을 보고 문서에 쓰라고 바꿨다.
- 「main에 들어감」: 열린 대화, main보다 0 앞, 끝이 갈라진 판과 다름. 「반영 대기 t-NNNN」은 그 thread 링크다. 보는 문서를 이 대화가 고친 적이 없으면 「이 대화가 고치지 않음」.
- 사건 자리(`web/src/chatMarks.ts`): 짝 말을 못 찾으면 바로 앞 사건 자리, 보내는 중인 말이면 맨 끝.

## 시험

| 계약 | 위치 | 실행 |
| --- | --- | --- |
| 사건 자리: 짝 말 뒤·맨 앞·못 찾으면 앞 사건 자리·보내는 중이면 끝 | `web/test/chatMarks.test.mjs` | `cd web && node --test test/chatMarks.test.mjs` |
| 그림 경로 떼기·그림 폴더(기획 폴더 밖·이름만 같은 폴더)·그림 칩·그림 붙임 사건 문구·main에 들어감 판정 | `web/test/designModel.test.mjs` | `cd web && node --test test/designModel.test.mjs` |
| 실제 host·agent·git: 반영 대기 thread 링크, 입력창 그림이 보는 문서 폴더에 commit·사람 고침에 없음·thread가 받은 줄 「(보는 문서)·(그림)·말」, 이 대화가 고치지 않음 | E2E `136-design-branches` | `ci-local --only web,e2e,public --spec 136` |
| 옮긴 대화가 main에 병합된 뒤 「main에 들어감」, 단추 없음 | E2E `137-idea-move` | `ci-local --only web,e2e,public --spec 137` |

- 사건 자리 시험은 옛 동작(못 찾으면 맨 끝)으로 되돌리면 셋 중 둘이 실패한다.
- 재현·시안 촬영은 저장소 밖 임시 E2E로 했다(fake thread pane에 Claude SessionStart hook으로 가짜 기록을 붙임).

## 남긴 것

- 그림 칩은 이름만 보인다(작은 그림 없음).
- 다른 대화가 고친 문서를 보며 붙인 그림도 그 문서 폴더(기획 폴더 안이면)에 들어간다.
- 붙인 뒤 떼거나 보내지 않은 그림도 commit은 그 branch에 남아 반영 때 main에 들어간다(이름이 내용 hash라 다시 붙여도 하나다).
- 이미 떠 있는 대화 thread는 옛 brief(「그림 붙이기는 아직 없다」)를 받았다. 「(그림: …)」 줄로 경로는 받는다.
