# Windows 기획 main worktree 경로 검증 (t-2013)

`db78cd8f`의 Windows CI에서 같은 worktree를 처음 열었을 때는 Rust의 `\\?\` 확장 경로가, 쓰기 잠금 중 다시 열었을 때는 Git의 `C:/…` 경로가 반환됐다. 새 worktree를 찾을 때만 설정 경로와 실제 경로의 표기를 Git 목록과 직접 비교했다. Git이 드라이브 경로의 대소문자 표기를 고치거나 junction을 해석하면 그 비교가 빗나갔다. [Windows CI 실패](https://github.com/cLazyZombie/aaa/actions/runs/44261473159/job/134655590127)

생성 직후, 기존 자리 재사용, 쓰기 잠금 중 열기, 생성 실패 뒤 정리가 모두 실제 위치로 Git 목록을 찾는다. host가 기획 대화에 기록하는 경로는 Git 목록의 표기를 사용한다. 기존 기획 branch도 생성과 재사용에 Git 목록의 표기를 쓴다. `dunce`의 호환 표기는 Git이 등록한 문자열까지 맞춰 주지 않으므로 기존 `real_location`으로 위치를 비교한다. [Git worktree 목록](https://git-scm.com/docs/git-worktree#_list_output_format) · [Rust canonicalize](https://doc.rust-lang.org/std/fs/fn.canonicalize.html) · [dunce](https://docs.rs/dunce/latest/dunce/)

## 검증

- Windows 실기에서 기준 `0a239b29`의 기존 시험은 임시 폴더의 대소문자 표기가 다르거나 junction을 거치면 CI와 같은 경로 단언에서 실패했다. 기존 시험에 대소문자가 다른 작업 root를 넣었을 때도 제품 수정 전에는 같은 단언에서 실패했다.
- 수정 뒤에는 원래 경로 문자열 단언과 5초 상한을 유지하면서 첫 응답이 Git 목록에 그대로 있는지 확인한다. 대소문자를 구분하지 않는 Windows 작업 볼륨에서 대소문자가 다른 root를 사용한다.
- Windows 네이티브 `aaa-agent --lib` 195개와 junction 및 대소문자가 다른 임시 경로의 개별 시험이 통과했다. 첫 전수에서는 실기 Git의 `core.autocrlf=true` 때문에 기존 `design_write::tests::mark_ids_adds_markers_in_one_commit`의 LF 단언이 실패했다. CI와 같은 `core.autocrlf=false`를 시험 프로세스에만 지정해 줄 끝 조건을 맞췄다.
- macOS `aaa-agent` 232개, macOS·Linux·Windows 관련 crate의 strict Clippy와 fmt가 통과했다.
