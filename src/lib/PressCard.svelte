<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { drawCard, cardPng, paperColours } from './press';
  import type { Card } from './press';
  let { card, onclose }: { card: Card; onclose: () => void } = $props();
  let dialog: HTMLDialogElement, sheet: HTMLDivElement;
  let preview = $state('');
  let entering = $state(false);
  let error = $state(''), message = $state(''), busy = $state(false), ready = $state(false);
  let alive = true, revision = 0, png: Promise<number[]> | undefined;
  let colours: ReturnType<typeof paperColours>;
  async function perform(action: 'save' | 'copy') {
    if (busy || !ready) return;
    busy = true; error = ''; message = '';
    try {
      png ??= cardPng(card, colours).catch(error => { png = undefined; throw error; });
      const bytes = await png;
      if (!alive) return;
      const result = await invoke<boolean>('export_card', { bytes, copy: action === 'copy', date: card.date });
      if (alive && result) message = action === 'copy' ? 'Image copied.' : 'Image saved.';
    } catch (cause) { if (alive) error = `Could not ${action === 'copy' ? 'copy' : 'save'} the image. ${String(cause)}`; }
    finally { if (alive) busy = false; }
  }
  function trapTab(event: KeyboardEvent) {
    if (event.key !== 'Tab') return;
    event.preventDefault();
    const buttons = Array.from(dialog.querySelectorAll<HTMLButtonElement>('button:not(:disabled)'));
    const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
    const next = at < 0 ? (event.shiftKey ? buttons.length - 1 : 0) : (at + (event.shiftKey ? buttons.length - 1 : 1)) % buttons.length;
    buttons[next].focus();
  }
  onMount(() => {
    entering = !document.hidden;
    colours = paperColours(getComputedStyle(dialog));
    dialog.showModal();
    const resize = new ResizeObserver(async () => {
      const current = ++revision, width = sheet.clientWidth;
      ready = false;
      try {
        const drawing = document.createElement('canvas');
        await drawCard(drawing, card, colours, width, true);
        if (!alive || current !== revision) return;
        preview = drawing.toDataURL('image/png'); ready = true; error = '';
      } catch (cause) { if (alive && current === revision) error = String(cause); }
    });
    resize.observe(sheet);
    return () => { alive = false; resize.disconnect(); dialog.close(); };
  });
</script>

<svelte:document onvisibilitychange={() => { if (document.hidden) entering = false; }} />
<dialog class="press-dialog" class:entering bind:this={dialog} aria-labelledby="press-title" onkeydown={trapTab} oncancel={event => { event.preventDefault(); onclose(); }}>
  <div class="press-scrim" aria-hidden="true"></div>
  <h2 id="press-title" class="screen-reader">Pressed card</h2>
  <div class="press-sheet" bind:this={sheet} role="img" aria-label={`${card.text} — ${card.archive} / ${card.time}${card.label ? ` / ${card.label}` : ''}${card.shortened ? '. Selection shortened.' : ''}`}>{#if preview}<img src={preview} alt="" />{/if}
  </div>
  <div class="press-actions">
    <button disabled={!ready || busy} onclick={() => perform('save')}>Save Image…</button>
    <button disabled={!ready || busy} onclick={() => perform('copy')}>Copy</button>
    <button onclick={onclose}>Done</button>
  </div>
  <p class="press-status" role="status" aria-live="polite">{error || message || (card.shortened ? 'Selection shortened to fit 160 characters.' : '')}</p>
</dialog>

<style>
  .screen-reader { position:absolute; width:1px; height:1px; padding:0; overflow:hidden; clip-path:inset(50%); white-space:nowrap }
  .press-dialog { inset:0; width:100%; height:100%; max-width:none; max-height:none; margin:0; padding:.25rem; border:0; background:transparent; color:var(--ink) }
  .press-dialog[open] { display:flex; flex-direction:column; align-items:center; justify-content:center }
  .press-dialog::backdrop { background:transparent }
  .press-scrim { position:absolute; inset:0; z-index:-1; background:color-mix(in srgb,var(--paper) 74%,transparent); backdrop-filter:blur(6px); -webkit-backdrop-filter:blur(6px) }
  .press-sheet { width:min(48vh,calc(100vw - 3rem)); height:min(80vh,calc((100vw - 3rem) * 5 / 3)); flex-shrink:0 }
  .entering .press-scrim { animation:press-fade 160ms linear both }
  .entering .press-sheet { animation:press-settle 240ms cubic-bezier(.2,.7,.2,1) both }
  img { display:block; width:100%; height:100% }
  .press-actions { display:flex; gap:1rem; margin-top:.6rem }
  .press-status { margin:.25rem 0 0; min-height:1.4em; max-width:34rem; text-align:center; font:400 13px/1.4 'IBM Plex Mono',Menlo,monospace; color:var(--ink-2) }
  @keyframes press-fade { from { opacity:0 } to { opacity:1 } }
  @keyframes press-settle { from { opacity:0; transform:translateY(6px) scale(.97) } to { opacity:1; transform:none } }
  @media(prefers-reduced-motion:reduce) { .entering .press-sheet { animation:press-fade 160ms linear both } }
  @media(prefers-reduced-transparency:reduce) { .press-scrim { background:var(--paper); backdrop-filter:none; -webkit-backdrop-filter:none } }
</style>
