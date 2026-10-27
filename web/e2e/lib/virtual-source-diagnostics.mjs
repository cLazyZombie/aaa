import fs from 'node:fs';
import path from 'node:path';

// 소스 본문·주소는 남기지 않고, 실제 shadow 행과 scroll root의 배치를 관측한다.
export async function watchVirtualSource(page, line) {
  if (process.env.AAA_FILE_VIEWER_TRACE === '1' && !page.__aaaVirtualTrace) {
    await page.context().tracing.start({ screenshots: true, snapshots: true });
    page.__aaaVirtualTrace = true;
  }
  await page.evaluate((line) => {
    window.__aaaVirtualSource?.stop();
    const events = [];
    const rect = (el) => el ? { top: el.getBoundingClientRect().top, height: el.getBoundingClientRect().height, width: el.getBoundingClientRect().width } : null;
    const read = () => {
      const host = document.querySelector('.virtual-source-file');
      const root = document.querySelector('.virtual-source-scroll');
      const row = host?.shadowRoot?.querySelector(`[data-line="${line}"]`);
      const code = host?.shadowRoot?.querySelector('[data-code]');
      const rowRect = rect(row), viewRect = rect(root);
      return {
        viewer: Boolean(document.querySelector('[data-testid="file-viewer"]')),
        settings: Boolean(document.querySelector('[role="region"][aria-label="설정"]')),
        settingsEntry: Boolean(history.state?.['aaa:settings']),
        preview: Boolean(history.state?.['aaa:file-preview']),
        line, present: Boolean(row), selected: row?.hasAttribute('data-selected-line') ?? false,
        row: rowRect, view: viewRect,
        centerDelta: rowRect && viewRect ? rowRect.top + rowRect.height / 2 - (viewRect.top + viewRect.height / 2) : null,
        scrollTop: root?.scrollTop, scrollHeight: root?.scrollHeight, clientHeight: root?.clientHeight,
        overflow: host?.shadowRoot?.querySelector('pre')?.getAttribute('data-overflow'),
        whiteSpace: row ? getComputedStyle(row).whiteSpace : null,
        sideways: code ? code.scrollWidth - code.clientWidth : null,
        longRow: rect(host?.shadowRoot?.querySelector(`[data-line="${line + 1}"]`)),
        rows: host?.shadowRoot?.querySelectorAll('[data-line]').length ?? 0,
      };
    };
    const record = (event) => {
      events.push({ at: performance.now(), event, ...read() });
      if (events.length > 160) events.shift();
    };
    let host, root, shadow, lastState;
    const resize = new ResizeObserver((entries) => record(`resize:${entries.map(({ target }) => target === root ? 'viewport' : 'file').join(',')}`));
    const rows = new MutationObserver(() => record('rows'));
    const attach = () => {
      const nextHost = document.querySelector('.virtual-source-file');
      const nextRoot = document.querySelector('.virtual-source-scroll');
      const state = `${Boolean(document.querySelector('[data-testid="file-viewer"]'))}:${Boolean(document.querySelector('[role="region"][aria-label="설정"]'))}`;
      if (state !== lastState) { lastState = state; record('viewer/settings'); }
      if (host !== nextHost || root !== nextRoot) {
        resize.disconnect();
        root?.removeEventListener('scroll', onScroll);
        host = nextHost; root = nextRoot;
        if (host) resize.observe(host);
        if (root) { resize.observe(root); root.addEventListener('scroll', onScroll); }
        record('mount');
      }
      if (host?.shadowRoot !== shadow) {
        rows.disconnect(); shadow = host?.shadowRoot;
        if (shadow) rows.observe(shadow, { childList: true, subtree: true, attributes: true });
      }
    };
    const onScroll = () => record('scroll');
    const onPopState = () => record('popstate');
    window.addEventListener('popstate', onPopState);
    const documentObserver = new MutationObserver(attach);
    documentObserver.observe(document.body, { childList: true, subtree: true });
    attach();
    window.__aaaVirtualSource = {
      snapshot: () => ({ current: read(), events, fileOpen: window.__aaaFileOpen, terminalWire: window.__aaaTermLog }),
      stop: () => { documentObserver.disconnect(); rows.disconnect(); resize.disconnect(); root?.removeEventListener('scroll', onScroll); window.removeEventListener('popstate', onPopState); },
    };
  }, line);
}

export async function saveVirtualSource(page, directory, name) {
  fs.mkdirSync(directory, { recursive: true });
  const snapshot = await page.evaluate(() => window.__aaaVirtualSource?.snapshot());
  fs.writeFileSync(path.join(directory, `${name}.json`), JSON.stringify(snapshot, null, 2));
  await page.screenshot({ path: path.join(directory, `${name}.png`) });
  if (page.__aaaVirtualTrace) {
    await page.context().tracing.stop({ path: path.join(directory, `${name}-trace.zip`) });
    page.__aaaVirtualTrace = false;
  }
  return snapshot?.current;
}
