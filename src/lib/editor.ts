import { EditorState, StateField } from '@codemirror/state';
import { Decoration, EditorView, keymap, WidgetType } from '@codemirror/view';
import type { DecorationSet } from '@codemirror/view';
import { defaultKeymap, history, historyKeymap } from '@codemirror/commands';
import { markdown, markdownKeymap } from '@codemirror/lang-markdown';
import { syntaxTree } from '@codemirror/language';

const bullet = new class extends WidgetType {
  toDOM() { const span = document.createElement('span'); span.textContent = '•'; return span; }
};

function formatting(state: EditorState): DecorationSet {
  const ranges: ReturnType<Decoration['range']>[] = [];
  syntaxTree(state).iterate({ enter(node) {
    if (node.name === 'StrongEmphasis' || node.name === 'Emphasis') {
      ranges.push(Decoration.mark({ class: node.name === 'StrongEmphasis' ? 'md-strong' : 'md-em' }).range(node.from, node.to));
    }
    if (/^ATXHeading[1-6]$/.test(node.name)) {
      ranges.push(Decoration.line({ class: node.name === 'ATXHeading1' ? 'md-heading' : 'md-subheading' }).range(state.doc.lineAt(node.from).from));
    }
    if (node.name === 'ListItem' || node.name === 'Blockquote') {
      const first = state.doc.lineAt(node.from), last = state.doc.lineAt(node.to);
      for (let n = first.number; n <= last.number; n++) {
        ranges.push(Decoration.line({ class: node.name === 'ListItem' ? 'md-list' : 'md-quote' }).range(state.doc.line(n).from));
      }
    }
    if (['EmphasisMark', 'HeaderMark', 'QuoteMark'].includes(node.name)) {
      let to = node.to;
      if (node.name !== 'EmphasisMark' && state.doc.sliceString(to, to + 1) === ' ') to++;
      ranges.push(Decoration.replace({}).range(node.from, to));
    }
    if (node.name === 'ListMark' && /^[-+*]$/.test(state.doc.sliceString(node.from, node.to))) {
      ranges.push(Decoration.replace({ widget: bullet }).range(node.from, node.to));
    }
  } });
  return Decoration.set(ranges, true);
}

const decorations = StateField.define<DecorationSet>({
  create: formatting,
  update(value, transaction) {
    return transaction.docChanged || syntaxTree(transaction.startState) !== syntaxTree(transaction.state)
      ? formatting(transaction.state) : value;
  },
  provide: field => EditorView.decorations.from(field),
});

export function createEditor(parent: HTMLElement, content: string, label: string, readOnly: boolean,
  changed: (content: string, appended: boolean) => void): EditorView {
  return new EditorView({ parent, state: EditorState.create({
    doc: content,
    extensions: [markdown(), history(), decorations, EditorView.lineWrapping,
      EditorState.readOnly.of(readOnly), EditorView.editable.of(!readOnly),
      EditorView.contentAttributes.of({ 'aria-label': label, 'aria-multiline': 'true', spellcheck: 'true' }),
      keymap.of([...markdownKeymap, ...defaultKeymap, ...historyKeymap]),
      EditorView.updateListener.of(update => {
        if (!update.docChanged) return;
        const before = update.startState;
        const selection = before.selection.main;
        let appended = false;
        if (selection.empty && !before.doc.sliceString(selection.head).trim()) {
          update.changes.iterChanges((from, to, _a, _b, inserted) => {
            if (from === selection.head && from === to && inserted.length > 0) appended = true;
          });
        }
        if (update.transactions.some(t => t.isUserEvent('undo') || t.isUserEvent('redo'))) appended = false;
        changed(update.state.doc.toString(), appended);
      }),
    ],
  }) });
}
