import { test } from 'node:test';
import assert from 'node:assert/strict';
import { JSDOM } from 'jsdom';
const dom = new JSDOM('<!doctype html><body></body>', { pretendToBeVisual: true });
for (const key of ['window', 'document', 'navigator', 'MutationObserver', 'HTMLElement', 'Node', 'DOMRect', 'getComputedStyle', 'requestAnimationFrame', 'cancelAnimationFrame']) {
  Object.defineProperty(globalThis, key, { value: (dom.window as any)[key], configurable: true });
}
const { createEditor } = await import('../src/lib/editor.ts');
const { undo, redo } = await import('@codemirror/commands');
const { forceParsing } = await import('@codemirror/language');
const { EditorView } = await import('@codemirror/view');

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
