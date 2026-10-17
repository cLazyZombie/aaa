// 새 pane 창의 agent 고르기(herdr-web-ui aa624cb src/components/AgentPicker.tsx를 옮김, 모양 herdr/NewSessionDialog.css).
// 다른 곳: herdr가 아는 agent 종류 대신 이 머신 agent.toml의 profile이 줄이 되고, 표시는 그 profile의 harness 표시다.
// 마지막 고른 것은 참고와 같은 열쇠에 둔다.
import { forwardRef, useEffect, useId, useRef, useState, type CSSProperties, type KeyboardEvent } from "react";
import { ChevronDown, Terminal } from "lucide-react";

import { AgentMark } from "./herdr/AgentMark.tsx";

/** 고를 수 있는 줄: profile 이름, 보이는 글, harness(표시). 셸은 kind "" */
export interface AgentKind {
  kind: string;
  label: string;
  harness?: string;
}

/** 셸 줄도 옆 agent처럼 프로그램 이름이라 영어로 둔다(참고와 같다) */
const SHELL_LABEL = "Shell";

const LAST_AGENT_KEY = "herdr-web-ui:new-session-agent";

/** The agent the last creation dialog started; "" is a plain shell. */
export function rememberedAgent(): string {
  try {
    return window.localStorage.getItem(LAST_AGENT_KEY) ?? "";
  } catch {
    return "";
  }
}

export function rememberAgent(kind: string): void {
  try {
    window.localStorage.setItem(LAST_AGENT_KEY, kind);
  } catch {
    /* private mode: the choice simply is not remembered */
  }
}

export interface AgentPickerProps {
  agents: AgentKind[];
  /** the chosen kind; "" is a plain shell */
  value: string;
  disabled?: boolean;
  labelledBy: string;
  onChange: (kind: string) => void;
}

function OptionMark({ option }: { option: AgentKind }) {
  return option.kind && option.harness && option.harness !== "shell" && option.harness !== "custom"
    ? <AgentMark agent={option.harness} size={16} />
    : <span className="agent-picker-shell"><Terminal aria-hidden="true" /></span>;
}

/**
 * A select whose rows carry each agent's mark: a native <option> cannot hold an icon.
 * The marks are the inline SVGs the sidebar already renders, so the list costs nothing extra.
 */
export const AgentPicker = forwardRef<HTMLButtonElement, AgentPickerProps>(function AgentPicker({ agents, value, disabled, labelledBy, onChange }, ref) {
  const listId = useId();
  const rootRef = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const [place, setPlace] = useState<CSSProperties>({});
  const options: AgentKind[] = [{ kind: "", label: SHELL_LABEL }, ...agents];
  const selectedIndex = Math.max(0, options.findIndex((option) => option.kind === value));
  const selected = options[selectedIndex]!;

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent): void => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    const onMove = (event: Event): void => {
      // Only an ancestor's scroll can move the anchor; text inputs scroll too.
      if (event.type === "resize" || (event.target instanceof Node && rootRef.current !== null && event.target.contains(rootRef.current))) setOpen(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("scroll", onMove, true);
    window.addEventListener("resize", onMove);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("scroll", onMove, true);
      window.removeEventListener("resize", onMove);
    };
  }, [open]);

  useEffect(() => {
    if (disabled) setOpen(false);
  }, [disabled]);

  useEffect(() => {
    if (!open) return;
    const option = document.getElementById(`${listId}-${active}`);
    const menu = option?.parentElement;
    if (!option || !menu) return;
    // Scroll only the list; moving its modal would close the anchored picker.
    const optionBounds = option.getBoundingClientRect();
    const menuBounds = menu.getBoundingClientRect();
    if (optionBounds.top < menuBounds.top) menu.scrollTop -= menuBounds.top - optionBounds.top;
    else if (optionBounds.bottom > menuBounds.bottom) menu.scrollTop += optionBounds.bottom - menuBounds.bottom;
  }, [open, active, listId]);

  const show = (): void => {
    // fixed to the trigger, so the modal body's scroll box does not clip the list
    const rect = rootRef.current!.getBoundingClientRect();
    const below = window.innerHeight - rect.bottom - 16;
    const up = below < 160 && rect.top > below;
    const room = up ? rect.top - 16 : below;
    setPlace({
      left: rect.left,
      width: rect.width,
      maxHeight: Math.min(320, room),
      ...(up ? { bottom: window.innerHeight - rect.top + 4 } : { top: rect.bottom + 4 }),
    });
    setActive(selectedIndex);
    setOpen(true);
  };

  const choose = (index: number): void => {
    onChange(options[index]!.kind);
    setOpen(false);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>): void => {
    const last = options.length - 1;
    if (!open) {
      if (event.key === "ArrowDown" || event.key === "ArrowUp" || event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        show();
      }
      return;
    }
    if (event.key === "Escape") {
      // cancel: close the list only, not the dialog around it
      event.preventDefault();
      event.stopPropagation();
      setOpen(false);
    } else if (event.key === "Tab") {
      // a select-only combobox takes the row it is on as focus moves on, as a native select does
      choose(active);
    } else if (event.key === "ArrowDown") {
      event.preventDefault();
      setActive((index) => Math.min(last, index + 1));
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      setActive((index) => Math.max(0, index - 1));
    } else if (event.key === "Home" || event.key === "End") {
      event.preventDefault();
      setActive(event.key === "Home" ? 0 : last);
    } else if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      choose(active);
    }
  };

  return (
    <div className="agent-picker" ref={rootRef}>
      <button
        ref={ref}
        type="button"
        className="select agent-picker-trigger"
        role="combobox"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={listId}
        aria-labelledby={labelledBy}
        aria-activedescendant={open ? `${listId}-${active}` : undefined}
        disabled={disabled}
        onClick={() => (open ? setOpen(false) : show())}
        onKeyDown={onKeyDown}
      >
        <OptionMark option={selected} />
        <span className="agent-picker-label">{selected.label}</span>
        <ChevronDown className="agent-picker-chevron" aria-hidden="true" />
      </button>
      {open && (
        <div id={listId} className="menu agent-picker-menu" role="listbox" aria-labelledby={labelledBy} style={place}>
          {options.map((option, index) => (
            <div
              id={`${listId}-${index}`}
              key={option.kind || "shell"}
              className="menu-item"
              role="option"
              aria-selected={index === selectedIndex}
              data-active={index === active || undefined}
              onPointerEnter={() => setActive(index)}
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => choose(index)}
            >
              <OptionMark option={option} />
              <span className="menu-item-main">{option.label}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
});
