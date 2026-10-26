/**
 * Copyright (c) 2016 The xterm.js authors. All rights reserved.
 * @license MIT
 */

import { IRenderService } from 'browser/services/Services';
import { IBufferService, ICoreService, IOptionsService, IUnicodeService } from 'common/services/Services';
import { C0 } from 'common/data/EscapeSequences';
import { IBuffer } from 'common/buffer/Types';
import { IMarker } from 'common/Types';
import { isIpad, isIphone, isMac, isSafari } from 'common/Platform';

/** Hangul jamo and syllables: a key or an edit that a Korean input method composes. */
const HANGUL_KEY = /^[ᄀ-ᇿ㄰-㆏가-힯]$/;
const HANGUL_TEXT = /^[ᄀ-ᇿ㄰-㆏가-힯]+$/;
const GRAPHEMES = new Intl.Segmenter(undefined, { granularity: 'grapheme' });

// aaa: xterm 5.5.0 + herdr aa624cb의 입력 계약을 한곳에서 관리한다(MIT 고지: patches/README.md).
// DOM range/commit queue/anchor own the state; the selected renderer is its only DOM writer.
export interface CompositionState {
  active: boolean;
  pending: boolean;
  sending: boolean;
  text: string;
  row: number;
  col: number;
  visible: boolean;
}

interface IPosition {
  start: number;
  end: number;
}

/**
 * Encapsulates the logic for handling compositionstart, compositionupdate and compositionend
 * events, displaying the in-progress composition to the UI and forwarding the final composition
 * to the handler.
 */
export class CompositionHelper {
  /**
   * Whether input composition is currently happening, eg. via a mobile keyboard, speech input or
   * IME. This variable determines whether the compositionText should be displayed on the UI.
   */
  private _isComposing: boolean;
  // A Korean word can commit a syllable and begin the next before the PTY echo.
  // Keep ONE chain origin and derive its advance from the actual DOM commit range.
  // Output/status cursors never choose the next syllable's origin. A delimiter,
  // navigation, blur, paste, reset or a removed buffer releases this marker.
  private _compositionAnchor: { buffer: IBuffer; line: IMarker; x: number; start: number } | undefined;
  private _text = '';
  private _sending = false;
  private readonly _listeners = new Set<() => void>();
  private _renderer: (state: CompositionState) => void = state => this._renderDefault(state);

  public get state(): CompositionState {
    const anchor = this._compositionAnchor;
    let row = anchor ? anchor.line.line - this._bufferService.buffer.ydisp : 0;
    let col = Math.min(anchor?.x ?? 0, this._bufferService.cols - 1);
    if (anchor) {
      const committed = this._textarea.value.substring(anchor.start, this._compositionPosition.start);
      for (const { segment } of GRAPHEMES.segment(committed)) {
        const width = this._unicodeService.getStringCellWidth(segment);
        if (col + width > this._bufferService.cols) { row++; col = 0; }
        col += width;
      }
      if (col >= this._bufferService.cols) { row++; col = 0; }
    }
    return {
      active: this._isComposing, pending: this._pendingSends.length > 0, sending: this._sending,
      text: this._text, row, col: Math.min(col, this._bufferService.cols - 1),
      visible: !!anchor && anchor.buffer === this._bufferService.buffer && !anchor.line.isDisposed
        && row >= 0 && row < this._bufferService.rows
    };
  }

  public observeState(listener: () => void): { dispose(): void } {
    this._listeners.add(listener);
    listener();
    return { dispose: () => this._listeners.delete(listener) };
  }

  public setRenderer(renderer: (state: CompositionState) => void): { dispose(): void } {
    this._renderer = renderer;
    this.updateCompositionElements(true);
    return { dispose: () => { this._renderer = state => this._renderDefault(state); } };
  }

  private _notify(): void { for (const listener of this._listeners) listener(); }

  private _send(data: string): void {
    if (!data) return;
    this._sending = true;
    this._notify();
    try { this._coreService.triggerDataEvent(data, true); }
    finally { this._sending = false; this._notify(); }
  }

  private _clearCompositionAnchor(): void {
    this._compositionAnchor?.line.dispose();
    this._compositionAnchor = undefined;
  }
  public get isComposing(): boolean { return this._isComposing; }

