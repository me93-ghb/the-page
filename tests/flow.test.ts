import { test } from 'node:test';
import assert from 'node:assert/strict';
import { historyBatch, historyGap, gapNote, navigationPlan } from '../src/lib/flow.ts';

test('history batches preserve date order across gaps and future pages', () => {
  const dates = Array.from({ length: 17 }, (_, i) => `2026-09-${String(i + 1).padStart(2, '0')}`);
  assert.deepEqual(historyBatch(dates, '2026-09-17', -1), dates.slice(8, 16));
  assert.deepEqual(historyBatch(dates, '2026-09-09', -1), dates.slice(0, 8));
  assert.deepEqual(historyBatch(dates, '2026-09-01', -1), []);
  assert.deepEqual(historyBatch(['2026-09-01', '2026-09-10', '2026-09-20'], '2026-09-12', 1), ['2026-09-20']);
  assert.equal(gapNote('2026-09-01', '2026-09-02'), '');
  assert.equal(gapNote('2026-09-01', '2026-09-03'), 'a day');
  assert.equal(gapNote('2026-09-01', '2026-09-06'), 'four days');
  assert.equal(gapNote('2026-09-01', '2026-09-23'), 'three weeks');
  assert.equal(gapNote('2026-01-01', '2026-09-01'), 'some months');
});

test('travel fills missing saved dates inside the loaded interval in batches of eight', () => {
  const saved = ['2026-09-11', '2026-09-12', '2026-09-13'];
  const loaded = ['2026-09-11', '2026-09-13'];
  assert.deepEqual(historyGap(saved, loaded), ['2026-09-12']);
  assert.equal(gapNote(loaded[0], loaded[1], saved), '', 'a saved page is not an unwritten day');
  assert.deepEqual(historyGap(saved, saved), []);
  assert.deepEqual(historyGap(saved, []), []);
  assert.deepEqual(historyGap(saved, ['2026-09-13']), []);
  const many = Array.from({ length: 20 }, (_, i) => `2026-09-${String(i + 1).padStart(2, '0')}`);
  let interval = [many[0], many.at(-1)!];
  const sizes = [];
  for (let batch; (batch = historyGap(many, interval)).length;) {
    sizes.push(batch.length); interval = [...interval, ...batch].sort();
  }
  assert.deepEqual(sizes, [8, 8, 2]);
  assert.deepEqual(interval, many);
});

test('navigation bounds movement, keeps long-jump fades and respects reduced motion', () => {
  assert.deepEqual(navigationPlan(0, 500, true, false), { duration: 240, fade: false });
  assert.deepEqual(navigationPlan(1500, 500, true, false), { duration: 410, fade: false });
  assert.deepEqual(navigationPlan(1501, 500, true, false), { duration: 0, fade: true });
  assert.deepEqual(navigationPlan(4000, 2000, true, false), { duration: 480, fade: false });
  assert.deepEqual(navigationPlan(1000, 500, false, false), { duration: 300, fade: false });
  assert.deepEqual(navigationPlan(1000, 500, true, true), { duration: 0, fade: false });
  assert.deepEqual(navigationPlan(4000, 500, true, true), { duration: 0, fade: true });
});

test('prepending preserves a visible line through late layout; user scrolling and input cancel navigation', async () => {
  const { JSDOM } = await import('jsdom');
  const { Flow } = await import('../src/lib/flow.ts');
  const dom = new JSDOM('<main><div><div class="cm-line">Keep this line</div></div></main>');
  let resize = () => {}, frame = (_time: number) => {};
  Object.assign(globalThis, {
    ResizeObserver: class { constructor(callback: () => void) { resize = callback; } observe() {} disconnect() {} },
    requestAnimationFrame: (callback: (time: number) => void) => { frame = callback; return 1; },
    cancelAnimationFrame: () => { frame = () => {}; }, matchMedia: () => ({ matches: false }),
  });
  const root = dom.window.document.querySelector('main')!, content = root.firstElementChild as HTMLElement, line = content.firstElementChild as HTMLElement;
  let height = 5000, inserted = 0;
  Object.defineProperties(root, { scrollHeight: { get: () => height }, clientHeight: { value: 500 } });
  content.getBoundingClientRect = () => ({ height }) as DOMRect;
  root.scrollTop = 1000;
  root.getBoundingClientRect = () => ({ top: 52, bottom: 552 }) as DOMRect;
  line.getBoundingClientRect = () => ({ top: 120 + inserted - (root.scrollTop - 1000), bottom: 150 + inserted - (root.scrollTop - 1000) }) as DOMRect;
  const flow = new Flow(root, content);
  try {
    await flow.preserve(async () => { inserted = 800; height += 800; }, true);
    assert.equal(root.scrollTop, 1800);
    inserted += 200; height += 200; resize();
    assert.equal(line.getBoundingClientRect().top, 120);
    root.scrollTop += 25; root.dispatchEvent(new dom.window.Event('scroll'));
    inserted += 100; height += 100; resize();
    assert.equal(root.scrollTop, 2025, 'native scrolling releases the saved anchor');
    let ready!: () => void, focused = false;
    const delayed = flow.navigate(async () => {
      await new Promise<void>(resolve => ready = resolve);
      return { top: () => 4000, focus: () => focused = true };
    }, false);
    dom.window.document.dispatchEvent(new dom.window.KeyboardEvent('keydown'));
    ready(); await delayed;
    assert.equal(focused, false);
    assert.equal(root.scrollTop, 2025, 'input cancels navigation even while a page is loading');
    const moving = flow.navigate(async () => ({ top: () => 4000, focus: () => focused = true }), false);
    await Promise.resolve(); frame(performance.now() + 100);
    assert.ok(root.scrollTop > 2025 && root.scrollTop < 4000);
    const beforeLayout = root.scrollTop;
    height += 100; root.scrollTop += 80; root.dispatchEvent(new dom.window.Event('scroll'));
    frame(performance.now() + 200);
    assert.ok(root.scrollTop > beforeLayout + 80, 'CodeMirror layout compensation does not cancel navigation');
    root.scrollTop += 25; root.dispatchEvent(new dom.window.Event('scroll'));
    const stopped = root.scrollTop; await moving;
    assert.equal(root.scrollTop, stopped);
    assert.equal(focused, false);
  } finally { flow.destroy(); dom.window.close(); }
});
