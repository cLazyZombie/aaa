// herdr-web-ui `src/components/AgentMark.tsx`(aa624cb)를 그대로 옮겼다(import 경로만, README 「AgentMark」, plan §8.56).
// 표시(Claude·OpenAI 등 로고)는 각 상표권자의 것이다.
import { useId } from "react";

import { fallbackInitial } from "./agentMark.ts";
import { AGENT_SVG_MARKS, CLAUDE_COLOR_PATH } from "./agentSvgMarks.ts";
import "./AgentMark.css";

/**
 * Provider marks for the agents herdr names, ported from chatmux's
 * llm-logo-provider set (same owner, same visual language). Monochrome marks
 * (codex, cursor, the fallback) ride currentColor; claude/omp/opencode/gjc/omo
 * carry their own brand fills.
 */

export interface AgentMarkProps {
  agent: string;
  /** box size in px */
  size?: number;
  className?: string;
}

// Claude는 LobeHub Icons `claude-color`(@lobehub/icons-static-svg 1.95.1 `icons/claude-color.svg`, MIT, web/src/herdr/README.md 고지)로
// 바꿨다(사용자 요청 10-19, plan §8.251). 참고의 타일(주황 둥근 사각형 + 흰 별)은 없고 주황 별 하나다. 「모노」 표시에서는 글자색이 된다(AgentMark.css)
function ClaudeMark({ size }: { size: number }) {
  return (
    <svg viewBox="0 0 24 24" width={size} height={size} role="img" aria-label="claude">
      <path className="agent-mark-glyph" fill="#D97757" fillRule="nonzero" d={CLAUDE_COLOR_PATH} />
    </svg>
  );
}

function CodexMark({ size }: { size: number }) {
  return (
    <svg viewBox="100 100 520 520" width={size} height={size} role="img" aria-label="codex">
      <path d="M304.246 294.611V249.028C304.246 245.189 305.687 242.309 309.044 240.392L400.692 187.612C413.167 180.415 428.042 177.058 443.394 177.058C500.971 177.058 537.44 221.682 537.44 269.182C537.44 272.54 537.44 276.379 536.959 280.218L441.954 224.558C436.197 221.201 430.437 221.201 424.68 224.558L304.246 294.611ZM518.245 472.145V363.224C518.245 356.505 515.364 351.707 509.608 348.349L389.174 278.296L428.519 255.743C431.877 253.826 434.757 253.826 438.115 255.743L529.762 308.523C556.154 323.879 573.905 356.505 573.905 388.171C573.905 424.636 552.315 458.225 518.245 472.141V472.145ZM275.937 376.182L236.592 353.152C233.235 351.235 231.794 348.354 231.794 344.515V238.956C231.794 187.617 271.139 148.749 324.4 148.749C344.555 148.749 363.264 155.468 379.102 167.463L284.578 222.164C278.822 225.521 275.942 230.319 275.942 237.039V376.186L275.937 376.182ZM360.626 425.122L304.246 393.455V326.283L360.626 294.616L417.002 326.283V393.455L360.626 425.122ZM396.852 570.989C376.698 570.989 357.989 564.27 342.151 552.276L436.674 497.574C442.431 494.217 445.311 489.419 445.311 482.699V343.552L485.138 366.582C488.495 368.499 489.936 371.379 489.936 375.219V480.778C489.936 532.117 450.109 570.985 396.852 570.985V570.989ZM283.134 463.99L191.486 411.211C165.094 395.854 147.343 363.229 147.343 331.562C147.343 294.616 169.415 261.509 203.48 247.593V356.991C203.48 363.71 206.361 368.508 212.117 371.866L332.074 441.437L292.729 463.99C289.372 465.907 286.491 465.907 283.134 463.99ZM277.859 542.68C223.639 542.68 183.813 501.895 183.813 451.514C183.813 447.675 184.294 443.836 184.771 439.997L279.295 494.698C285.051 498.056 290.812 498.056 296.568 494.698L417.002 425.127V470.71C417.002 474.549 415.562 477.429 412.204 479.346L320.557 532.126C308.081 539.323 293.206 542.68 277.854 542.68H277.859ZM396.852 599.776C454.911 599.776 503.37 558.513 514.41 503.812C568.149 489.896 602.696 439.515 602.696 388.176C602.696 354.587 588.303 321.962 562.392 298.45C564.791 288.373 566.231 278.296 566.231 268.224C566.231 199.611 510.571 148.267 446.274 148.267C433.322 148.267 420.846 150.184 408.37 154.505C386.775 133.392 357.026 119.958 324.4 119.958C266.342 119.958 217.883 161.22 206.843 215.921C153.104 229.837 118.557 280.218 118.557 331.557C118.557 365.146 132.95 397.771 158.861 421.283C156.462 431.36 155.022 441.437 155.022 451.51C155.022 520.123 210.682 571.466 274.978 571.466C287.931 571.466 300.407 569.549 312.883 565.228C334.473 586.341 364.222 599.776 396.852 599.776Z" fill="currentColor" />
    </svg>
  );
}

