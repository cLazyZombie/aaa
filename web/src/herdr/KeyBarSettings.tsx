// herdr-web-ui(MIT, aa624cb) src/components/KeyBarSettings.tsx를 옮긴 것. 바꾼 곳: useSettings 대신 aaa settings.ts(useKeyBarItems·setKeyBarItems),
// i18n 대신 참고 i18n.ko.ts 문구(KO). 아이콘(lucide)·단추 class(icon-button·input·btn)는 참고 그대로다(plan §8.95). 고지는 web/src/herdr/README.md.
import { useEffect, useState } from "react";

import "./KeyBarSettings.css";

import { ChevronDown, ChevronUp, Plus, X } from "lucide-react";
import { setKeyBarItems, useKeyBarItems } from "../settings";
import { DEFAULT_KEY_BAR_ITEMS, KEY_BAR_ITEMS_MAX, KEY_BAR_KEYS, keyBarInputSequence, keyBarItemId, keyBarItemLabel, type KeyBarItem } from "./keyBar.ts";
import { NO_STICKY_MODIFIERS, type StickyModifiers } from "./keys.ts";

/** 참고 i18n.ko.ts의 문구 */
const KO: Record<string, string> = {
  "Choose the keys and their order under the terminal. The keyboard mode button stays first.": "터미널 아래에 표시할 키와 순서를 고릅니다. 키보드 모드 버튼은 맨 앞에 유지됩니다.",
  "Key bar keys": "키 바의 키",
  "Held modifier": "고정 보조 키",
  "Saved combination": "저장된 조합",
  "Move {name} up": "{name} 위로 이동",
  "Move {name} down": "{name} 아래로 이동",
  "Remove {name}": "{name} 제거",
  "No keys selected.": "선택한 키가 없습니다.",
  "Add key": "키 추가",
  "All keys added": "모든 키가 추가됨",
  "Held modifiers": "고정 보조 키",
  "Custom combination": "사용자 키 조합",
  "Saved combinations use only the selected modifiers. Other keys use held modifiers.": "저장된 조합은 선택한 보조 키만 사용합니다. 나머지 키에는 고정한 보조 키가 적용됩니다.",
  "Base key": "기본 키",
  "Single character": "한 글자",
  Character: "문자",
  "Combination modifiers": "조합에 사용할 보조 키",
  "Enter one character.": "한 글자를 입력하세요.",
  "Add combination": "조합 추가",
  "Already added": "이미 추가됨",
  "Restore defaults": "기본값으로 되돌리기",
};
function useT() {
  return (text: string, vars?: Record<string, string>) => (KO[text] ?? text).replace(/\{(\w+)\}/g, (_, k: string) => vars?.[k] ?? "");
}

const MODIFIERS: readonly (keyof StickyModifiers)[] = ["ctrl", "alt", "shift"];
const MODIFIER_CAPS: Record<keyof StickyModifiers, string> = { ctrl: "Ctrl", alt: "Alt", shift: "Shift" };
const CHARACTER_KEY = "character";

