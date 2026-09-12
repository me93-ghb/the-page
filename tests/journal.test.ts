import { test } from 'node:test';
import assert from 'node:assert/strict';
import { journalDay, inputPlan, foldTier, timestamp } from '../src/lib/journal.ts';

test('pause and depth boundaries use elapsed instants, clamped at zero', () => {
  const end = '2026-09-12T10:00:00+01:00';
  const plan = (at: string, active = true) => inputPlan('2026-09-12', end, new Date(at), active);
  assert.equal(plan('2026-09-12T10:29:59+01:00').kind, 'continue');
  assert.equal(plan('2026-09-12T10:30:00+01:00').kind, 'fold');
  assert.equal(plan('2026-09-12T09:00:00+01:00').kind, 'continue');
  assert.equal(plan('2026-09-13T12:00:00+01:00', false).kind, 'continue');
  assert.deepEqual([0, 7199999, 7200000, 28799999, 28800000].map(ms => foldTier(ms)),
    [{ height: 4, depth: .55 }, { height: 4, depth: .55 }, { height: 5.5, depth: .8 }, { height: 5.5, depth: .8 }, { height: 7.5, depth: 1 }]);
});

test('calendar days, midnight, DST and travel preserve continuous writing until a pause', () => {
  const original = process.env.TZ;
  try {
    process.env.TZ = 'Europe/London';
    for (const [at, day] of [
      ['2026-09-13T00:00:00+01:00', '2026-09-12'],
      ['2026-09-13T03:59:59+01:00', '2026-09-12'],
      ['2026-09-13T04:00:00+01:00', '2026-09-13'],
      ['2026-03-29T03:59:59+01:00', '2026-03-28'],
      ['2026-03-29T04:00:00+01:00', '2026-03-29'],
      ['2026-10-25T03:59:59+00:00', '2026-10-24'],
      ['2026-10-25T04:00:00+00:00', '2026-10-25'],
    ]) assert.equal(journalDay(new Date(at)), day);
    assert.equal(inputPlan('2026-09-12', '2026-09-12T23:00:00+01:00', new Date('2026-09-13T00:00:00+01:00'), true).kind, 'fold');
    assert.equal(inputPlan('2026-09-12', '2026-09-13T03:59:59+01:00', new Date('2026-09-13T04:00:00+01:00'), true).kind, 'continue');
    assert.deepEqual(inputPlan('2026-09-12', '2026-09-13T04:10:00+01:00', new Date('2026-09-13T04:40:00+01:00'), true), { kind: 'rollover', date: '2026-09-13' });
    process.env.TZ = 'America/Los_Angeles';
    const arrival = new Date('2026-09-13T04:00:00+01:00');
    assert.equal(journalDay(arrival), '2026-09-12');
    assert.equal(timestamp(arrival), '2026-09-12T20:00:00.000-07:00');
    assert.deepEqual(inputPlan('2026-09-13', '2026-09-13T02:00:00+01:00', arrival, true), { kind: 'rollover', date: '2026-09-12' });
  } finally { if (original === undefined) delete process.env.TZ; else process.env.TZ = original; }
});
