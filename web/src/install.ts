// 홈 화면에 설치(PWA). 브라우저가 설치할 수 있다고 알리면(beforeinstallprompt, Chrome·Android) 단추를 보인다.
// iOS Safari는 이 신호가 없다: 공유 → 「홈 화면에 추가」로 설치한다(manifest·아이콘은 같다).
import { useEffect, useState } from "react";

type InstallEvent = Event & { prompt: () => Promise<void>; userChoice: Promise<{ outcome: string }> };

let pending: InstallEvent | null = null;
const subs = new Set<() => void>();

window.addEventListener("beforeinstallprompt", (e) => {
  e.preventDefault();
  pending = e as InstallEvent;
  subs.forEach((f) => f());
});
window.addEventListener("appinstalled", () => {
  pending = null;
  subs.forEach((f) => f());
});

/** 설치할 수 있으면 설치 함수를, 아니면 null */
export function useInstall(): (() => Promise<void>) | null {
  const [, tick] = useState(0);
  useEffect(() => {
    const f = () => tick((n) => n + 1);
    subs.add(f);
    return () => {
      subs.delete(f);
    };
  }, []);
  if (!pending) return null;
  return async () => {
    const e = pending;
    if (!e) return;
    await e.prompt();
    await e.userChoice.catch(() => null);
    pending = null;
    subs.forEach((f) => f());
  };
}
