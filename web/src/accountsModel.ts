// 설정 「계정」의 순수 규칙(관리 GUI W5a, plan §8.346): host 계정 목록·상태 낱말·로그인 진행 글.
// 판정의 정본은 host다(웹 조작 기기·세션 밖 CLI만 로그인·기본 변경, 메일은 그 둘에만 실린다). 인증 주소·토큰은 화면에 없다.
import { apiAtLeast } from "./automationModel.ts";
import type { AccountDefault, AccountHealth, AccountLoginView, AccountView } from "./gen/proto";

/** 웹 로그인·기본 변경·메일·상태는 1.79 이상 host만 안다(옛 host는 목록만 준다) */
export const canAccounts = (frontApi: string | null | undefined): boolean => apiAtLeast(frontApi, 1, 79);

export const HARNESSES = ["claude", "codex"] as const;
export const HARNESS_LABEL: Record<string, string> = { claude: "Claude", codex: "Codex" };

/** host `valid_name`과 같다: 영문·숫자·`-`·`_`, 64자까지 */
export const validAlias = (name: string): boolean => /^[A-Za-z0-9_-]{1,64}$/.test(name);

/** 날짜만(이 기기의 시간대). 모르면 「모름」 */
export function day(ms: number | undefined | null): string {
  if (ms === undefined || ms === null) return "모름";
  const d = new Date(ms);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

export type Tone = "ok" | "warn" | "bad" | "none";

/** 상태 배지: 끊김은 만료와 갱신 거부 둘이다. 곧 만료는 남은 날 수(올림) */
export function healthWord(h: AccountHealth | undefined, expiresMs: number | undefined, now: number): { text: string; tone: Tone } {
  switch (h) {
    case "ok":
      return { text: "정상", tone: "ok" };
    case "expiring": {
      const left = expiresMs === undefined ? null : Math.max(0, Math.ceil((expiresMs - now) / 86_400_000));
      return { text: left === null ? "곧 만료" : `곧 만료(${left}일 남음)`, tone: "warn" };
    }
    case "expired":
      return { text: "끊김(만료)", tone: "bad" };
    case "rejected":
      return { text: "끊김(갱신 거부)", tone: "bad" };
    default:
      return { text: "", tone: "none" };
  }
}

export type AccountRow = AccountView & { isDefault: boolean };

/** harness마다(Claude·Codex 차례), 그 안은 이름 차례. 계정이 없는 harness도 묶음을 둔다(거기서 로그인한다) */
export function groupAccounts(accounts: AccountView[], defaults: AccountDefault[]): { harness: string; rows: AccountRow[] }[] {
  return HARNESSES.map((harness) => ({
    harness,
    rows: accounts
      .filter((a) => a.harness === harness)
      .sort((a, b) => a.name.localeCompare(b.name))
      .map((a) => ({ ...a, isDefault: defaults.some((d) => d.harness === harness && d.account === a.name) })),
  }));
}

/** 로그인 진행 글(사용자 답 4A: 인증은 host Mac 브라우저에서, 주소는 보이지 않는다) */
export function loginText(v: AccountLoginView): { title: string; lines: string[]; tone: Tone; done: boolean } {
  const who = `${v.account}(${HARNESS_LABEL[v.harness] ?? v.harness})`;
  switch (v.state) {
    case "waiting":
      return {
        title: `${who} 로그인 중`,
        lines: [
          "host Mac 브라우저에서 인증을 마치세요. 인증 주소는 이 화면에 보이지 않습니다.",
          ...(v.expect_email ? [`이 메일로 로그인하세요: ${v.expect_email}`] : []),
        ],
        tone: "none",
        done: false,
      };
    case "mismatch":
      return {
        title: `${who}: 다른 메일로 로그인했습니다`,
        lines: [
          `기록된 메일: ${v.expect_email ?? "모름"}`,
          `로그인한 메일: ${v.email ?? "모름"}`,
          "바꾸면 이 별칭이 새 메일의 인증을 씁니다. 바꾸지 않으면 옛 인증이 그대로 남습니다.",
        ],
        tone: "warn",
        done: false,
      };
    case "succeeded":
      return { title: `${who} 로그인했습니다`, lines: v.email ? [`메일: ${v.email}`] : [], tone: "ok", done: true };
    case "kept":
      return { title: `${who}: 옛 인증을 그대로 두었습니다`, lines: [], tone: "none", done: true };
    case "cancelled":
      return { title: `${who} 로그인을 취소했습니다`, lines: [], tone: "none", done: true };
    case "lost":
      // 웹이 붙인 상태: host가 그 로그인을 더는 모른다(다시 시작했거나 오래된 작업을 정리했다)
      return {
        title: `${who} 로그인 진행을 더 읽지 못했습니다`,
        lines: ["host가 다시 시작했거나 오래된 로그인이다. 결과는 아래 계정 목록에서 확인한다."],
        tone: "warn",
        done: true,
      };
    default:
      return {
        title: `${who} 로그인하지 못했습니다`,
        lines: ["공식 CLI 설치·브라우저 인증·저장 권한을 확인한 뒤 다시 로그인합니다."],
        tone: "bad",
        done: true,
      };
  }
}