function CursorMark({ size }: { size: number }) {
  return (
    <svg viewBox="0 0 24 24" width={size} height={size} role="img" aria-label="cursor">
      <path d="M11.925 24l10.425-6-10.425-6L1.5 18l10.425 6z" fill="currentColor" opacity=".39" />
      <path d="M22.35 18V6L11.925 0v12l10.425 6z" fill="currentColor" opacity=".8" />
      <path d="M11.925 0L1.5 6v12l10.425-6V0z" fill="currentColor" opacity=".6" />
      <path d="M22.35 6L11.925 24V12L22.35 6z" fill="currentColor" opacity=".72" />
      <path d="M22.35 6l-10.425 6L1.5 6h20.85z" fill="currentColor" opacity=".95" />
    </svg>
  );
}

function OpenCodeMark({ size }: { size: number }) {
  return (
    <svg viewBox="0 0 300 300" width={size} height={size} role="img" aria-label="opencode">
      <g transform="translate(30 0)">
        <path d="M180 240H60V120H180V240Z" fill="#4B4646" />
        <path d="M180 60H60V240H180V60ZM240 300H0V0H240V300Z" fill="#F1ECEC" />
      </g>
    </svg>
  );
}

function OmpMark({ size }: { size: number }) {
  const gradientId = useId();
  return (
    <svg viewBox="0 0 64 64" width={size} height={size} role="img" aria-label="omp">
      <defs>
        <linearGradient id={gradientId} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#ed4abf" />
          <stop offset=".5" stopColor="#9b4dff" />
          <stop offset="1" stopColor="#5ad8e6" />
        </linearGradient>
      </defs>
      <rect width="64" height="64" rx="12" fill="#0f0a14" />
      <path fill={`url(#${gradientId})`} d="M14 16h36v8H40v32h-8V24h-6v22h-8V24h-4z" />
    </svg>
  );
}

