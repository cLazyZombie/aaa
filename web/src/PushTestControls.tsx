// 설정 → 알림 「테스트 알림」 줄(herdr-web-ui src/components/PushTestControls.tsx를 옮김: 문구는 참고 i18n.ko.ts,
// 설정 읽기만 aaa 것). 구독을 고치지 않고 지금 구독에 보내 본다. 없거나 버려졌으면 「알림 다시 켜기」를 권한다.
import { useState } from "react";
import { pushErrorCode, pushSupported, testDevicePush } from "./push";
import { useAlertSetting } from "./settings";

type Result = Awaited<ReturnType<typeof testDevicePush>> | "gone" | "failed" | "sending" | null;

export function PushTestControls({ onEnable }: { onEnable: () => Promise<boolean> }) {
  const alertsOn = useAlertSetting("alertsOn");
  const [result, setResult] = useState<Result>(null);
  const supported = pushSupported();
  const denied = globalThis.Notification?.permission === "denied";
  const busy = result === "sending";
  const enable = !alertsOn || result === "missing" || result === "gone" || result === "permission";

  const send = async (repair: boolean) => {
    setResult("sending");
    try {
      if (repair) setResult((await onEnable()) ? "sent" : "failed");
      else setResult(await testDevicePush());
    } catch (error) {
      // gone: the push service retired the endpoint; Turn alerts on again subscribes this device anew
      const code = pushErrorCode(error);
      setResult(code === "subscription_not_found" ? "missing" : code === "subscription_gone" ? "gone" : "failed");
    }
  };

  return (
    <div className="settings-item">
      <div className="settings-row">
        <div className="settings-row-text">
          <span className="settings-label">테스트 알림</span>
          <span className="settings-description">이 기기가 푸시 알림을 받을 수 있는지 확인합니다</span>
          {/* always in the document, empty until there is something to say: a live region */}
          <span className="settings-hint" role="status" data-testid="push-test-result">
            {!supported || result === "unsupported"
              ? "푸시 알림은 HTTPS 또는 localhost가 필요하고, iPhone에서는 홈 화면 앱이어야 합니다."
              : denied
                ? "브라우저 설정에서 알림을 허용한 뒤 다시 시도하세요."
                : result === "sent"
                  ? "테스트 알림을 보냈습니다. 이 기기에 알림이 도착했는지 확인하세요."
                  : result === "missing"
                    ? "이 기기의 푸시 구독이 없습니다. 알림을 다시 켜세요."
                    : result === "gone"
                      ? "푸시 서비스가 이 기기의 구독을 끝냈습니다. 알림을 다시 켜세요."
                      : result === "permission"
                        ? "이 기기에서 알림을 테스트하려면 알림을 허용하세요."
                        : result === "failed"
                          ? "테스트 알림을 보내지 못했습니다. 다시 시도하세요."
                          : !alertsOn
                            ? "알림 꺼짐"
                            : null}
          </span>
        </div>
        <button type="button" className="btn" disabled={busy || !alertsOn || !supported || denied} onClick={() => void send(false)}>
          {busy ? "보내는 중…" : "테스트 보내기"}
        </button>
      </div>
      {enable && supported && !denied && (
        <div className="settings-actions">
          <button type="button" className="btn" disabled={busy} onClick={() => void send(true)}>
            알림 다시 켜기
          </button>
        </div>
      )}
    </div>
  );
}