/** The phone's key row, edited in its display order without requiring a drag gesture. */
export function KeyBarSettings() {
  const t = useT();
  const items = useKeyBarItems();
  const update = ({ keyBarItems }: { keyBarItems: readonly KeyBarItem[] }) => setKeyBarItems(keyBarItems);
  const ids = new Set(items.map(keyBarItemId));
  const available = KEY_BAR_KEYS.filter((key) => !ids.has(keyBarItemId({ type: "key", key })));
  const [chosenKey, setChosenKey] = useState<string>(available[0] ?? "");
  // The combo editor names a base key; shortcuts such as ^C already include a modifier.
  const comboKeys = KEY_BAR_KEYS.filter((key) => !key.startsWith("ctrl-") && key !== "BackTab" && !["pipe", "tilde", "slash"].includes(key));
  const [comboKey, setComboKey] = useState<string>("ArrowLeft");
  const [character, setCharacter] = useState("");
  const [comboModifiers, setComboModifiers] = useState<StickyModifiers>(NO_STICKY_MODIFIERS);
  const full = items.length >= KEY_BAR_ITEMS_MAX;

  useEffect(() => {
    if (!available.includes(chosenKey)) setChosenKey(available[0] ?? "");
  }, [chosenKey, items]);

  const remove = (index: number): void => update({ keyBarItems: items.filter((_, at) => at !== index) });
  const move = (index: number, by: -1 | 1): void => {
    const target = index + by;
    if (target < 0 || target >= items.length) return;
    const next = [...items];
    [next[index], next[target]] = [next[target]!, next[index]!];
    update({ keyBarItems: next });
  };
  const add = (item: KeyBarItem): void => {
    if (full || ids.has(keyBarItemId(item))) return;
    update({ keyBarItems: [...items, item] });
  };

  const base = comboKey === CHARACTER_KEY ? character : comboKey;
  const characterValid = comboKey !== CHARACTER_KEY || ([...character].length === 1 && keyBarInputSequence(character, false) !== null);
  const combo: KeyBarItem = { type: "key", key: base, modifiers: comboModifiers };
  const comboExists = characterValid && ids.has(keyBarItemId(combo));

  return (
    <div className="key-bar-settings">
      <p className="settings-hint">{t("Choose the keys and their order under the terminal. The keyboard mode button stays first.")}</p>
      <ol className="key-bar-settings-list" aria-label={t("Key bar keys")}>
        {items.map((item, index) => {
          const name = keyBarItemLabel(item);
          return (
            <li className="key-bar-settings-row" key={keyBarItemId(item)}>
              <span className="key-bar-settings-cap" title={name}>{name}</span>
              <span className="key-bar-settings-kind">{item.type === "modifier" ? t("Held modifier") : item.modifiers ? t("Saved combination") : null}</span>
              <span className="key-bar-settings-actions">
                <button type="button" className="icon-button" disabled={index === 0} aria-label={t("Move {name} up", { name })} title={t("Move {name} up", { name })} onClick={() => move(index, -1)}><ChevronUp aria-hidden="true" /></button>
                <button type="button" className="icon-button" disabled={index === items.length - 1} aria-label={t("Move {name} down", { name })} title={t("Move {name} down", { name })} onClick={() => move(index, 1)}><ChevronDown aria-hidden="true" /></button>
                <button type="button" className="icon-button" aria-label={t("Remove {name}", { name })} title={t("Remove {name}", { name })} onClick={() => remove(index)}><X aria-hidden="true" /></button>
              </span>
            </li>
          );
        })}
      </ol>
      {items.length === 0 && <p className="settings-hint">{t("No keys selected.")}</p>}
      <div className="key-bar-settings-add">
        <label className="key-bar-settings-field">
          <span className="settings-label">{t("Add key")}</span>
          <select className="input" value={chosenKey} disabled={full || available.length === 0} onChange={(event) => setChosenKey(event.target.value)}>
            {available.length === 0 && <option value="">{t("All keys added")}</option>}
            {available.map((key) => <option value={key} key={key}>{keyBarItemLabel({ type: "key", key })}</option>)}
          </select>
        </label>
        <button type="button" className="btn" disabled={full || chosenKey === ""} onClick={() => add({ type: "key", key: chosenKey })}><Plus aria-hidden="true" />{t("Add key")}</button>
      </div>
      <div className="key-bar-settings-modifiers" role="group" aria-label={t("Held modifiers")}>
        <span className="settings-label key-bar-settings-modifier-label">{t("Held modifiers")}</span>
        {MODIFIERS.map((modifier) => {
          const item: KeyBarItem = { type: "modifier", modifier };
          const present = ids.has(keyBarItemId(item));
          return <button type="button" className="key-bar-settings-modifier" key={modifier} aria-pressed={present} disabled={full && !present} onClick={() => present ? update({ keyBarItems: items.filter((current) => keyBarItemId(current) !== keyBarItemId(item)) }) : add(item)}>{MODIFIER_CAPS[modifier]}</button>;
        })}
      </div>
      <div className="key-bar-settings-combo">
        <span className="settings-label">{t("Custom combination")}</span>
        <p className="settings-hint">{t("Saved combinations use only the selected modifiers. Other keys use held modifiers.")}</p>
        <div className="key-bar-settings-combo-key">
          <label className="key-bar-settings-field">
            <span className="settings-label">{t("Base key")}</span>
            <select className="input" value={comboKey} onChange={(event) => setComboKey(event.target.value)}>
              {comboKeys.map((key) => <option value={key} key={key}>{keyBarItemLabel({ type: "key", key })}</option>)}
              <option value={CHARACTER_KEY}>{t("Single character")}</option>
            </select>
          </label>
          {comboKey === CHARACTER_KEY && <label className="key-bar-settings-field">
            <span className="settings-label">{t("Character")}</span>
            <input className="input" value={character} maxLength={2} autoCapitalize="off" autoCorrect="off" spellCheck={false} onChange={(event) => setCharacter(event.target.value)} />
          </label>}
        </div>
        <div className="key-bar-settings-modifiers" role="group" aria-label={t("Combination modifiers")}>
          {MODIFIERS.map((modifier) => <button type="button" className="key-bar-settings-modifier" key={modifier} aria-pressed={comboModifiers[modifier]} onClick={() => setComboModifiers((current) => ({ ...current, [modifier]: !current[modifier] }))}>{MODIFIER_CAPS[modifier]}</button>)}
        </div>
        <div className="key-bar-settings-combo-actions">
          <span className="key-bar-settings-preview" aria-live="polite">{characterValid ? keyBarItemLabel(combo) : t("Enter one character.")}</span>
          <button type="button" className="btn" disabled={full || !characterValid || base === "" || comboExists} onClick={() => add(combo)}><Plus aria-hidden="true" />{t("Add combination")}</button>
        </div>
        {comboExists && <p className="settings-hint">{t("Already added")}</p>}
      </div>
      <button type="button" className="btn btn-ghost key-bar-settings-restore" onClick={() => update({ keyBarItems: [...DEFAULT_KEY_BAR_ITEMS] })}>{t("Restore defaults")}</button>
    </div>
  );
}
