// 설정 → 기기: pairing한 기기 목록과, 하나 더 연결할 코드(herdr-web-ui aa624cb src/components/DevicesPanel.tsx, 모양 herdr/DevicesPanel.css
// 그대로, 문구는 참고 i18n.ko.ts). host Front API `devices`·`web_pair`·`device_rename`·`device_revoke`를 쓴다(plan §8.81).
// 다른 곳:
// - 역할(reader·controller)을 줄마다 보인다(§6.4 P3). 웹에서 만드는 코드는 controller만이다(D6). 보기 기기는 host 머신의 `aaa web pair --reader`로.
// - 보기 권한 기기는 목록만 본다: 코드·이름 바꾸기·폐기 단추를 끄고 까닭을 적는다(host도 막는다).
// - 참고의 「아무도 페어링하지 않아 열려 있음」(via open·pairSelf)은 aaa에 없다: 웹은 처음부터 pairing이 있어야 들어온다.
import { useCallback, useEffect, useRef, useState } from "react";
import { Check, Pencil, RefreshCw, Trash2, X } from "lucide-react";
import "./herdr/DevicesPanel.css";
import type { DeviceRole, DeviceView } from "./gen/proto";
import { ApiError, call } from "./api";
import { copyText } from "./herdr/clipboard";
import { QrCode } from "./herdr/QrCode";

const POLL_MS = 3000;

function lastSeen(value: number | null): string {
  if (value === null) return "없음";
  const minutes = Math.round((Date.now() - value) / 60_000);
  if (minutes < 2) return "방금";
  if (minutes < 60) return `${minutes}분 전`;
  if (minutes < 60 * 48) return `${Math.round(minutes / 60)}시간 전`;
  return new Date(value).toLocaleDateString("ko-KR");
}

const message = (e: unknown): string => (e instanceof ApiError || e instanceof Error ? e.message : String(e));

async function fetchDevices(): Promise<DeviceView[]> {
  const r = await call({ op: "devices" });
  if (r.kind !== "devices") throw new Error(`기기 목록 대신 ${r.kind}`);
  return r.devices;
}

