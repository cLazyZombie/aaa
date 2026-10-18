// 웹 빌드가 함께 넣은 npm 패키지의 라이선스 고지(plan §8.175). 빌드 bundle(주 빌드와 worker 빌드)이 실제로 그린 module의
// 경로에서 패키지를 모아, 그 패키지의 LICENSE·NOTICE 글을 dist/licenses/THIRD_PARTY_NOTICES.txt 하나로 낸다. host는 dist를
// 실행 파일에 넣어 `/licenses/THIRD_PARTY_NOTICES.txt`로 보낸다. 고지 글에는 저자 email이 있어(공개 저장소 검사
// scripts/check-public.sh가 막는다) 저장소에 두지 않고 빌드마다 node_modules에서 모은다.
import { mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import type { Plugin } from "vite";

/** module 경로의 앞부분 → 그 소스를 낸 패키지 폴더(node_modules 밖으로 옮긴 소스, 예: IME 패치를 붙인 xterm) */
export type Sources = readonly (readonly [prefix: string, dir: string])[];

/** module 경로가 든 패키지 폴더(마지막 node_modules 아래 이름). `.cache`처럼 점으로 시작하는 폴더는 패키지가 아니다 */
export function packageDir(id: string, sources: Sources = []): string | null {
  for (const [prefix, dir] of sources) if (id.startsWith(prefix)) return dir;
  return /^(.*\/node_modules\/(?:@[^/.][^/]*\/)?[^/.][^/]*)\//.exec(id)?.[1] ?? null;
}

const NOTICE_FILE = /^(?:licen[cs]e|copying|notice)(?:[.-].*)?$/i;

function person(author: unknown): string {
  if (typeof author === "string") return author;
  if (author && typeof author === "object" && "name" in author) {
    const { name, email } = author as { name: string; email?: string };
    return email ? `${name} <${email}>` : name;
  }
  return "적지 않음";
}

/** 한 패키지의 고지: 이름·판·라이선스와 그 LICENSE·NOTICE 글. 글을 싣지 않은 패키지는 package.json의 license·author */
export function packageNotice(dir: string): string {
  const pkg = JSON.parse(readFileSync(join(dir, "package.json"), "utf8"));
  const license = typeof pkg.license === "string" ? pkg.license : JSON.stringify(pkg.license ?? pkg.licenses ?? "적지 않음");
  const files = readdirSync(dir).filter((file) => NOTICE_FILE.test(file)).sort();
  const body =
    files.length > 0
      ? files.map((file) => `--- ${file}\n${readFileSync(join(dir, file), "utf8").trimEnd()}`).join("\n\n")
      : `(이 패키지는 라이선스 글을 싣지 않았다. package.json: license ${license}, author ${person(pkg.author)})`;
  return `${pkg.name}@${pkg.version} (${license})\n${body}`;
}

/** 패키지 폴더들의 고지 한 파일(이름 차례) */
export function notices(dirs: Iterable<string>): string {
  const parts = [...new Set(dirs)].map(packageNotice).sort();
  const rule = `\n\n${"=".repeat(78)}\n\n`;
  return `aaa 웹 빌드가 함께 넣은 npm 패키지 ${parts.length}개의 라이선스 고지. 빌드가 node_modules에서 모았다(web/scripts/license-notices.ts).${rule}${parts.join(rule)}\n`;
}

/**
 * 고지를 모으는 plugin 둘: 주 빌드용(끝에 dist/licenses/에 쓴다)과 worker 빌드용(`worker.plugins`). 둘이 같은 목록을 채운다.
 * worker 빌드는 주 빌드가 그 worker를 만날 때 돌아 주 빌드의 writeBundle보다 먼저 끝난다. 지금 worker 빌드의 패키지는 모두
 * 주 빌드에도 있어(재어 봄) worker 쪽이 더하는 것은 없다. worker 묶음도 따로 실려 나가므로 둔다. 그리지 않은(tree-shake로 빠진)
 * module은 세지 않는다. licenses/는 assets/ 밖이라 service worker의 빌드 이름·파일 목록(service-worker-build.ts)에 들지 않는다.
 */
export function licenseNotices(sources: Sources = []): { main: Plugin; worker: () => Plugin } {
  const dirs = new Set<string>();
  const collect = (name: string): Plugin => ({
    name,
    apply: "build",
    generateBundle(_options, bundle) {
      for (const chunk of Object.values(bundle)) {
        if (chunk.type !== "chunk") continue;
        for (const [id, module] of Object.entries(chunk.modules)) {
          const dir = module.renderedLength > 0 ? packageDir(id, sources) : null;
          if (dir !== null) dirs.add(dir);
        }
      }
    },
  });
  return {
    worker: () => collect("license-notices-worker"),
    main: {
      ...collect("license-notices"),
      writeBundle(options) {
        if (!options.dir) return;
        mkdirSync(join(options.dir, "licenses"), { recursive: true });
        writeFileSync(join(options.dir, "licenses", "THIRD_PARTY_NOTICES.txt"), notices(dirs));
      },
    },
  };
}
