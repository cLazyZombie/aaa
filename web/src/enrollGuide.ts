// 설정 「머신」의 「직접 설치」 안내(웹 머신 추가 M1, plan §8.373). SSH 없이 새 머신에서 사람이 칠 명령을 만든다. 주소 후보·host 지문·판은
// host `enroll_guide`가 준다(옛 host는 없어 주소 자리에 `<host 주소>`). 승인은 같은 화면의 「승인…」(6자리 코드)이다.
import { apiAtLeast } from "./automationModel.ts";
import type { EnrollGuide } from "./gen/proto";

/** 1.89 이상 host만 `enroll_guide`를 안다 */
export const canGuide = (frontApi: string | null | undefined): boolean => apiAtLeast(frontApi, 1, 89);

/** host `machine_name_ok`와 같다: 영문·숫자·`.`·`-`·`_` 1~64자 */
export const machineNameOk = (name: string): boolean => /^[A-Za-z0-9._-]{1,64}$/.test(name);

/** 명령에 그대로 넣을 수 있는 주소(이름·IPv4·[IPv6]와 포트). 그 밖이면 자리표시로 둔다 */
export const addressOk = (address: string): boolean => /^[A-Za-z0-9.:[\]_-]{1,255}$/.test(address);

export const SOURCE: Record<string, string> = {
  tailnet: "tailnet 이름",
  hostname: "이 머신 이름",
  listen: "연 주소 그대로",
  loopback: "이 host 머신 안에서만",
};

export type GuideStep = { title: string; command?: string; note?: string };

const BIN = "~/.local/bin/aaa";

/**
 * 새 머신에서 할 차례. 이름·주소가 쓸 수 없으면 그 자리를 `<…>`로 두고(명령이 틀린 채 복사되지 않게) 칸 옆에 까닭을 보인다.
 * 등록 창은 같은 화면에서 먼저 연다(창이 닫혀 있으면 host가 등록 요청을 거부한다)
 */
export function guideSteps(guide: EnrollGuide | null, address: string, name: string): GuideStep[] {
  const addr = addressOk(address) ? address : "<host 주소>";
  const who = machineNameOk(name) ? name : "<이름>";
  const fp = guide ? `${guide.host_fingerprint.slice(0, 16)}…` : null;
  return [
    {
      title: `새 머신에 ${guide ? `같은 판(${guide.software})` : "host와 같은 판"}의 aaa를 ${BIN}에 설치한다`,
      note: "README 「설치」 1·2단계. 판이 다르면 연결이 거부될 수 있다.",
    },
    { title: "새 머신에서 등록을 요청한다", command: `${BIN} agent enroll ${addr} --name ${who}` },
    {
      title: "새 머신 화면을 이 화면과 맞춘다",
      note: `${fp ? `host fingerprint가 ${fp}인지 보고, ` : ""}나온 6자리 코드를 위 「승인을 기다리는 머신」의 「승인…」에 넣는다.`,
    },
    {
      title: "새 머신이 host로 계속 붙게 설정한다",
      command: `mkdir -p ~/.config/aaa && ${BIN} config example agent | sed 's/^host = "local"/host = "${addr}"/' > ~/.config/aaa/agent.toml`,
    },
    {
      title: "agent를 서비스로 띄운다(macOS)",
      command: `${BIN} service install --role agent --aaa ${BIN}`,
      note: "다른 OS는 아직 서비스 등록이 없다: 그 머신에서 aaa agent를 직접 띄운다.",
    },
  ];
}
