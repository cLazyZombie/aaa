// 보내는 글의 보이지 않는 문자(plan §8.266). zero-width·bidi 제어·tag·variation selector처럼 화면에 아무것도 그리지 않는
// 문자는 agent는 읽지만 보내는 사람은 볼 수 없어, 붙여 넣은 글에 지시를 숨기는 흔한 길이다. 대화 입력창·터미널 입력줄은
// 보내기 전에 이것을 빼고, 뺐으면 몇 개를 뺐는지 알린다. 붙여넣기의 ESC 빼기(pasteText.ts, plan §8.123)는 그대로다.
// 규칙은 t3code(MIT) `packages/shared/src/promptLinks.ts` `HIDDEN_CHARACTERS`·`sanitizeLinkedPrompt`(1451e7cb2)를 옮겼다.
// 다른 곳: 참고는 프롬프트 링크의 글을 다듬어(trim) 줄바꿈을 \n으로 모은다. 여기는 친 글이라 앞뒤 공백·줄바꿈을 그대로 두고
// 문자만 뺀다(탭·줄바꿈·CR은 남긴다). 한글 자모 채움 문자(U+115F·U+1160·U+3164)는 참고대로 뺀다: macOS·iOS 한글 입력은
// 완성형을 내므로 사람이 친 글에는 나오지 않는다.

const HIDDEN_CHARACTERS =
  /[\u0000-\u0008\u000B\u000C\u000E-\u001F\u007F-\u009F­͏؜ᅟᅠ឴឵᠋-᠏​‌‎‏‪-‮⁠-⁯ㅤ︀-︍﻿ﾠ￰-￻\u{1BCA0}-\u{1BCA3}\u{1D173}-\u{1D17A}\u{E0000}-\u{E0FFF}]/gu;

/** 그림 문자와 그 이음의 조각(그림 기호·피부색·그림 표시 선택자): 이 둘 사이의 ZWJ는 👩‍💻 같은 그림 하나를 만든다 */
const isEmojiPart = (codePoint: number | undefined) =>
  codePoint !== undefined && ((codePoint >= 0x1f000 && codePoint <= 0x1faff) || (codePoint >= 0x2190 && codePoint <= 0x2bff) || codePoint === 0xfe0f);

/** 보이지 않는 문자를 뺀 글과 뺀 수. 그림 문자 사이의 ZWJ만 남긴다 */
export function stripHidden(raw: string): { text: string; removed: number } {
  let removed = 0;
  const visible = Array.from(
    raw.replace(HIDDEN_CHARACTERS, () => {
      removed++;
      return "";
    }),
  );
  const text = visible
    .filter((character, index) => {
      if (character !== "‍") return true;
      const kept = isEmojiPart(visible[index - 1]?.codePointAt(0)) && isEmojiPart(visible[index + 1]?.codePointAt(0));
      if (!kept) removed++;
      return kept;
    })
    .join("");
  return { text, removed };
}

/** 보이지 않는 문자뿐이라 보내지 않았을 때 */
export const HIDDEN_ONLY = "보이지 않는 문자뿐이라 보내지 않았습니다.";

/** 뺐을 때 입력창·입력줄이 보이는 한 줄 */
export const hiddenNote = (removed: number) => `보이지 않는 문자 ${removed}개를 빼고 보냈습니다. 숨겨진 지시일 수 있으니 붙여 넣은 곳을 확인하세요.`;
