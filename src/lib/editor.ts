import { Annotation, EditorState, Facet, StateEffect, StateField, Transaction } from '@codemirror/state';
import { Decoration, EditorView, keymap, WidgetType } from '@codemirror/view';
import type { DecorationSet } from '@codemirror/view';
import { defaultKeymap, history, historyKeymap, invertedEffects, isolateHistory } from '@codemirror/commands';
import { markdown, markdownKeymap, markdownLanguage } from '@codemirror/lang-markdown';
import { syntaxTree } from '@codemirror/language';
import { foldTier, inputPlan, journalDay, sessionTime, timestamp } from './journal';
import type { WritingSession } from './journal';

type Boundary = Omit<WritingSession, 'content'> & { from: number; fresh?: boolean };
type Options = { date: string; sessions: WritingSession[]; lastEnd: string | null; now: () => Date; active: () => boolean; readPhotograph?: (path: string) => Promise<string>; photograph?: (file: File) => void; focusTemplates?: () => void };
type PendingInput = { from: number; to: number; batch: { date: string; at: string; lastEnd: string; completed: boolean } };
export type WritingChange = { appended: boolean; pending: PendingInput | null };
type Ownership = { boundaries: Boundary[]; pending: PendingInput | null };
const restoreOwnership = StateEffect.define<Ownership & { transferred?: PendingInput['batch'] }>({
  map: value => value.transferred?.completed ? undefined : value,
});
const settings = Facet.define<Options, Options>({ combine: values => values[0] });
export const inputTime = Annotation.define<string>();
export const endClock = StateEffect.define<string>();
export const refreshFolds = StateEffect.define<null>();

const writing = StateField.define<Ownership & { lastEnd: string | null; compositionFrom: number | null; change?: WritingChange }>({
  create(state) {
    const options = state.facet(settings);
    let from = 0;
    const boundaries = options.sessions.map(({ content, ...session }) => { const boundary = { ...session, from }; from += content.length; return boundary; });
    return { boundaries, pending: null, lastEnd: options.lastEnd ?? boundaries.at(-1)?.start ?? null, compositionFrom: null };
  },
  update(value, tr) {
    const clock = tr.effects.find(effect => effect.is(endClock));
    if (!tr.docChanged) return clock ? { ...value, lastEnd: clock.value } : value;
    const options = tr.state.facet(settings), selection = tr.startState.selection.main;
    const at = tr.annotation(inputTime) ?? timestamp(options.now());
    const composing = tr.isUserEvent('input.type.compose');
    const continuing = composing && !tr.isUserEvent('input.type.compose.start') && value.compositionFrom !== null;
    let appended = false, count = 0;
    tr.changes.iterChanges((from, to, _a, _b, inserted) => {
      count++;
      appended = inserted.length > 0 && (continuing
        ? from >= value.compositionFrom! && !tr.startState.doc.sliceString(to).trim()
        : selection.empty && from === selection.head && from === to && !tr.startState.doc.sliceString(selection.head).trim());
    });
    appended = appended && !tr.annotation(correction) && count === 1 && !tr.isUserEvent('undo') && !tr.isUserEvent('redo');
    let boundaries = value.boundaries.map(boundary => ({ ...boundary, from: tr.changes.mapPos(boundary.from, -1) }));
    let lastEnd = value.lastEnd;
    let pending = value.pending && { ...value.pending, from: tr.changes.mapPos(value.pending.from, -1), to: tr.changes.mapPos(value.pending.to, 1) };
    if (pending && appended) pending.batch.lastEnd = at;
    if (pending && pending.from === pending.to) pending = null;
    if (appended && !value.pending && options.active()) {
      const plan = inputPlan(options.date, lastEnd, new Date(at), !continuing);
      if (plan.kind === 'rollover') pending = { from: selection.head, to: tr.newDoc.length, batch: { date: plan.date!, at, lastEnd: at, completed: false } };
      else {
        if (!boundaries.length) boundaries = [{ from: 0, start: at, previous_end: null }];
        else if (plan.kind === 'fold') {
          const last = value.boundaries.at(-1)!;
          const boundary = { from: tr.changes.mapPos(selection.head, -1), start: at, previous_end: lastEnd, fresh: true };
          if (value.boundaries.length > 1 && !tr.startState.doc.sliceString(last.from).trim()) boundaries[boundaries.length - 1] = boundary;
          else boundaries.push(boundary);
        }
        lastEnd = at;
      }
    }
    for (const effect of tr.effects) if (effect.is(restoreOwnership)) {
      const empty = boundaries.at(-1);
      boundaries = effect.value.boundaries;
      // Keep one empty session available after undoing its first input.
      if (!boundaries.length && value.boundaries.length) boundaries = [{ ...value.boundaries[0], from: 0 }];
      else if (empty?.from === tr.newDoc.length && boundaries.at(-1)!.from < tr.newDoc.length) boundaries = [...boundaries, empty];
      pending = effect.value.pending?.batch.completed ? null : effect.value.pending;
    }
    if (!boundaries.length && !pending && tr.newDoc.length) {
      boundaries = [{ from: 0, start: at, previous_end: null }];
      lastEnd ??= at;
    }
    return { boundaries, pending, lastEnd, change: { appended, pending }, compositionFrom: composing && appended ? tr.changes.mapPos(continuing ? value.compositionFrom! : selection.head, -1) : null };
  },
});

