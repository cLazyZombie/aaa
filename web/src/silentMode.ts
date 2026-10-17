// iPhone·iPad 무음 스위치를 켜 두어도 알림 소리를 낸다(plan §8.72, 사용자 보고 10-18). 참고(herdr-web-ui alertSound.ts)에는 없다.
// WebKit은 Web Audio만 쓰는 페이지를 AmbientSound 범주로 둔다(MediaSessionManagerCocoa.mm). 이 범주는 다른 앱 소리와
// 섞이는 대신 무음 스위치를 따른다. 무음 스위치를 무시하는 범주는 Audio Session API(Safari 16.4+)의 "playback"뿐이다.
// "transient"·"ambient"도 WebKit에서는 AmbientSound라 무음에서 들리지 않는다(DOMAudioSession.cpp).
// "playback"은 섞이지 않아(AudioSessionIOS.mm, 옵션 없음) 그 동안 음악 같은 다른 앱 소리를 멈춘다. 되돌려도 저절로 다시
// 재생되지 않는다. 그래서 설정 「무음 모드에서도」를 켠 기기에서, 소리를 내는 동안만 "playback"으로 두고 끝나면 "auto"로 되돌린다.
import { alertSetting } from "./settings";

interface AudioSessionLike {
  type: string;
}

function session(): AudioSessionLike | null {
  return (navigator as Navigator & { audioSession?: AudioSessionLike }).audioSession ?? null;
}

/** 이 브라우저가 소리 범주를 바꿀 수 있다(Safari 16.4+). 아니면 설정 줄을 보이지 않는다 */
export function canPlayInSilentMode(): boolean {
  return typeof navigator !== "undefined" && session() !== null;
}

// 소리 하나(두 음 0.46초)와 그 뒤에 이어 울릴 수 있는 미리 듣기 하나를 넉넉히 덮는다(alertSound.ts NOTE_GAP_S·NOTE_LENGTH_S)
const HOLD_MS = 1500;
let restore: ReturnType<typeof setTimeout> | undefined;

/** 소리를 내기 바로 전에 부른다. 켜 두었으면 그 소리가 끝날 때까지 무음 스위치를 무시하는 범주로 둔다 */
export function beforeAlertSound() {
  const s = session();
  if (!s || !alertSetting("alertSilent")) return;
  try {
    if (s.type !== "playback") s.type = "playback";
  } catch {
    return;
  }
  clearTimeout(restore);
  restore = setTimeout(() => {
    try {
      s.type = "auto";
    } catch {
      /* 되돌리지 못하면 다음 소리 뒤에 다시 해 본다 */
    }
  }, HOLD_MS);
}
