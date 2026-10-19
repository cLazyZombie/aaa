# host 보고서 운영

보고서는 host가 `/v/`에서 같은 sandbox·서명 링크 규칙으로 보여 준다. 공유 링크는 기본 30일이며 폐기할 수 있다.
만료·폐기돼도 pairing한 앱에서는 「앱에서 열기」로 본다.

## 올리기와 실시간 폴더

host 머신의 사람·coordinator CLI에서 실행한다. 원격 머신 publish와 웹에서 로컬 경로를 등록하는 것은 지원하지 않는다.

```sh
aaa library publish "<보고서 폴더>" --project P --name summary
aaa library publish "<그림 파일>" --project P --name screenshot
aaa library publish "<저장소의 실제 폴더>" --project P --name design --live
aaa library list --project P
aaa doctor
```

같은 이름으로 publish하면 최신 파일로 바뀌고 없어진 파일도 사본에서 지운다. 유효한 링크는 계속 최신을 가리킨다.
`--live`는 복사하지 않는다. 등록 폴더의 수정·추가·삭제를 즉시 반영하며, 저장소 착지 때 다시 publish하는 routine은 필요 없다.
등록 원본을 옮겼으면 새 실제 폴더로 다시 등록한다. 원본 경로는 웹 목록에 노출하지 않는다.

이름은 소문자·숫자·`-`·`_` 1~100자다. `t-` 접두사는 새 thread의 이름 공간이라 받지 않고 Windows 예약 이름도 받지 않는다.
숨은 이름·symlink·일반 파일 아닌 것은 가져오지 않는다. 등록 폴더 밖 경로, `..`, 역슬래시, 드라이브/ADS 경로도 열지 않는다.
symlink를 원본으로 넘기지 말고 그것이 가리키는 실제 폴더를 명시한다.

## 여러 보고서 가져오기

```sh
aaa library import "<이전 보고서 루트>" --project P --prefix archive-
```

하위 폴더 각각을 보고서로 올린다. 예를 들어 `t-0001/`은 `archive-t-0001`이 된다. 기존 대상 이름과 충돌하면 덮지 않고 건너뛴다.
결과에는 가져온 목록과 **원본 항목별 건너뛴 이유**가 있다. 접두사 없이도 쓸 수 있지만 `t-` 이름들은 거부된다.
숨은 이름·symlink·낱 파일도 건너뛴다. 필요한 낱 파일은 `publish`하고 symlink 항목은 실제 폴더로 `--live` 등록한다.

복사와 실시간 등록은 모두 host.toml `[projects] library_copy_mb` 상한(기본 **200MiB**)을 따른다.
상한을 넘는 보고서는 일부 파일만 성공한 것처럼 올리지 않고 전체를 거부한다. 1~4GB 캡처 묶음은 건너뜀 목록에 남는다.
필요한 HTML·선택한 그림·로그를 상한 안의 별도 폴더로 준비해 publish한다. 큰 원본은 보관하고 상한을 임의로 늘리지 않는다.
숨은 파일이나 symlink에 기대던 HTML의 상대 자원은 그 항목을 빼면 표시되지 않으므로 이전 뒤 확인한다.

## 찾기·정리·키 교체

웹 왼쪽 활동 막대의 「보고서」가 본문을 보고서 화면으로 바꾼다. 프로젝트·이름·크기·갱신·만료와 링크 상태를 보고, 「열기」는 본문에서, 「새 창」은 새 창에서 연다. 복사·QR·폐기·재발급은 thread 화면과 같다.
Reader는 공유 링크를 받지 않고 앱 안 보기만 한다. `aaa doctor`의 사본 바이트 합계에는 실시간 원본을 넣지 않는다.

```sh
aaa library prune --dry-run
aaa library prune --dry-run --project P
aaa library prune --older-than 90 --project P
aaa library rotate
```

prune은 최근 게시/회수가 90일보다 오래된 사본을 삭제한다. 진행 중 thread·실시간 원본·report.md·thread 기록은 남긴다.
삭제된 보고서의 공유 링크(401)와 앱 안 보기 경로는 끝난다.

host ticker는 시작 뒤 첫 tick에서, 그 뒤 **6시간마다** 사본을 자동 정리한다. host.toml `[projects]` 설정은
`library_max_days = 30`, `library_max_mb = 20480`(20GiB)이며 생략하면 이 기본값을 쓴다.
각각 `0` 또는 `""`이면 해당 제한을 끈다. 둘 다 끄면 자동 정리를 하지 않는다.
기한을 넘은 사본을 먼저 지우고, 남은 합계가 상한을 넘으면 최근 게시·회수가 가장 오래된 순으로 지운다.
진행 중 thread는 항상 보호한다. 실시간 원본은 합계에도 세지 않으며 만료 정리도 하지 않는다.
보호 사본만으로 상한을 넘으면 그 회차에는 삭제하지 않고 doctor와 관련 프로젝트 inbox에 알린다.
같은 초과 상태에서는 재시작·inbox 완료 처리 뒤에도 다시 알리지 않는다. 정상으로 돌아온 뒤 새 초과가 생기면 다시 알린다.

`--dry-run`은 이 자동 규칙의 전체 후보·바이트·기한/용량 사유를 쓰기 없이 보여 준다.
`--project`는 후보만 제한하며 합계·상한은 host 전체다. `--older-than N`을 함께 주면 미리 볼 기한만 N일로 바꾼다.
실제 수동 prune은 기존대로 필수 `--older-than N`의 기한만 적용한다.
doctor와 웹 보고서 목록의 합계·상한·기한 역시 host 전체 값이다.
배포 전 새 binary로 dry-run을 확인한다. 이 요청은 FRONT_API 1.40부터이며 옛 host는 삭제하지 않고 거부한다.
rotate는 보고서 서명 키를 교체해 **기존 공유 링크를 모두 끝낸다**. pairing·TLS 키·보고서는 남으며 앱 안 보기도 된다.
필요한 공유 링크는 `aaa library link P NAME` 또는 웹 「새 링크 만들기」로 다시 발급한다.
