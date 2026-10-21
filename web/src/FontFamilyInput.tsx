// 세 글꼴 설정이 함께 쓰는 입력·설치 글꼴 선택·기본값 복원.
import { useEffect, useMemo, useRef, useState } from "react";
import { Search } from "lucide-react";
import { FONT_FAMILY_MAX_CHARS, sanitizeFontFamily } from "./herdr/fontFamily.ts";
import { canListInstalledFonts, filterFamilies, isMonospaceFamily, queryInstalledFonts } from "./herdr/installedFonts.ts";

const FONT_LIST_LIMIT = 150;
const FONT_FAMILY_PLACEHOLDER = 'D2Coding, "Cascadia Mono"';

/**
 * 글꼴 목록 칸(참고 SettingsDialog FontFamilyInput). 칸을 떠날 때·Enter·창을 닫을 때 저장한다. 누를 때마다 저장하면
 * 치는 중인 쉼표·빈칸이 걸러지고, 글꼴이 바뀔 때마다 터미널이 다시 맞춰 pane 크기가 바뀐다.
 * 「찾아보기」는 깔린 글꼴을 물을 수 있는 브라우저(Chromium)에만 있고, 화면 글꼴에는 비례폭도 고를 수 있다.
 */
export function FontFamilyInput({ value, label, onCommit, monospace = true }: { value: string; label: string; onCommit: (family: string) => void; monospace?: boolean }) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  const commit = (): void => {
    const next = sanitizeFontFamily(draft);
    setDraft(next);
    if (next !== value) onCommit(next);
  };
  // Esc로 창을 닫으면 blur 없이 칸이 사라진다
  const commitRef = useRef(commit);
  commitRef.current = commit;
  useEffect(() => () => commitRef.current(), []);
  const [families, setFamilies] = useState<string[] | null>(null);
  const [browsing, setBrowsing] = useState(false);
  const [query, setQuery] = useState("");
  const [refused, setRefused] = useState("");
  const [unavailable, setUnavailable] = useState(false);
  // 허락 창은 누름(gesture)에서만 뜬다. API가 없는 브라우저에는 단추가 없다
  const browse = async (): Promise<void> => {
    if (browsing) {
      setBrowsing(false);
      return;
    }
    setRefused("");
    setQuery("");
    if (families === null) {
      const result = await queryInstalledFonts();
      if (result.status !== "granted") {
        setUnavailable(true);
        return;
      }
      setUnavailable(false);
      setFamilies(result.families);
    }
    setBrowsing(true);
  };
  const matches = useMemo(() => filterFamilies(families ?? [], query), [families, query]);
  const pick = (family: string): void => {
    if (monospace && !isMonospaceFamily(family)) {
      setRefused(family);
      return;
    }
    const next = sanitizeFontFamily(family);
    setDraft(next);
    setBrowsing(false);
    if (next !== value) onCommit(next);
  };
  return (
    <div className="settings-font-field">
      <div className="settings-font-line">
        <input
          className="input settings-font-input"
          value={draft}
          placeholder={monospace ? FONT_FAMILY_PLACEHOLDER : 'Pretendard, "Noto Sans KR"'}
          maxLength={FONT_FAMILY_MAX_CHARS}
          aria-label={label}
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
          onChange={(event) => setDraft(event.target.value)}
          onBlur={commit}
          onKeyDown={(event) => {
            // IME의 Enter는 둔다(WebKit이 compositionend 뒤에 보내는 key code 229 포함)
            if (event.key !== "Enter" || event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229) return;
            event.preventDefault();
            commit();
          }}
        />
        {canListInstalledFonts() && (
          <button type="button" className="btn settings-font-browse" aria-expanded={browsing} aria-label={`${label}에 설치된 글꼴 찾아보기`} onClick={() => void browse()}>
            <Search aria-hidden="true" />
            찾아보기
          </button>
        )}
      </div>
      <button type="button" className="btn btn-ghost settings-font-reset" aria-label={`${label} 기본값으로`} disabled={draft === "" && value === ""} onClick={() => {
        setDraft("");
        onCommit("");
      }}>
        기본값으로
      </button>
      {unavailable && (
        <p className="settings-font-note" role="status">
          설치된 글꼴을 불러오지 못했습니다. 이름을 직접 입력하세요.
        </p>
      )}
      {browsing && families !== null && (
        <div className="settings-font-picker">
          <input
            className="input settings-font-search"
            type="search"
            value={query}
            placeholder="글꼴 검색"
            aria-label="글꼴 검색"
            spellCheck={false}
            autoFocus
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Escape" && !event.nativeEvent.isComposing) {
                // 창이 아니라 목록을 닫는다
                event.stopPropagation();
                setBrowsing(false);
              }
            }}
          />
          {refused !== "" && (
            <p className="settings-font-note" role="status">
              {refused}은(는) 고정폭 글꼴이 아닙니다. {label}에는 고정폭 글꼴이 필요하여 현재 글꼴을 유지했습니다.
            </p>
          )}
          <ul className="settings-font-list" aria-label="설치된 글꼴">
            {matches.slice(0, FONT_LIST_LIMIT).map((family) => (
              <li key={family}>
                <button type="button" className="settings-font-option" style={{ fontFamily: `"${family}", var(--font-ui)` }} onClick={() => pick(family)}>
                  {family}
                </button>
              </li>
            ))}
            {matches.length === 0 && <li className="settings-font-empty">글꼴을 찾을 수 없습니다</li>}
          </ul>
          {matches.length > FONT_LIST_LIMIT && <p className="settings-font-note">처음 {FONT_LIST_LIMIT}개의 글꼴을 표시합니다. 검색해서 목록을 좁히세요.</p>}
        </div>
      )}
    </div>
  );
}
