// 아이디어 노트 그림 편집(t-1575): 그림 메뉴가 고치는 노트 글(설명·바꾸기·지우기·위아래)과 올릴 파일 판정.
// 메뉴는 글을 바꿔 새 판으로 저장하므로, 글 변환이 다른 줄·코드 예시를 건드리지 않는 것이 계약이다.
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { ideaMediaFile, ideaMediaSpot, ideaMediaSpots, movedMedia, withMediaAlt, withMediaName, withoutMedia } from "../src/ideasModel.ts";

const A = "0123456789abcdef.png";
const B = "fedcba9876543210.jpg";
const V = "00112233445566ff.mp4";
const note = `# 간판\n\n## 시안\n첫 생각\n둘째 줄\n\n![가게 앞](${A})\n\n![밤](${B})\n\n\`\`\`md\n![예](${A})\n\`\`\`\n\n## 결정\n- 글 사이 ![작은](${V}) 그림\n`;

describe("아이디어 노트 그림", () => {
  // 렌더러가 그리는 차례와 같아야 화면의 n번째 그림이 글의 같은 자리를 고친다: 코드 예시는 세지 않는다
  it("그림 자리", () => {
    const spots = ideaMediaSpots(note);
    assert.deepEqual(spots.map((s) => [s.name, s.alt, s.nth, s.alone]), [[A, "가게 앞", 0, true], [B, "밤", 0, true], [V, "작은", 0, false]]);
    assert.equal(note.slice(spots[0].start, spots[0].end), `![가게 앞](${A})`);
  });

  it("설명·바꾸기·지우기", () => {
    const a = ideaMediaSpot(note, A, 0);
    assert.ok(withMediaAlt(note, a, " 간판 [밤]\n").includes(`![간판 밤](${A})\n`), "한 줄로, 괄호는 빈칸");
    assert.ok(withMediaAlt(note, a, "").includes(`![그림](${A})`));
    const swapped = withMediaName(note, a, "aaaaaaaaaaaaaaaa.webp");
    assert.ok(swapped.includes("![가게 앞](aaaaaaaaaaaaaaaa.webp)") && swapped.includes(`![예](${A})`), "코드 예시는 그대로");
    assert.equal(withoutMedia(note, a), note.replace(`![가게 앞](${A})\n\n`, ""), "줄째, 남는 빈 줄 하나도");
    const inline = ideaMediaSpot(note, V, 0);
    assert.ok(withoutMedia(note, inline).endsWith("- 글 사이  그림\n"), "글 가운데는 그림만");
  });

  // 위아래는 덩이(머리·혼자 선 그림·코드 울타리·빈 줄로 나뉜 글) 단위로 자리를 바꾸고 사이 빈 줄은 둔다
  it("위로·아래로", () => {
    const b = ideaMediaSpot(note, B, 0);
    const up = movedMedia(note, b, -1);
    assert.ok(up.includes(`![밤](${B})\n\n![가게 앞](${A})`), up);
    const twice = movedMedia(up, ideaMediaSpot(up, B, 0), -1);
    assert.ok(twice.includes(`## 시안\n![밤](${B})\n\n첫 생각\n둘째 줄\n`), twice);
    const down = movedMedia(note, b, 1);
    assert.ok(down.includes(`![가게 앞](${A})\n\n\`\`\`md\n![예](${A})\n\`\`\`\n\n![밤](${B})\n\n## 결정`), down);
    assert.equal(movedMedia(note, ideaMediaSpot(note, V, 0), 1), null, "글 가운데 그림은 옮기지 않는다");
    const last = "# 끝\n\n![밤](" + B + ")\n";
    assert.equal(movedMedia(last, ideaMediaSpot(last, B, 0), 1), null, "맨 끝");
  });

  it("올릴 파일 판정", () => {
    const named = ideaMediaFile(new File([new Uint8Array(4)], "IMG_1.JPG", { type: "" }));
    assert.equal(named.type, "image/jpeg", "형식 없는 파일은 확장자로");
    assert.equal(typeof ideaMediaFile(new File(["x"], "a.heic", { type: "image/heic" })), "string");
    assert.equal(typeof ideaMediaFile(new File([], "a.png", { type: "image/png" })), "string", "빈 파일");
    assert.equal(ideaMediaFile(new File([new Uint8Array(4)], "clip.mp4", { type: "video/mp4" })).type, "video/mp4");
  });
});
