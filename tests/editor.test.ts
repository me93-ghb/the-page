import { test } from 'node:test';
import assert from 'node:assert/strict';
import { JSDOM } from 'jsdom';
const dom = new JSDOM('<!doctype html><body></body>', { pretendToBeVisual: true });
for (const key of ['window', 'document', 'navigator', 'MutationObserver', 'HTMLElement', 'Node', 'DOMRect', 'getComputedStyle', 'requestAnimationFrame', 'cancelAnimationFrame']) {
  Object.defineProperty(globalThis, key, { value: (dom.window as any)[key], configurable: true });
}
const { createEditor } = await import('../src/lib/editor.ts');
const { undo, redo, isolateHistory } = await import('@codemirror/commands');
const { forceParsing } = await import('@codemirror/language');
const { EditorView } = await import('@codemirror/view');
const { editorSnapshot, transferEndInput } = await import('../src/lib/editor.ts');

test('undo and redo restore session ownership across a fold', () => {
  const parent = document.body.appendChild(document.createElement('div'));
  const sessions = [
    { start: '2026-09-12T08:00:00+01:00', previous_end: null, content: 'First words' },
    { start: '2026-09-12T09:00:00+01:00', previous_end: '2026-09-12T08:00:00+01:00', content: 'Second words' },
  ];
  const view = createEditor(parent, sessions.map(s => s.content).join(''), 'Page', false, () => {}, {
    date: '2026-09-12', sessions, lastEnd: sessions[1].start, active: () => false,
  });
  try {
    view.dispatch({ selection: { anchor: 6, head: 17 } });
    view.dispatch({ changes: { from: 6, to: 17 }, selection: { anchor: 6 }, userEvent: 'delete.selection' });
    const deleted = editorSnapshot(view);
    assert.ok(undo(view));
    assert.deepEqual(editorSnapshot(view).sessions, sessions);
    assert.equal(editorSnapshot(view).lastEnd, sessions[1].start);
    assert.ok(redo(view));
    assert.deepEqual(editorSnapshot(view), deleted);
    assert.ok(undo(view));
    assert.deepEqual(editorSnapshot(view).sessions, sessions);
  } finally { view.destroy(); parent.remove(); }
});

test('cancelled rollover cannot transfer text restored by undo; redo restores only its original input', () => {
  const sourceHost = document.body.appendChild(document.createElement('div'));
  const targetHost = document.body.appendChild(document.createElement('div'));
  let now = new Date('2026-09-13T03:00:10+01:00');
  const start = '2026-09-13T03:00:00+01:00';
  const source = createEditor(sourceHost, 'oldABC', 'Old page', false, () => {}, {
    date: '2026-09-12', sessions: [{ start, previous_end: null, content: 'oldABC' }], lastEnd: start, now: () => now,
  });
  const target = createEditor(targetHost, '', 'New page', false, () => {}, { date: '2026-09-13' });
  try {
    source.dispatch({ changes: { from: 3, to: 6 }, selection: { anchor: 3 }, userEvent: 'delete', annotations: isolateHistory.of('full') });
    now = new Date('2026-09-13T04:40:00+01:00');
    source.dispatch({ changes: { from: 3, insert: 'NEW' }, selection: { anchor: 6 }, userEvent: 'input.type', annotations: isolateHistory.of('full') });
    now = new Date('2026-09-13T04:40:02+01:00');
    source.dispatch({ changes: { from: 6, insert: 'TAIL' }, selection: { anchor: 10 }, userEvent: 'input.type', annotations: isolateHistory.of('full') });
    assert.ok(undo(source));
    assert.ok(undo(source));
    assert.ok(undo(source));
    transferEndInput(source, target);
    assert.equal(source.state.doc.toString(), 'oldABC', 'a late open_page must not collect historical text');
    assert.equal(target.state.doc.toString(), '');
    assert.ok(redo(source));
    assert.ok(redo(source));
    transferEndInput(source, target);
    assert.equal(source.state.doc.toString(), 'old');
    assert.equal(target.state.doc.toString(), 'NEW');
    assert.equal(editorSnapshot(target).lastEnd, '2026-09-13T04:40:02.000+01:00', 'undo must not rewind pending end-input time');
    assert.equal(editorSnapshot(source).lastEnd, start);
    assert.ok(undo(source));
    assert.equal(source.state.doc.toString(), 'oldABC', 'transferred input must leave no empty undo step on the old page');
    assert.ok(redo(source));
    assert.equal(source.state.doc.toString(), 'old');
    assert.equal(transferEndInput(source, target), false, 'source history cannot transfer completed input again');
    assert.equal(target.state.doc.toString(), 'NEW');
  } finally { source.destroy(); target.destroy(); sourceHost.remove(); targetHost.remove(); }
});

