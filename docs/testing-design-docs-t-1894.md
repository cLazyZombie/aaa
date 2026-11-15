# 기획 1단계: 저장소 기획 폴더 읽기 검증(t-1894)

설계는 [plan §8.565](plan.md#8565-기획-저장소-기획-폴더-읽기t-1894), 사용법은 [usage 「기획」](usage.md#기획)에 있다.

## owner

| 계약 | 시험 | 실행 |
| --- | --- | --- |
| origin 기본 branch의 git 객체만 읽고 checkout HEAD·작업 파일·FETCH_HEAD는 그대로, 같은 이름의 로컬 branch(`refs/heads/origin/main`)에 가려지지 않음, 기획 폴더 밖·없는 문서 거부 | `aaa-agent` `design::tests::tree_reads_origin_main_without_touching_checkout`·`read_refuses_outside_and_missing` | `cargo test -p aaa-agent --lib design::` |
| 트리를 읽은 commit으로 고정한 읽기는 origin이 앞서 가도 옛 글, 고르지 않으면 새 기본 branch, 기본 branch에 없는 로컬 commit은 거부 | `pinned_commit_reads_the_old_text_after_origin_moves` | 위와 같음 |
| origin을 받지 못하면 가진 ref로 읽고 `fetch_error`를 싣는다. 1분 안에 다시 읽어도 같은 까닭 | `fetch_failure_keeps_the_held_tree` | 위와 같음 |
| symlink(120000)·submodule(160000)은 목록에도 없고 읽지도 않음, 글 512KiB 상한, 조각 범위(크기만·끝 조각·끝·범위 밖) | `links_are_not_documents_and_slices_stay_in_bounds` | 위와 같음 |
| 같은 이름 저장소는 `design`을 단 항목, 설정 차례 유지, 잘못된 `design`은 없는 것, 경로는 기획 폴더 안 | `aaa-host` `design::tests::` | `cargo test -p aaa-host --lib design::` |
| 저장소 더하기·빼기·제자리 고치기 뒤에도 `design` 칸이 남는다 | `project::tests::design_folder_survives_repo_edits` | `cargo test -p aaa-host --lib project::tests::design` |
| 경로 정리·폴더 안 판정·제목(YAML 머리 건너뜀) | `aaa-proto` `design::tests` | `cargo test -p aaa-proto` |
| 트리 차례(폴더 먼저·README 먼저·폴더 제목), 상대 경로 풀기(저장소 밖·주소·절대 경로 거부, 퍼센트 부호의 `..`), 그림 주소 | `web/test/designModel.test.mjs` | `cd web && node --test test/designModel.test.mjs` |
| 실제 host·agent·git origin으로 트리 → 문서 → 같은 commit 그림 → 영상 재생 → 문서 안 링크 → 주소 다시 열기, 기획 폴더 없는 저장소 줄, 보기 기기 읽기, 폴더 밖 400·없는 범위 416·Range 206·기기 없음 401, 512KiB 조각을 이어 받은 0.9MiB 파일의 SHA-256과 조각 경계를 넘는 Range | 웹 E2E `135-design-docs`(Chromium·WebKit × 1280·390, 조합마다 한 단계) | `bash scripts/ci-local.sh --only web,e2e,public --spec 135` |

- git 실험실은 `-c init.defaultBranch=main`·`-c user.*`를 주고 index에 symlink·gitlink를 직접 넣어 OS의 symlink 권한과 기본 branch 설정에 기대지 않는다.
- 1분 받기 간격은 시험마다 새 임시 저장소(다른 toplevel)라 서로 막지 않는다. 고정 commit 시험은 clone에서 직접 `git fetch`해 시계를 기다리지 않는다.
- E2E의 영상은 기존 fixture `report-video.mp4`이고, WebKit은 응답 기록에 206이 있는지까지 본다. 그림은 `naturalWidth`로 실제 디코드를 본다.

## 변형 확인

소스는 변형마다 백업에서 바이트로 되돌렸다(`cmp` 확인).

- 일반 파일 판정(`regular`)에서 mode 검사를 빼면 `links_are_not_documents_and_slices_stay_in_bounds`가 목록에 `docs/link.md`가 들어와 실패했다.
- 받은 commit의 조상 검사(`merge-base --is-ancestor`)를 빼면 `pinned_commit_reads_the_old_text_after_origin_moves`가 실패했다.
- 1분 간격 안에서 지난 받기 실패를 버리면 `fetch_failure_keeps_the_held_tree`가 실패했다.
- 기본 ref 고르기를 줄인 이름(`rev-parse --abbrev-ref origin/HEAD` → `origin/main`)인 옛 판으로 되돌리면 `tree_reads_origin_main_without_touching_checkout`가 로컬 branch의 옛 판을 읽어 실패했다. fallback 이름만 줄여서는 앞의 `symbolic-ref`가 온전한 이름을 주어 통과한다.

## 독립 리뷰에서 고친 것

- `log.showSignature=true`인 머신에서 서명된 commit이면 `show --format=%ct` 출력 앞에 서명 줄이 붙어 commit 시각을 읽지 못했다. git 호출마다 `-c log.showSignature=false`를 준다(`orch.rs`와 같은 처리). 서명 열쇠가 필요해 시험은 없다.
- origin 받기 실패가 웹에 보이지 않았고 1분 안에 다시 읽으면 까닭이 사라졌다. agent가 까닭을 기억하고 웹 저장소 줄 경고가 `fetch_error`·`truncated`도 보인다.
- 짧게 읽힌 조각을 그대로 보내 commit 주소로 1년 캐시될 수 있었다. agent와 host가 받은 길이를 확인한다.
- 조각마다 blob을 처음부터 풀어 큰 파일은 제곱 비용이다. 그림·영상 상한을 아이디어 그림과 같은 16MiB로 낮췄다.
- CLI front frame(1MiB)을 넘는 문서가 있어 글 상한을 512KiB로 낮췄다.
- 문서 「새로 읽기」가 host가 기억한 옛 판만 다시 읽었다. 트리를 다시 읽고 판이 바뀌면 문서도 다시 읽는다. host가 정리한 경로로 돌아온 문서가 빈 화면이던 것과 앞 문서 오류가 남던 것도 고쳤다.
- 같은 이름의 다른 저장소가 한 줄로 합쳐지는 것은 고치지 않았다: 웹 작업 폴더 트리도 경로 끝 이름이 같으면 한 저장소로 보는 규칙이다.

## 실제 실행(11-16, macOS)

- `verify.py develop --package aaa-proto --package aaa-agent --package aaa-host --spec 135,130,120`: Rust 시험 proto 57·agent 213·host 928 통과, strict clippy 통과, 웹 단위 723 통과, 공개 검사 통과(리뷰 수리 뒤 다시 돌린 결과도 같다).
- 이 Mac의 전역 Playwright가 1.58.2라 첫 E2E는 준비에서 멈췄다. 전역은 두고 1.64.1 npm 패키지만 작업 폴더 `target/` 아래에 두고 `NODE_PATH`로 돌렸다. 브라우저는 공용 캐시에 있던 판을 썼다.
- 첫 E2E 실행은 135의 마지막 단계에서 실패했다. 400 오류 응답에도 host 공통 CSP 머리가 붙는데 시험이 「없음」을 기대했다. 기대값을 상태·Content-Range만으로 고친 뒤 `ci-local --only web,e2e,public --spec 135,130,120`이 통과했다(135 화면 단계는 첫 실행에서도 네 조합 모두 통과).
- `aaa` crate: `--test deps`·`--bin aaa cli::`·strict clippy 통과. Linux·Windows strict cross clippy 통과.
- 화면 확인(검수): 실험실 host·agent와 origin이 있는 견본 저장소 셋으로 넓은 창·390px 휴대폰에서 트리·문서·그림·링크를 직접 봤다.