/** gjc (Gajae Code): its mascot, a straw hat over a lobster-red visor. Ids are per instance. */
function GjcMark({ size }: { size: number }) {
  const id = useId();
  return (
    <svg viewBox="76 62 872 872" width={size} height={size} role="img" aria-label="gjc">
      <defs>
        <linearGradient id={`${id}-hatGold`} x1="0" y1="0" x2="0.85" y2="1">
          <stop offset="0" stopColor="#FFD65B"/>
          <stop offset="0.48" stopColor="#FFB52B"/>
          <stop offset="1" stopColor="#F69A15"/>
        </linearGradient>
        <linearGradient id={`${id}-hatHighlight`} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#FFE17A"/>
          <stop offset="1" stopColor="#F5A11C"/>
        </linearGradient>
        <linearGradient id={`${id}-hatBand`} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#9C5317"/>
          <stop offset="1" stopColor="#5D2A0E"/>
        </linearGradient>
        <linearGradient id={`${id}-lobsterRed`} x1="0" y1="0" x2="0.8" y2="1">
          <stop offset="0" stopColor="#FF5128"/>
          <stop offset="0.52" stopColor="#FF3518"/>
          <stop offset="1" stopColor="#E91F12"/>
        </linearGradient>
        <linearGradient id={`${id}-lobsterRedHighlight`} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#FF6030"/>
          <stop offset="1" stopColor="#F32615"/>
        </linearGradient>
        <linearGradient id={`${id}-eyeGreen`} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#A5FFB8"/>
          <stop offset="0.5" stopColor="#83FCA5"/>
          <stop offset="1" stopColor="#6FEF98"/>
        </linearGradient>
        <filter id={`${id}-eyeGlow`} x="-80%" y="-250%" width="260%" height="600%" colorInterpolationFilters="sRGB">
          <feGaussianBlur stdDeviation="12" result="blur"/>
          <feColorMatrix in="blur" type="matrix" values="0 0 0 0 0.29  0 0 0 0 1  0 0 0 0 0.55  0 0 0 0.55 0" result="greenBlur"/>
          <feMerge>
            <feMergeNode in="greenBlur"/>
            <feMergeNode in="SourceGraphic"/>
          </feMerge>
        </filter>
      </defs>
      <path d="M284 493 C307 423 337 375 377 349 C416 318 463 303 512 303 C561 303 608 318 647 349 C687 375 717 423 740 493 C615 454 409 454 284 493 Z" fill={`url(#${id}-hatGold)`}/>
      <path d="M294 455 C411 408 613 408 730 455 L739 508 C610 465 414 465 285 508 Z" fill={`url(#${id}-hatBand)`} opacity="0.96"/>
      <path d="M371 478 C355 367 304 260 197 229" fill="none" stroke={`url(#${id}-lobsterRedHighlight)`} strokeWidth="38" strokeLinecap="round"/>
      <path d="M653 478 C669 367 720 260 827 229" fill="none" stroke={`url(#${id}-lobsterRedHighlight)`} strokeWidth="38" strokeLinecap="round"/>
      <ellipse cx="512" cy="613" rx="372" ry="104" fill="#06162F"/>
      <path d="M98 613 C109 510 295 451 512 451 C729 451 915 510 926 613 C935 681 850 744 759 765 L763 717 C833 698 883 655 881 613 C875 554 711 513 512 513 C313 513 149 554 143 613 C141 655 191 698 261 717 L265 765 C174 744 89 681 98 613 Z" fill={`url(#${id}-hatHighlight)`}/>
      <rect x="262" y="560" width="500" height="226" rx="94" fill={`url(#${id}-lobsterRed)`}/>
      <rect x="293" y="586" width="438" height="174" rx="71" fill="#06162F"/>
      <rect x="300" y="593" width="424" height="160" rx="65" fill="#071A35" opacity="0.36"/>
      <g filter={`url(#${id}-eyeGlow)`}>
        <rect x="350" y="654" width="116" height="31" rx="15.5" fill={`url(#${id}-eyeGreen)`}/>
        <rect x="558" y="654" width="116" height="31" rx="15.5" fill={`url(#${id}-eyeGreen)`}/>
      </g>
    </svg>
  );
}

