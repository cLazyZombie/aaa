# 기획 대화 CLI 그림 삭제 (t-1995)

- `aaa` m13의 `design_chat_thread_on_the_repo_machine_saves_only_its_own_branch`에서 실제 CLI·host·원격 agent·Git을 실행했다. 같은 PNG를 기획 폴더와 하위 폴더에 첨부하고, 옛 경로를 지운 뒤 문서 링크를 새 경로로 고쳤다. 삭제만 담긴 commit에 thread ID가 남고 origin에서 받은 branch에는 새 위치의 그림과 링크가 남았다.
- 상대 참조의 쿼리·절 표지를 제외한 경로가 같으면 문서 이름을 표준 오류에 알린다. JSON 응답에는 `warnings`가 있다. 대괄호가 있는 파일 이름은 문자 그대로 지우며, 비슷한 이름의 다른 파일은 남는다.
- 다른 branch·main·기획 폴더 밖·상위/절대 경로·Markdown·그림으로 이름 붙인 텍스트·이미 지운 경로를 거부했다. 사람 터미널과 thread 세션 ID를 흉내 낸 프로세스도 거부했다. main checkout은 그대로였다.

```sh
aaa verify -- nice -n 15 cargo test --profile verify --locked -p aaa --test m13 design_branch
aaa verify -- nice -n 15 cargo test --profile verify --locked -p aaa-agent -p aaa-host --lib design
aaa verify -- nice -n 15 cargo test --profile verify --locked -p aaa --test deps
aaa verify -- nice -n 15 cargo test --profile verify --locked -p aaa-proto typescript
```

참조 검사는 CommonMark 이벤트에서 실제 링크 목적지를 읽는다. 대안으로 트리 전체를 제공하는 Comrak(BSD 2-Clause)과 markdown-rs(MIT)를 검토했다. Pulldown Cmark(MIT)는 이벤트 순회만으로 링크를 읽을 수 있어 렌더링 기능을 끄고 사용했다. [Pulldown Cmark](https://github.com/pulldown-cmark/pulldown-cmark) · [Comrak](https://github.com/kivikakk/comrak) · [markdown-rs](https://github.com/wooorm/markdown-rs)
