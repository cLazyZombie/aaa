// 머신 등록·기기·웹 공개·보고서 올리기·키 교체 규칙(src/adminModel.ts, 관리 GUI W5b, plan §8.352). 권한·폐기 거부·올리기 상한의
// 정본은 host다(lib.rs `the_web_keeps_its_last_controller_and_the_host_machine`, library_upload/tests.rs).
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { canAdmin, enrollCode, enrollLeft, lastController, liveLinks, newUploadId, onPublicAddress, otherRoutes, reportNameOk, uploadPlan } from "../src/adminModel.ts";

const MB = 1 << 20;
const publish = (url) => ({ tailscale: "running", dns_name: "mac-a.example.ts.net", url, owned_port: url ? 443 : null, web_port: 7781, message: null });

describe("관리 W5b", () => {
  it("웹 머신 등록·공개 바꾸기·올리기·키 교체는 1.80 이상 host만", () => {
    for (const v of [undefined, null, "1.79", "2.80"]) assert.equal(canAdmin(v), false, String(v));
    for (const v of ["1.80", "1.91"]) assert.equal(canAdmin(v), true, v);
  });
  it("확인 코드는 숫자 6자리(사이 공백·하이픈은 뺀다)", () => {
    assert.equal(enrollCode("123 456"), "123456");
    assert.equal(enrollCode("123-456"), "123456");
    for (const bad of ["", "12345", "1234567", "abcdef"]) assert.equal(enrollCode(bad), null, bad);
  });
  it("등록 창 남은 분은 올림, 닫혔으면 없다", () => {
    assert.equal(enrollLeft(undefined, 0), null);
    assert.equal(enrollLeft(1000, 1000), null);
    assert.equal(enrollLeft(60_001, 0), 2);
    assert.equal(enrollLeft(60_000, 0), 1);
  });
  it("마지막 조작 기기는 조작 기기가 하나일 때 그 기기뿐(보기 기기는 세지 않는다)", () => {
    const d = (id, role) => ({ id, label: id, role, created_at_ms: 0, last_seen_ms: null });
    assert.equal(lastController([d("a", "controller"), d("r", "reader")], "a"), true);
    assert.equal(lastController([d("a", "controller"), d("r", "reader")], "r"), false);
    assert.equal(lastController([d("a", "controller"), d("b", "controller")], "a"), false);
  });
  it("공개 주소로 열린 화면만 「끊길 수 있음」, 다른 경로는 host 로컬과 포트 전달", () => {
    const p = publish("https://mac-a.example.ts.net");
    assert.equal(onPublicAddress("https://mac-a.example.ts.net", p), true);
    assert.equal(onPublicAddress("http://127.0.0.1:7781", p), false);
    assert.equal(onPublicAddress("https://mac-a.example.ts.net", publish(null)), false);
    assert.equal(onPublicAddress("https://mac-a.example.ts.net", null), false);
    const routes = otherRoutes(p);
    assert.match(routes[0], /http:\/\/127\.0\.0\.1:7781$/);
    assert.match(routes[1], /ssh -L 7781:127\.0\.0\.1:7781/);
    // tailnet 이름의 http 주소는 host가 Host 검사로 거부한다: 경로로 내지 않는다
    assert.ok(!routes.some((r) => r.includes("mac-a.example.ts.net")));
  });
  it("보고서 이름은 host와 같은 꼴(t-로 시작하면 thread 사본 자리라 거부)", () => {
    for (const ok of ["plan-review", "a_1", "x".repeat(100)]) assert.equal(reportNameOk(ok), true, ok);
    for (const bad of ["", "Plan", "a b", "t-0538", "../x", "x".repeat(101), "con", "com1", "lpt9"]) assert.equal(reportNameOk(bad), false, bad);
  });
  it("폴더를 고르면 맨 위 폴더를 빼고, 숨은 파일은 빼며, 상한을 넘으면 까닭을 준다", () => {
    const folder = uploadPlan([
      { name: "index.html", relative: "out/index.html", size: 10 },
      { name: "a.png", relative: "out/img/a.png", size: 10 },
      { name: ".DS_Store", relative: "out/.DS_Store", size: 10 },
      { name: "x", relative: "out/.git/x", size: 10 },
    ]);
    assert.deepEqual(folder.paths, [
      { path: "index.html", index: 0 },
      { path: "img/a.png", index: 1 },
    ]);
    assert.deepEqual(folder.skipped, ["out/.DS_Store", "out/.git/x"]);
    assert.equal(folder.error, null);
    assert.deepEqual(uploadPlan([{ name: "a.md", relative: "", size: 1 }]).paths, [{ path: "a.md", index: 0 }]);
    assert.equal(uploadPlan([]).error, "올릴 파일이 없다");
    // 뺀 숨은 파일은 16MB를 넘어도 막지 않는다
    const packed = uploadPlan([
      { name: "p.pack", relative: "r/.git/p.pack", size: 16 * MB + 1 },
      { name: "index.html", relative: "r/index.html", size: 1 },
    ]);
    assert.equal(packed.error, null);
    assert.deepEqual(packed.skipped, ["r/.git/p.pack"]);
    assert.match(uploadPlan([{ name: "big.bin", relative: "", size: 16 * MB + 1 }]).error, /big\.bin: 파일 하나는 16MB/);
    const five = Array.from({ length: 5 }, (_, i) => ({ name: `${i}.bin`, relative: "", size: 16 * MB }));
    assert.equal(uploadPlan(five.slice(0, 4)).error, null);
    assert.equal(uploadPlan(five).error, "합계 64MB까지 올린다");
  });
  it("키를 바꾸면 끊길 링크는 폐기되지 않고 만료 전인 것", () => {
    const l = (id, revoked, expires) => ({ id, revoked, expires_at_ms: expires });
    assert.deepEqual(
      liveLinks([l("a", false, 10), l("b", true, 10), l("c", false, 5)], 5).map((x) => x.id),
      ["a"],
    );
  });
  it("올리기 이름은 소문자 hex 32자", () => {
    const id = newUploadId();
    assert.match(id, /^[0-9a-f]{32}$/);
    assert.notEqual(newUploadId(), id);
  });
});
