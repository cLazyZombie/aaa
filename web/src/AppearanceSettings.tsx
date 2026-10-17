// 설정 → 모양(herdr-web-ui SettingsDialog.tsx `AppearancePage`·`PaletteSelect` 235~290줄). 문구는 참고 i18n.ko.ts 그대로다.
// 옮긴 줄: 테마·색상(시스템이면 라이트·다크 색상 따로)·사이드바 행·상태 표시. 밀도·언어·Agents 순서·로고 색은 그 기능을 옮길 때 넣는다.
import { Segmented, SettingsGroup, SettingsRow } from "./SettingsDialog";
import { updateAppearance, useAppearance, type Palette } from "./theme";

function PaletteSelect({ id, value, onChange }: { id: string; value: Palette; onChange: (palette: Palette) => void }) {
  return (
    <select id={id} className="settings-select" value={value} onChange={(event) => onChange(event.target.value as Palette)}>
      <option value="amber">앰버</option>
      <option value="charcoal">차콜</option>
      <option value="catppuccin">Catppuccin</option>
    </select>
  );
}

export function AppearanceSettings() {
  const { appearance: a } = useAppearance();
  return (
    <SettingsGroup title="모양">
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
          <SettingsRow label="라이트 색상" description="시스템 모드에서는 두 팔레트를 오갑니다" htmlFor="settings-palette-light">
            <PaletteSelect id="settings-palette-light" value={a.paletteLight} onChange={(paletteLight) => updateAppearance({ paletteLight })} />
          </SettingsRow>
          <SettingsRow label="다크 색상" htmlFor="settings-palette-dark">
            <PaletteSelect id="settings-palette-dark" value={a.paletteDark} onChange={(paletteDark) => updateAppearance({ paletteDark })} />
          </SettingsRow>
        </>
      ) : (
        <SettingsRow label="색상" htmlFor="settings-palette">
          <PaletteSelect id="settings-palette" value={a.palette} onChange={(palette) => updateAppearance({ palette })} />
        </SettingsRow>
      )}
      <SettingsRow label="사이드바 행" description="이름만 한 줄로 보여주거나, 하는 일을 위에 두고 자리를 아래에 보여줍니다" wide>
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
      <SettingsRow label="상태 표시" description="기본은 확인이 필요한 상태만 표시합니다. 점과 기호는 herdr TUI처럼 모든 상태를 표시합니다" wide>
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
  );
}
