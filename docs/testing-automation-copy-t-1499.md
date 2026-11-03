# 자동화 보고·기억 회수 수리(t-1499)

## 원인

자동화 기억은 agent가 `WorkFileList` 또는 구판 `MissionStateList`에서 원본 크기와 SHA-256을 계산하고,
`WorkFileRead`/`MissionStateRead`의 `FileChunk`로 전송한다. host Agent link는 이 응답을 파일 보기와
같은 표시 자료로 간주해 비밀값 조각을 가렸다. 고정 파일도 전송 뒤 바이트가 바뀌므로 원본 해시와 맞지 않는다.

또한 보고 수집은 library와 불변 보고 판을 먼저 저장한 뒤 기억을 회수하고, 기억 회수에 성공해야
`threads/<ID>.md`, `report_hash`, 완료 단계와 보고 시각을 갱신했다. 기억 검증 실패가 보고 전체를
막아 library만 보이고 자동 종료에 필요한 새 완료 보고가 없는 상태가 된다.

이는 코드 경로와 고정 입력으로 재현한 원인이다. 운영에서 어느 파일·조각이 가림에 걸렸는지는
당시 원문을 수집하지 않았으므로 특정하지 않는다. 특정 배포 변경이 직접 원인이라는 결론도 내리지 않는다.

## 수리 계약

- 요청 ID와 살아 있는 대기 요청의 종류를 묶어 작업 파일 목록·조각만 원본으로 전달한다.
  `FileView` 등 파일 보기의 `FileChunk`는 기존 가림을 유지한다. 새 wire 태그나 기능 협상을 추가하지 않는다.
- 기억 검증 실패도 새 보고 본문·완료 단계·ack를 보존한다. 사본 상태는 `failed`이며 원인을 남긴다.
- 기억 hash·크기·symlink 검사와 3-way 병합, host 원문 보존을 유지한다.
  마지막 사본에 실패한 resolve는 계속 거부한다. 시한이나 재시도 횟수는 바꾸지 않는다.
- 전송 오류에는 대상 상대 경로와 크기/해시 검증 단계를 남기며 원문·해시값은 기록하지 않는다.

## 공개 구현 참고

설계 전에 다음 둘을 비교했다. 코드는 복사하지 않았고 의존성을 추가하지 않았다.

- [Syncthing 동기화](https://docs.syncthing.net/users/syncing.html)와
  [블록 교환 계약](https://docs.syncthing.net/specs/bep-v1.html),
  [MPL-2.0](https://github.com/syncthing/syncthing/blob/main/LICENSE):
  전송 검증에 원본 바이트와 hash를 함께 유지하는 원칙을 따랐다.
- [rclone local](https://rclone.org/local/),
  [MIT](https://github.com/rclone/rclone/blob/master/COPYING):
  원본이 변한 경우 검증을 느슨하게 하지 않고 실패로 남기는 동작을 따랐다.

## 회귀 시험

`aaa-host::link::work_file_tests`는 실제 host·DB·Agent link frame과 가짜 agent를 연결한다.
운영 서비스나 다른 세션은 사용하지 않고, 고정 sleep 없이 요청·응답 완료로 끝난다.

- 신형 작업 파일 전송: 384KiB 조각 경계에 가림 대상 시험 조각을 넣은 고정 원본을 회수한다.
  원본 크기·해시와 바이트, 기존 host history 보존, 새 완료 보고·ack·종료 판정과 마지막 사본을 확인한다.
- 구판 Mission 파일 전송: 같은 계약을 기존 요청 태그로 검사한다.
- 변조된 기억: 목록 뒤 첫 조각을 바꿔 검증 실패를 만든다. 새 완료 보고는 남고 host 기억은 바뀌지 않으며
  resolve가 실패하는지 확인한다. 명시적으로 결함을 제거한 뒤 같은 보고 판의 마지막 사본이 복구되는지도 본다.
- 위 시험은 같은 연결의 `FileView`가 계속 가려지는지 함께 확인한다.
- 요청 취소: 신형·구형 목록/읽기 요청이 실제 frame에 실린 뒤 취소하고, 응답 대기와 원본 전송 예외가 남지 않는지 확인한다.

이 시험들은 보고 누락·고정 파일 해시 불일치·실제 변조 허용·표시 경계 가림 해제와 취소된 요청의 대기 누적을 막기 위해 필요하다.

## 검증 결과

수리 전 회귀 3개가 모두 실패했다. 정상 파일의 신형·구형 전송은 `기억 hash가 바뀌었다`로,
변조 기억은 완료 보고의 `report_hash=None` 단언으로 실패했다(합계 0.75초).
수리 뒤 같은 3개는 0.80초에 모두 통과했다.

첫 관련 owner 검사에서 835개 시험은 통과했으며 기존 2개는 ignored였다.
strict Clippy는 새 응답 wrapper의 큰 오류 반환값을 지적했다. 실패 경로에서만 오류를 Box로 감싸
동작·원문을 보존하면서 지적을 수리했다. 요청 취소 회귀도 추가해 관련 owner를 다시 확인했다.
시험 작성 중 잘못된 바이트 문자열과 match 소유권에 대한 컴파일 오류도 수정했다.

최종 개발 검사:

```sh
aaa verify -- nice -n 15 python3 scripts/verify.py develop --base 6d56d5b3 --package aaa-host
aaa verify -- cargo fmt --all -- --check
```

- `aaa-host`: 회귀 4개를 포함해 836개 통과, 기존 수동 실행 전용 2개 ignored(19.05초).
  ignored는 설치된 Codex의 무인증 설정 확인과 실제 외부 HTTPS 알림 시험이다. 새로 제외한 시험은 없다.
- 같은 owner의 모든 target strict Clippy(`-D warnings`) 통과.
- 공개 저장소 검사와 fmt, Git 공백 검사 통과.
- backend 의존성·protocol·agent 코드·Windows 전용 경로는 바꾸지 않았다.
  전체 착지 gate와 OS cross는 승인받은 착지 창의 최종 통합 SHA에서 수행한다.
