# 머신별 workspace 자동 배정 허용

머신 이름을 누르면 오른쪽 기기 정보 패널 맨 위에 workspace가 나온다. Owner(웹 조작 기기·aaa 세션 밖 CLI)는 각 저장소와 「작업 폴더」를 켜고 끈다. Reader는 현재 값만 읽는다. safety·profile 권한이나 설정을 바꾸지 않는다.

기존 `project.toml [[repos]]` 또는 `aaa.toml [[workspaces]]`의 path·machine이 저장소 위치의 유일한 정본이다. 허용은 경로를 복사하지 않고 host DB의 `workspace-access:<project>`에 머신 ID별 workspace 이름 집합으로 저장한다. 저장소 이름은 할 일 저장소 칸과 같은 마지막 폴더 이름이며, 같은 이름의 여러 머신 경로는 한 workspace다. 작업 폴더는 저장소가 없는 일이며 각 agent의 작업 root를 쓴다. 이 허용은 자동 배정 정책이며 파일 접근 권한이 아니다.

## 처음 전환과 새 항목

- 처음 새 host를 실행할 때 DB에 등록된 모든 머신 × 기존 모든 프로젝트의 모든 workspace(작업 폴더 포함)를 켠다. 경로가 없는 조합도 허용은 켜지만, 저장소가 실제 등록된 머신에서만 기존 배정 규칙대로 실행한다. 전환 marker와 허용값을 한 SQLite transaction으로 저장한다.
- marker는 한 번만 적용한다. 이후 재시작·재배포 때 사용자가 끈 값을 다시 켜지 않는다. 이 작업은 배포된 host 시작 때 일어난다. 개발용 별도 host DB에서 시험하며 운영 설정은 작업 thread가 건드리지 않는다.
- 새 머신·새 프로젝트·새 workspace 이름은 기본 꺼짐을 추천하며 구현도 이를 따른다. 사용자가 그 머신에서 작업할 준비를 확인하고 켠 뒤 자동 배정에 참가한다. 기존 조합 보존과 이후 미설정 머신 제외를 함께 만족한다.
- workspace 이름이 같고 경로만 옮기거나 같은 이름의 저장소 경로를 추가하면 같은 허용을 쓴다. 머신 ID가 바뀌는 재등록은 새 머신이며 꺼짐이다. 프로젝트 이름 변경·삭제는 기존 DB 프로젝트 metadata 이전·삭제 경로를 따른다.

## 배정

`thread start --from-task`와 예약 시작 모두 허용을 읽는다. 자동일 때 연결·점검·저장소 보유·허용 조건을 만족하는 머신 중 독점 예약·착지 중 머신을 빼고 열린 thread÷비중 순서로 고른다([머신 배정](machine-assignment.md)). 저장소 칸이 비면 작업 폴더 허용을 본다. 등록되지 않은 임의 경로는 자동 배정하지 않는다. 예약은 후보가 없으면 기동하거나 설정 확인 상태로 바꾸지 않고 「자리 대기: … 자동 배정이 허용된 연결 머신이 없다」를 남긴다. 다른 예약은 계속 판정한다.

사용자가 할 일이나 CLI·웹에서 고정한 머신은 존중한다. 자동 허용이 꺼졌으면 경고와 함께 시작한다. 저장소가 그 머신에 없는 경우는 기존대로 거부한다. 실행 중인 thread는 끄기 영향을 받지 않는다. 계정 배정 대기 중인 자동 시작은 실제 기동 직전에 현재 허용·경로를 다시 확인하고, 달라졌으면 대기 상태를 유지한다. 자동화의 명시된 do.start 머신 목록은 이 작업의 자동 task 배정 범위와 별개다.

웹 「바로 시작」의 기본 머신 항목은 자동이며 요청에도 machine=null을 보낸다. 사용자가 직접 머신을 고른 경우만 고정값을 보낸다. 저장 응답이 오기 전에는 체크값을 성공 상태로 보이지 않고, 다른 기기의 변경은 task 상태 사건으로 다시 읽는다. revision이 달라진 저장은 conflict로 거부한다.

## CLI·Front API

```sh
aaa machine workspaces mac-a
aaa machine workspaces mac-a --allow app
aaa machine workspaces mac-a --deny '작업 폴더'
```

FRONT_API 1.117: `workspace_access { project?, machine }` → `workspace_access { project, machine, revision, workspaces: [{ workspace, enabled, paths }] }`.
`workspace_access_set { project?, machine, workspace, enabled, revision }` → 같은 최신 view. Reader와 세션 안 agent의 쓰기는 forbidden, 판 충돌은 conflict, 없는 workspace는 bad_request다. thread_start_preview의 workspace_access는 머신 이름 → 허용 workspace 이름 목록이다. AGENT_LINK는 바꾸지 않는다.

## 참고

[herdr-projects v0.2.35 project.rs](https://github.com/eliasstravik/herdr-projects/blob/v0.2.35/src/project.rs)의 Repo(path·machine)와 원자적 설정 변경, [MIT 라이선스](https://github.com/eliasstravik/herdr-projects/blob/v0.2.35/LICENSE)를 확인했다. [herdr-web-ui 고정판 AgentSidebar](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/src/components/AgentSidebar.tsx)의 머신 안 workspace 구분과 [workspacePath](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/src/lib/workspacePath.ts)의 폴더 기준 표시, [MIT 라이선스](https://github.com/devswha/herdr-web-ui/blob/aa624cb56f550f9a7dd8bf4d59ed99ffc3921741/LICENSE)를 확인했다. 두 프로젝트에 같은 자동 배정 허용 정책은 없어 기존 aaa host의 DB·권한·task 큐를 확장했다. 외부 코드나 의존성을 추가하지 않는다.