test('folds separate Markdown formatting and stay outside the selected document text', () => {
  const parent = document.body.appendChild(document.createElement('div'));
  const sessions = [
    { start: '2026-09-12T08:00:00+01:00', previous_end: null, content: '# Heading' },
    { start: '2026-09-12T09:00:00+01:00', previous_end: '2026-09-12T08:00:00+01:00', content: 'normal words' },
  ];
  const source = sessions.map(session => session.content).join('');
  const view = createEditor(parent, source, '12 September', false, () => {}, { date: '2026-09-12', sessions });
  try {
    assert.ok(![...parent.querySelectorAll('.md-heading')].some(node => node.textContent?.includes('normal words')));
    view.dispatch({ selection: { anchor: 0, head: source.length } });
    assert.equal(view.state.sliceDoc(view.state.selection.main.from, view.state.selection.main.to), source);
    assert.equal(parent.querySelector('[role=separator]')?.getAttribute('aria-label'), 'Writing session at 09:00');
  } finally { view.destroy(); parent.remove(); }
});

test('rollover transfers queued end-input once into an existing page and preserves historical clocks', () => {
  const sourceHost = document.body.appendChild(document.createElement('div'));
  const targetHost = document.body.appendChild(document.createElement('div'));
  const oldEnd = '2026-09-13T04:10:00+01:00';
  const source = createEditor(sourceHost, 'old', '12 September', false, () => {}, {
    date: '2026-09-12', sessions: [{ start: oldEnd, previous_end: null, content: 'old' }], lastEnd: oldEnd, now: () => new Date('2026-09-13T04:40:00+01:00'),
  });
  const targetStart = '2026-09-13T04:00:00+01:00';
  const target = createEditor(targetHost, 'existing', '13 September', false, () => {}, {
    date: '2026-09-13', sessions: [{ start: targetStart, previous_end: null, content: 'existing' }], lastEnd: targetStart,
  });
  try {
    for (const text of ['new', ' ', '😊']) {
      const from = source.state.doc.length;
      source.dispatch({ selection: { anchor: from } });
      source.dispatch({ changes: { from, insert: text }, selection: { anchor: from + text.length }, userEvent: 'input.type' });
    }
    const end = '2026-09-13T04:40:00.000+01:00';
    transferEndInput(source, target);
    transferEndInput(source, target);
    assert.equal(source.state.doc.toString(), 'old');
    assert.equal(editorSnapshot(source).lastEnd, oldEnd);
    assert.equal(target.state.doc.toString(), 'existingnew 😊');
    assert.equal(editorSnapshot(target).sessions[1].content, 'new 😊');
    assert.equal(editorSnapshot(target).lastEnd, end);
    assert.ok(undo(target));
    assert.equal(target.state.doc.toString(), 'existing');
  } finally { source.destroy(); target.destroy(); sourceHost.remove(); targetHost.remove(); }
});

