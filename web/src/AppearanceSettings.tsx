// 설정 → 모양(herdr-web-ui SettingsDialog.tsx `AppearancePage`·`PaletteSelect` 235~290줄). 문구는 참고 i18n.ko.ts 그대로다.
// 옮긴 줄: 테마·색상(시스템이면 라이트·다크 색상 따로)·밀도·사이드바 행·상태 표시, 「타이포그래피」의 인터페이스 글자 크기(차이 목록 T4).
// Agents 순서·로고 색은 그 기능을 옮길 때 넣는다. 언어는 두지 않는다(한국어 고정, 사용자 결정, plan §8.169). 밀도·글자 크기는 참고처럼 이 기기 설정이다(SETTING_SCOPE "device").
import { FontFamilyInput } from "./FontFamilyInput";
import { setUiFontFamily, setMonoFontFamily, useUiFontFamily, useMonoFontFamily } from "./settings";
import { Segmented, SettingsGroup, SettingsRow, Stepper } from "./herdr/SettingsControls.tsx";
import { UI_FONT_MAX, UI_FONT_MIN } from "./herdr/typography.ts";
import { uiFontSize, updateAppearance, useAppearance, type Palette } from "./theme";
import { useState } from "react";
import { THEME_CATALOG } from "./themeCatalog.ts";
import { catalogTheme, catalogTokens } from "./catalogTheme.ts";
import "./ThemeGallery.css";

const BASE_PALETTES = [
  { id: "amber", label: "앰버", bg: "#181613", text: "#d8d0c3", accent: "#f0a830", mode: "밝음 · 어두움" },
  { id: "charcoal", label: "차콜", bg: "#171717", text: "#cbc7c0", accent: "#cbc7c0", mode: "밝음 · 어두움" },
  { id: "catppuccin", label: "Catppuccin", bg: "#1e1e2e", text: "#cdd6f4", accent: "#cba6f7", mode: "Latte · Mocha" },
  { id: "github-light", label: "GitHub Light", bg: "#ffffff", text: "#24292f", accent: "#0550ae", mode: "밝음" },
  { id: "github-dark", label: "GitHub Dark", bg: "#0d1117", text: "#c9d1d9", accent: "#58a6ff", mode: "어두움" },
  { id: "github-dark-dimmed", label: "GitHub Dark Dimmed", bg: "#22272e", text: "#adbac7", accent: "#6cb6ff", mode: "어두움" },
] as const;
const searchable = (text: string) => text.normalize("NFD").replace(/[\u0300-\u036f]/g, "").toLocaleLowerCase();
const GALLERY = [...BASE_PALETTES, ...THEME_CATALOG.map((theme) => {
  const tokens = catalogTokens(theme.id);
  return { id: theme.id, label: theme.label, bg: tokens["--bg"], text: tokens["--text"], accent: tokens["--accent"], mode: theme.mode === "dark" ? "어두움" : "밝음" };
})];

function PaletteSelect({ id, value, onChange }: { id: string; value: Palette; onChange: (palette: Palette) => void }) {
  const [query, setQuery] = useState("");
  return (
    <div className="theme-picker">
      <select id={id} className="select settings-select" value={value} onChange={(event) => onChange(event.target.value as Palette)}>
        <optgroup label="기본">{BASE_PALETTES.map((theme) => <option key={theme.id} value={theme.id}>{theme.label}</option>)}</optgroup>
        <optgroup label="어두움">{THEME_CATALOG.filter((theme) => theme.mode === "dark").map((theme) => <option key={theme.id} value={theme.id}>{theme.label}</option>)}</optgroup>
        <optgroup label="밝음">{THEME_CATALOG.filter((theme) => theme.mode === "light").map((theme) => <option key={theme.id} value={theme.id}>{theme.label}</option>)}</optgroup>
      </select>
      <details className="theme-gallery">
        <summary>테마 미리보기 · {GALLERY.length}종</summary>
        <input className="input" type="search" aria-label="테마 찾기" placeholder="이름 또는 밝음·어두움으로 찾기" value={query} onChange={(event) => setQuery(event.target.value)} />
        <div className="theme-gallery-grid" role="group" aria-label="테마 미리보기">
          {GALLERY.filter((theme) => searchable(`${theme.label} ${theme.mode}`).includes(searchable(query.trim()))).map((theme) => (
            <button key={theme.id} type="button" className="theme-card" aria-pressed={value === theme.id} onClick={() => onChange(theme.id)}>
              <span className="theme-card-preview" style={{ background: theme.bg, color: theme.text }} aria-hidden="true">
                <span style={{ color: theme.accent }}>● 작업 중</span><span>안녕하세요 Aa</span><code style={{ color: theme.accent }}>fn main() {'{ }'}</code>
              </span>
              <span className="theme-card-label">{theme.label}{value === theme.id ? " ✓" : ""}</span><span className="theme-card-mode">{theme.mode}</span>
            </button>
          ))}
        </div>
        {!GALLERY.some((theme) => searchable(`${theme.label} ${theme.mode}`).includes(searchable(query.trim()))) && <p role="status">맞는 테마가 없습니다.</p>}
        <p className="theme-gallery-note">밝기를 바꾸면 같은 계열의 짝을 씁니다. 터미널은 ‘앱 팔레트 따름’에서 함께 바뀝니다.</p>
      </details>
    </div>
  );
}

