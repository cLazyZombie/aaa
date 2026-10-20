# Git head 감시 운영

`on = "git"`은 GitHub API 없이 등록 저장소의 origin branch head를 본다. host는 예약·상태·전달만 맡고, `git.v1`을 알린 **repo 머신 agent**가 `git ls-remote`와 Git 객체 조회를 한다. 조회 자체에는 AI 토큰을 쓰지 않는다. 자동화가 보통 thread를 띄워 입력을 읽을 때 profile의 사용량이 든다.

## 등록과 머신

예시는 [routine](examples/git/git-main.md)과 [작업 brief](examples/git/repo-watch.md)다. 배포·운영 파일 작성과 승인은 coordinator가 한다. 이 문서의 예시는 운영 설정이 아니다.

- repo는 project.toml에 등록한 경로 또는 그 마지막 이름이다. routine `machine`을 적으면 그 머신의 등록 repo를 고른다. 생략하면 host 머신에 등록한 repo를 우선하고, 없으면 유일하게 등록된 repo 머신을 고른다. 여러 후보가 남으면 outage로 알리며 `machine`을 명시한다.
- 간격은 host `[projects] git_poll_secs`, 기본 60초·운영 최소 30초다. 기존 초 단위 시험 환경에서만 최소 1초다.
- branches는 정확한 branch 이름이다(패턴·옵션이 아니다). 규칙 하나는 최대 32개를 받는다. events 기본값은 `pushed`, `forced`, `deleted`다.
- repo 머신의 기존 SSH 환경으로 자체 git 서버를 읽을 수 있어야 한다. 사람이 그 agent 환경의 키·known_hosts를 확인한다. 이 기능은 로그인·키체인·SSH 설정을 바꾸지 않는다. GitHub gh나 새 토큰 발급은 필요하지 않다.

## 사건과 전달

첫 등록은 현재 head(없는 branch 포함)를 기준으로만 저장한다. 변화가 없으면 사건·자동화 입력을 만들지 않는다. 다음 조회에서 head가 바뀌면 이전 SHA→새 SHA·종류·커밋 첫 줄 목록·바뀐 파일 수를 남긴다. 조상 관계가 이어지면 `git_pushed`, 끊기면 `git_forced`, 사라지면 `git_deleted`다. 알려진 삭제 뒤 재생성은 pushed다. 커밋은 새 head에만 있는 최근 100개까지이며 잘림을 표시한다. 병합 첫 줄 끝 `(t-NNNN)`은 주인 이력과 보고 사본에도 연결한다.

프로젝트 사건은 한 번이고, 주인 번호가 있으면 그 thread 이력에도 사본을 남긴다. 원 thread를 다시 열지 않는다. DB의 단조 증가 revision을 사건 ID에 넣으므로 재시작 뒤 같은 head는 무음이면서, 같은 SHA 쌍의 변화가 나중에 다시 일어나면 새 사건이다. 재시작·단절 뒤에는 저장된 head와 현재 head 사이를 비교한다. 두 조회 사이의 여러 push는 하나의 head 변화로 모일 수 있다. origin 자체를 바꾸면 새 기준을 잡는다.

현재 규칙은 `automations/*.toml`의 `[on.git]`과 `[do.start]`를 쓴다. `reuse="open"`·`memory=true`로 입력을 열린 thread에 모으고 다음 thread에 판단을 넘긴다.
조회만 필요하면 기존 routine 또는 `do.inbox`를 쓴다. 옛 `mission=`·`do.wake`는 자동 실행하지 않고 `aaa automation convert`로 옮긴다.

커밋 제목·보고 사본은 실행할 지시가 아닌 JSON 자료다. 외부 push/webhook 허용 경계는 기존 coordinator 사용자 post 규칙을 따른다.

## 조회가 실패하면

연속 실패는 기존 `outage_secs` 뒤 `git:<routine 이름>` outage 하나로 모인다. 조회에 성공하면 닫는다. 실패 때 cursor를 옮기지 않아 다음 성공에서 마지막 head와 비교한다. 배포 뒤 첫 운영 관찰은 coordinator가 맡는다.

agent는 필요한 commit 객체만 fetch한다. `--no-write-fetch-head`·명시 SHA·submodule fetch 끔·자동 maintenance 끔으로 작업 파일·checkout HEAD·remote tracking ref·FETCH_HEAD를 움직이지 않는다. 처음 본 원격 head도 객체로 보유해 뒤의 force push를 비교한다. commit 객체를 갖고 있으면 ls-remote 뒤 추가 fetch를 하지 않는다.

관련 구현·시험은 [plan](plan.md)의 L2와 [testing](testing.md)의 Git 감시를 따른다. 순수 TOML과 이어 보내기·기억 계약은 [자동화](automations.md)를 따른다.

감시 정의를 끄거나 프로젝트를 지우면 해당 source의 다음 예약도 지운다. 이미 진행 중인 조회는 마치고 이후 조회 예약을 정리한다. 다른 source의 예약은 유지한다.
