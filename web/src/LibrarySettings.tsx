import { useCallback, useEffect, useState } from "react";
import type { DeviceRole, LibraryRetentionView } from "./gen/proto";
import { call, me } from "./api";
import { SettingsGroup, SettingsRow } from "./herdr/SettingsControls.tsx";

const say = (e: unknown) => e instanceof Error ? e.message : String(e);

export function LibrarySettings({ role }: { role: DeviceRole }) {
  const [view, setView] = useState<LibraryRetentionView | null>(null);
  const [days, setDays] = useState("");
  const [mb, setMb] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState(false);
  const apply = useCallback((v: LibraryRetentionView) => {
    setView(v);
    setDays(String(v.max_days));
    setMb(String(v.max_mb));
  }, []);
  useEffect(() => {
    if (role === "reader") return;
    let live = true;
    me().then(async (m) => {
      const [major, minor] = (m?.front_api ?? "").split(".").map(Number);
      if (major !== 1 || minor < 168 || !Number.isFinite(minor)) throw new Error("host 업데이트가 필요하다");
      const r = await call({ op: "library_retention" });
      if (live && r.kind === "library_retention") apply(r);
    }).catch((e: unknown) => live && setError(say(e)));
    return () => { live = false; };
  }, [role, apply]);
  if (role === "reader") return <p className="op-muted">보기 전용 기기</p>;

  const valid = [days, mb].every((v) => /^\d+$/.test(v) && Number.isSafeInteger(Number(v)))
    && Number(days) <= 213503982334
    && Number(mb) <= 17592186044415;
  const changed = view !== null && (Number(days) !== view.max_days || Number(mb) !== view.max_mb);
  const save = async () => {
    if (!view || !valid) return;
    setBusy(true);
    setError(null);
    setSaved(false);
    try {
      const r = await call({ op: "library_retention_set", max_days: Number(days), max_mb: Number(mb), expected: view.rev });
      if (r.kind === "library_retention") {
        apply(r);
        setSaved(true);
      }
    } catch (e) {
      setError(say(e));
      // 충돌 뒤 다시 저장할 때는 새 판을 읽고 사람이 값을 다시 고른다.
      try {
        const r = await call({ op: "library_retention" });
        if (r.kind === "library_retention") apply(r);
      } catch (reloadError) {
        setError(`${say(e)} · ${say(reloadError)}`);
      }
    } finally {
      setBusy(false);
    }
  };
  return <div className="op-page" data-testid="settings-library">
    {error && <p role="alert" className="op-warn">{error}</p>}
    {!view && !error && <p className="op-muted">읽는 중…</p>}
    {view && <SettingsGroup title="보고서 보존 · host 전체">
      <SettingsRow label="보존 기간 (일, 0=무제한)" wide>
        <input aria-label="보존 기간 (일)" className="admin-input" type="number" min="0" step="1" value={days}
          disabled={busy || !view.editable} onChange={(e) => { setDays(e.target.value); setSaved(false); }} />
      </SettingsRow>
      <SettingsRow label="사본 용량 (MiB, 0=무제한)" wide>
        <input aria-label="사본 용량 (MiB)" className="admin-input" type="number" min="0" step="1" value={mb}
          disabled={busy || !view.editable} onChange={(e) => { setMb(e.target.value); setSaved(false); }} />
      </SettingsRow>
      <div className="settings-item">
        {!view.editable && <p className="op-muted">host.local.toml에서 지정됨</p>}
        {!valid && <p role="alert" className="op-warn">0 이상의 정수를 입력한다. 너무 큰 값은 쓸 수 없다.</p>}
        {view.editable && <button type="button" className="btn" disabled={busy || !valid || !changed} onClick={() => void save()}>{busy ? "저장 중…" : "저장"}</button>}
        {saved && <span role="status">저장됨</span>}
      </div>
    </SettingsGroup>}
  </div>;
}
