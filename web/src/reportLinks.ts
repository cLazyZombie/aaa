// 대화 글의 링크 중 휴대폰에서 앱 안으로 여는 것(plan §8.299). Markdown 링크는 새 창(`target=_blank`)인데, iPhone에서 새 창으로
// 연 보고서에는 앱으로 돌아올 길이 없다(Safari 새 탭은 뒤로 칸이 없고, 홈 화면 앱은 뒤로 단추가 없다). 그래서 같은 host의
// 보고서 보기 주소(`/v/<cap>/…`)와 앱 주소(`/#report=…`·`/#thread=…`)만 골라 앱 안에서 연다. 다른 host 주소는 그 문서를
// 앱 안 frame에 넣을 수 없으니(host VIEW_CSP `frame-ancestors 'self'`) 새 창 그대로다.

/** 앱 안에서 여는 링크: 보고서 보기 경로의 파일, 앱 보고서 화면의 보고서, 그 밖의 앱 주소 */
export type InAppLink =
  | { kind: "view"; base: string; path: string }
  | { kind: "report"; project: string; name: string }
  | { kind: "app"; hash: string };

/**
 * `href`가 이 앱(`origin`) 안에서 열 것이면 그 대상. `/v/<cap>/<path>`는 보기 경로 맨 위(`base`)와 그 안 경로(풀어 쓴 것),
 * `/#report=<project>/<name>`은 그 보고서, 그 밖의 `/#…`는 앱 주소다. 내려받기(`?download=1`)·파일 읽기 경로(`/f/`)·API는 고르지 않는다.
 */
export function inAppLink(href: string, origin: string): InAppLink | null {
  let url: URL;
  try {
    url = new URL(href, origin);
  } catch {
    return null;
  }
  if (url.origin !== origin || url.search !== "") return null;
  const view = /^\/v\/([^/]+)\/(.*)$/.exec(url.pathname);
  if (view) {
    try {
      return { kind: "view", base: `/v/${view[1]}/`, path: view[2]!.split("/").map(decodeURIComponent).join("/") };
    } catch {
      return null;
    }
  }
  if (url.pathname !== "/" || url.hash.length < 2) return null;
  const report = /^#report=(.+)$/.exec(url.hash);
  if (report) {
    let key: string;
    try {
      key = decodeURIComponent(report[1]!);
    } catch {
      return null;
    }
    // 프로젝트 이름에는 /가 없다(App reportOf)
    const at = key.indexOf("/");
    return at > 0 && at < key.length - 1 ? { kind: "report", project: key.slice(0, at), name: key.slice(at + 1) } : null;
  }
  return { kind: "app", hash: url.hash };
}

/** 그 누름이 브라우저 몫인지: 보조 키·가운데 단추는 새 탭·새 창·내려받기 그대로다 */
export function browserClick(e: { button: number; metaKey: boolean; ctrlKey: boolean; shiftKey: boolean; altKey: boolean }): boolean {
  return e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey;
}
