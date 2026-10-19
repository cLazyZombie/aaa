// 탭 아이콘의 안 본 알림 수(plan §8.266). t3code(MIT) `apps/web/src/components/ThreadNotificationCoordinator.tsx:38~70`
// (`pending` tag별 Map, 창 focus에 지움)과 `apps/web/src/threadNotifications.ts:25` `setNotificationBadge`(1451e7cb2)를 옮겼다:
// 창이 focus를 잃은 동안 알린 것을 자리(tag)마다 하나로 세고, 창에 focus가 오면 모두 지운다. 수가 있으면 favicon `<link>`를
// 빨간 원과 수(9 넘으면 「9+」)를 그린 64px 그림으로 바꾸고, 0이면 원래 `<link>`를 되돌린다.
// 다른 곳(aaa 구조): 참고는 OS 알림을 띄웠을 때만 센다. aaa는 push를 구독한 기기에 host가 알림을 보내 이 페이지가 OS 알림을 띄우지
// 않으므로, 이 페이지가 알린 것(raiseAlert: 앱 안 알림·소리·탭 알림·push가 맡은 것)을 focus가 없는 동안이면 센다.

const pending = new Set<string>();
let badgeFavicon: HTMLLinkElement | undefined;
let originalFavicon: HTMLLinkElement | undefined;

/** 참고 setNotificationBadge의 브라우저 갈래 그대로 */
function setNotificationBadge(count: number): void {
  let image: string | null = null;
  if (count > 0) {
    const canvas = document.createElement("canvas");
    canvas.width = canvas.height = 64;
    const context = canvas.getContext("2d");
    if (context) {
      context.fillStyle = "#e5484d";
      context.beginPath();
      context.arc(32, 32, 28, 0, Math.PI * 2);
      context.fill();
      context.fillStyle = "white";
      context.font = `600 ${count > 9 ? 30 : 40}px "Segoe UI", sans-serif`;
      context.textAlign = "center";
      context.textBaseline = "middle";
      context.fillText(count > 9 ? "9+" : String(count), 32, 34);
      image = canvas.toDataURL("image/png");
    }
  }
  if (image) {
    if (!badgeFavicon) {
      originalFavicon = document.querySelector<HTMLLinkElement>('link[rel="icon"]') ?? undefined;
      badgeFavicon = document.createElement("link");
      badgeFavicon.rel = "icon";
      badgeFavicon.type = "image/png";
      badgeFavicon.sizes.value = "64x64";
      originalFavicon?.remove();
      document.head.append(badgeFavicon);
    }
    badgeFavicon.href = image;
    // 시험이 읽는 표지(그림은 픽셀이라 수를 글로 둔다)
    badgeFavicon.dataset.badge = count > 9 ? "9+" : String(count);
  } else if (badgeFavicon) {
    badgeFavicon.remove();
    badgeFavicon = undefined;
    if (originalFavicon) document.head.append(originalFavicon);
    originalFavicon = undefined;
  }
}

/** 알림 하나를 냈다: 창에 focus가 없으면 그 자리(tag)를 센다(참고 onNotification) */
export function noteAlert(tag: string): void {
  if (document.hasFocus()) return;
  pending.add(tag);
  setNotificationBadge(pending.size);
}

/** 창에 focus가 오면 지운다(참고 `window.addEventListener("focus", clear)`). 앱이 한 번 건다 */
export function watchBadgeFocus(): () => void {
  const clear = () => {
    pending.clear();
    setNotificationBadge(0);
  };
  clear();
  window.addEventListener("focus", clear);
  return () => {
    window.removeEventListener("focus", clear);
    clear();
  };
}
