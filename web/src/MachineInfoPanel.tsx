// herdr-web-ui의 정보 행과 기존 task 상세 시트의 테마 토큰을 따른다.
import { useEffect, useRef } from "react";
import { X } from "lucide-react";
import type { MachineView } from "./gen/proto";
import { machineInfoGroups } from "./machineDetail";
import { DrawerToggle } from "./drawer";
import "./MachineInfoPanel.css";

export function MachineInfoPanel({ machine, onClose, page = false }: { machine: MachineView; onClose: () => void; page?: boolean }) {
  const close = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    const before = document.activeElement;
    close.current?.focus();
    const key = (e: KeyboardEvent) => {
      if (!page && e.key === "Tab" && window.matchMedia("(max-width: 640px)").matches) { e.preventDefault(); close.current?.focus(); }
      if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); onClose(); }
    };
    document.addEventListener("keydown", key, true);
    return () => {
      document.removeEventListener("keydown", key, true);
      if (before instanceof HTMLElement && before.isConnected) before.focus();
    };
  }, [onClose, page]);
  return <aside className={`machine-info-panel${page ? " is-page" : ""}`} aria-label={`${machine.name} 기기 정보`} data-testid="machine-info-panel">
    <header className={`machine-info-head${page ? " term-head" : ""}`}>
      {page && <DrawerToggle />}
      <div><span className="machine-info-caption">기기 정보</span><h2>{machine.name}</h2></div>
      <button ref={close} type="button" className="ghost icon" aria-label="기기 정보 닫기" onClick={onClose}><X aria-hidden="true" /></button>
    </header>
    <div className="machine-info-body">
      {machine.connection !== "connected" && <p className="machine-info-stale">연결이 끊겼습니다. 마지막으로 받은 정보입니다.</p>}
      {machineInfoGroups(machine).map(({ title, rows }) => <section key={title}>
        <h3>{title}</h3><dl>{rows.map(r => <div key={r.label} data-label={r.label}><dt>{r.label}</dt><dd>{r.value}</dd></div>)}</dl>
      </section>)}
    </div>
  </aside>;
}
