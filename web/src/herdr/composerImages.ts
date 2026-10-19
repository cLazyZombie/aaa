// herdr-web-ui aa624cb src/lib/compose.ts의 첨부 경로 삽입. MIT 고지: README.md.
const MAX_COMPOSER_CHARS = 100_000;
export function imageMention(path: string): string {
  return `@${path} `;
}

/**
 * Where an attached file's mention goes: at the start of the message, after a leading `/command`
 * (it must stay first to be one), and after the mentions of files picked before it in the same
 * batch that are still there, so a batch reads in pick order however its uploads finish. An upload
 * finishes while the user may be typing, so the mention never goes where the caret is: the caller
 * shifts the caret and selection by `length` when they sit at or after `at`. The mention goes in
 * whole, with its separating space, or not at all (`length` 0): a cut `@path` would name a file
 * that does not exist, so the caller keeps the attachment waiting until there is room; `missing`
 * says how many characters the message is short of it.
 */
export function insertLeadingMention(
  text: string,
  mention: string,
  earlier: readonly string[],
): { text: string; at: number; length: number; missing?: number } {
  let at = /^\/\S+\s*/u.exec(text)?.[0].length ?? 0;
  for (const previous of earlier) {
    const found = text.indexOf(previous);
    if (found >= 0) at = Math.max(at, found + previous.length);
  }
  const before = text.slice(0, at);
  const snippet = before.length > 0 && !/\s$/u.test(before) ? ` ${mention}` : mention;
  const missing = text.length + snippet.length - MAX_COMPOSER_CHARS;
  if (missing > 0) return { text, at, length: 0, missing };
  return { text: before + snippet + text.slice(at), at, length: snippet.length };
}