  /**
   * The position within the input textarea's value of the current composition.
   */
  private _compositionPosition: IPosition;

  /** Finished compositions may overlap before their deferred sends run (#6089). */
  private readonly _pendingSends: (() => void)[] = [];
  private _sentUpTo = 0;
  private _generation = 0;
  /** macOS Safari can replace Hangul in the editor without any composition events. */
  private _replacementComposition = false;
  /** A replacement edit arrived with no keydown yet; Safari 26 sends that keydown after it. */
  private _editAwaitingKey = false;
  /** Text whose insertion ended a replacement after its keydown was held back (keydown 229). */
  private _commitInsert: string | undefined;
  // iOS prediction can keep even ASCII in a composition until the next key.
  // Remember only the editor suffix we sent, so a replacement can undo that suffix.
  private _liveValue: string | undefined;
  private _liveEdit = false;

  /**
   * Data already sent due to keydown event.
   */
  private _dataAlreadySent: string;

  constructor(
    private readonly _textarea: HTMLTextAreaElement,
    private readonly _compositionView: HTMLElement,
    @IBufferService private readonly _bufferService: IBufferService,
    @IOptionsService private readonly _optionsService: IOptionsService,
    @ICoreService private readonly _coreService: ICoreService,
    @IRenderService private readonly _renderService: IRenderService,
    @IUnicodeService private readonly _unicodeService: IUnicodeService
  ) {
    this._isComposing = false;
    this._compositionPosition = { start: 0, end: 0 };
    this._dataAlreadySent = '';
  }

  /** A reset/disposal must not forward an old pane's deferred input into its replacement. */
  public reset(): void {
    this._clearCompositionAnchor();
    this._generation++;
    this._liveValue = undefined;
    this._liveEdit = false;
    this._replacementComposition = false;
    this._editAwaitingKey = false;
    this._commitInsert = undefined;
    this._pendingSends.length = 0;
    this._isComposing = false;
    this._sentUpTo = 0;
    this._dataAlreadySent = '';
    this._compositionPosition = { start: 0, end: 0 };
    this._textarea.value = '';
    this._text = '';
    this.updateCompositionElements(true);
  }

  /** Safari #432: beforeinput supplies the pre-edit range; keydown 229 arrives AFTER input. */
  public beforeinput(ev: InputEvent, keyDownSeen = false): void {
    // A following plain insertion/paste can arrive before a completed composition's timer.
    // Drain while the DOM still contains only the committed text, before the new edit changes it.
    if (!ev.isComposing && !this._isComposing) this._drainPending();
    this._liveEdit = this._liveValue === this._textarea.value
      && (ev.inputType === 'insertReplacementText' || ev.inputType === 'deleteContentBackward')
      && /^[\x20-\x7e]*$/.test(ev.data ?? '');
    if (this._liveEdit) return;
    if (!this._replacementComposition) this._editAwaitingKey = false;
    if (!(isMac || isIpad || isIphone) || !isSafari || ev.isComposing || this._optionsService.rawOptions.screenReaderMode
      || (this._isComposing && !this._replacementComposition)) return;
    if (this._replacementComposition && (ev.inputType === 'insertReplacementText'
      || ev.inputType === 'deleteContentBackward' || ev.inputType === 'deleteContentForward')) return;
    // keydown() held back this key while the syllable was pending, so Terminal will treat
    // its insertion as already handled by keydown. Mark it to be forwarded once.
    const commitInsert = this._replacementComposition && keyDownSeen && ev.inputType === 'insertText' ? ev.data ?? undefined : undefined;
    const hangul = ev.inputType === 'insertText' && !!ev.data && HANGUL_TEXT.test(ev.data);
    this.finishInput(!hangul);
    this._commitInsert = commitInsert;
    // Only a Hangul preedit starts this path. Paste, emoji, dictation blocks and browsers
    // emitting real composition events retain their existing input handling. iPadOS puts two
    // syllables back at once when a final consonant moves on (\ub9dd + \u314f: \ub9c8\uc544).
    if (ev.inputType === 'insertText' && ev.data && HANGUL_TEXT.test(ev.data)) {
      this._commitInsert = undefined;
      this.compositionstart();
      this._replacementComposition = true;
    }
  }

