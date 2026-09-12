<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { undo, redo } from '@codemirror/commands';
  import { createEditor } from './lib/editor';
  import type { EditorView } from '@codemirror/view';

  type Page = { date: string; content: string; created: string | null; last_end_input: string | null; label: string; error: string | null };
  let page = $state<Page | null>(null);
  let host = $state<HTMLDivElement>();
  let closeDialog = $state<HTMLDialogElement>();
  let editor: EditorView | undefined;
  let loading = $state(true), notice = $state(''), closing = $state(false);
  let dirty = $state(false), now = $state(new Date());
  let revision = 0, content = '', started = '', lastEnd = '';
  let timer: ReturnType<typeof setTimeout> | undefined;
  let saving: Promise<boolean> | undefined;

  function timestamp(date = new Date()) {
    const offset = -date.getTimezoneOffset(), sign = offset < 0 ? '-' : '+';
    const local = new Date(date.getTime() + offset * 60000).toISOString().slice(0, 23);
    return `${local}${sign}${String(Math.floor(Math.abs(offset) / 60)).padStart(2, '0')}:${String(Math.abs(offset) % 60).padStart(2, '0')}`;
  }
  const dateLabel = $derived(page ? new Date(`${page.date}T12:00:00`).toLocaleDateString('en-GB', {
    weekday: 'short', day: 'numeric', month: 'short', ...(page.date.slice(0, 4) !== String(now.getFullYear()) ? { year: 'numeric' } : {})
  }) : '');
  const timeLabel = $derived(page?.created ? page.created.slice(11, 16) : now.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' }));

  async function display(next: Page | null) {
    editor?.destroy(); editor = undefined; page = next;
    notice = next?.error ?? ''; dirty = false; revision = 0;
    content = next?.content ?? ''; started = next?.created ?? ''; lastEnd = next?.last_end_input ?? started;
    await tick();
    if (next && host) {
      editor = createEditor(host, content, dateLabel, !!next.error, (text, appended) => {
        content = text; revision++; dirty = true;
        if (!started) { started = timestamp(); lastEnd = started; page!.created = started; }
        if (appended) lastEnd = timestamp();
        clearTimeout(timer); timer = setTimeout(() => void save(), 1000);
      });
      if (!next.error) { editor.focus(); editor.dispatch({ selection: { anchor: content.length }, scrollIntoView: true }); }
    }
  }
  async function open(choose = false) {
    if (dirty) return;
    loading = true;
    try {
      const next = await invoke<Page | null>(choose ? 'choose_folder' : 'open_page');
      if (!choose || next) await display(next);
    } catch (error) { notice = `Cannot open the journal. ${String(error)}`; }
    finally { loading = false; }
  }
  function save(): Promise<boolean> {
    clearTimeout(timer);
    if (saving) return saving;
    saving = (async () => {
      while (dirty && !page?.error) {
        const sentRevision = revision;
        try {
          await invoke('save_page', { content, started, lastEnd });
          if (revision === sentRevision) dirty = false;
          notice = '';
        } catch (error) {
          notice = `Save not confirmed. ${String(error)} Your latest writing remains in this window. Keep it open until saving succeeds.`;
          return false;
        }
      }
      return !dirty;
    })().finally(() => { saving = undefined; });
    return saving;
  }
  async function requestExit() {
    if (await save()) await invoke('exit_now');
    else closing = true;
  }
  $effect(() => { if (closing) closeDialog?.showModal(); else closeDialog?.close(); });
  async function quitWithoutRetrying() { await invoke('exit_now'); }

  onMount(() => {
    const listeners = [
      listen<string>('edit-requested', event => { if (editor) (event.payload === 'undo' ? undo : redo)(editor); }),
      listen('save-requested', () => void save()),
      listen('exit-requested', () => void requestExit()),
      listen<string>('native-error', event => { notice = event.payload; }),
    ];
    const clock = setInterval(() => now = new Date(), 1000);
    const keydown = (event: KeyboardEvent) => {
      if (editor && (event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'z') { event.preventDefault(); (event.shiftKey ? redo : undo)(editor); return; }
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 's') { event.preventDefault(); void save(); }
    };
    window.addEventListener('keydown', keydown, true);
    Promise.all(listeners).then(() => open());
    return () => { clearInterval(clock); clearTimeout(timer); editor?.destroy(); window.removeEventListener('keydown', keydown, true); listeners.forEach(p => p.then(unlisten => unlisten())); };
  });
</script>

<div class="sheet">
  <div class="titlebar" data-tauri-drag-region></div>
  {#if page}
    <main class="flow" aria-label="Journal">
      <article class="page">
        <div class="archive" aria-label={`Page dated ${dateLabel}`}>
          <span>{dateLabel}</span><span class="slash" aria-hidden="true"> / </span>
          <time class:current={!page.error}>{timeLabel}</time>
          {#if page.label}<span class="slash" aria-hidden="true"> / </span><span>{page.label}</span>{/if}
        </div>
        <div class="writing" bind:this={host}></div>
      </article>
    </main>
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
      <button onclick={() => dirty ? save() : open()} disabled={loading}>Try again</button>
      {#if !dirty && !page}<button onclick={() => open(true)} disabled={loading}>Choose folder…</button>{/if}
    </aside>
  {/if}
  <dialog bind:this={closeDialog} oncancel={() => { closing = false; }} aria-labelledby="close-title" class="close-dialog">
        <h2 id="close-title">Saving could not be confirmed.</h2>
        <p>Your writing is still available in this window. If you quit, any changes that did not reach disk will be lost.</p>
        <button onclick={() => { closing = false; editor?.focus(); }}>Keep writing</button>
        <button onclick={quitWithoutRetrying}>Quit without retrying</button>
  </dialog>
</div>
