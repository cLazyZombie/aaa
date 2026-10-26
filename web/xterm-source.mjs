// xterm 5.5.0을 읽을 수 있는 소스에 IME 패치를 붙여 빌드한다(herdr-web-ui와 같은 방식, patches/README.md).
// herdr-web-ui는 bun patchedDependencies로 node_modules 소스를 고치고 scripts/build-xterm.ts로 빌드한다.
// npm에는 그 기능이 없어서 소스를 cache로 복사한 뒤 같은 patch를 엄격히(문맥이 정확히 맞을 때만) 붙인다.
import ts from "typescript-api";
import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const VERSION = "5.5.0";

/** unified diff를 붙인다. 각 hunk의 옛 줄 묶음이 파일에 정확히 한 번 있어야 한다(아니면 실패). */
export function applyPatch(root, patch) {
  const files = patch.replace(/\r\n/g, "\n").split(/^diff --git /m).filter((s) => s.trim());
  for (const part of files) {
    const lines = part.split("\n");
    const path = /^a\/(\S+) b\//.exec(lines[0])?.[1];
    if (!path) throw new Error(`patch 머리를 읽지 못했다: ${lines[0]}`);
    const file = join(root, path);
    let text = readFileSync(file, "utf8").replace(/\r\n/g, "\n");
    let i = lines.findIndex((l) => l.startsWith("@@"));
    while (i !== -1 && i < lines.length) {
      const oldL = [];
      const newL = [];
      let j = i + 1;
      for (; j < lines.length && !lines[j].startsWith("@@"); j++) {
        const l = lines[j];
        if (l.startsWith(" ")) {
          oldL.push(l.slice(1));
          newL.push(l.slice(1));
        } else if (l.startsWith("-")) oldL.push(l.slice(1));
        else if (l.startsWith("+")) newL.push(l.slice(1));
        else if (l === "" && j === lines.length - 1) break;
        else if (l === "") {
          // 빈 문맥 줄(앞 공백이 지워진 diff)
          oldL.push("");
          newL.push("");
        } else throw new Error(`${path}: 모르는 patch 줄: ${l}`);
      }
      const from = oldL.join("\n");
      const at = text.indexOf(from);
      if (at === -1 || text.indexOf(from, at + 1) !== -1) {
        throw new Error(`${path}: hunk ${lines[i]}의 문맥이 ${at === -1 ? "없다" : "여러 곳에 있다"}`);
      }
      text = text.slice(0, at) + newL.join("\n") + text.slice(at + from.length);
      i = j < lines.length && lines[j].startsWith("@@") ? j : -1;
    }
    writeFileSync(file, text);
  }
}

/** 패치한 xterm 소스를 빌드하고 그 폴더를 돌려준다. */
export function buildXtermSource(webRoot) {
  const pkg = join(webRoot, "node_modules/@xterm/xterm");
  const version = JSON.parse(readFileSync(join(pkg, "package.json"), "utf8")).version;
  if (version !== VERSION) throw new Error(`xterm ${version}: IME 패치는 ${VERSION}에서 확인했다. 판을 바꾸면 패치를 다시 확인한다`);
  const cache = join(webRoot, "node_modules/.cache/aaa-xterm");
  rmSync(cache, { recursive: true, force: true });
  const source = join(cache, "src");
  const output = join(cache, "out");
  mkdirSync(cache, { recursive: true });
  cpSync(join(pkg, "src"), source, { recursive: true });
  applyPatch(cache, readFileSync(join(webRoot, "patches/xterm-5.5.0.patch"), "utf8"));
  // IME는 한 controller가 소유한다. Terminal patch는 입력 사건을 이 경계로 연결하기만 한다.
  cpSync(join(webRoot, "xterm/CompositionHelper.ts"), join(source, "browser/input/CompositionHelper.ts"));
  // aaa: reset의 0 높이·자체 동기화와 합쳐진 사용자 scroll·연속 키(patches/README.md, plan §8.105·t-0839·t-0870)
  applyPatch(cache, readFileSync(join(webRoot, "patches/xterm-5.5.0-viewport.patch"), "utf8"));
  // 배율 monitor도 terminal 수명에 묶는다. 해제한 terminal의 resize/media listener를 남기지 않는다.
  applyPatch(cache, readFileSync(join(webRoot, "patches/xterm-5.5.0-lifecycle.patch"), "utf8"));
  // xterm 소스는 .d.ts의 const enum을 쓴다: TypeScript가 먼저 풀어 줘야 vite가 읽는다
  const program = ts.createProgram(ts.sys.readDirectory(source, [".ts"], undefined, ["**/*"]), {
    target: ts.ScriptTarget.ES2022,
    module: ts.ModuleKind.ESNext,
    moduleResolution: ts.ModuleResolutionKind.Bundler,
    rootDir: source,
    outDir: output,
    paths: { "browser/*": [join(source, "browser/*")], "common/*": [join(source, "common/*")] },
    experimentalDecorators: true,
    useDefineForClassFields: false,
    skipLibCheck: true,
    types: [],
    noEmitOnError: true,
  });
  const emitted = program.emit();
  if (emitted.emitSkipped) {
    const diagnostics = ts.getPreEmitDiagnostics(program).concat(emitted.diagnostics);
    throw new Error(
      ts.formatDiagnosticsWithColorAndContext(diagnostics, {
        getCanonicalFileName: (n) => n,
        getCurrentDirectory: () => webRoot,
        getNewLine: () => "\n",
      }),
    );
  }
  return output;
}
