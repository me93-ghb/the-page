<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen, TauriEvent } from '@tauri-apps/api/event';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { undo, redo } from '@codemirror/commands';
  import { createEditor, editorSnapshot, refreshFolds, pendingEndInput, transferEndInput, templates, insertTemplate, stagePhotograph, photographCopy } from './lib/editor';
  import { Flow, gapNote, historyBatch, historyGap } from './lib/flow';
  import { persistPages } from './lib/persistence';
  import PressCard from './lib/PressCard.svelte';
  import Appearance from './lib/Appearance.svelte';
  import { makeCard } from './lib/press';
  import type { Card } from './lib/press';
  import type { EditorSelection } from '@codemirror/state';
  import type { SaveResult } from './lib/persistence';
  import type { PendingInput, PendingImageData, PendingPhotograph } from './lib/editor';
  import { journalDay, sessionTime, inputPlan, timestamp } from './lib/journal';
  import type { WritingSession } from './lib/journal';
  import type { EditorView } from '@codemirror/view';

  type Page = { id: string; base: string | null; recovered: string | null; pending: PendingImageData[]; pending_input: PendingInput | null; date: string; content: string; sessions: WritingSession[]; created: string | null; last_end_input: string | null; label: string; error: string | null };
  type OpenPage = Page & { dirty: boolean; revision: number; recoveredRevision?: number; editor?: EditorView; labelEditing?: boolean; template?: typeof templates[number] };
  type Transfer = { source: OpenPage; date: string; running: boolean; target?: OpenPage };
  let pages = $state<OpenPage[]>([]), activeDate = $state('');
  let closeDialog = $state<HTMLDialogElement>();
  let loading = $state(true), notice = $state(''), closing = $state(false), now = $state(new Date());
  let focused: OpenPage | undefined;
  let pressed = $state.raw<{ card: Card; editor: EditorView; selection: EditorSelection } | null>(null);
  let pendingImages = $state.raw<{ page: OpenPage; pending: PendingPhotograph }[]>([]);
  let noticeTimer: ReturnType<typeof setTimeout> | undefined;
  let importing = 0;
  const imageReads = new Map<string, Promise<string>>();
  let transfer = $state.raw<Transfer | null>(null);
  let timer: ReturnType<typeof setTimeout> | undefined;
  let saving: Promise<boolean> | undefined;

  let root = $state<HTMLElement>(), content = $state<HTMLElement>(), topSentinel = $state<HTMLElement>(), bottomSentinel = $state<HTMLElement>();
  let flow = $state<Flow>(), savedDates = $state<string[]>([]), indexed = $state(false), batching = $state(false), todayVisible = $state(true);
  const todayDate = $derived(journalDay(now));
  const dates = $derived([...new Set([...savedDates, ...pages.map(page => page.date), todayDate])].sort());
  const older = $derived(pages.length ? historyBatch(savedDates, pages[0].date, -1) : []);
  const middle = $derived(historyGap(savedDates, pages.map(page => page.date)));
  const newer = $derived(pages.length ? historyBatch(savedDates, pages.at(-1)!.date, 1) : []);

  $effect(() => {
    if (!root || !content) return;
    const controller = new Flow(root, content); flow = controller;
    return () => { controller.destroy(); flow = undefined; };
  });
  $effect(() => {
    if (!root || !indexed || batching) return;
    const observer = new IntersectionObserver(entries => {
      for (const entry of entries) if (entry.isIntersecting) void loadHistory(entry.target === topSentinel ? -1 : 1);
    }, { root, rootMargin: '150% 0px 0px 0px' });
    if (topSentinel && older.length) observer.observe(topSentinel);
    if (bottomSentinel && newer.length) observer.observe(bottomSentinel);
    return () => observer.disconnect();
  });
  $effect(() => { if (indexed && !batching && middle.length) void loadHistory(0); });
  $effect(() => {
    pages; todayDate;
    if (!root) return;
    const page = root.querySelector<HTMLElement>(`#page-${todayDate}`);
    if (!page) { todayVisible = false; return; }
    const viewport = root.getBoundingClientRect(), rect = page.getBoundingClientRect();
    todayVisible = rect.bottom > viewport.top && rect.top < viewport.bottom;
    const observer = new IntersectionObserver(entries => { todayVisible = entries[0].isIntersecting; }, { root });
    observer.observe(page); return () => observer.disconnect();
  });
  async function indexPages() {
    savedDates = await invoke<string[]>('list_pages'); indexed = true;
  }
  async function insert(next: Page[]) {
    const prepend = !!pages.length && next.some(page => page.date < pages[0].date);
    const update = async () => {
      const merged = new Map(pages.map(page => [page.date, page]));
      for (const page of next) if (!merged.has(page.date) || merged.get(page.date)!.error) merged.set(page.date, { ...page, dirty: !!page.recovered, revision: 0, recoveredRevision: page.recovered ? 0 : -1 });
      pages = [...merged.values()].sort((a, b) => a.date.localeCompare(b.date));
      await tick();
    };
    if (flow) await flow.preserve(update, prepend); else await update();
  }
  async function loadDates(requested: string[]) {
    const loaded = await invoke<Page[]>('load_pages', { dates: requested });
    savedDates = savedDates.filter(date => !requested.includes(date) || loaded.some(page => page.date === date));
    await insert(loaded);
  }
  async function loadHistory(direction: -1 | 0 | 1) {
    if (batching || !indexed) return;
    const batch = direction < 0 ? older : direction > 0 ? newer : middle;
    if (!batch.length) return;
    batching = true;
    try { await loadDates(batch); }
    catch (error) { notice = `Cannot load journal history. ${String(error)}`; indexed = false; }
    finally { batching = false; }
  }
  async function navigate(command: string) {
    if (pressed) return;
    if (!flow) return;
    if (transfer) { await finishTransfer(); if (transfer) return; }
    try {
      await flow.navigate(async signal => {
        let page: OpenPage | undefined;
        if (command === 'today') {
          page = await getPage(journalDay(new Date()));
          if (signal.aborted) return;
          activeDate = page.date;
        } else {
          const from = focused?.date ?? activeDate, direction = command === 'previous' ? -1 : 1;
          while (!signal.aborted) {
            const date = dates[dates.indexOf(from) + direction];
            if (!date) return;
            page = pages.find(page => page.date === date);
            if (page) break;
            if (date === todayDate) { page = await getPage(date); break; }
            await loadDates([date]);
          }
        }
        if (signal.aborted || !page?.editor) return;
        const editor = page.editor, atEnd = command === 'today';
        return {
          top: () => root!.scrollTop + editor.documentTop - root!.getBoundingClientRect().top + (atEnd ? editor.lineBlockAt(editor.state.doc.length).bottom - root!.clientHeight * .55 : 0),
          focus: () => {
            if (page.error) { notice = page.error; return; }
            editor.focus(); editor.dispatch({ selection: { anchor: atEnd ? editor.state.doc.length : 0 }, scrollIntoView: atEnd }); focused = page;
          },
        };
      }, command === 'today');
    } catch (error) { notice = `Cannot navigate the journal. ${String(error)}`; }
  }

  $effect(() => { activeDate; for (const page of pages) page.editor?.dispatch({ effects: refreshFolds.of(null) }); });

  function dateLabel(page: Page) {
    return new Date(`${page.date}T12:00:00`).toLocaleDateString('en-GB', {
      weekday: 'short', day: 'numeric', month: 'short', ...(page.date.slice(0, 4) !== String(now.getFullYear()) ? { year: 'numeric' } : {})
    });
  }
  function timeLabel(page: Page) {
    const start = page.sessions[0]?.start ?? page.created;
    return start ? sessionTime(start, page.date) : now.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' });
  }
  function refresh(page: OpenPage) {
    const snapshot = editorSnapshot(page.editor!);
    page.content = snapshot.content; page.sessions = snapshot.sessions; page.last_end_input = snapshot.lastEnd;
    page.created ??= snapshot.sessions[0]?.start ?? null;
    page.revision++; page.dirty = snapshot.sessions.length > 0 || !!page.label;
    if (!snapshot.content.trim()) page.template = 'Blank';
  }
  function archiveSpace(node: HTMLElement, page: OpenPage) {
    const resize = new ResizeObserver(() => {
      node.parentElement!.style.setProperty('--archive-height', `${node.offsetHeight}px`);
      page.editor?.requestMeasure();
    });
    resize.observe(node);
    return { destroy() { resize.disconnect(); } };
  }
  function attach(host: HTMLDivElement, page: OpenPage) {
    page.editor = createEditor(host, page.content, dateLabel(page), !!page.error, (_text, _appended, change) => {
      refresh(page);
      if (transfer?.source === page && !change.pending) { transfer = null; notice = ''; }
      if (change.pending && !transfer) {
        transfer = { source: page, date: change.pending.batch.date, running: false };
        queueMicrotask(() => void finishTransfer());
      }
      clearTimeout(timer); timer = setTimeout(() => void save(), 1000);
    }, { date: page.date, sessions: page.sessions, lastEnd: page.last_end_input, pending: page.pending_input,
      active: () => page.date === activeDate && transfer?.source !== page,
      focusTemplates: () => host.nextElementSibling?.querySelector<HTMLButtonElement>('[tabindex="0"]')?.focus(),
      photograph: file => void importPhotograph(page, file),
      readPhotograph: path => {
        const key = `${page.date}/${path}`;
        if (!imageReads.has(key)) imageReads.set(key, invoke<string>('read_photograph', { date: page.date, path }).catch(error => { imageReads.delete(key); throw error; }));
        return imageReads.get(key)!;
      } });
    const focus = () => { focused = page; };
    const compositionEnd = () => { setTimeout(() => { if (transfer?.source === page) void finishTransfer(); }, 30); };
    for (const image of page.pending ?? []) {
      page.editor.dispatch({ selection: { anchor: image.from, head: image.to } });
      const pending = stagePhotograph(page.editor, new Uint8Array(image.bytes), bytes => invoke<string>('store_photograph', { date: page.date, bytes: Array.from(bytes) }), image.at, image.corrected);
      pendingImages = [...pendingImages, { page, pending }];
    }
    page.pending = [];
    if (page.pending_input) {
      transfer = { source: page, date: page.pending_input.batch.date, running: false };
      page.pending_input = null;
      queueMicrotask(() => void finishTransfer());
    }
    host.addEventListener('focusin', focus); host.addEventListener('compositionend', compositionEnd);
    return { destroy() { host.removeEventListener('focusin', focus); host.removeEventListener('compositionend', compositionEnd); page.editor?.destroy(); page.editor = undefined; } };
  }
  async function importPhotograph(page: OpenPage, file: File) {
    if (page.error || !page.editor) return;
    importing++;
    let staged: PendingPhotograph | undefined;
    try {
      if (file.size > 32 * 1024 * 1024) throw new Error('Photographs must be at most 32 MB.');
      const at = new Date(), selection = page.editor.state.selection.main;
      const plan = inputPlan(page.date, page.last_end_input, at,
        page.date === activeDate && selection.empty && !page.editor.state.doc.sliceString(selection.head).trim());
      if (plan.kind === 'rollover') {
        page = await getPage(plan.date!);
        if (page.error) throw new Error(page.error);
        activeDate = page.date; focusEnd(page);
      }
      const pending = stagePhotograph(page.editor!, file.arrayBuffer().then(buffer => new Uint8Array(buffer)),
        bytes => invoke<string>('store_photograph', { date: page.date, bytes: Array.from(bytes) }), timestamp(at));
      staged = pending;
      pendingImages = [...pendingImages, { page, pending }];
      await pending.retry();
      pendingImages = pendingImages.filter(item => item.pending !== pending);
    } catch (error) {
      if (staged && String(error).startsWith('Unsupported photograph.')) {
        staged.discard(); pendingImages = pendingImages.filter(item => item.pending !== staged);
      }
      notice = `Photograph not inserted. ${String(error)} Your writing remains here.${pendingImages.some(image => image.pending.bytes) ? ' Pending image bytes remain in this window for retry.' : ''}`;
    } finally {
      importing--;
      if (pendingImages.some(item => item.page === page)) {
        page.dirty = true; page.revision++;
        clearTimeout(timer); timer = setTimeout(() => void save(), 1000);
      }
    }
  }
  async function retryImages() {
    for (const { page, pending } of pendingImages) {
      try { await pending.retry(); pendingImages = pendingImages.filter(item => item.pending !== pending); }
      catch (error) {
        if (String(error).startsWith('Unsupported photograph.')) { pending.discard(); pendingImages = pendingImages.filter(item => item.pending !== pending); }
        page.dirty = true;
      }
    }
  }
  function editLabel(page = focused ?? pages.find(page => page.date === activeDate)) {
    if (pressed) return;
    if (!page || page.error) return;
    page.labelEditing = true;
  }
  function focusLabel(input: HTMLInputElement) { input.focus(); input.select(); }
  function updateLabel(page: OpenPage, input: HTMLInputElement) {
    const label = input.value.trim();
    if (label === page.label) return;
    page.label = label; page.revision++; page.dirty = true;
    clearTimeout(timer); timer = setTimeout(() => void save(), 1000);
  }
  function labelKey(event: KeyboardEvent, page: OpenPage) {
    if (event.isComposing || !['Enter', 'Escape'].includes(event.key)) return;
    event.preventDefault(); updateLabel(page, event.currentTarget as HTMLInputElement);
    page.labelEditing = false; page.editor?.focus();
  }
  function chooseTemplate(page: OpenPage, name: typeof templates[number]) {
    page.template = name; insertTemplate(page.editor!, name);
  }
  function templateKey(event: KeyboardEvent, page: OpenPage) {
    const button = event.currentTarget as HTMLButtonElement;
    if (['ArrowLeft', 'ArrowRight'].includes(event.key)) {
      event.preventDefault();
      const buttons = Array.from(button.parentElement!.querySelectorAll('button'));
      const index = (buttons.indexOf(button) + (event.key === 'ArrowLeft' ? -1 : 1) + buttons.length) % buttons.length;
      buttons.forEach((item, i) => item.tabIndex = i === index ? 0 : -1); buttons[index].focus();
    } else if (event.key === 'Escape') { event.preventDefault(); page.editor?.focus(); }
  }
  async function put(next: Page): Promise<OpenPage> {
    await insert([next]);
    return pages.find(page => page.date === next.date)!;
  }
  async function getPage(date: string): Promise<OpenPage> {
    const cached = pages.find(page => page.date === date && !page.error);
    if (cached) return cached;
    const next = await invoke<Page | null>('open_page', { date });
    if (!next) throw new Error('Choose a journal folder first.');
    return put(next);
  }
  function focusEnd(page: OpenPage) {
    if (!page.error && page.editor) {
      page.editor.focus(); page.editor.dispatch({ selection: { anchor: page.editor.state.doc.length }, scrollIntoView: true });
    }
  }
  async function finishTransfer() {
    const pending = transfer;
    if (!pending || pending.running) return;
    pending.running = true;
    try {
      pending.target ??= await getPage(pending.date);
      if (transfer !== pending) return;
      if (pending.target.error) { pending.target = undefined; throw new Error('The new day’s page is not writable. Correct its file, then try again.'); }
      if (pending.source.editor?.compositionStarted) return;
      activeDate = pending.target.date;
      transferEndInput(pending.source.editor!, pending.target.editor!);
      refresh(pending.target);
      const target = pending.target;
      transfer = null; notice = ''; focusEnd(target);
      void save();
    } catch (error) {
      if (transfer !== pending) return;
      notice = `Cannot continue on the new day. ${String(error)} Your writing remains in this window. Keep it open and try again.`;
    } finally { pending.running = false; }
  }
  async function open(choose = false) {
    if (transfer) { await finishTransfer(); return; }
    loading = true;
    try {
      if (choose) {
        const next = await invoke<Page | null>('choose_folder');
        if (!next) return;
        pages = []; savedDates = []; indexed = false; await tick();
        activeDate = next.date; const page = await put(next); notice = page.error ?? ''; focusEnd(page);
      } else {
        const next = await invoke<Page | null>('open_page');
        if (next) { activeDate = next.date; const page = await put(next); notice = page.error ?? ''; focusEnd(page); }
      }
      if (pages.length) {
        const restored = await invoke<Page[]>('recovered_pages');
        await insert(restored);
        if (restored.length) {
          notice = `Recovered writing from ${restored.at(-1)!.recovered!.slice(11, 16)}.`;
          clearTimeout(timer); timer = setTimeout(() => void save(), 1000);
        }
        await indexPages();
      }
    } catch (error) { notice = `Cannot open the journal. ${String(error)}`; }
    finally { loading = false; }
  }
  function save(): Promise<boolean> {
    clearTimeout(timer);
    if (saving) return saving;
    saving = (async () => {
      await retryImages();
      const hadFailure = !!notice;
      const { durable, results } = await persistPages(pages, async page => {
        try {
          const drafts = pages.filter(page => page.dirty).map(page => ({ date: page.date, id: page.id, base: page.base, sessions: page.sessions, lastEnd: page.last_end_input, label: page.label,
            pendingInput: page.editor ? pendingEndInput(page.editor) : page.pending_input,
            pending: pendingImages.filter(item => item.page === page).flatMap(item => { const data = item.pending.snapshot(); return data ? [data] : []; }) }));
          const draft = drafts.find(draft => draft.date === page.date)!;
          // Checkpoint every dirty page before clearing a source page's recovery after rollover.
          await invoke('checkpoint_pages', { drafts });
          return await invoke<SaveResult>('save_page', draft);
        } catch (error) { return { saved: false, recovered: false, base: page.base, cause: String(error), conflicts: [] }; }
      });
      for (const page of pages) if (!page.dirty) { page.recovered = null; if (page.base && !savedDates.includes(page.date)) savedDates = [...savedDates, page.date].sort(); }
      const failures = results.filter(result => !result.saved), conflicts = results.flatMap(result => result.conflicts);
      if (results.length) clearTimeout(noticeTimer);
      if (failures.length) {
        const memory = failures.some(result => !result.recovered);
        notice = `Not saved. ${failures.map(result => result.cause).filter(Boolean).join(' ')} ${memory ? 'Some writing exists only in this window. Keep it open until saving succeeds.' : 'A recovery copy exists on this Mac. The Page will retry while this window is open.'}`;
        const recovered = pages.find(page => page.recovered)?.recovered;
        if (recovered) notice = `Recovered writing from ${recovered.slice(11, 16)}. ${notice}`;
        if (conflicts.length) notice += ` External versions were preserved at ${conflicts.join(', ')}.`;
      } else if (conflicts.length) notice = `Saved your writing. External versions were preserved at ${conflicts.join(', ')}.`;
      else if (results.length && hadFailure) {
        notice = `Saved to the journal, ${new Date().toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' })}.`;
        const acknowledgement = notice;
        noticeTimer = setTimeout(() => { if (notice === acknowledgement) notice = ''; }, 2600);
      }
      return durable && !importing;
    })().finally(() => { saving = undefined; });
    return saving;
  }
  async function saveCopy() {
    if (pressed) return;
    const page = focused ?? pages.find(page => page.date === activeDate);
    if (!page || page.error) return;
    try {
      await retryImages();
      const images = pendingImages.filter(item => item.page === page).map(item => ({ ...item, path: `${page.date}/pending-${crypto.randomUUID()}.png` }));
      if (images.some(image => !image.pending.bytes) || importing) throw new Error('The photograph is still being read. Try again in a moment.');
      const snapshot = photographCopy(page.editor!, images);
      if (await invoke<boolean>('save_copy', { date: page.date, id: page.id, base: page.base, sessions: snapshot.sessions, lastEnd: snapshot.lastEnd, label: page.label,
        assets: images.map(image => ({ path: image.path, bytes: Array.from(image.pending.bytes!) })) })) notice = 'Saved a copy. Your journal and recovery state are unchanged.';
    } catch (error) { notice = `Cannot save a copy. ${String(error)}`; }
  }
  async function refreshCleanPages() {
    const clean = pages.filter(page => !page.dirty).map(page => ({ page, revision: page.revision }));
    if (!clean.length) return;
    const refreshed = await invoke<Page[]>('refresh_pages', { dates: clean.map(item => item.page.date) });
    for (const next of refreshed) {
      const item = clean.find(item => item.page.date === next.date)!;
      if (item.page.dirty || item.page.revision !== item.revision || item.page.base === next.base && item.page.error === next.error) continue;
      imageReads.clear();
      pages = pages.map(page => page === item.page ? { ...next, dirty: false, revision: 0 } : page);
      if (focused === item.page) focused = undefined;
    }
    await tick();
  }
  async function retrySave() {
    if (!pages.length) { await open(); return; }
    if (transfer) await finishTransfer();
    await save();
    try { await refreshCleanPages(); if (!indexed) await indexPages(); }
    catch (error) { notice = `Cannot check the journal. ${String(error)}`; }
  }
  function pressLine() {
    if (pressed || closing || document.activeElement instanceof HTMLInputElement) return;
    const page = focused, editor = page?.editor;
    if (!page || !editor || page.error || editor.state.selection.main.empty) return;
    try {
      const selection = editor.state.selection, range = selection.main;
      const snapshot = editorSnapshot(editor);
      const sections = snapshot.sessions.length ? snapshot.sessions.map(session => session.content) : [snapshot.content];
      const card = makeCard(sections, { date: page.date, start: page.sessions[0]?.start ?? page.created ?? '', label: page.label }, range);
      if (card) { flow?.interrupt(); pressed = { card, editor, selection }; }
    } catch (error) { notice = `Cannot press this selection. ${String(error)}`; }
  }
  async function closePress() {
    const previous = pressed; pressed = null;
    await tick();
    if (previous?.editor.dom.isConnected) {
      previous.editor.focus(); previous.editor.dispatch({ selection: previous.selection });
    }
  }
  async function requestExit() {
    if (transfer) await finishTransfer();
    if (await save()) await invoke('exit_now'); else closing = true;
  }
  $effect(() => { if (closing) closeDialog?.showModal(); else closeDialog?.close(); });
  async function quitWithoutRetrying() { await invoke('exit_now'); }

  onMount(() => {
    const activate = async () => {
      if (loading || closing || pressed || !pages.length) return;
      flow?.interrupt();
      try { await save(); await refreshCleanPages(); }
      catch (error) { notice = `Cannot check external changes. ${String(error)}`; }
      if (journalDay(new Date()) !== activeDate) void open();
      else { const page = pages.find(page => page.date === activeDate); if (page) focusEnd(page); }
    };
    const listeners = [
      listen<string>('edit-requested', event => {
        if (pressed) return;
        if (document.activeElement instanceof HTMLInputElement) document.execCommand(event.payload);
        else if (focused?.editor) (event.payload === 'undo' ? undo : redo)(focused.editor);
      }),
      listen('press-requested', pressLine),
      listen('label-requested', () => editLabel()),
      listen('save-requested', () => void save()),
      listen('save-copy-requested', () => void saveCopy()),
      getCurrentWindow().listen(TauriEvent.WINDOW_FOCUS, activate),
      listen<string>('navigate-requested', event => void navigate(event.payload)),
      listen('exit-requested', () => void requestExit()),
      listen<string>('native-error', event => { notice = event.payload; }),
    ];
    const retry = setInterval(() => { if (pages.some(page => page.dirty) || pendingImages.length) void save(); }, 30000);
    const hidden = () => { if (document.hidden) void save(); };
    document.addEventListener('visibilitychange', hidden);
    window.addEventListener('blur', save);
    const clock = setInterval(() => now = new Date(), 1000);
    const keydown = (event: KeyboardEvent) => {
      if (pressed) return;
      if ((event.metaKey || event.ctrlKey) && event.shiftKey && event.key.toLowerCase() === 'p') { event.preventDefault(); event.stopPropagation(); pressLine(); return; }
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 't') { event.preventDefault(); event.stopPropagation(); void navigate('today'); return; }
      if ((event.metaKey || event.ctrlKey) && event.altKey && ['ArrowUp', 'ArrowDown'].includes(event.key)) { event.preventDefault(); event.stopPropagation(); void navigate(event.key === 'ArrowUp' ? 'previous' : 'next'); return; }
      if ((event.metaKey || event.ctrlKey) && event.shiftKey && event.key.toLowerCase() === 'l') { event.preventDefault(); editLabel(); return; }
      if (focused?.editor?.hasFocus && (event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'z') { event.preventDefault(); (event.shiftKey ? redo : undo)(focused.editor); return; }
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 's') { event.preventDefault(); void save(); }
    };
    window.addEventListener('keydown', keydown, true);
    Promise.all(listeners).then(() => open());
    return () => { clearInterval(retry); clearTimeout(noticeTimer); document.removeEventListener('visibilitychange', hidden); window.removeEventListener('blur', save); clearInterval(clock); clearTimeout(timer); window.removeEventListener('keydown', keydown, true); listeners.forEach(p => p.then(unlisten => unlisten())); };
  });
