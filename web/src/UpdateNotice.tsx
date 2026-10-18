// 배포 뒤 옛 웹이 남을 때 다시 열게 하는 줄. herdr-web-ui(aa624cb) src/components/UpdateControls.tsx `UpdateNotice`의
// reload "updated" 줄(문구·단추·class)을 옮겼다. 참고는 서버 상태를 보이는 동안 물어 commit을 견주고(lib/updateNotes.ts
// `reloadReason`), aaa는 host가 events 연결마다 알리는 웹 빌드를 이 페이지의 빌드와 견준다. 스스로 다시 열지 않는다(참고와 같다).
// 입력줄·대화 입력 초안은 localStorage에 있어 다시 열어도 남는다(herdr/terminalDraft.ts·herdr/composerDraft.ts, plan §8.162)
import { BUILD } from "./api";
import "./herdr/UpdateNotice.css";

/** host가 이 페이지와 다른 웹을 준다(참고 `reloadReason`의 "updated"). 어느 쪽이든 모르면 아니다 */
export function webUpdated(hostBuild: string | null, ownBuild = BUILD): boolean {
  return hostBuild !== null && hostBuild !== "" && ownBuild !== "?" && hostBuild !== ownBuild;
}

export function UpdateNotice({ hostBuild }: { hostBuild: string | null }) {
  if (!webUpdated(hostBuild)) return null;
  return (
    <div className="update-notice is-line is-warning" role="status" data-testid="web-update">
      <span>앱이 업데이트됐다. 다시 열기 전에 보내지 않은 초안을 저장한다.</span>
      <button type="button" className="btn" onClick={() => window.location.reload()}>
        앱 다시 열기
      </button>
    </div>
  );
}
