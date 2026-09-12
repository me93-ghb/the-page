export type WritingSession = { start: string; previous_end: string | null; content: string };

export function journalDay(now: Date): string {
  const day = new Date(now.getFullYear(), now.getMonth(), now.getDate() - (now.getHours() < 4 ? 1 : 0));
  return `${day.getFullYear()}-${String(day.getMonth() + 1).padStart(2, '0')}-${String(day.getDate()).padStart(2, '0')}`;
}

export function timestamp(date = new Date()): string {
  const offset = -date.getTimezoneOffset(), sign = offset < 0 ? '-' : '+';
  const local = new Date(date.getTime() + offset * 60000).toISOString().slice(0, 23);
  return `${local}${sign}${String(Math.floor(Math.abs(offset) / 60)).padStart(2, '0')}:${String(Math.abs(offset) % 60).padStart(2, '0')}`;
}

export function inputPlan(date: string, lastEnd: string | null, now: Date, active: boolean): { kind: 'continue' | 'fold' | 'rollover'; date?: string } {
  if (!active) return { kind: 'continue' };
  const paused = !lastEnd || Math.max(0, now.getTime() - Date.parse(lastEnd)) >= 30 * 60000;
  if (paused && journalDay(now) !== date) return { kind: 'rollover', date: journalDay(now) };
  return { kind: lastEnd && paused ? 'fold' : 'continue' };
}

export function foldTier(pause: number): { height: number; depth: number } {
  return pause < 2 * 3600000 ? { height: 4, depth: .55 } : pause < 8 * 3600000 ? { height: 5.5, depth: .8 } : { height: 7.5, depth: 1 };
}

export function sessionTime(start: string, date: string): string {
  return `${start.slice(0, 10) > date ? `${start.slice(0, 10)} ` : ''}${start.slice(11, 16)}`;
}