test('folds preserve clocks through corrections, undo, empty sessions and composed input', () => {
  const parent = document.body.appendChild(document.createElement('div'));
  let now = new Date('2026-09-12T08:29:59+01:00');
  const start = '2026-09-12T08:00:00+01:00';
  const view = createEditor(parent, 'first', '12 September', false, () => {}, {
    date: '2026-09-12', sessions: [{ start, previous_end: null, content: 'first' }], lastEnd: start, now: () => now,
  });
  const input = (from: number, to: number, text: string, event = 'input.type') => {
    view.dispatch({ selection: { anchor: from } });
    view.dispatch({ changes: { from, to, insert: text }, selection: { anchor: from + text.length }, userEvent: event });
  };
  try {
    input(0, 1, 'F');
    assert.equal(editorSnapshot(view).lastEnd, start);
    now = new Date('2026-09-12T08:30:00+01:00');
    input(5, 5, '漢', 'input.type.compose.start');
    let snapshot = editorSnapshot(view);
    assert.equal(snapshot.sessions.length, 2);
    assert.equal(snapshot.sessions[0].content, 'First');
    assert.equal(snapshot.sessions[1].content, '漢');
    assert.equal(snapshot.sessions[1].previous_end, start);
    assert.equal(parent.querySelectorAll('[role=separator]').length, 1);
    assert.equal(view.state.doc.toString(), 'First漢');
    now = new Date('2026-09-12T09:01:00+01:00');
    input(5, 6, '漢字', 'input.type.compose');
    assert.equal(editorSnapshot(view).sessions.length, 2, 'composition must not split across a pause');
    assert.ok(undo(view));
    assert.equal(view.state.doc.toString(), 'First');
    assert.equal(editorSnapshot(view).sessions.length, 1, 'an empty final fold is not serialized');
    const lastEnd = editorSnapshot(view).lastEnd;
    now = new Date('2026-09-12T09:31:00+01:00');
    input(5, 5, 'return', 'input.paste');
    snapshot = editorSnapshot(view);
    assert.equal(snapshot.sessions.length, 2, 'reuse the empty final session');
    assert.equal(snapshot.sessions[1].previous_end, lastEnd);
    assert.equal(snapshot.sessions[1].content, 'return');
    assert.ok(undo(view));
    assert.ok(redo(view));
    assert.equal(editorSnapshot(view).lastEnd, snapshot.lastEnd);
  } finally { view.destroy(); parent.remove(); }
});

test('historical input never starts sessions; rollover leaves the input available exactly once', () => {
  const parent = document.body.appendChild(document.createElement('div'));
  let active = false;
  const redirects: string[] = [];
  const start = '2026-09-12T01:00:00+01:00';
  const view = createEditor(parent, 'old', '11 September', false, (_text, _appended, change) => {
    if (change.pending) redirects.push(change.pending.batch.date);
  }, { date: '2026-09-11', sessions: [{ start, previous_end: null, content: 'old' }], lastEnd: start,
    now: () => new Date('2026-09-13T12:00:00Z'), active: () => active });
  try {
    view.dispatch({ selection: { anchor: 3 } });
    view.dispatch({ changes: { from: 3, insert: ' edit' }, userEvent: 'input.type' });
    assert.equal(editorSnapshot(view).lastEnd, start);
    assert.equal(editorSnapshot(view).sessions.length, 1);
    active = true;
    view.dispatch({ selection: { anchor: view.state.doc.length } });
    view.dispatch({ changes: { from: view.state.doc.length, insert: 'new day' }, userEvent: 'input.type' });
    assert.equal(redirects.length, 1);
    assert.equal(view.state.doc.toString(), 'old editnew day');
    assert.equal(editorSnapshot(view).lastEnd, start, 'pending new-day input must not alter the old page clock');
  } finally { view.destroy(); parent.remove(); }
});

test('background parsing formats the end of a long reopened page without editing', () => {
  const parent = document.body.appendChild(document.createElement('div'));
  const source = 'Plain paragraph.\n\n'.repeat(500) + '**tail**';
  const view = createEditor(parent, source, 'Page', false, () => {});
  try {
    assert.ok(forceParsing(view, source.length, 500));
    let strong = false;
    for (const set of view.state.facet(EditorView.decorations)) {
      if (typeof set !== 'function') set.between(source.length - 8, source.length,
        (_from, _to, decoration) => { if (decoration.spec.class === 'md-strong') strong = true; });
    }
    assert.ok(strong, 'parsed tail must be formatted without a document edit');
    assert.equal(view.state.doc.toString(), source);
  } finally { view.destroy(); parent.remove(); }
});