export function editorSnapshot(view: EditorView): { content: string; sessions: WritingSession[]; lastEnd: string | null } {
  const content = view.state.doc.toString(), value = view.state.field(writing);
  const sessions = value.boundaries.map(({ from, fresh: _fresh, ...session }, i) => ({ ...session, content: content.slice(from, value.boundaries[i + 1]?.from) }));
  while (sessions.length > 1 && !sessions.at(-1)!.content.trim()) sessions.pop();
  return { content, sessions, lastEnd: value.lastEnd };
}

export function transferEndInput(source: EditorView, target: EditorView) {
  const pending = source.state.field(writing).pending;
  if (!pending || pending.batch.completed || pending.batch.date !== target.state.facet(settings).date) return false;
  const text = source.state.doc.sliceString(pending.from, pending.to), end = target.state.doc.length;
  if (!text) return false;
  target.dispatch({ selection: { anchor: end } });
  target.dispatch({ changes: { from: end, insert: text }, selection: { anchor: end + text.length },
    annotations: inputTime.of(pending.batch.at), userEvent: 'input.paste', scrollIntoView: true });
  target.dispatch({ effects: endClock.of(pending.batch.lastEnd) });
  pending.batch.completed = true;
  source.dispatch({ changes: { from: pending.from, to: pending.to }, annotations: Transaction.addToHistory.of(false), userEvent: 'delete' });
  return true;
}

export const templates = ['Blank', 'Morning', 'Evening', 'Letter'] as const;
export function insertTemplate(view: EditorView, name: typeof templates[number]) {
  if (view.state.readOnly || view.state.doc.toString().trim()) return;
  const prompts = name === 'Morning' ? ['Slept', 'Woke thinking about', 'Today'] : ['What happened', 'What stayed with me', 'Tomorrow'];
  const text = name === 'Blank' ? '' : name === 'Letter' ? 'Dear ,\n\n' : prompts.map(prompt => `*${prompt}*\n\n`).join('');
  view.dispatch({ selection: { anchor: 0 } });
  if (text) view.dispatch({ changes: { from: 0, insert: text },
    selection: { anchor: name === 'Letter' ? 5 : prompts[0].length + 3 }, userEvent: 'input.template', annotations: isolateHistory.of('full') });
  view.focus();
}

const correction = Annotation.define<boolean>();
type ImageAnchor = { id: object; from: number; to: number; corrected?: boolean };
const imageAnchor = StateEffect.define<ImageAnchor>();
const removeImageAnchor = StateEffect.define<object>();
const imageAnchors = StateField.define<ImageAnchor[]>({
  create: () => [],
  update(value, tr) {
    let next: ImageAnchor[] = value.map(anchor => {
      const from = tr.changes.mapPos(anchor.from, 1);
      return { ...anchor, from, to: tr.docChanged ? from : anchor.to, corrected: anchor.corrected || tr.docChanged };
    });
    for (const effect of tr.effects) {
      if (effect.is(imageAnchor)) next.push(effect.value);
      if (effect.is(removeImageAnchor)) next = next.filter(anchor => anchor.id !== effect.value);
    }
    return next;
  },
});