export function AppearanceSettings() {
  const { appearance: a, resolvedPalette } = useAppearance();
  const uiSize = uiFontSize(a);
  const uiFontFamily = useUiFontFamily();
  const monoFontFamily = useMonoFontFamily();
  return (
    <>
      <SettingsGroup>
        <SettingsRow label="테마" wide>
          <Segmented
            label="테마"
            value={a.theme}
            onChange={(theme) => updateAppearance({ theme })}
            options={[
              { value: "dark", label: "어둡게" },
              { value: "light", label: "밝게" },
              { value: "system", label: "시스템" },
            ]}
          />
        </SettingsRow>
        {a.theme === "system" ? (
          <>
            <SettingsRow label="라이트 색상" htmlFor="settings-palette-light" wide>
              <PaletteSelect id="settings-palette-light" value={a.paletteLight} onChange={(paletteLight) => updateAppearance({ paletteLight })} />
            </SettingsRow>
            <SettingsRow label="다크 색상" htmlFor="settings-palette-dark" wide>
              <PaletteSelect id="settings-palette-dark" value={a.paletteDark} onChange={(paletteDark) => updateAppearance({ paletteDark })} />
            </SettingsRow>
          </>
        ) : (
          <SettingsRow label="색상" htmlFor="settings-palette" wide>
            <PaletteSelect id="settings-palette" value={resolvedPalette} onChange={(palette) => {
                if (palette.startsWith("github-")) updateAppearance({ palette, theme: palette === "github-light" ? "light" : "dark" });
                else updateAppearance({ palette, ...(catalogTheme(palette) ? { theme: catalogTheme(palette)!.mode } : {}) });
              }} />
          </SettingsRow>
        )}
        <SettingsRow label="밀도">
          <Segmented
            label="밀도"
            value={a.density}
            onChange={(density) => updateAppearance({ density })}
            options={[
              { value: "comfortable", label: "여유" },
              { value: "compact", label: "촘촘" },
            ]}
          />
        </SettingsRow>
        <SettingsRow label="사이드바 행" wide>
          <Segmented
            label="사이드바 행"
            value={a.sidebarRows}
            onChange={(sidebarRows) => updateAppearance({ sidebarRows })}
            options={[
              { value: "one", label: "한 줄" },
              { value: "two", label: "두 줄" },
            ]}
          />
        </SettingsRow>
        {/* 참고 SettingsDialog 「Agents order」(문구 i18n.ko.ts)를 「작업」 트리의 폴더 안 차례로(plan §8.280). 값 이름(workspace)은
            공유 설정 호환으로 그대로 두고, 그 뜻은 목록 기본 차례(확인 필요 → 검토 → 병합 → 작업 → 쉼, herdr-projects rank)다 */}
        <SettingsRow label="폴더 안 정렬" wide>
          <Segmented
            label="폴더 안 정렬"
            value={a.agentOrder}
            onChange={(agentOrder) => updateAppearance({ agentOrder })}
            options={[
              { value: "workspace", label: "상태순" },
              { value: "activity", label: "활동순" },
            ]}
          />
        </SettingsRow>
        <SettingsRow label="상태 표시" wide>
          <Segmented
            label="상태 표시"
            value={a.statusIndicators}
            onChange={(statusIndicators) => updateAppearance({ statusIndicators })}
            options={[
              { value: "classic", label: "기본" },
              { value: "dots", label: "점" },
              { value: "symbols", label: "기호" },
            ]}
          />
        </SettingsRow>
      </SettingsGroup>
      <SettingsGroup title="타이포그래피" section="typography">
        <SettingsRow label="화면 글꼴" wide>
          <FontFamilyInput value={uiFontFamily} label="화면 글꼴" onCommit={setUiFontFamily} monospace={false} />
        </SettingsRow>
        <SettingsRow label="고정폭 글꼴" wide>
          <FontFamilyInput value={monoFontFamily} label="고정폭 글꼴" onCommit={setMonoFontFamily} />
        </SettingsRow>
        <SettingsRow label="인터페이스 글자 크기">
          <Stepper
            label="인터페이스 글자 크기"
            value={uiSize}
            text={`${uiSize}px`}
            min={UI_FONT_MIN}
            max={UI_FONT_MAX}
            decreaseLabel="인터페이스 글자 크기 줄이기"
            increaseLabel="인터페이스 글자 크기 키우기"
            onChange={(size) => updateAppearance({ uiFontSize: size })}
          />
        </SettingsRow>
      </SettingsGroup>
    </>
  );
}