test('completed Markdown becomes formatting while unmatched markers stay editable; undo restores text', () => {
  const parent = document.body.appendChild(document.createElement('div'));
  let saved = '';
  const view = createEditor(parent, '', 'Saturday, 12 September', false, (text) => { saved = text; });
  view.dispatch({ changes: { from: 0, insert: '**hello** and *unfinished' }, selection: { anchor: 24 } });
  assert.equal(view.state.doc.toString(), '**hello** and *unfinished');
  assert.equal(saved, view.state.doc.toString());
  assert.equal(parent.querySelector('.md-strong')?.textContent, 'hello');
  assert.ok(parent.textContent?.includes('*unfinished'));
  assert.ok(!parent.textContent?.includes('**'));
  assert.ok(undo(view));
  assert.equal(view.state.doc.toString(), '');
  assert.ok(redo(view));
  assert.equal(view.state.doc.toString(), '**hello** and *unfinished');
  assert.equal(parent.querySelector('[role=textbox]')?.getAttribute('aria-label'), 'Saturday, 12 September');
  view.destroy(); parent.remove();
});

test('authored headings and links remain text, not executable content', () => {
  const parent = document.body.appendChild(document.createElement('div'));
  const source = '# Own heading\n\n## 08:30\n\n> A quote\n\n- item\n\n<script>alert(1)</script>\n\n![image](https://example.com/a.png)';
  const view = createEditor(parent, source, 'Page', true, () => {});
  assert.equal(view.state.doc.toString(), source);
  assert.ok(parent.querySelector('.md-heading'));
  assert.ok(parent.querySelector('.md-quote'));
  assert.ok(parent.querySelector('.md-list'));
  assert.ok(parent.textContent?.includes('• item'));
  assert.ok(!parent.textContent?.includes('- item'));
  assert.equal(parent.querySelector('script, iframe, img[src^="http"]')?.outerHTML, undefined);
  assert.equal(view.contentDOM.getAttribute('contenteditable'), 'false');
  view.destroy(); parent.remove();
});

test('each template inserts once, formats immediately and undoes to a blank page', async () => {
  const { insertTemplate } = await import('../src/lib/editor.ts');
  for (const [name, text, caret] of [
    ['Morning', '*Slept*\n\n*Woke thinking about*\n\n*Today*\n\n', 8],
    ['Evening', '*What happened*\n\n*What stayed with me*\n\n*Tomorrow*\n\n', 16],
    ['Letter', 'Dear ,\n\n', 5],
  ] as const) {
    const parent = document.body.appendChild(document.createElement('div'));
    const view = createEditor(parent, '', 'Page', false, () => {});
    try {
      insertTemplate(view, name);
      assert.equal(view.state.doc.toString(), text);
      assert.equal(view.state.selection.main.head, caret);
      if (name !== 'Letter') assert.ok(parent.querySelector('.md-em'));
      assert.ok(undo(view));
      assert.equal(view.state.doc.toString(), '');
      assert.ok(redo(view));
      assert.equal(view.state.doc.toString(), text);
    } finally { view.destroy(); parent.remove(); }
  }
});

test('pending photographs retain bytes on failure, map their insertion point, and retry once', async () => {
  const { stagePhotograph } = await import('../src/lib/editor.ts');
  const parent = document.body.appendChild(document.createElement('div'));
  const view = createEditor(parent, 'before after', 'Page', false, () => {});
  const bytes = new Uint8Array([1, 2, 3]);
  let fails = true, calls = 0;
  try {
    view.dispatch({ selection: { anchor: 7 } });
    const pending = stagePhotograph(view, bytes, async data => {
      calls++; assert.deepEqual(data, bytes);
      if (fails) throw new Error('read only');
      return '2026-09-12/photo.png';
    });
    await assert.rejects(pending.retry(), /read only/);
    assert.deepEqual(pending.bytes, bytes);
    assert.equal(view.state.doc.toString(), 'before after');
    view.dispatch({ changes: { from: 0, insert: 'new ' } });
    fails = false;
    await pending.retry(); await pending.retry();
    assert.equal(calls, 2);
    assert.equal(view.state.doc.toString(), 'new before \n![](2026-09-12/photo.png)\nafter');
    assert.ok(undo(view));
    assert.equal(view.state.doc.toString(), 'new before after');
  } finally { view.destroy(); parent.remove(); }
});

