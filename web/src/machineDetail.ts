// 설정 「머신」의 머신 줄 요약과 상세(웹 머신 추가 M0, plan §8.372). 값은 host `state` 사건의 MachineView다: OS·CPU 종류·판·SHA는
// 마지막 hello(host DB에도 남아 끊긴 뒤에도 보인다), 기능·자가 점검은 이 host 실행 중 마지막 연결의 것, 건강은 연결 중일 때의 보고.
import type { MachineView } from "./gen/proto";

const OS: Record<string, string> = { macos: "macOS", linux: "Linux", windows: "Windows" };
const ARCH: Record<string, string> = { aarch64: "arm64", x86_64: "x86_64" };

const gb = (b: number) => `${(b / 2 ** 30).toFixed(1)} GB`;

export const osLabel = (os: string | null | undefined): string | null => (os ? (OS[os] ?? os) : null);
export const archLabel = (arch: string | null | undefined): string | null => (arch ? (ARCH[arch] ?? arch) : null);

/** 「3분 전」 꼴(1분 안은 「방금」) */
export function ago(at: number, now: number): string {
  const s = Math.max(0, Math.round((now - at) / 1000));
  if (s < 60) return "방금";
  if (s < 3600) return `${Math.floor(s / 60)}분 전`;
  if (s < 86_400) return `${Math.floor(s / 3600)}시간 전`;
  return `${Math.floor(s / 86_400)}일 전`;
}

/** 목록 줄의 짧은 요약: OS · CPU 종류 · 판(모르는 것은 뺀다) */
export function machineLine(m: MachineView): string {
  return [osLabel(m.os), archLabel(m.arch), m.software ?? null].filter(Boolean).join(" · ");
}

export type DetailRow = { label: string; value: string; warn?: boolean; mono?: boolean };

/** 상세의 줄들(읽기만). 모르는 값은 「모름」으로 둔다: 옛 host·옛 agent는 칸을 싣지 않는다 */
export function machineRows(m: MachineView, now: number): DetailRow[] {
  const connected = m.connection === "connected";
  const rows: DetailRow[] = [
    {
      label: "연결",
      value: m.local ? "host 머신" : connected ? "연결됨" : m.last_seen_ms ? `끊김(마지막 ${ago(m.last_seen_ms, now)})` : "끊김(붙은 적 없음)",
      warn: !connected,
    },
    { label: "OS", value: [osLabel(m.os), archLabel(m.arch)].filter(Boolean).join(" · ") || "모름" },
    { label: "판", value: m.software ? (m.software_differs ? `${m.software} (host와 다름)` : m.software) : "모름", warn: m.software_differs === true },
    { label: "SHA-256", value: m.sha256 ?? "모름", mono: true },
  ];
  if (m.os === "windows")
    rows.push({ label: "Windows 제한", value: "셸 안에서 실행한 harness는 자동 감지하지 않습니다. Job Object를 통한 세션 자손 수명 관리는 아직 지원하지 않습니다.", warn: true });
  if (m.dev_mode) rows.push({ label: "개발 모드", value: "켜짐" });
  const h = connected ? m.health : null;
  if (h) {
    const disk =
      h.disk_free_bytes !== null && h.disk_total_bytes !== null ? `${gb(h.disk_free_bytes)} 남음 / ${gb(h.disk_total_bytes)}` : "모름";
    rows.push({ label: "작업 디스크", value: disk });
    if (h.load_x100 !== null) rows.push({ label: "부하", value: `${(h.load_x100 / 100).toFixed(2)} / CPU ${h.cpus}개` });
    if (h.on_ac === false) rows.push({ label: "전원", value: "배터리", warn: true });
    for (const t of h.toolchain) rows.push({ label: t.name, value: t.version, warn: !t.ok });
    if (m.clock_skew_ms !== null && Math.abs(m.clock_skew_ms) >= 1000)
      rows.push({ label: "시계 차이", value: `${Math.round(m.clock_skew_ms / 1000)}초(host 기준)` });
    for (const w of h.warnings) rows.push({ label: "주의", value: w, warn: true });
  } else if (connected) rows.push({ label: "건강", value: "보고를 기다린다" });
  for (const p of m.problems) rows.push({ label: "문제", value: p.text, warn: true });
  if (m.selfcheck)
    rows.push({
      label: "시작 점검",
      value: m.selfcheck.pass ? "통과" : `실패: ${m.selfcheck.reasons.join("; ") || "이유 없음"}`,
      warn: !m.selfcheck.pass,
    });
  return rows;
}

/** 판은 실제 조회 결과만 표시한다. 구판·조회 실패는 모름, 끊겨도 마지막 표본을 보인다. */
export function machineInfoGroups(m: MachineView): { title: string; rows: DetailRow[] }[] {
  const h = m.health;
  const info = h?.info;
  const value = (v: string | number | null | undefined) => v == null || v === "" ? "모름" : String(v);
  const at = (n: number | null | undefined) => n ? new Date(n).toLocaleString("ko-KR", { hour12: false }) : "모름";
  return [
    { title: "연결", rows: [
      { label: "상태", value: m.connection === "connected" ? "연결됨" : "끊김" },
      { label: "역할", value: m.local ? "host 머신" : "agent 머신" },
      { label: m.connection === "connected" ? "agent 연결" : "마지막 연결", value: at(m.last_seen_ms) },
      { label: "지표 수집", value: at(h?.at_ms) },
      { label: "정보 수집", value: at(info?.at_ms) },
    ] },
    { title: "시스템", rows: [
      { label: "OS", value: [osLabel(m.os), info?.os_version, archLabel(m.arch)].filter(Boolean).join(" · ") || "모름" },
      { label: "CPU 모델", value: value(info?.cpu_model) },
      { label: "논리 코어", value: value(h?.cpus) },
      { label: "CPU 사용", value: h?.cpu_pct == null ? "모름" : `${h.cpu_pct}%` },
      { label: "메모리", value: h?.mem_used_bytes != null && h?.mem_total_bytes != null ? `${gb(h.mem_used_bytes)} / ${gb(h.mem_total_bytes)}` : "모름" },
      { label: "디스크 여유", value: h?.disk_free_bytes != null && h?.disk_total_bytes != null ? `${gb(h.disk_free_bytes)} / ${gb(h.disk_total_bytes)}` : "모름" },
      { label: "1분 부하", value: h?.load_x100 == null ? "모름" : (h.load_x100 / 100).toFixed(2) },
    ] },
    { title: "소프트웨어", rows: [
      { label: "aaa", value: value(m.software) },
      { label: "Claude Code", value: value(info?.claude) },
      { label: "Codex", value: value(info?.codex) },
      { label: "설치 방식", value: value(info?.installation) },
      { label: "작업 폴더", value: value(info?.work_root) },
    ] },
  ];
}