/** omo: the project's light tile with its dark mark. */
function OmoMark({ size }: { size: number }) {
  return (
    <svg viewBox="0 0 1024 1024" width={size} height={size} role="img" aria-label="omo">
      <path fill="#F4F4F4" d="M512.00 100.50 C591.13 100.50 697.66 102.37 749.39 106.27 C801.11 110.17 802.35 115.84 822.35 123.90 C842.34 131.96 856.40 141.68 869.36 154.64 C882.32 167.60 892.04 181.66 900.10 201.65 C908.16 221.65 913.83 222.89 917.73 274.61 C921.63 326.34 923.50 432.87 923.50 512.00 C923.50 591.13 921.63 697.66 917.73 749.39 C913.83 801.11 908.16 802.35 900.10 822.35 C892.04 842.34 882.32 856.40 869.36 869.36 C856.40 882.32 842.34 892.04 822.35 900.10 C802.35 908.16 801.11 913.83 749.39 917.73 C697.66 921.63 591.13 923.50 512.00 923.50 C432.87 923.50 326.34 921.63 274.61 917.73 C222.89 913.83 221.65 908.16 201.65 900.10 C181.66 892.04 167.60 882.32 154.64 869.36 C141.68 856.40 131.96 842.34 123.90 822.35 C115.84 802.35 110.17 801.11 106.27 749.39 C102.37 697.66 100.50 591.13 100.50 512.00 C100.50 432.87 102.37 326.34 106.27 274.61 C110.17 222.89 115.84 221.65 123.90 201.65 C131.96 181.66 141.68 167.60 154.64 154.64 C167.60 141.68 181.66 131.96 201.65 123.90 C221.65 115.84 222.89 110.17 274.61 106.27 C326.34 102.37 432.87 100.50 512.00 100.50Z"/>
      <g transform="scale(0.5)">
        <g transform="translate(0.000000,2048.000000) scale(0.050000,-0.050000)">
          <path fill="#041617" fillRule="evenodd" d="M9270 32167 c-309 -64 -456 -152 -621 -373 -562 -748 -776 -2689 -547 -4984 118 -1187 325 -2600 570 -3890 143 -753 152 -653 -128 -1380 -403 -1045 -643 -1873 -812 -2800 -853 -4689 1979 -7986 7858 -9148 1140 -225 2514 -397 3202 -400 l361 -2 79 55 c264 182 210 642 -111 942 -387 362 -950 532 -2801 845 -3004 507 -4249 965 -5520 2030 -1358 1138 -2012 2878 -1809 4810 545 5195 6617 8726 13529 7866 5506 -684 9480 -4255 9480 -8518 0 -2607 -1462 -4522 -4157 -5448 -927 -318 -1534 -459 -3203 -740 -1851 -313 -2414 -483 -2801 -845 -321 -300 -375 -760 -111 -942 l79 -55 361 2 c688 3 2062 175 3202 400 5879 1162 8711 4459 7858 9148 -169 927 -409 1755 -812 2800 -280 727 -271 627 -128 1380 245 1290 452 2703 570 3890 271 2720 -77 4861 -855 5259 -758 388 -2310 -238 -5021 -2024 -868 -571 -764 -557 -1489 -211 -3271 1558 -6755 1558 -10026 0 -725 -346 -621 -360 -1489 211 -2371 1562 -3959 2278 -4708 2122z m4132 -3345 c175 -175 124 -268 -409 -739 -1028 -910 -1995 -2053 -2713 -3209 -97 -156 -200 -318 -228 -359 -221 -321 -646 -76 -469 270 153 301 883 1334 1388 1965 578 723 1466 1614 2029 2035 191 142 288 151 402 37z m14507 -1 c496 -341 1482 -1323 2080 -2071 505 -631 1235 -1664 1388 -1965 177 -346 -248 -591 -469 -270 -28 41 -131 203 -228 359 -718 1156 -1685 2299 -2713 3209 -535 473 -585 564 -407 742 100 100 199 99 349 -4z M19910 25308 c-4039 -205 -7212 -1666 -9086 -4184 -1840 -2473 -1753 -5794 196 -7540 1161 -1040 2447 -1518 5420 -2015 1374 -229 1998 -380 2456 -591 754 -348 1163 -1000 1033 -1650 -10 -49 -1 -72 47 -124 l60 -64 444 0 444 0 60 64 c48 52 57 75 47 124 -130 650 279 1302 1033 1650 458 211 1082 362 2456 591 2973 497 4259 975 5420 2015 2084 1866 2026 5374 -131 7956 -2063 2469 -5995 3966 -9899 3768z m-4644 -4374 c1356 -360 2272 -1909 1998 -3378 -426 -2278 -2856 -3144 -4424 -1576 -1497 1496 -924 4218 1031 4899 393 137 996 161 1395 55z m11480 35 c1491 -261 2504 -1921 2154 -3532 -487 -2247 -3108 -3024 -4525 -1341 -1719 2042 -129 5310 2371 4873z m-6948 -2980 c172 -40 341 -133 495 -273 95 -86 143 -116 187 -116 44 0 92 30 187 116 718 650 1716 188 1854 -860 35 -267 18 -968 -25 -1048 -119 -219 -350 -251 -491 -68 -68 89 -73 124 -85 670 -13 623 -25 675 -200 838 -212 198 -516 199 -735 2 -180 -161 -192 -212 -205 -840 -12 -546 -17 -581 -85 -670 -122 -159 -308 -159 -430 0 -68 89 -73 124 -85 670 -13 628 -25 679 -205 840 -219 197 -523 196 -735 -2 -175 -163 -187 -215 -200 -838 -12 -546 -17 -581 -85 -670 -141 -183 -372 -151 -491 68 -74 136 -54 1039 30 1332 170 594 758 978 1304 849z M14340 20290 c-1858 -372 -2230 -3228 -546 -4191 1280 -732 2786 338 2786 1981 0 1391 -1068 2445 -2240 2210z M25990 20291 c-1733 -343 -2208 -2981 -732 -4069 1287 -949 2981 108 2982 1860 0 1392 -1070 2443 -2250 2209z"/>
        </g>
      </g>
    </svg>
  );
}

