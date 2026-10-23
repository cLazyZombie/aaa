import { logIgnoredFailure } from "./errorDiagnostic.ts";
// 질문 안 자료를 같은 Markdown 파서로 찾는다. code·그림을 보고서 링크로 세지 않으며 기기·API·로컬 절대 경로는 열지 않는다.
import { parseMarkdown, type InlineNode, type MarkdownBlock } from "./herdr/markdown.ts";
import { appLinkOf, appRefOf, splitAppRefs } from "./appRef.ts";
import { inAppLink } from "./reportLinks.ts";
import { askFileUrl } from "./asksModel.ts";
import type { ThreadQuestion } from "./gen/proto.ts";

export type QuestionMaterial = { key: string; label: string } & (
  { kind: "report"; name: string; project?: string } | { kind: "url"; url: string }
);
export function questionMaterials({ items, body, reportId, project, base, origin }: {
  items: ThreadQuestion[]; body?: string | null; reportId?: string | null; project?: string; base?: string | null; origin: string;
}): QuestionMaterial[] {
  const found = new Map<string, QuestionMaterial>();
  const report = (name: string, at: string | undefined, label: string) => {
    at ??= project;
    const key = `${at ?? ""}/${name}`;
    if (!found.has(key)) found.set(key, { key, label, kind: "report", name, project: at });
  };
  if (reportId) report(reportId, project, reportId);
  const add = (href: string, label: string) => {
    const ref = appRefOf(href) ?? appLinkOf(href, origin);
    const inside = inAppLink(href, origin);
    if (ref?.kind === "report") return report(ref.target, inside?.kind === "report" ? inside.project : undefined, label);
    const local = askFileUrl(base, href);
    let url: URL;
    try { url = new URL(local ?? href, origin); } catch (ignoredError) { logIgnoredFailure(ignoredError, "questionMaterials.ts:28"); return; }
    if (!local && inside?.kind !== "view" && (url.origin === origin || !/^https?:$/.test(url.protocol))) return;
    const key = url.href;
    if (!found.has(key)) found.set(key, { key, label, kind: "url", url: local ?? url.href });
  };
  const text = (nodes: InlineNode[]): string => nodes.map((n) => "value" in n ? n.value : "children" in n ? text(n.children) : "").join("");
  const inline = (nodes: InlineNode[]) => {
    for (const n of nodes) {
      if (n.type === "link") add(n.href, text(n.children));
      else if (n.type === "file") add(n.path, text(n.children));
      else if (n.type === "text") for (const ref of splitAppRefs(n.value)) { if (typeof ref !== "string" && ref.kind === "report") report(ref.target, project, ref.target); }
      else if ("children" in n) inline(n.children);
    }
  };
  const blocks = (rows: MarkdownBlock[]) => {
    for (const b of rows) {
      if (b.type === "heading") inline(b.content);
      else if (b.type === "paragraph") b.lines.forEach(inline);
      else if (b.type === "list") b.items.forEach((i) => { inline(i.content); if (i.blocks) blocks(i.blocks); });
      else if (b.type === "blockquote") blocks(b.blocks);
      else if (b.type === "table") { b.header.forEach(inline); b.rows.forEach((row) => row.forEach(inline)); }
    }
  };
  for (const markdown of [body ?? "", ...items.flatMap((q) => [q.body ?? "", ...q.choices.map((c) => c.body ?? ""), q.after ?? ""])]) blocks(parseMarkdown(markdown));
  return [...found.values()];
}