export function stagePhotograph(view: EditorView, input: Uint8Array | Promise<Uint8Array>, store: (bytes: Uint8Array) => Promise<string>, at = timestamp(view.state.facet(settings).now())) {
  const id = {}, selection = view.state.selection.main;
  view.dispatch({ effects: imageAnchor.of({ id, from: selection.from, to: selection.to }) });
  let running: Promise<void> | undefined, completed = false;
  let bytes = input instanceof Uint8Array ? input : null;
  return { get bytes() { return bytes; }, retry(): Promise<void> {
    if (completed) return Promise.resolve();
    if (running) return running;
    running = (async () => {
      bytes ??= await input;
      const path = await store(bytes);
      const anchor = view.state.field(imageAnchors).find(anchor => anchor.id === id)!;
      const before = anchor.from && view.state.doc.sliceString(anchor.from - 1, anchor.from) !== '\n' ? '\n' : '';
      const text = `${before}![](${path})\n`;
      const selected = view.state.selection.main;
      const unchanged = selected.from === anchor.from && selected.to === anchor.to;
      view.dispatch({ changes: { from: anchor.from, to: anchor.to, insert: text },
        ...(unchanged ? { selection: { anchor: anchor.from + text.length } } : {}),
        effects: removeImageAnchor.of(id), userEvent: 'input.paste', annotations: [isolateHistory.of('full'), correction.of(!!anchor.corrected || !unchanged || Date.parse(editorSnapshot(view).lastEnd ?? at) > Date.parse(at)), inputTime.of(at)], scrollIntoView: unchanged });
      completed = true;
    })().finally(() => { running = undefined; });
    return running;
  } };
}
export type PendingPhotograph = ReturnType<typeof stagePhotograph>;

function imageParts(source: string) {
  const match = /^!\[((?:\\.|[^\]\\])*)\]\(([^\s()]+)\)$/.exec(source);
  return match ? { caption: match[1].replace(/\\(.)/g, '$1'), path: match[2] } : null;
}
function escapeCaption(caption: string) { return caption.replace(/[\\\[\]]/g, '\\$&').replace(/[\r\n]/g, ' '); }

class Photograph extends WidgetType {
  constructor(readonly source: string, readonly caption: string, readonly path: string) { super(); }
  eq(other: Photograph) { return this.source === other.source; }
  updateDOM(figure: HTMLElement, _view: EditorView, previous: Photograph) {
    if (this.path !== previous.path) return false;
    figure.dataset.source = this.source;
    const image = figure.querySelector('img'), caption = figure.querySelector('input'), label = figure.querySelector('figcaption');
    if (image) image.alt = this.caption;
    if (caption && caption.value !== this.caption) caption.value = this.caption;
    if (label) label.textContent = this.caption;
    return true;
  }
  toDOM(view: EditorView) {
    const figure = document.createElement('figure'), image = document.createElement('img');
    figure.className = 'photograph'; figure.contentEditable = 'false';
    figure.dataset.source = this.source;
    image.alt = this.caption;
    figure.append(image);
    const reader = view.state.facet(settings).readPhotograph;
    const error = document.createElement('span'); error.className = 'photograph-error';
    const load = reader && !/^(?:[a-z]+:|\/)/i.test(this.path) && !this.path.split('/').includes('..')
      ? reader(this.path) : Promise.reject(new Error('Use a local photograph inside the journal.'));
    load.then(source => { image.src = source; view.requestMeasure(); }).catch(reason => {
      image.remove(); error.textContent = String(reason); figure.prepend(error); view.requestMeasure();
    });
    if (view.state.readOnly) {
      const caption = document.createElement('figcaption'); caption.textContent = this.caption; figure.append(caption);
    } else {
      const caption = document.createElement('input'); caption.className = 'photograph-caption';
      caption.setAttribute('aria-label', 'Photograph caption'); caption.placeholder = 'Add a caption'; caption.value = this.caption;
      let original = caption.value;
      caption.addEventListener('focus', () => { original = caption.value; });
      const commit = () => {
        const source = figure.dataset.source!, from = view.posAtDOM(figure), to = from + source.length;
        const next = `![${escapeCaption(caption.value)}](${this.path})`;
        if (view.state.doc.sliceString(from, to) !== source || next === source) return;
        view.dispatch({ changes: { from, to, insert: next },
          userEvent: 'input.caption', annotations: [correction.of(true), isolateHistory.of('full')] });
      };
      caption.addEventListener('input', commit);
      caption.addEventListener('change', commit);
      caption.addEventListener('keydown', event => {
        if (event.isComposing) return;
        if (event.key === 'Enter' || event.key === 'Escape') {
          event.preventDefault(); event.stopPropagation();
          const from = view.posAtDOM(figure);
          if (event.key === 'Escape') caption.value = original;
          commit();
          const next = view.state.doc.lineAt(from).to;
          view.focus(); view.dispatch({ selection: { anchor: next } });
        }
      });
      figure.append(caption);
    }
    return figure;
  }
  ignoreEvent() { return true; }
}