  /**
   * iPadOS Safari with a hardware keyboard reports each Hangul jamo as a printable keydown (its own
   * keyCode, not 229) and then composes in the editor with deleteContentBackward and insertText,
   * never with composition events. Sending the key on keydown cancels that edit, so every jamo
   * reached the PTY on its own (한 as ㅎㅏㄴ). Such a key is left to the editor: beforeinput takes
   * its insertion as a replacement preedit, which the next other key, paste or blur commits.
   */
  public passHangulKey(ev: KeyboardEvent): boolean {
    if (!isSafari || !(isMac || isIpad || isIphone) || ev.keyCode === 229 || ev.isComposing || ev.ctrlKey || ev.altKey || ev.metaKey
      || this._optionsService.rawOptions.screenReaderMode || (this._isComposing && !this._replacementComposition)
      || !HANGUL_KEY.test(ev.key)) return false;
    this._editAwaitingKey = false;
    this._commitInsert = undefined;
    return true;
  }

  /** Whether this insertion ended a replacement and still has to be forwarded (see beforeinput). */
  public takeCommitInsert(ev: InputEvent): boolean {
    if (this._commitInsert === undefined || ev.inputType !== 'insertText' || ev.data !== this._commitInsert) return false;
    this._commitInsert = undefined;
    return true;
  }

  /** Keep Safari's provisional DOM edits out of Terminal._inputEvent and its 229 fallback. */
  public input(ev: InputEvent, keyDownSeen = false): boolean {
    if (this._inputLiveComposition(ev, keyDownSeen)) return true;
    if (!this._replacementComposition) {
      if (!this._isComposing && !ev.isComposing && !this._pendingSends.length) this._clearCompositionAnchor();
      return false;
    }
    if (!keyDownSeen) this._editAwaitingKey = true;
    const start = this._compositionPosition.start;
    this._compositionPosition.end = Math.max(start, this._textarea.selectionEnd ?? this._textarea.value.length);
    this._text = this._textarea.value.substring(start, this._compositionPosition.end);
    this.updateCompositionElements();
    if (this._compositionPosition.end === start) {
      // Backspace removed the entire preedit. Nothing has reached the PTY to delete.
      this._replacementComposition = false;
      this._isComposing = false;
      this._clearCompositionAnchor();
      this.updateCompositionElements(true);
    }
    return true;
  }

  /** Stream only printable ASCII on iOS; CJK/dead-key preedits still wait for commit. */
  private _inputLiveComposition(ev: InputEvent, keyDownSeen: boolean): boolean {
    if (!isSafari || !(isIphone || isIpad) || this._optionsService.rawOptions.screenReaderMode
      || this._replacementComposition) return false;
    const composing = this._isComposing;
    const start = this._compositionPosition.start;
    const value = this._textarea.value;
    const preedit = value.substring(start);
    const ascii = /^[\x20-\x7e]*$/.test(preedit);
    const liveEdit = this._liveEdit;
    this._liveEdit = false;
    if (!liveEdit && (!composing || (!ascii && this._liveValue === undefined))) return false;
    // A layout/dead-key change can turn a streamed ASCII preedit into CJK. Undo only
    // that streamed tail and let the existing composition queue commit the new text.
    const next = composing && !ascii ? value.substring(0, start) : value;
    for (const send of this._pendingSends.splice(0)) send();
    const previous = this._liveValue ?? value.substring(0, start);
    let same = 0;
    while (same < previous.length && same < next.length && previous[same] === next[same]) same++;
    const removed = previous.substring(same);
    // Never guess the width/deletion behavior of an unknown or non-ASCII editor prefix.
    if (!/^[\x20-\x7e]*$/.test(removed)) { this._liveValue = undefined; return false; }
    const data = C0.DEL.repeat(removed.length) + next.substring(same);
    this._liveValue = composing && !ascii ? undefined : next;
    // This input already handled the DOM edit; cancel any queued keydown-229 fallback.
    this._generation++;
    if (liveEdit && ev.inputType === 'deleteContentBackward' && !keyDownSeen) this._editAwaitingKey = true;
    this._sentUpTo = next.length;
    this._compositionPosition.end = value.length;
    this._dataAlreadySent = '';
    if (ascii) {
      this._text = '';
      this.updateCompositionElements(true);
    }
    this._send(data);
    return true;
  }

