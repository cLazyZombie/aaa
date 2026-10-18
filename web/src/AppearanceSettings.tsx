// 설정 → 모양(herdr-web-ui SettingsDialog.tsx `AppearancePage`·`PaletteSelect` 235~290줄). 문구는 참고 i18n.ko.ts 그대로다.
// 옮긴 줄: 테마·색상(시스템이면 라이트·다크 색상 따로)·밀도·사이드바 행·상태 표시, 「타이포그래피」의 인터페이스 글자 크기(차이 목록 T4).
// Agents 순서·로고 색은 그 기능을 옮길 때 넣는다. 언어는 두지 않는다(한국어 고정, 사용자 결정, plan §8.169). 밀도·글자 크기는 참고처럼 이 기기 설정이다(SETTING_SCOPE "device").
import { Segmented, SettingsGroup, SettingsRow, Stepper } from "./herdr/SettingsControls.tsx";
import { UI_FONT_MAX, UI_FONT_MIN } from "./herdr/typography.ts";
import { uiFontSize, updateAppearance, useAppearance, type Palette } from "./theme";

function PaletteSelect({ id, value, onChange }: { id: string; value: Palette; onChange: (palette: Palette) => void }) {
  return (
    <select id={id} className="select settings-select" value={value} onChange={(event) => onChange(event.target.value as Palette)}>
      <option value="amber">앰버</option>
      <option value="charcoal">차콜</option>
      <option value="catppuccin">Catppuccin</option>
    </select>
  );
}

export function AppearanceSettings() {
  const { appearance: a } = useAppearance();
  const uiSize = uiFontSize(a);
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
        {/* 참고 SettingsDialog 「Agents order」(문구 i18n.ko.ts): aaa는 머신 목록 차례가 herdr workspace 차례 자리다 */}
        <SettingsRow label="에이전트 정렬" description="활동순은 응답을 기다리는 에이전트를 맨 위에 두고, 그다음 가장 최근에 바뀐 순서로 놓습니다. 머신 목록의 순서는 바뀌지 않습니다" wide>
          <Segmented
            label="에이전트 정렬"
            value={a.agentOrder}
            onChange={(agentOrder) => updateAppearance({ agentOrder })}
            options={[
              { value: "workspace", label: "워크스페이스순" },
              { value: "activity", label: "활동순" },
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
      <SettingsGroup title="타이포그래피" section="typography">
        <SettingsRow label="인터페이스 글자 크기" description="메뉴, 목록, 대화 상자, 사이드바에 적용됩니다. 채팅은 별도 크기를 정하기 전까지 이 크기를 따르고, 터미널은 자체 크기를 유지합니다.">
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