export function removePhotograph(view: EditorView) {
  if (view.state.readOnly || !view.state.selection.main.empty) return false;
  const head = view.state.selection.main.head;
  let range: { from: number; to: number } | undefined;
  view.state.field(decorations).between(0, head, (from, to, value) => {
    if (value.spec.widget instanceof Photograph && (to === head || view.state.doc.sliceString(to, head) === '\n')) range = { from, to: head };
  });
  if (!range) return false;
  view.dispatch({ changes: range, selection: { anchor: range.from }, userEvent: 'delete.backward', annotations: isolateHistory.of('full') });
  return true;
}

class Fold extends WidgetType {
  constructor(readonly session: Boundary, readonly date: string, readonly current: boolean) { super(); }
  eq(other: Fold) { return this.session.start === other.session.start && this.session.previous_end === other.session.previous_end && this.current === other.current; }
  toDOM() {
    const fold = document.createElement('div'), time = document.createElement('time');
    const tier = foldTier(Math.max(0, Date.parse(this.session.start) - Date.parse(this.session.previous_end!)));
    fold.className = `fold${this.session.fresh ? ' entering' : ''}`;
    fold.style.setProperty('--fold-height', `${tier.height}rem`); fold.style.setProperty('--depth', String(tier.depth));
    fold.setAttribute('role', 'separator'); fold.setAttribute('aria-label', `Writing session at ${sessionTime(this.session.start, this.date)}`);
    fold.contentEditable = 'false';
    time.dateTime = this.session.start; time.textContent = sessionTime(this.session.start, this.date); time.classList.toggle('current', this.current);
    fold.append(time); return fold;
  }
  updateDOM(dom: HTMLElement, _view: EditorView, previous: Fold) {
    if (this.session.start !== previous.session.start || this.session.previous_end !== previous.session.previous_end) return false;
    dom.querySelector('time')!.classList.toggle('current', this.current); return true;
  }
  get estimatedHeight() { return foldTier(Math.max(0, Date.parse(this.session.start) - Date.parse(this.session.previous_end!))).height * 16; }
  ignoreEvent() { return true; }
}

const bullet = new class extends WidgetType {
  toDOM() { const span = document.createElement('span'); span.textContent = '•'; return span; }
};

