// 붙여넣기 꼬리표 걷기(plan §8.262 붙여넣기 꼬리표, 사용자 10-19 「이건 왜 나오지?」). 웹 입력창 글은 pane에 bracketed paste로 들어가고,
// Claude Code(2.1.303)는 붙여넣은 글을 대화 기록(JSONL)에 `<pasted_content id="7f06">`·`</pasted_content id="7f06">` 줄로 감싸 남긴다.
// 대화 보기와 보내기 확인(기록에 같은 글이 올랐나)이 이것으로 사람이 친 글을 되찾는다. 꼬리표가 글 전체를 감싸고 두 id가 같을
// 때만 걷는다: 사람이 실제로 쓴 같은 모양 글(설명 속 꼬리표, 한쪽만 있는 꼬리표)은 그대로 둔다. 안의 글은 바꾸지 않는다.

const WRAPPED = /^<pasted_content id="([\w-]+)">\r?\n?([\s\S]*?)\r?\n?<\/pasted_content id="\1">$/;

/** 꼬리표가 글 전체를 감쌌으면 안의 글, 아니면 그대로 */
export function unwrapPasted(text: string): string {
  const m = WRAPPED.exec(text.trim());
  return m ? m[2]! : text;
}