/** The agent nobody has drawn yet: its initial in a quiet disc. */
/** A mark kept as SVG markup; its ids get this instance's prefix so copies never collide. */
function SvgMark({ agent, size }: { agent: string; size: number }) {
  const prefix = useId().replace(/[^\w-]/g, "");
  const { viewBox, body } = AGENT_SVG_MARKS[agent]!;
  const html = body.replace(/(id="|url\(#|href="#)([\w-]+)/g, `$1${prefix}-$2`);
  return <svg viewBox={viewBox} width={size} height={size} role="img" aria-label={agent} dangerouslySetInnerHTML={{ __html: html }} />;
}

function FallbackMark({ agent, size }: { agent: string; size: number }) {
  const initial = fallbackInitial(agent);
  return (
    <svg viewBox="0 0 24 24" width={size} height={size} role="img" aria-label={agent}>
      <circle cx="12" cy="12" r="11" fill="var(--bg-elevated)" stroke="var(--border)" strokeWidth="1" />
      <text x="12" y="16" textAnchor="middle" fontSize="11" fill="currentColor">
        {initial}
      </text>
    </svg>
  );
}

const DRAWN_MARKS = new Set(["claude", "omp", "codex", "cursor", "opencode", "gjc", "omo"]);
/** the drawn marks with brand fills; codex and cursor ride currentColor */
const BRAND_MARKS = new Set(["claude", "omp", "opencode", "gjc", "omo"]);

/**
 * Whether `agent`'s mark carries its own brand colors (as against riding currentColor, like codex,
 * cursor and the fallback initial): Settings → Appearance → Agent logos → Mono greys only these.
 */
export function hasBrandColors(agent: string): boolean {
  if (BRAND_MARKS.has(agent)) return true;
  const svg = AGENT_SVG_MARKS[agent];
  return svg !== undefined && /fill="#(?!fff\b|ffffff\b)/i.test(svg.body);
}

/** Whether `agent` has a mark of its own rather than the fallback's initial. */
export function hasAgentMark(agent: string): boolean {
  return DRAWN_MARKS.has(agent) || Object.hasOwn(AGENT_SVG_MARKS, agent);
}

export function AgentMark({ agent, size = 16, className }: AgentMarkProps) {
  const mark =
    agent === "claude" ? (
      <ClaudeMark size={size} />
    ) : agent === "omp" ? (
      <OmpMark size={size} />
    ) : agent === "codex" ? (
      <CodexMark size={size} />
    ) : agent === "cursor" ? (
      <CursorMark size={size} />
    ) : agent === "opencode" ? (
      <OpenCodeMark size={size} />
    ) : agent === "gjc" ? (
      <GjcMark size={size} />
    ) : agent === "omo" ? (
      <OmoMark size={size} />
    ) : Object.hasOwn(AGENT_SVG_MARKS, agent) ? (
      <SvgMark agent={agent} size={size} />
    ) : (
      <FallbackMark agent={agent} size={size} />
    );
  return <span className={["agent-mark", agent, hasBrandColors(agent) ? "is-brand" : null, className].filter(Boolean).join(" ")}>{mark}</span>;
}
