// 입력칸 「/」 목록(plan §8.323). herdr-web-ui Composer.tsx 1169~1205줄의 입력칸에 붙은 listbox: 빈 「/」만 출처별 머리, 치면 순위 한
// 목록, 줄은 「/이름 안내 · 설명」. 다른 곳: 줄 끝 표지 `터미널`(인자 없이 보내면 선택창이 뜨거나 결과가 화면에만 남는다)·`확인`(맥락을
// 비운다), skill·내 명령·프로젝트 출처 표지. focus는 글 칸에 남고(aria-activedescendant), 줄은 누르거나 ↑↓·Enter·Tab으로 고른다.
import { useEffect, useRef } from "react";
import type { SlashCommand } from "./gen/proto";
import { groups, SOURCE_LABEL } from "./slashCommands";

export interface SlashMenuProps {
  id: string;
  items: SlashCommand[];
  /** 빈 「/」: 출처별 머리를 단다 */
  grouped: boolean;
  active: number;
  onHover: (index: number) => void;
  onPick: (command: SlashCommand) => void;
}

export const slashOptionId = (menu: string, index: number) => `${menu}-${index}`;

export function SlashMenu({ id, items, grouped, active, onHover, onPick }: SlashMenuProps) {
  const list = useRef<HTMLDivElement>(null);
  useEffect(() => {
    list.current?.querySelector(`#${CSS.escape(slashOptionId(id, active))}`)?.scrollIntoView({ block: "nearest" });
  }, [id, active]);
  if (items.length === 0)
    return (
      <div className="slash-menu is-empty" role="status" data-testid="slash-menu">
        맞는 명령이 없다.
      </div>
    );
  const row = (c: SlashCommand) => {
    const index = items.indexOf(c);
    const terminal = c.kind === "picker" || c.kind === "screen";
    return (
      <div
        key={`${c.source}:${c.name}`}
        id={slashOptionId(id, index)}
        role="option"
        aria-selected={index === active}
        className={`slash-item${index === active ? " is-active" : ""}`}
        data-testid="slash-item"
        data-name={c.name}
        // focus는 글 칸에 둔다(휴대폰 자판이 닫히지 않게)
        onPointerDown={(event) => event.preventDefault()}
        onPointerMove={() => onHover(index)}
        onClick={() => onPick(c)}
      >
        <span className="slash-item-name">{c.insert.trim()}</span>
        {c.hint && <span className="slash-item-hint">{c.hint}</span>}
        <span className="slash-item-desc">{c.description}</span>
        <span className="slash-item-tags">
          {!grouped && c.source !== "builtin" && <span className="slash-tag">{SOURCE_LABEL[c.source]}</span>}
          {terminal && <span className="slash-tag is-terminal" title={c.kind === "picker" ? "인자 없이 보내면 터미널에 선택창이 뜬다" : "결과는 터미널 화면에만 보인다"}>터미널</span>}
          {c.kind === "confirm" && <span className="slash-tag is-confirm" title="보내기 전에 확인한다">확인</span>}
        </span>
      </div>
    );
  };
  return (
    <div ref={list} id={id} className="slash-menu" role="listbox" aria-label="명령" data-testid="slash-menu">
      {grouped
        ? groups(items).map((g) => (
          <div key={g.source} role="group" aria-label={SOURCE_LABEL[g.source]}>
            <div className="slash-group" aria-hidden="true">{SOURCE_LABEL[g.source]}</div>
            {g.items.map(row)}
          </div>
        ))
        : items.map(row)}
    </div>
  );
}