export function DevicesPanel({ pairUrl, role, device }: { pairUrl: string | null; role: DeviceRole; device: string }) {
  const canManage = role !== "reader";
  const [devices, setDevices] = useState<DeviceView[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [code, setCode] = useState<{ code: string; expiresAt: number } | null>(null);
  const [left, setLeft] = useState("");
  const [renaming, setRenaming] = useState<{ id: string; label: string } | null>(null);
  const [confirming, setConfirming] = useState<string | null>(null);
  const [justPaired, setJustPaired] = useState<string | null>(null);
  const [linkCopied, setLinkCopied] = useState(false);
  const linkRef = useRef<HTMLAnchorElement>(null);
  const known = useRef(new Set<string>());

  const load = useCallback(async () => {
    try {
      const list = await fetchDevices();
      setDevices(list);
      setError(null);
      // 코드가 나가 있는 동안 생긴 기기는 그 코드로 들어온 것이다
      const fresh = list.filter((d) => !known.current.has(d.id));
      if (known.current.size > 0 || list.length === 0)
        for (const d of fresh) {
          setJustPaired(d.label);
          setCode(null);
        }
      known.current = new Set(list.map((d) => d.id));
    } catch (e) {
      setDevices((old) => old ?? []);
      setError(message(e));
    }
  }, []);
  useEffect(() => {
    void load();
  }, [load]);

  // 코드가 나가 있는 동안: 남은 시간을 세고, 그 코드로 들어오는 기기를 지켜본다
  useEffect(() => {
    if (code === null) return;
    const tick = () => {
      const ms = code.expiresAt - Date.now();
      if (ms <= 0) {
        setCode(null);
        return;
      }
      setLeft(`${Math.floor(ms / 60_000)}:${String(Math.floor((ms % 60_000) / 1000)).padStart(2, "0")}`);
    };
    tick();
    const timer = window.setInterval(tick, 1000);
    const poll = window.setInterval(() => void load(), POLL_MS);
    return () => {
      window.clearInterval(timer);
      window.clearInterval(poll);
    };
  }, [code, load]);

  const pair = async () => {
    setJustPaired(null);
    try {
      const r = await call({ op: "web_pair", role: "controller" });
      if (r.kind !== "pair_code") throw new Error(`코드 대신 ${r.kind}`);
      setCode({ code: r.code, expiresAt: Date.now() + r.expires_in_s * 1000 });
      setError(null);
    } catch (e) {
      setError(message(e));
    }
  };
  const rename = async () => {
    if (!renaming) return;
    try {
      await call({ op: "device_rename", id: renaming.id, label: renaming.label });
      setRenaming(null);
      await load();
    } catch (e) {
      setError(message(e));
    }
  };
  const revoke = async (id: string) => {
    if (confirming !== id) {
      setConfirming(id);
      window.setTimeout(() => setConfirming((c) => (c === id ? null : c)), 4000);
      return;
    }
    setConfirming(null);
    try {
      await call({ op: "device_revoke", id });
      await load();
    } catch (e) {
      setError(message(e));
    }
  };

  const spaced = code ? `${code.code.slice(0, 3)} ${code.code.slice(3)}` : "";
  const qrValue = code && pairUrl ? `${pairUrl.replace(/\/$/, "")}/?pair=${code.code}` : null;

  return (
    <div className="devices-panel">
      <p className="settings-description">{role === "reader" ? "보기만 하는 기기입니다." : "페어링된 기기입니다."}</p>
      <p className="settings-description">
        페어링된 기기는 여기서 폐기하기 전까지, 이 host에 닿는 어디서든 스스로 들어옵니다. 코드는 이미 페어링된 기기의 이 화면에서, 또는 host 머신 터미널의{" "}
        <code>aaa web pair</code>로 받습니다. 보기만 할 기기는 <code>aaa web pair --reader</code>로 받습니다.
      </p>
      {devices === null ? (
        <p className="settings-hint" role="status">
          불러오는 중…
        </p>
      ) : devices.length === 0 ? (
        <p className="settings-hint">아직 페어링된 기기가 없습니다.</p>
      ) : (
        <ul className="devices-list">
          {devices.map((d) => (
            <li key={d.id} className="devices-row" data-device={d.id}>
              {renaming?.id === d.id ? (
                <form
                  className="devices-rename"
                  onSubmit={(e) => {
                    e.preventDefault();
                    void rename();
                  }}
                >
                  <input
                    className="input"
                    value={renaming.label}
                    maxLength={48}
                    autoFocus
                    aria-label="기기 이름"
                    onChange={(e) => setRenaming({ id: d.id, label: e.target.value })}
                  />
                  <button type="submit" className="icon-button" aria-label="이름 저장">
                    <Check />
                  </button>
                  <button type="button" className="icon-button" aria-label="취소" onClick={() => setRenaming(null)}>
                    <X />
                  </button>
                </form>
              ) : (
                <>
                  <div className="devices-name">
                    <span className="settings-label">
                      {d.label}
                      {d.id === device && <span className="devices-current">이 기기</span>}
                    </span>
                    <span className="settings-description">
                      {d.role === "reader" ? "보기만 · " : ""}마지막 접속 {lastSeen(d.last_seen_ms)}
                    </span>
                  </div>
                  <div className="devices-actions">
                    <button
                      type="button"
                      className="icon-button"
                      aria-label={`${d.label} 이름 바꾸기`}
                      disabled={!canManage}
                      onClick={() => setRenaming({ id: d.id, label: d.label })}
                    >
                      <Pencil />
                    </button>
                    <button type="button" className={`btn ${confirming === d.id ? "btn-danger" : "btn-ghost"}`} disabled={!canManage} onClick={() => void revoke(d.id)}>
                      <Trash2 aria-hidden="true" />
                      {confirming === d.id ? "폐기할까요?" : "폐기"}
                    </button>
                  </div>
                </>
              )}
            </li>
          ))}
        </ul>
      )}
      {justPaired !== null && (
        <p className="settings-hint" role="status">
          페어링됨: {justPaired}.
        </p>
      )}
      {!canManage ? (
        <p className="settings-hint">보기 권한 기기에서는 기기를 페어링하거나 바꿀 수 없습니다.</p>
      ) : code === null ? (
        <div className="phone-actions">
          <button type="button" className="btn btn-primary" onClick={() => void pair()}>
            기기 페어링
          </button>
        </div>
      ) : (
        <div className="devices-pairing" role="status">
          {qrValue !== null && <QrCode value={qrValue} label={`코드 ${code.code}로 폰을 페어링하는 QR 코드`} />}
          <div>
            <p className="settings-label">다른 기기에서 이 코드를 입력하세요</p>
            <p className="devices-code">{spaced}</p>
            <p className="settings-description">
              {qrValue !== null ? "또는 QR 코드를 스캔하세요. 코드가 채워진 상태로 앱이 열립니다." : "그 기기에서 앱 주소를 열고 입력하세요."} {left} 뒤 만료됩니다.
            </p>
            {qrValue !== null && (
              <p className="devices-link">
                <span className="settings-description">또는 이 링크를 그 기기로 보내세요:</span>
                <a ref={linkRef} href={qrValue}>
                  {qrValue}
                </a>
                <button
                  type="button"
                  className="btn btn-ghost"
                  onClick={() =>
                    void copyText(qrValue, linkRef.current).then((ok) => {
                      if (ok) {
                        setLinkCopied(true);
                        window.setTimeout(() => setLinkCopied(false), 1600);
                      }
                    })
                  }
                >
                  {linkCopied ? "복사됨" : "복사"}
                </button>
              </p>
            )}
            <div className="phone-actions">
              <button type="button" className="btn" onClick={() => void pair()}>
                <RefreshCw aria-hidden="true" />새 코드
              </button>
              <button type="button" className="btn btn-ghost" onClick={() => setCode(null)}>
                완료
              </button>
            </div>
          </div>
        </div>
      )}
      {error !== null && (
        <p className="settings-hint" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