test('photographs are atomic, caption edits preserve clocks, and removal undoes with the same reference', async () => {
  const { removePhotograph } = await import('../src/lib/editor.ts');
  const parent = document.body.appendChild(document.createElement('div'));
  const source = '![the shelf](2026-09-12/photo.png)';
  const start = '2026-09-12T08:00:00+01:00';
  const view = createEditor(parent, source, 'Page', false, () => {}, {
    date: '2026-09-12', sessions: [{ start, previous_end: null, content: source }], lastEnd: start,
    now: () => new Date('2026-09-12T10:00:00+01:00'), readPhotograph: async () => 'data:image/png;base64,AA==',
  });
  try {
    await new Promise(resolve => setTimeout(resolve, 0));
    assert.equal(parent.querySelector('img')?.alt, 'the shelf');
    const caption = parent.querySelector<HTMLInputElement>('input[aria-label="Photograph caption"]')!;
    caption.value = 'a [new] \\ caption';
    caption.dispatchEvent(new dom.window.Event('change'));
    assert.equal(editorSnapshot(view).lastEnd, start);
    assert.equal(editorSnapshot(view).sessions.length, 1);
    assert.equal(view.state.doc.toString(), '![a \\[new\\] \\\\ caption](2026-09-12/photo.png)');
    const edited = view.state.doc.toString();
    view.dispatch({ selection: { anchor: view.state.doc.length } });
    assert.ok(removePhotograph(view));
    assert.equal(view.state.doc.toString(), '');
    assert.ok(undo(view));
    assert.equal(view.state.doc.toString(), edited);
    assert.equal(editorSnapshot(view).lastEnd, start);
    const ranges = view.state.facet(EditorView.atomicRanges).flatMap(value => {
      const positions: number[] = []; value(view).between(0, view.state.doc.length, (from, to) => { positions.push(from, to); }); return positions;
    });
    assert.deepEqual(ranges, [0, edited.length]);
  } finally { view.destroy(); parent.remove(); }
});


