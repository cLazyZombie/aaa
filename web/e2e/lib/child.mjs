// spec 하나를 새 실험실에서 돈다. 입구(web.e2e.mjs)가 파일마다 이 프로세스를 띄운다.
// 직접: NODE_PATH=$(npm root -g) node web/e2e/lib/child.mjs web/e2e/specs/07-scroll-history.mjs
import path from "node:path";
import { pathToFileURL } from "node:url";

// harness를 먼저 불러 실험실을 만든 뒤 spec을 부른다(spec도 같은 harness를 불러 쓴다)
const { runSpec } = await import("./harness.mjs");
const spec = await import(pathToFileURL(path.resolve(process.argv[2])).href);
await runSpec(spec.default);
