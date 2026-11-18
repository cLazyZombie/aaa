# 기획 대화 CLI 그림 첨부 (t-1993)

- `aaa` m13의 `design_chat_thread_on_the_repo_machine_saves_only_its_own_branch`에서 실제 host·원격 agent·Git으로 확인한다. Front 소켓이 없는 기획 대화 pane에서 512KiB를 넘는 PNG를 붙여 CLI의 기본 경로 출력과 `--dir`의 JSON 응답을 확인한다.
- 내용 해시 파일 이름, 작성한 thread ID가 든 commit, 같은 그림을 다시 붙일 때 같은 commit을 확인한다. origin의 기획 branch에서 시작한 worktree에서 PNG 원본 바이트를 대조하고 main checkout도 확인한다.
- 다른 기획 branch와 main, 기획 폴더 밖·상위 경로·절대 경로, 문서 파일과 16MiB를 넘는 파일은 거부한다.
- 첫 실행에서 첨부 요청이 계정 로그인 권한 분기에 들어가 원격 pane의 첫 첨부가 거부됐다. 해당 요청을 기획 권한 분기로 보내고 같은 통합 시험에서 첨부와 권한 거부를 확인했다.

```sh
aaa verify -- nice -n 15 cargo test --profile verify --locked -p aaa --test m13 design_branch
```
