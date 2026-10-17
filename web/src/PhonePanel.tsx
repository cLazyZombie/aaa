// 설정 → 폰: 휴대폰이 지금 열 수 있는 주소와 QR, 아니면 host 머신에서 남은 한 걸음(herdr-web-ui aa624cb src/components/PhonePanel.tsx,
// 모양 herdr/PhonePanel.css 그대로, 문구는 참고 i18n.ko.ts에서 「PC」를 「host 머신」으로).
// 다른 곳: 참고는 GET /api/access(RemoteAccess)를 읽는다. aaa는 같은 것을 `web_status`(PublishState: tailscale 상태·DNS 이름·
// 이미 aaa 웹을 proxy하는 HTTPS 주소)로 읽고, 공개 명령은 `aaa web publish`다. 참고의 README 링크·접근 토큰 문장은 aaa에 없어 뺐다.
import "./herdr/PhonePanel.css";

import { useRef, useState } from "react";
import { RefreshCw } from "lucide-react";
import type { PublishState } from "./gen/proto";
import { copyText } from "./herdr/clipboard";
import { isLoopbackHost } from "./herdr/phone";
import { QrCode } from "./herdr/QrCode";

/** 설정 → 폰이 보이는 것: 열 주소, 칠 명령, 또는 막힌 까닭(참고 `PhonePlan`) */
export type PhonePlan =
  /** 이 페이지가 휴대폰이 열 수 있는 HTTPS 주소에 있다 */
  | { kind: "here"; url: string }
  /** host 머신의 Tailscale이 이미 aaa 웹을 proxy한다 */
  | { kind: "served"; url: string }
  /** host 머신에서 명령 하나로 공개한다 */
  | { kind: "command"; command: string }
  | { kind: "stopped" }
  | { kind: "missing" }
  /** host가 말하지 않았다(요청 실패·tailscale 오류) */
  | { kind: "unknown"; message: string | null };

/** 참고 `phonePlan`: 지금 페이지가 HTTPS 공개 주소면 그것, 아니면 host의 공개 상태 */
export function phonePlan(page: { protocol: string; hostname: string; origin: string; secure: boolean }, publish: PublishState | null): PhonePlan {
  if (page.protocol === "https:" && page.secure && !isLoopbackHost(page.hostname)) return { kind: "here", url: page.origin };
  if (publish === null) return { kind: "unknown", message: null };
  if (publish.tailscale === "missing") return { kind: "missing" };
  if (publish.tailscale === "stopped") return { kind: "stopped" };
  if (publish.tailscale !== "running") return { kind: "unknown", message: publish.message };
  if (publish.url !== null) return { kind: "served", url: publish.url };
  return { kind: "command", command: "aaa web publish" };
}

export function PhonePanel({ plan, loading, onRefresh }: { plan: PhonePlan; loading: boolean; onRefresh: () => void }) {
  return (
    <div className="phone-panel">
      {loading && plan.kind !== "here" ? (
        <p className="settings-hint" role="status">
          host 머신의 Tailscale 상태를 확인하는 중…
        </p>
      ) : (
        <Plan plan={plan} loading={loading} onRefresh={onRefresh} />
      )}
    </div>
  );
}

function Plan({ plan, loading, onRefresh }: { plan: PhonePlan; loading: boolean; onRefresh: () => void }) {
  const checkAgain = (
    <button type="button" className="btn" onClick={onRefresh} disabled={loading}>
      <RefreshCw aria-hidden="true" />
      다시 확인
    </button>
  );
  switch (plan.kind) {
    case "here":
    case "served":
      return (
        <>
          <Address url={plan.url} title={plan.kind === "here" ? "폰에서 이 주소를 여세요" : "Tailscale이 이미 이 host 머신을 서빙하고 있습니다"} />
          <p className="settings-description">열리면 앱을 설치한 뒤 목록 머리의 더 보기 메뉴에서 알림을 누르세요.</p>
          {plan.kind === "served" && <Sharing />}
        </>
      );
    case "command":
      return (
        <>
          <p className="settings-description">host 머신의 터미널에서 앱을 테일넷에 공개하세요. 테일넷 안의 기기만 이 주소를 열 수 있습니다.</p>
          <Command command={plan.command} />
          <div className="phone-actions">{checkAgain}</div>
          <Sharing />
        </>
      );
    case "stopped":
      return (
        <>
          <p className="settings-description">host 머신에 Tailscale이 있지만 연결되어 있지 않습니다. 거기서 tailscale up을 실행한 뒤 다시 확인하세요.</p>
          <div className="phone-actions">{checkAgain}</div>
        </>
      );
    case "missing":
      return (
        <>
          <p className="settings-description">
            Tailscale이 가장 빠른 길입니다. host 머신에 설치하고 tailscale up을 실행한 뒤 다시 확인하세요. SSH 터널이나 직접 만든 HTTPS 프록시도 됩니다.
          </p>
          <p className="settings-description">
            <a href="https://tailscale.com/download" target="_blank" rel="noreferrer">
              Tailscale 받기
            </a>
          </p>
          <div className="phone-actions">{checkAgain}</div>
        </>
      );
    default:
      return (
        <>
          <p className="settings-description">
            host 머신이 어떻게 접속할 수 있는지 알려주지 않았습니다.{plan.message ? ` ${plan.message}` : ""}
          </p>
          <div className="phone-actions">{checkAgain}</div>
        </>
      );
  }
}

function Address({ url, title }: { url: string; title: string }) {
  return (
    <div className="phone-address">
      <QrCode value={url} label={`QR 코드: ${url}`} />
      <div>
        <p className="settings-label">{title}</p>
        <p className="phone-url">
          <a href={url} target="_blank" rel="noreferrer">
            {url}
          </a>
        </p>
      </div>
    </div>
  );
}

function Command({ command }: { command: string }) {
  const codeRef = useRef<HTMLElement>(null);
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    if (!(await copyText(command, codeRef.current))) return;
    setCopied(true);
    setTimeout(() => setCopied(false), 1600);
  };
  return (
    <div className="phone-command">
      <code ref={codeRef}>{command}</code>
      <button type="button" className="btn" onClick={() => void copy()}>
        {copied ? "복사됨" : "복사"}
      </button>
    </div>
  );
}

function Sharing() {
  return <p className="settings-hint">테일넷에 닿는 기기도 들어오려면 한 번 페어링해야 합니다. 아래 기기 항목을 보세요.</p>;
}
