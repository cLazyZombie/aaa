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