</script>

{#if pressed}<PressCard card={pressed.card} onclose={closePress} />{/if}
<div class="sheet">
  <div class="titlebar" data-tauri-drag-region></div>
  <Appearance onerror={message => { notice = message; }} />
  {#if pages.length}
    <main class="flow" aria-label="Journal" bind:this={root}
      onscroll={event => event.currentTarget.parentElement!.style.setProperty('--paper-offset', `${-event.currentTarget.scrollTop}px`)}>
      <div class="flow-content" bind:this={content}>
      {#if indexed && !older.length}
        <div class="first-page"><span>THE FIRST PAGE</span><time>{dateLabel(pages[0])}</time></div>
      {:else}<div class="sentinel" bind:this={topSentinel} aria-hidden="true"></div>{/if}
      {#each pages as page, i (page.date)}
        {@const gap = i > 0 ? gapNote(pages[i - 1].date, page.date, savedDates) : ''}
        <article class="page" class:after-page={i > 0} class:after-gap={!!gap} id={`page-${page.date}`}>
          {#if gap}<p class="gap-note">{gap}</p>{/if}
          <div class="archive" aria-label={`Page dated ${dateLabel(page)}`} use:archiveSpace={page}>
            <span>{dateLabel(page)}</span><span class="slash" aria-hidden="true"> / </span>
            <time class:current={!page.error && page.date === activeDate && page.sessions.length <= 1}>{timeLabel(page)}</time>
            {#if page.label}<span class="slash" aria-hidden="true"> / </span>{/if}
            {#if page.labelEditing}
              <input class="page-label" aria-label="Page label" value={page.label} use:focusLabel
                oninput={event => { if (!(event instanceof InputEvent && event.isComposing)) updateLabel(page, event.currentTarget); }}
                oncompositionend={event => updateLabel(page, event.currentTarget)}
                onkeydown={event => labelKey(event, page)}
                onblur={event => { updateLabel(page, event.currentTarget); page.labelEditing = false; }} />
            {:else}
              <button class="label-trigger" aria-label={page.label ? `Edit label: ${page.label}` : 'Label this page'} disabled={!!page.error} onclick={() => editLabel(page)}>{page.label || ' '}</button>
            {/if}
          </div>
          {#key page}<div class="writing" class:blank={!page.content.trim()} use:attach={page}></div>{/key}
          {#if !page.content.trim() && !page.error}
            <nav class="templates" aria-label="Begin with">
              {#each templates as name}
                <button class:chosen={(page.template ?? 'Blank') === name} aria-current={(page.template ?? 'Blank') === name ? 'true' : undefined}
                  tabindex={name === (page.template ?? 'Blank') ? 0 : -1} onkeydown={event => templateKey(event, page)} onclick={() => chooseTemplate(page, name)}>{name}</button>
              {/each}
            </nav>
          {/if}
        </article>
      {/each}
      {#if newer.length}<div class="sentinel" bind:this={bottomSentinel} aria-hidden="true"></div>{/if}
      </div>
    </main>
    {#if !todayVisible}<button class="today" onclick={() => navigate('today')}>Today</button>{/if}
  {:else}
    <main class="welcome">
      <p class="eyebrow">THE PAGE</p>
      <h1>A place for your words.</h1>
      <p>Choose a folder for your journal.<br />Your pages live in the folder you choose.</p>
      <button onclick={() => open(true)} disabled={loading}>Choose journal folder…</button>
    </main>
  {/if}
  {#if notice}
    <aside class="notice" role="status" aria-live="polite">
      <p>{notice}</p>
      <button onclick={retrySave} disabled={loading}>Try again</button>
      {#if pages.length}<button onclick={saveCopy}>Save a copy…</button>{/if}
      {#if !pages.length}<button onclick={() => open(true)} disabled={loading}>Choose folder…</button>{/if}
    </aside>
  {/if}
  <dialog bind:this={closeDialog} oncancel={() => { closing = false; }} aria-labelledby="close-title" class="close-dialog">
    <h2 id="close-title">Saving could not be confirmed.</h2>
    <p>Your writing is still available in this window. If you quit, any changes that did not reach disk will be lost.</p>
    <button onclick={() => { closing = false; focused?.editor?.focus(); }}>Keep writing</button>
    <button onclick={quitWithoutRetrying}>Quit without retrying</button>
  </dialog>
</div>
