<script lang="ts">
  import { onMount } from 'svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  let { onerror }: { onerror: (message: string) => void } = $props();
  const preference = window.matchMedia('(prefers-color-scheme: dark)');
  let dark = $state(preference.matches), changing = $state(false);

  async function choose(theme: 'light' | 'dark') {
    changing = true;
    try {
      await getCurrentWindow().setTheme(theme);
      dark = theme === 'dark';
      try { localStorage.setItem('appearance', theme); } catch { /* The choice still applies to this session. */ }
    } catch (error) { onerror(`Cannot change appearance. ${String(error)}`); }
    finally { changing = false; }
  }
  onMount(() => {
    const changed = () => { dark = preference.matches; };
    preference.addEventListener('change', changed);
    try {
      const saved = localStorage.getItem('appearance');
      if (saved === 'light' || saved === 'dark') void choose(saved);
    } catch { /* Use the system appearance when preferences are unavailable. */ }
    return () => preference.removeEventListener('change', changed);
  });
</script>

<div class="appearance" role="group" aria-label="Appearance">
  <button aria-label="Light appearance" title="Light appearance" aria-pressed={!dark} disabled={changing} onclick={() => choose('light')}>
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" aria-hidden="true">
      <circle cx="12" cy="12" r="3.5" /><path d="M12 2v2m0 16v2M2 12h2m16 0h2M4.93 4.93l1.42 1.42m11.3 11.3 1.42 1.42M4.93 19.07l1.42-1.42m11.3-11.3 1.42-1.42" />
    </svg>
  </button>
  <button aria-label="Dark appearance" title="Dark appearance" aria-pressed={dark} disabled={changing} onclick={() => choose('dark')}>
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <path d="M20.4 14.3A8.5 8.5 0 0 1 9.7 3.6a8.5 8.5 0 1 0 10.7 10.7Z" />
    </svg>
  </button>
</div>

<style>
  .appearance { position:absolute; top:9px; right:16px; z-index:4; display:flex; gap:2px; padding:3px; border:1px solid var(--rule); border-radius:20px; background:var(--paper) }
  button { display:grid; place-items:center; width:30px; height:26px; padding:0; border-radius:15px; color:var(--ink-2) }
  button:hover { color:var(--ink) }
  button[aria-pressed="true"] { background:var(--paper-2); color:var(--wet) }
</style>