  /** Commit the corrected DOM range before a following key, paste, syllable or blur. */
  public finishInput(endInput = true): void {
    this._liveValue = undefined;
    this._liveEdit = false;
    if (this._replacementComposition || (endInput && this._isComposing)) this._finalizeComposition(false);
    if (endInput) { this._drainPending(); this._clearCompositionAnchor(); }
  }

  private _drainPending(): void {
    for (const send of this._pendingSends.splice(0)) send();
    this._notify();
  }

  /**
   * Handles the compositionstart event, activating the composition view.
   */
  public compositionstart(): void {
    this.finishInput(false);
    const buffer = this._bufferService.buffer;
    const anchor = this._compositionAnchor;
    // WebKit rewrites a preceding NBSP to SPACE when marked text is inserted.
    // The existing DOM range still has the same offset; its spelling is not an input boundary.
    if (!anchor || anchor.buffer !== buffer || anchor.line.isDisposed || this._textarea.value.length < anchor.start) {
      this._clearCompositionAnchor();
      this._compositionAnchor = { buffer, line: buffer.addMarker(buffer.ybase + buffer.y), x: buffer.x, start: this._textarea.value.length };
    }
    this._isComposing = true;
    this._compositionPosition = { start: this._textarea.value.length, end: this._textarea.value.length };
    this._sentUpTo = Math.min(this._sentUpTo, this._compositionPosition.start);
    this._text = '';
    this._dataAlreadySent = '';
    this.updateCompositionElements(true);
  }

  /**
   * Handles the compositionupdate event, updating the composition view.
   * @param ev The event.
   */
  public compositionupdate(ev: Pick<CompositionEvent, 'data'>): void {
    if (!this._isComposing) return;
    this._text = ev.data;
    this.updateCompositionElements();
    const generation = this._generation;
    setTimeout(() => {
      if (generation !== this._generation) return;
      this._compositionPosition.end = this._textarea.value.length;
    }, 0);
  }

  /**
   * Handles the compositionend event, hiding the composition view and sending the composition to
   * the handler.
   */
  public compositionend(): void {
    if (!this._isComposing) return;
    this._finalizeComposition(true);
  }

  /**
   * Handles the keydown event, routing any necessary events to the CompositionHelper functions.
   * @param ev The keydown event.
   * @returns Whether the Terminal should continue processing the keydown event.
   */
  public keydown(ev: KeyboardEvent): boolean {
    if (ev.keyCode !== 229 && !ev.isComposing && ![16, 17, 18, 20, 91, 93].includes(ev.keyCode)) {
      this._liveValue = undefined;
      this._liveEdit = false;
    }
    const editBefore = this._editAwaitingKey;
    this._editAwaitingKey = false;
    this._commitInsert = undefined;
    const deleteKey = (ev.keyCode === 8 || ev.keyCode === 46) && !ev.ctrlKey && !ev.altKey && !ev.metaKey;
    // A native marked-text editor may report Backspace as 8 rather than 229.
    // Let it edit/cancel the preedit; there is no committed PTY character to delete.
    if (this._isComposing && ev.isComposing && deleteKey) return false;
    if (editBefore && deleteKey) {
      // Safari already applied this Backspace/Delete to the pending syllable (and may have
      // emptied it). Sending DEL would delete text that reached the PTY earlier.
      ev.preventDefault();
      return false;
    }
    if (this._replacementComposition) {
      // Switching layouts and holding modifiers do not commit a provisional syllable.
      if ([229, 16, 17, 18, 20, 91, 93, 129].includes(ev.keyCode)) return false;
      // Backspace/Delete edit the pending syllable; input() tracks the DOM edit that follows.
      if (deleteKey) return false;
      this.finishInput();
    }
    if (this._isComposing || this._pendingSends.length > 0) {
      if (ev.keyCode === 229) {
        // Continue composing if the keyCode is the "composition character"
        return false;
      }
      if (ev.keyCode === 16 || ev.keyCode === 17 || ev.keyCode === 18) {
        // Continue composing if the keyCode is a modifier key
        return false;
      }
      // Finish composition immediately. This is mainly here for the case where enter is
      // pressed and the handler needs to be triggered before the command is executed.
      this._finalizeComposition(false);
    }

    if (ev.keyCode === 229) {
      // If the "composition character" is used but gets to this point it means a non-composition
      // character (eg. numbers and punctuation) was pressed when the IME was active.
      this._handleAnyTextareaChanges();
      return false;
    }

    this._clearCompositionAnchor();
    return true;
  }

