// 관리 GUI W5b(plan §8.352): 설정 「머신」(등록 창·대기·승인·폐기), 「폰 및 기기」의 웹 공개 켜기·끄기, 보고서 「올리기」 창,
// 「이 aaa」의 공유 링크 키 교체. host가 판정한다(웹 조작 기기만, 마지막 조작 기기·host 머신 폐기 거부, 등록 코드 3번 틀리면 버림).
// 머신 등록은 새 머신 화면의 6자리 코드를 넣어야 승인한다(사용자 답 1A: CLI와 같은 보증). 등록 창·대기 목록은 host `enroll` 사건으로
// 다시 읽는다. M1(웹 머신 추가)은 같은 요청(`enroll_*`·`machine_revoke`)을 쓴다.
import "./ThreadActions.css";
import "./AdminPages.css";

import { type ReactNode, useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { DeviceRole, LinkView, MachineView, PendingEnroll, PublishState } from "./gen/proto";
import { ApiError, call, me, onEnrollChanged, uploadReportFile } from "./api";
import { ConfirmDialog } from "./ConfirmDialog";
import { SettingsGroup } from "./herdr/SettingsControls.tsx";
import { nativeModalOver, useFocusTrap } from "./herdr/useFocusTrap.ts";
import { usePhoneSheetBack } from "./herdr/sheetHistory.ts";
import { MachineSetup } from "./SettingsAdminPages";
import { canAdmin, enrollCode, enrollLeft, liveLinks, newUploadId, onPublicAddress, otherRoutes, reportNameOk, uploadPlan } from "./adminModel.ts";

const say = (e: unknown) => (e instanceof ApiError ? e.message : e instanceof Error ? e.message : String(e));

/** 입력이 있는 창(PaneRename과 같은 꼴): Esc·바깥 누름·폰 「뒤로」는 닫기, 일하는 동안은 닫지 않는다 */
export function AdminDialog({ label, busy, onClose, children }: { label: string; busy: boolean; onClose: () => void; children: ReactNode }) {
  const surface = useFocusTrap<HTMLDivElement>(true);
  usePhoneSheetBack(() => {
    if (busy) return false;
    onClose();
  });
  useEffect(() => {
    const key = (event: KeyboardEvent): void => {
      if (event.key !== "Escape" || nativeModalOver(surface.current)) return;
      event.preventDefault();
      event.stopPropagation();
      if (!busy) onClose();
    };
    window.addEventListener("keydown", key, true);
    return () => window.removeEventListener("keydown", key, true);
  }, [busy, onClose]); // eslint-disable-line react-hooks/exhaustive-deps
  return createPortal(
    <div
      className="modal-scrim"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !busy) onClose();
      }}
    >
      <div ref={surface} className="modal admin-dialog" role="dialog" aria-modal="true" aria-label={label} tabIndex={-1}>
        <header className="modal-header">
          <h2 className="modal-title">{label}</h2>
        </header>
        {children}
      </div>
    </div>,
    document.body,
  );
}

/** 1.80 이상 host인가(모르면 null) */
export function useAdmin(): boolean | null {
  const [ok, setOk] = useState<boolean | null>(null);
  useEffect(() => {
    let live = true;
    me()
      .then((m) => live && setOk(canAdmin(m?.front_api)))
      .catch(() => live && setOk(false));
    return () => {
      live = false;
    };
  }, []);
  return ok;
}