function formatting(state: EditorState): DecorationSet {
  const ranges: ReturnType<Decoration['range']>[] = [];
  const boundaries = state.field(writing).boundaries;
  const sections = boundaries.length > 1 ? boundaries : [{ from: 0 }];
  for (let index = 0; index < sections.length; index++) {
    const offset = sections[index].from, end = sections[index + 1]?.from ?? state.doc.length;
    const tree = boundaries.length > 1 ? markdownLanguage.parser.parse(state.doc.sliceString(offset, end)) : syntaxTree(state);
    tree.iterate({ enter(part) {
    const node = { name: part.name, from: part.from + offset, to: part.to + offset };
    if (node.name === 'Image') {
      const source = state.doc.sliceString(node.from, node.to), image = imageParts(source);
      if (image) {
        ranges.push(Decoration.replace({ widget: new Photograph(source, image.caption, image.path), block: true }).range(node.from, node.to));
        return false;
      }
    }
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
  }
  let last = boundaries.length - 1;
  while (last > 0 && !state.doc.sliceString(boundaries[last].from).trim()) last--;
  for (let i = 1; i <= last; i++) ranges.push(Decoration.widget({ widget: new Fold(boundaries[i], state.facet(settings).date, i === last && state.facet(settings).active()), block: true, side: -1 }).range(boundaries[i].from));
  return Decoration.set(ranges, true);
}

const decorations = StateField.define<DecorationSet>({
  create: formatting,
  update(value, transaction) {
    return transaction.docChanged || transaction.effects.some(effect => effect.is(refreshFolds)) || syntaxTree(transaction.startState) !== syntaxTree(transaction.state)
      ? formatting(transaction.state) : value;
  },
  provide: field => EditorView.decorations.from(field),
});

export function createEditor(parent: HTMLElement, content: string, label: string, readOnly: boolean,
  changed: (content: string, appended: boolean, change: WritingChange) => void, options: Partial<Options> = {}): EditorView {
  const now = options.now ?? (() => new Date());
  const initial = options.sessions ?? (content ? [{ start: timestamp(now()), previous_end: null, content }] : []);
  return new EditorView({ parent, state: EditorState.create({
    doc: content,
    extensions: [settings.of({ date: options.date ?? journalDay(now()), sessions: initial, lastEnd: options.lastEnd ?? null, active: options.active ?? (() => true), now, readPhotograph: options.readPhotograph, photograph: options.photograph }),
      markdown(), history(), writing, imageAnchors, decorations, EditorView.lineWrapping,
      invertedEffects.of(tr => {
        if (!tr.docChanged) return [];
        const before = tr.startState.field(writing), after = tr.state.field(writing);
        const sameBoundaries = before.boundaries.length === after.boundaries.length && before.boundaries.every((b, i) => {
          const next = after.boundaries[i];
          return b.from === next.from && b.start === next.start && b.previous_end === next.previous_end;
        });
        return [restoreOwnership.of({ boundaries: before.boundaries, pending: before.pending,
          transferred: sameBoundaries ? (after.pending ?? before.pending)?.batch : undefined })];
      }),
      EditorState.readOnly.of(readOnly), EditorView.editable.of(!readOnly),
      EditorView.contentAttributes.of({ 'aria-label': label, 'aria-multiline': 'true', spellcheck: 'true' }),
      EditorView.atomicRanges.of(view => view.state.field(decorations).update({ filter: (_from, _to, value) => value.spec.widget instanceof Photograph })),
      EditorView.domEventHandlers({
        paste(event, view) {
          const files = Array.from(event.clipboardData?.files ?? []);
          if (!files.length || view.state.readOnly || !options.photograph) return false;
          event.preventDefault(); files.forEach(file => options.photograph!(file)); return true;
        },
        dragover(event) { if (event.dataTransfer?.types.includes('Files')) { event.preventDefault(); return true; } return false; },
        drop(event, view) {
          const files = Array.from(event.dataTransfer?.files ?? []);
          if (!files.length || view.state.readOnly || !options.photograph) return false;
          event.preventDefault();
          const at = view.posAtCoords({ x: event.clientX, y: event.clientY });
          if (at !== null) view.dispatch({ selection: { anchor: at } });
          view.focus(); files.forEach(file => options.photograph!(file)); return true;
        },
      }),
      keymap.of([{ key: 'Tab', run: view => {
        if (view.state.readOnly || view.state.doc.toString().trim() || !options.focusTemplates) return false;
        options.focusTemplates(); return true;
      } }, { key: 'Backspace', run: removePhotograph }, ...markdownKeymap, ...defaultKeymap, ...historyKeymap]),
      EditorView.updateListener.of(update => {
        if (!update.docChanged) return;
        const change = update.state.field(writing).change!;
        changed(update.state.doc.toString(), change.appended, change);
      }),
    ],
  }) });
}
