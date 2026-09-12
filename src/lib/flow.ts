export function historyBatch(dates: string[], boundary: string, direction: -1 | 1): string[] {
  const candidates = dates.filter(date => direction < 0 ? date < boundary : date > boundary);
  return direction < 0 ? candidates.slice(-8) : candidates.slice(0, 8);
}

export function historyGap(dates: string[], loaded: string[]): string[] {
  const present = new Set(loaded);
  return dates.filter(date => date > loaded[0] && date < loaded.at(-1)! && !present.has(date)).slice(0, 8);
}

export function gapNote(from: string, to: string, saved: string[] = []): string {
  if (saved.some(date => date > from && date < to)) return '';
  const days = Math.round((Date.parse(`${to}T12:00:00Z`) - Date.parse(`${from}T12:00:00Z`)) / 86400000) - 1;
  const words = ['a', 'two', 'three', 'four', 'five', 'six'];
  if (days < 1) return '';
  if (days < 7) return `${words[days - 1]} day${days === 1 ? '' : 's'}`;
  if (days < 28) { const weeks = Math.floor(days / 7); return `${words[weeks - 1]} week${weeks === 1 ? '' : 's'}`; }
  return 'some months';
}

export function navigationPlan(distance: number, height: number, today: boolean, reduced: boolean) {
  const fade = today && Math.abs(distance) > 3 * height;
  return { duration: reduced || fade ? 0 : today ? Math.min(480, Math.max(240, 160 + Math.abs(distance) / 6)) : 300, fade };
}

export class Flow {
  private expected: number;
  private height: number;
  private navigation?: AbortController;
  private anchor?: { element: HTMLElement; top: number };
  private resize: ResizeObserver;
  private inputEvents = ['pointerdown', 'touchstart', 'keydown', 'beforeinput'] as const;

  constructor(readonly root: HTMLElement, readonly content: HTMLElement) {
    this.expected = root.scrollTop; this.height = content.getBoundingClientRect().height;
    root.addEventListener('scroll', this.scrolled, { passive: true });
    for (const event of this.inputEvents) root.ownerDocument.addEventListener(event, this.interrupt, { capture: true, passive: true });
    this.resize = new ResizeObserver(() => this.restoreAnchor());
    this.resize.observe(content);
  }
  private scrolled = () => {
    // CodeMirror can adjust scroll while measuring previously off-screen text.
    const height = this.content.getBoundingClientRect().height;
    const delta = this.root.scrollTop - this.expected;
    if (Math.abs(delta) > 1 && Math.abs(height - this.height) <= 1) this.interrupt();
    this.expected = this.root.scrollTop; this.height = height;
  };
  interrupt = () => {
    this.navigation?.abort(); this.navigation = undefined; this.anchor = undefined;
  };
  private position(top: number) {
    this.root.scrollTop = Math.max(0, Math.min(top, this.root.scrollHeight - this.root.clientHeight));
    this.expected = this.root.scrollTop; this.height = this.content.getBoundingClientRect().height;
  }
  private restoreAnchor() {
    if (!this.anchor?.element.isConnected) { this.anchor = undefined; return; }
    this.position(this.root.scrollTop + this.anchor.element.getBoundingClientRect().top - this.anchor.top);
  }
  async preserve(update: () => Promise<void>, prepend: boolean) {
    const viewport = this.root.getBoundingClientRect();
    const element = Array.from(this.content.querySelectorAll<HTMLElement>('.cm-line, .archive')).find(element => {
      const rect = element.getBoundingClientRect(); return rect.bottom > viewport.top && rect.top < viewport.bottom;
    });
    const anchor = element ? { element, top: element.getBoundingClientRect().top } : undefined;
    this.anchor = anchor;
    const before = this.root.scrollHeight, top = this.root.scrollTop;
    await update();
    if (this.anchor !== anchor) return;
    if (prepend) this.position(top + this.root.scrollHeight - before);
    this.restoreAnchor();
  }
  async navigate(prepare: (signal: AbortSignal) => Promise<{ top: () => number; focus: () => void } | undefined>, today: boolean) {
    this.interrupt();
    const controller = new AbortController(); this.navigation = controller;
    const { signal } = controller;
    try {
      const destination = await prepare(signal);
      if (!destination || signal.aborted) return;
      this.anchor = undefined;
      const start = this.root.scrollTop;
      const plan = navigationPlan(destination.top() - start, this.root.clientHeight, today, matchMedia('(prefers-reduced-motion: reduce)').matches);
      const fade = async (from: number, to: number, duration: number) => {
        const animation = this.content.animate([{ opacity: from }, { opacity: to }], { duration, fill: 'forwards', easing: 'linear' });
        const cancel = () => animation.cancel();
        signal.addEventListener('abort', cancel, { once: true });
        try { await animation.finished; this.content.style.opacity = String(to); }
        finally { signal.removeEventListener('abort', cancel); animation.cancel(); }
      };
      if (plan.fade) {
        await fade(1, .35, 120);
        if (signal.aborted) return;
        this.position(destination.top());
        await fade(.35, 1, 220);
      } else if (!plan.duration) this.position(destination.top());
      else await new Promise<void>(resolve => {
        const began = performance.now(); let frame = 0;
        const cancel = () => { cancelAnimationFrame(frame); resolve(); };
        signal.addEventListener('abort', cancel, { once: true });
        const step = (now: number) => {
          const progress = Math.min(1, (now - began) / plan.duration);
          this.position(start + (destination.top() - start) * (1 - (1 - progress) ** 3));
          if (progress < 1) frame = requestAnimationFrame(step);
          else { signal.removeEventListener('abort', cancel); resolve(); }
        };
        frame = requestAnimationFrame(step);
      });
      if (!signal.aborted) { this.position(destination.top()); destination.focus(); }
    } catch (error) { if (!signal.aborted) throw error; }
    finally {
      if (this.navigation === controller) { this.navigation = undefined; this.content.style.opacity = ''; }
      else if (!this.navigation) this.content.style.opacity = '';
    }
  }
  destroy() {
    this.interrupt(); this.resize.disconnect(); this.root.removeEventListener('scroll', this.scrolled);
    for (const event of this.inputEvents) this.root.ownerDocument.removeEventListener(event, this.interrupt, true);
  }
}