export function MachinesPage({ role, machines }: { role: DeviceRole; machines: MachineView[] }) {
  const admin = useAdmin();
  const [pending, setPending] = useState<PendingEnroll[]>([]);
  const [openUntil, setOpenUntil] = useState<number | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [minutes, setMinutes] = useState("10");
  const [approving, setApproving] = useState<PendingEnroll | null>(null);
  const [revoking, setRevoking] = useState<string | null>(null);
  const [, tick] = useState(0);
  const seq = useRef(0);
  const canEdit = role !== "reader" && admin === true;
  // 등록 창·대기 목록(머신 목록은 host `state` 사건으로 App이 쥔다)
  const load = useCallback(() => {
    if (!canEdit) return;
    const mine = ++seq.current;
    call({ op: "enroll_pending" })
      .then((p) => {
        if (mine !== seq.current || p.kind !== "pending") return;
        setPending(p.pending);
        setOpenUntil(p.open_until_ms);
        setError(null);
      })
      .catch((e: unknown) => mine === seq.current && setError(say(e)));
  }, [canEdit]);
  useEffect(() => {
    load();
    return onEnrollChanged(load);
  }, [load]);
  // 남은 분을 다시 그린다(창이 닫히는 때는 host가 사건을 보내지 않는다: 시각으로 그린다)
  useEffect(() => {
    if (openUntil === undefined) return;
    const id = window.setInterval(() => tick((n) => n + 1), 15_000);
    return () => window.clearInterval(id);
  }, [openUntil]);
  const run = async (work: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    try {
      await work();
      load();
      return true;
    } catch (e) {
      setError(say(e));
      return false;
    } finally {
      setBusy(false);
    }
  };
  const left = enrollLeft(openUntil, Date.now());
  return (
    <div className="op-page" data-testid="settings-machines">
      {error && (
        <p className="op-warn" role="alert">
          {error}
        </p>
      )}
      {admin === false && role !== "reader" && <p className="op-muted">이 host는 웹 머신 등록을 모른다(host 터미널의 `aaa machine enroll`).</p>}
      {canEdit && (
        <SettingsGroup
          title="새 머신 등록"
          note="등록 창을 연 뒤 새 머신에서 aaa agent enroll <host 주소> --name <이름>을 실행한다. 새 머신 화면에 나온 6자리 코드를 여기 넣어 승인한다."
        >
          <div className="settings-item">
            {left !== null ? (
              <div className="admin-line" data-testid="enroll-open">
                <span>등록 창이 열려 있다(약 {left}분 남음).</span>
                <button type="button" className="btn" disabled={busy} onClick={() => void run(() => call({ op: "enroll_close" }))}>
                  창 닫기
                </button>
              </div>
            ) : (
              <form
                className="admin-line"
                onSubmit={(e) => {
                  e.preventDefault();
                  void run(() => call({ op: "enroll_open", minutes: Number(minutes) }));
                }}
              >
                <label className="op-field">
                  <span>열어 둘 시간</span>
                  <select value={minutes} onChange={(e) => setMinutes(e.target.value)} disabled={busy} data-testid="enroll-minutes">
                    {["5", "10", "30", "60"].map((m) => (
                      <option key={m} value={m}>
                        {m}분
                      </option>
                    ))}
                  </select>
                </label>
                <button type="submit" className="btn btn-primary" disabled={busy} data-testid="enroll-open-button">
                  등록 창 열기
                </button>
              </form>
            )}
            <h4 className="admin-sub">승인을 기다리는 머신</h4>
            {pending.length === 0 ? (
              <p className="op-muted">없다</p>
            ) : (
              <ul className="admin-list" data-testid="enroll-pending">
                {pending.map((p) => (
                  <li key={p.name} data-name={p.name}>
                    <span className="admin-name">{p.name}</span>
                    <span className="mono">fingerprint {p.fingerprint_prefix}…</span>
                    <button type="button" className="btn btn-primary small" disabled={busy} onClick={() => setApproving(p)}>
                      승인…
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </SettingsGroup>
      )}
      <SettingsGroup title="등록된 머신">
        <div className="settings-item">
          <ul className="admin-list" data-testid="machine-list">
            {machines.map((m) => (
              <li key={m.id} data-name={m.name}>
                <span className="admin-name">{m.name}</span>
                <span className="op-muted">{m.local ? "host 머신" : m.connection === "connected" ? "연결됨" : "끊김"}</span>
                {canEdit && !m.local && (
                  <button type="button" className="btn small" disabled={busy} onClick={() => setRevoking(m.name)} aria-label={`${m.name} 폐기`}>
                    폐기
                  </button>
                )}
              </li>
            ))}
          </ul>
        </div>
      </SettingsGroup>
      <MachineSetup role={role} machines={machines} />
      {approving && <ApproveDialog p={approving} onClose={() => setApproving(null)} onDone={load} />}
      {revoking && (
        <ConfirmDialog
          title={`${revoking}의 등록을 폐기할까요?`}
          body="그 머신의 agent 연결이 바로 끊기고 다시 들어오지 못한다. 그 머신의 세션은 목록에서 빠진다. 다시 쓰려면 새로 등록한다."
          confirmLabel="폐기"
          onConfirm={async () => {
            await call({ op: "machine_revoke", name: revoking });
            setRevoking(null);
            load();
          }}
          onClose={() => setRevoking(null)}
        />
      )}
    </div>
  );
}

function ApproveDialog({ p, onClose, onDone }: { p: PendingEnroll; onClose: () => void; onDone: () => void }) {
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const ok = enrollCode(code);
  return (
    <AdminDialog label={`${p.name} 승인`} busy={busy} onClose={onClose}>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          if (!ok) return;
          setBusy(true);
          setError(null);
          call({ op: "enroll_approve", name: p.name, code: ok })
            .then(() => {
              onDone();
              onClose();
            })
            .catch((e: unknown) => {
              setError(say(e));
              setBusy(false);
            });
        }}
      >
        <div className="modal-body">
          <p>
            새 머신 화면에 나온 6자리 코드를 넣는다. 이름이 「<strong>{p.name}</strong>」인지 그 화면과 맞춰 본다. 세 번 틀리면 이 등록 요청을 버린다.
          </p>
          <p className="op-muted mono">fingerprint {p.fingerprint_prefix}…</p>
          <label className="op-field">
            <span>확인 코드</span>
            <input
              autoFocus
              value={code}
              onChange={(e) => setCode(e.target.value)}
              inputMode="numeric"
              autoComplete="one-time-code"
              disabled={busy}
              data-testid="enroll-code"
            />
          </label>
          {error && (
            <p className="op-warn" role="alert">
              {error}
            </p>
          )}
        </div>
        <footer className="modal-footer">
          <button type="button" className="btn" onClick={onClose} disabled={busy}>
            닫기
          </button>
          <button type="submit" className="btn btn-primary" disabled={busy || !ok} data-testid="enroll-approve">
            승인
          </button>
        </footer>
      </form>
    </AdminDialog>
  );
}

/** 「폰 및 기기」의 웹 공개 켜기·끄기(사용자 답 3A): 공개 주소로 열린 화면에서 끄려면 다른 접속 경로를 확인해야 한다 */
export function PublishControls({ publish, onChanged }: { publish: PublishState | null | undefined; onChanged: (p: PublishState) => void }) {
  const admin = useAdmin();
  const [asking, setAsking] = useState<"on" | "off" | null>(null);
  const [checked, setChecked] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (!admin || !publish || publish.tailscale !== "running") return null;
  const here = onPublicAddress(location.origin, publish);
  const go = async (op: "web_publish" | "web_publish_off") => {
    const r = await call({ op });
    if (r.kind === "publish") onChanged(r);
    setAsking(null);
  };
  const off = () => {
    setBusy(true);
    setError(null);
    go("web_publish_off")
      .catch((e: unknown) => setError(say(e)))
      .finally(() => setBusy(false));
  };
  return (
    <div className="admin-publish" data-testid="publish-controls">
      {publish.url === null ? (
        <button type="button" className="btn btn-primary" disabled={busy} onClick={() => setAsking("on")}>
          tailnet에 공개
        </button>
      ) : publish.owned_port !== null ? (
        <button
          type="button"
          className="btn"
          disabled={busy}
          onClick={() => {
            setChecked(false);
            setAsking("off");
          }}
        >
          공개 끄기
        </button>
      ) : (
        <p className="op-muted">aaa가 만든 공개가 아니라 여기서 끄지 않는다(다른 서비스의 매핑).</p>
      )}
      {asking === "on" && (
        <ConfirmDialog
          title="tailnet에 공개할까요?"
          body="host의 Tailscale이 aaa 웹을 HTTPS 주소로 공개한다. 이 tailnet 안의 기기만 그 주소를 열 수 있고, 들어오려면 페어링 코드가 필요하다."
          confirmLabel="공개"
          onConfirm={() => go("web_publish")}
          onClose={() => setAsking(null)}
        />
      )}
      {asking === "off" && (
        <AdminDialog label="공개를 끌까요?" busy={busy} onClose={() => setAsking(null)}>
          <div className="modal-body">
            <p>{publish.url} 주소가 닫힌다. 휴대폰 등 그 주소로 연 화면은 끊긴다.</p>
            {here && (
              <p className="op-warn" data-testid="publish-off-here">
                이 화면이 끊길 수 있음: 지금 이 화면이 그 주소로 열려 있다.
              </p>
            )}
            <p>끈 뒤에 들어올 다른 경로:</p>
            <ul>
              {otherRoutes(publish).map((r) => (
                <li key={r}>{r}</li>
              ))}
            </ul>
            {here && (
              <label className="op-check">
                <input type="checkbox" checked={checked} onChange={(e) => setChecked(e.target.checked)} data-testid="publish-off-checked" />
                <span>다른 경로를 확인했다</span>
              </label>
            )}
            {error && (
              <p className="op-warn" role="alert">
                {error}
              </p>
            )}
          </div>
          <footer className="modal-footer">
            <button type="button" className="btn" onClick={() => setAsking(null)} disabled={busy}>
              닫기
            </button>
            <button type="button" className="btn btn-danger" disabled={busy || (here && !checked)} onClick={off} data-testid="publish-off">
              끄기
            </button>
          </footer>
        </AdminDialog>
      )}
    </div>
  );
}

/** 보고서 「올리기」 창(사용자 답 4A): 파일 여러 개나 폴더 하나 → 하나씩 올린 뒤 보고서 하나로 등록 */
export function UploadReportDialog({ project, onClose, onDone }: { project: string; onClose: () => void; onDone: () => void }) {
  const [files, setFiles] = useState<File[]>([]);
  const [name, setName] = useState("");
  const [title, setTitle] = useState("");
  const [toUser, setToUser] = useState(false);
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const upload = useRef(newUploadId());
  const plan = uploadPlan(files.map((f) => ({ name: f.name, relative: f.webkitRelativePath, size: f.size })));
  const hasIndex = plan.paths.some((p) => p.path === "index.html");
  const problem = files.length === 0 ? null : (plan.error ?? (toUser && !hasIndex ? "사용자 보고서는 맨 위에 index.html이 있어야 한다" : null));
  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      let n = done;
      for (const p of plan.paths.slice(done)) {
        await uploadReportFile(project, upload.current, p.path, files[p.index]!);
        setDone(++n);
      }
      await call({
        op: "library_upload_publish",
        project,
        upload: upload.current,
        name,
        files: plan.paths.length,
        to_user: toUser,
        ...(toUser && title.trim() ? { title: title.trim() } : {}),
      });
      onDone();
      onClose();
    } catch (e) {
      // 올린 자리가 오래돼 치워졌다(host가 수를 맞춰 본다): 다음 「올리기」는 새 자리에 처음부터
      if (e instanceof ApiError && e.body.code === "conflict") {
        upload.current = newUploadId();
        setDone(0);
      }
      setError(say(e));
    } finally {
      setBusy(false);
    }
  };
  const pick = (list: FileList | null) => {
    setFiles(list ? Array.from(list) : []);
    // 새로 고른 파일은 새 올리기다(앞 것과 섞이지 않게)
    upload.current = newUploadId();
    setDone(0);
  };
  return (
    <AdminDialog label="보고서 올리기" busy={busy} onClose={onClose}>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
      >
        <div className="modal-body">
          <p className="op-muted">파일 여러 개나 폴더 하나를 고른다. 파일당 16MB, 합계 64MB까지. 폴더를 고르면 그 안이 보고서가 된다.</p>
          <div className="op-buttons">
            <label className="btn">
              파일 고르기
              <input type="file" multiple hidden onChange={(e) => pick(e.target.files)} disabled={busy} data-testid="upload-files" />
            </label>
            <label className="btn">
              폴더 고르기
              <input type="file" hidden {...{ webkitdirectory: "" }} onChange={(e) => pick(e.target.files)} disabled={busy} data-testid="upload-folder" />
            </label>
          </div>
          {plan.paths.length > 0 && (
            <ul className="admin-files" data-testid="upload-plan">
              {plan.paths.slice(0, 20).map((p) => (
                <li key={p.path} className="mono">
                  {p.path}
                </li>
              ))}
              {plan.paths.length > 20 && <li className="op-muted">그 밖 {plan.paths.length - 20}개</li>}
            </ul>
          )}
          {plan.skipped.length > 0 && <p className="op-muted">숨은 파일 {plan.skipped.length}개는 뺐다.</p>}
          <label className="op-field">
            <span>보고서 이름(소문자·숫자·-·_)</span>
            <input value={name} onChange={(e) => setName(e.target.value.trim())} placeholder="plan-review" disabled={busy} data-testid="upload-name" />
          </label>
          <label className="op-check">
            <input type="checkbox" checked={toUser} onChange={(e) => setToUser(e.target.checked)} disabled={busy} data-testid="upload-to-user" />
            <span>사용자 보고서로 보낸다(「내 보고서」, 안 봄으로 표시)</span>
          </label>
          {toUser && (
            <label className="op-field">
              <span>제목(없으면 이름)</span>
              <input value={title} onChange={(e) => setTitle(e.target.value)} disabled={busy} data-testid="upload-title" />
            </label>
          )}
          {busy && plan.paths.length > 0 && (
            <p role="status">
              올리는 중 {done}/{plan.paths.length}
            </p>
          )}
          {(problem || error) && (
            <p className="op-warn" role="alert">
              {error ?? problem}
            </p>
          )}
        </div>
        <footer className="modal-footer">
          <button type="button" className="btn" onClick={onClose} disabled={busy}>
            닫기
          </button>
          <button
            type="submit"
            className="btn btn-primary"
            disabled={busy || files.length === 0 || !!problem || !reportNameOk(name)}
            data-testid="upload-submit"
          >
            올리기
          </button>
        </footer>
      </form>
    </AdminDialog>
  );
}

/** 「이 aaa」의 공유 링크 키 교체(사용자 답 5A): 지금 살아 있는 링크 수를 보이고 모두 끊긴다고 알린다 */
export function KeyRotate() {
  const admin = useAdmin();
  const [links, setLinks] = useState<LinkView[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState(false);
  if (!admin) return null;
  const open = () => {
    setError(null);
    call({ op: "library_links", project: null })
      .then((r) => setLinks(r.kind === "library_links" ? r.links : []))
      .catch((e: unknown) => setError(say(e)));
  };
  const live = links ? liveLinks(links, Date.now()) : [];
  return (
    <div className="op-section" data-testid="key-rotate">
      <h3>공유 링크 키</h3>
      <p>보고서 공유 링크를 서명하는 키를 새로 만든다. 지금까지 낸 공유 링크는 모두 끊긴다(보고서와 기기 짝짓기는 그대로).</p>
      {done && <p role="status">키를 바꿨다. 옛 공유 링크는 모두 끊겼다.</p>}
      {error && (
        <p className="op-warn" role="alert">
          {error}
        </p>
      )}
      <button type="button" className="btn" onClick={open} data-testid="key-rotate-open">
        키 교체…
      </button>
      {links && (
        <ConfirmDialog
          title="공유 링크 키를 바꿀까요?"
          body={`살아 있는 공유 링크 ${live.length}개가 모두 끊긴다. 보낸 사람에게는 새 링크를 다시 보내야 한다. 되돌리지 않는다.`}
          confirmLabel="키 교체"
          onConfirm={async () => {
            await call({ op: "library_rotate" });
            setLinks(null);
            setDone(true);
          }}
          onClose={() => setLinks(null)}
        />
      )}
    </div>
  );
}
