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

웹 목록 아래 「보고서」에서 프로젝트·이름·크기·갱신·만료와 링크 상태를 본다. 복사·QR·폐기·재발급은 thread 화면과 같다.
Reader는 공유 링크를 받지 않고 앱 안 보기만 한다. `aaa doctor`의 사본 바이트 합계에는 실시간 원본을 넣지 않는다.

```sh
aaa library prune --older-than 90 --project P
aaa library rotate
```

prune은 최근 게시/회수가 90일보다 오래된 사본을 삭제한다. 진행 중 thread·실시간 원본·report.md·thread 기록은 남긴다.
자동 삭제는 없고, 삭제된 보고서의 공유 링크와 앱 안 보기 경로는 끝난다.
rotate는 보고서 서명 키를 교체해 **기존 공유 링크를 모두 끝낸다**. pairing·TLS 키·보고서는 남으며 앱 안 보기도 된다.
필요한 공유 링크는 `aaa library link P NAME` 또는 웹 「새 링크 만들기」로 다시 발급한다.