test('focused caption input reaches save snapshots without blur and preserves typing and clocks', () => {
  const parent = document.body.appendChild(document.createElement('div'));
  const source = '![old caption](2026-09-12/photo.png)';
  const start = '2026-09-12T08:00:00+01:00';
  let saved = source, changes = 0;
  const view = createEditor(parent, source, 'Page', false, () => {
    saved = editorSnapshot(view).sessions[0].content; changes++;
  }, {
    date: '2026-09-12', sessions: [{ start, previous_end: null, content: source }], lastEnd: start,
    now: () => new Date('2026-09-12T10:00:00+01:00'), readPhotograph: async () => 'data:image/png;base64,AA==',
  });
  try {
    const caption = parent.querySelector<HTMLInputElement>('input')!;
    caption.focus();
    for (const text of ['new', 'new caption', 'new caption 日本']) {
      caption.value = text; caption.setSelectionRange(2, 2);
      caption.dispatchEvent(new dom.window.InputEvent('input', { bubbles: true, isComposing: text.endsWith('日本') }));
      assert.equal(saved, `![${text}](2026-09-12/photo.png)`, 'save/quit must receive focused input through the change callback');
      assert.equal(document.activeElement, caption);
      assert.equal(caption.selectionStart, 2);
      assert.equal(parent.querySelector('img')?.alt, text);
    }
    assert.equal(changes, 3);
    assert.equal(editorSnapshot(view).lastEnd, start);
    assert.equal(editorSnapshot(view).sessions.length, 1);
    caption.dispatchEvent(new dom.window.KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    const edited = saved;
    assert.ok(undo(view)); assert.notEqual(saved, edited);
    assert.ok(redo(view)); assert.equal(saved, edited);
    assert.equal(editorSnapshot(view).lastEnd, start);
  } finally { view.destroy(); parent.remove(); }
});

test('a delayed photograph never replaces edits made while its write is pending', async () => {
  const { stagePhotograph } = await import('../src/lib/editor.ts');
  const parent = document.body.appendChild(document.createElement('div'));
  const view = createEditor(parent, 'selected words', 'Page', false, () => {});
  let release!: (path: string) => void;
  try {
    view.dispatch({ selection: { anchor: 0, head: 14 } });
    const pending = stagePhotograph(view, new Uint8Array([1]), () => new Promise(resolve => release = resolve));
    const result = pending.retry();
    view.dispatch({ changes: { from: 9, insert: 'new ' }, userEvent: 'input.type' });
    release('2026-09-12/photo.png'); await result;
    assert.ok(view.state.doc.toString().includes('selected new words'));
    assert.ok(undo(view));
    assert.equal(view.state.doc.toString(), 'selected new words');
  } finally { view.destroy(); parent.remove(); }
});

test('Tab explicitly reaches templates only on an editable blank page', () => {
  const parent = document.body.appendChild(document.createElement('div'));
  let entered = 0;
  const view = createEditor(parent, '', 'Page', false, () => {}, { focusTemplates: () => entered++ });
  try {
    view.contentDOM.dispatchEvent(new dom.window.KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true }));
    assert.equal(entered, 1);
    view.dispatch({ changes: { from: 0, insert: 'writing' } });
    view.contentDOM.dispatchEvent(new dom.window.KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true }));
    assert.equal(entered, 1);
  } finally { view.destroy(); parent.remove(); }
});

test('an appended photograph after a pause starts one fold and its undo preserves the clock', async () => {
  const { stagePhotograph } = await import('../src/lib/editor.ts');
  const parent = document.body.appendChild(document.createElement('div'));
  const start = '2026-09-12T08:00:00+01:00';
  const view = createEditor(parent, 'words', 'Page', false, () => {}, {
    date: '2026-09-12', sessions: [{ start, previous_end: null, content: 'words' }], lastEnd: start,
    now: () => new Date('2026-09-12T08:30:00+01:00'),
  });
  try {
    view.dispatch({ selection: { anchor: 5 } });
    await stagePhotograph(view, new Uint8Array([1]), async () => '2026-09-12/photo.png').retry();
    const snapshot = editorSnapshot(view);
    assert.equal(snapshot.sessions.length, 2);
    assert.equal(snapshot.sessions[1].previous_end, start);
    assert.ok(snapshot.sessions[1].content.includes('![](2026-09-12/photo.png)'));
    assert.ok(undo(view));
    assert.equal(editorSnapshot(view).content, 'words');
    assert.equal(editorSnapshot(view).lastEnd, snapshot.lastEnd);
  } finally { view.destroy(); parent.remove(); }
});


test('choosing a template on a whitespace-only page follows end-input session rules', async () => {
  const { insertTemplate } = await import('../src/lib/editor.ts');
  const parent = document.body.appendChild(document.createElement('div'));
  const start = '2026-09-12T08:00:00+01:00';
  const view = createEditor(parent, '\n', 'Page', false, () => {}, {
    date: '2026-09-12', sessions: [{ start, previous_end: null, content: '\n' }], lastEnd: start,
    now: () => new Date('2026-09-12T08:30:00+01:00'),
  });
  try {
    insertTemplate(view, 'Morning');
    assert.equal(editorSnapshot(view).lastEnd, '2026-09-12T08:30:00.000+01:00');
    assert.ok(undo(view));
    assert.equal(view.state.doc.toString(), '\n');
  } finally { view.destroy(); parent.remove(); }
});
