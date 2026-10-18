// herdr-web-ui(aa624cb) `src/lib/workspaceOrder.ts`의 `placeBlock`만 옮겼다(aaa 목록 pane 줄 순서, plan §8.195). herdr에 옮기기를
// 보내는 `workspaceMoves`는 aaa에 없다: 순서는 host 공유 설정 `paneOrder`에 둔다.

/** `order` with `block` (one workspace, or a repository's workspace and its worktrees) put back before or after the workspaces of `around`. */
export function placeBlock(order: readonly string[], block: readonly string[], around: readonly string[], side: "before" | "after"): string[] {
  const rest = order.filter((id) => !block.includes(id));
  const at = around.map((id) => rest.indexOf(id)).filter((index) => index >= 0);
  if (at.length === 0) return [...order];
  const edge = side === "before" ? Math.min(...at) : Math.max(...at) + 1;
  return [...rest.slice(0, edge), ...block, ...rest.slice(edge)];
}