  /**
   * Finalizes the composition, resuming regular input actions. This is called when a composition
   * is ending.
   * @param waitForPropagation Whether to wait for events to propagate before sending
   *   the input. This should be false if a non-composition keystroke is entered before the
   *   compositionend event is triggered, such as enter, so that the composition is sent before
   *   the command is executed.
   */
  private _finalizeComposition(waitForPropagation: boolean): void {
    this._replacementComposition = false;
    this.updateCompositionElements(true);
    this._isComposing = false;

    if (!waitForPropagation) {
      // Drain older commits before the key that interrupts the current composition.
      this._drainPending();
      const end = Math.max(this._compositionPosition.end, this._textarea.selectionEnd ?? 0);
      const input = this._sliceUnsent(this._compositionPosition.start, end);
      this._send(input);
    } else {
      const start = this._compositionPosition.start;
      // Read the textarea after DOM edits propagate. compositionend.data can still contain
      // the previous final consonant when Korean moves it to the following syllable.
      const send = (): void => {
        const from = start + this._dataAlreadySent.length;
        const end = this._compositionPosition.start > from
          ? this._compositionPosition.start : this._textarea.value.length;
        const input = this._sliceUnsent(from, end);
        this._send(input);
        if (!input && !this._isComposing && !this._pendingSends.length) this._clearCompositionAnchor();
      };
      this._pendingSends.push(send);
      this._notify();
      setTimeout(() => {
        const index = this._pendingSends.indexOf(send);
        if (index !== -1) {
          this._pendingSends.splice(index, 1);
          send();
          this._notify();
        }
      }, 0);
    }
    this.updateCompositionElements(true);
  }

  private _sliceUnsent(start: number, end: number): string {
    const from = Math.max(start, this._sentUpTo);
    const to = Math.max(from, end);
    this._sentUpTo = Math.max(this._sentUpTo, to);
    return this._textarea.value.substring(from, to);
  }

  /**
   * Apply any changes made to the textarea after the current event chain is allowed to complete.
   * This should be called when not currently composing but a keydown event with the "composition
   * character" (229) is triggered, in order to allow non-composition text to be entered when an
   * IME is active.
   */
  private _handleAnyTextareaChanges(): void {
    const oldValue = this._textarea.value;
    const generation = this._generation;
    setTimeout(() => {
      if (generation !== this._generation) return;
      // Ignore if a composition has started since the timeout
      if (!this._isComposing) {
        const newValue = this._textarea.value;

        const diff = newValue.replace(oldValue, '');

        this._dataAlreadySent = diff;

        if (newValue.length > oldValue.length) {
          this._send(diff);
        } else if (newValue.length < oldValue.length) {
          this._send(C0.DEL);
        } else if ((newValue.length === oldValue.length) && (newValue !== oldValue)) {
          this._send(newValue);
        }

      }
    }, 0);
  }

  /** Rendering and app guards read this same state; task delays only settle DOM input ranges. */
  public updateCompositionElements(_dontRecurse?: boolean): void {
    this._renderer(this.state);
    this._notify();
  }

  /** Default for an isolated xterm. aaa selects its cell/font/color adapter after open(). */
  private _renderDefault(state: CompositionState): void {
    this._compositionView.textContent = state.text;
    this._compositionView.classList.toggle('active', state.active && !!state.text);
    this._compositionView.style.visibility = state.visible ? '' : 'hidden';
    if (!state.active || !state.visible) return;
    const cell = this._renderService.dimensions.css.cell;
    const left = state.col * cell.width + 'px';
    const top = state.row * cell.height + 'px';
    Object.assign(this._compositionView.style, {
      left, top, height: cell.height + 'px', lineHeight: cell.height + 'px',
      fontFamily: this._optionsService.rawOptions.fontFamily, fontSize: this._optionsService.rawOptions.fontSize + 'px'
    });
    const rect = this._compositionView.getBoundingClientRect();
    Object.assign(this._textarea.style, { left, top, width: Math.max(rect.width, 1) + 'px',
      height: Math.max(rect.height, 1) + 'px', lineHeight: rect.height + 'px' });
  }
}
