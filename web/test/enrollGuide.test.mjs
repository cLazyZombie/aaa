// 「직접 설치」 안내의 명령(src/enrollGuide.ts, 웹 머신 추가 M1, plan §8.373). 주소 후보의 정본은 host `enroll_guide::addresses`다
// (`candidates_follow_the_listen_address`).
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { addressOk, canGuide, guideSteps, machineNameOk } from "../src/enrollGuide.ts";

const guide = {
  listen: "0.0.0.0:7430",
  addresses: [{ address: "mac-a.example.ts.net:7430", source: "tailnet" }],
  host_fingerprint: "0123456789abcdef".repeat(4),
  software: "aaa 0.1.2",
};
const commands = (steps) => steps.filter((s) => s.command).map((s) => s.command);

describe("직접 설치 안내 M1", () => {
  it("1.89 이상 host만 주소를 준다", () => {
    for (const v of [undefined, null, "1.88"]) assert.equal(canGuide(v), false, String(v));
    assert.equal(canGuide("1.89"), true);
  });
  it("이름은 host 규칙과 같다", () => {
    for (const ok of ["mac-b", "Mac_B.1", "a".repeat(64)]) assert.equal(machineNameOk(ok), true, ok);
    for (const bad of ["", "mac b", "맥", "a".repeat(65), "mac;rm"]) assert.equal(machineNameOk(bad), false, bad);
  });
  it("주소는 이름·IP·포트 글자만 명령에 넣는다", () => {
    for (const ok of ["mac-a.example.ts.net:7430", "192.0.2.10:7430", "[fd7a::1]:7430"]) assert.equal(addressOk(ok), true, ok);
    for (const bad of ["", "a b:1", "x';rm -rf ~;'", "a/b:1"]) assert.equal(addressOk(bad), false, bad);
  });
  it("명령에 주소·이름·판·지문이 들어간다", () => {
    const steps = guideSteps(guide, "mac-a.example.ts.net:7430", "mac-b");
    assert.deepEqual(commands(steps), [
      "~/.local/bin/aaa agent enroll mac-a.example.ts.net:7430 --name mac-b",
      `mkdir -p ~/.config/aaa && ~/.local/bin/aaa config example agent | sed 's/^host = "local"/host = "mac-a.example.ts.net:7430"/' > ~/.config/aaa/agent.toml`,
      "~/.local/bin/aaa service install --role agent --aaa ~/.local/bin/aaa",
    ]);
    assert.match(steps[0].title, /같은 판\(aaa 0\.1\.2\)/);
    assert.match(steps[2].note, /0123456789abcdef…/);
  });
  it("쓸 수 없는 이름·주소는 자리표시로(틀린 명령을 복사하지 않게), 옛 host는 판·지문 없이", () => {
    const steps = guideSteps(null, "x';rm -rf ~;'", "mac b");
    assert.equal(commands(steps)[0], "~/.local/bin/aaa agent enroll <host 주소> --name <이름>");
    assert.match(steps[0].title, /host와 같은 판/);
    assert.doesNotMatch(steps[2].note, /fingerprint/);
  });
});
