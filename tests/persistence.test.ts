import { test } from 'node:test';
import assert from 'node:assert/strict';
import { persistPages, type SaveResult } from '../src/lib/persistence.ts';

test('older save completion cannot clean a newer edit or acknowledge its recovery', async () => {
  const page = { dirty: true, revision: 1, recoveredRevision: -1, base: 'base' };
  let release!: (result: SaveResult) => void;
  const seen: number[] = [];
  const saving = persistPages([page], async page => {
    seen.push(page.revision);
    if (seen.length === 1) return new Promise<SaveResult>(resolve => { release = resolve; });
    assert.equal(page.dirty, true);
    assert.equal(page.recoveredRevision, 1);
    assert.equal(page.base, 'first save');
    return { saved: false, recovered: false, base: page.base, cause: 'unavailable', conflicts: [] };
  });
  page.revision++;
  release({ saved: true, recovered: false, base: 'first save', cause: null, conflicts: [] });
  const result = await saving;
  assert.deepEqual(seen, [1, 2]);
  assert.equal(page.dirty, true);
  assert.equal(result.durable, false);
  const retry = await persistPages([page], async () => ({ saved: false, recovered: true, base: page.base, cause: 'journal unavailable', conflicts: [] }));
  assert.equal(retry.durable, true);
  assert.equal(page.recoveredRevision, 2);
  assert.equal(page.dirty, true);
});

test('conflict paths survive a newer revision and a subsequent failed save', async () => {
  const page = { dirty: true, revision: 1, base: 'base' };
  let count = 0;
  const result = await persistPages([page], async () => {
    if (++count === 1) {
      page.revision++;
      return { saved: true, recovered: false, base: 'first', cause: null, conflicts: ['preserved.conflict.md'] };
    }
    return { saved: false, recovered: true, base: 'first', cause: 'journal unavailable', conflicts: [] };
  });
  assert.deepEqual(result.results.flatMap(result => result.conflicts), ['preserved.conflict.md']);
  assert.equal(result.results.at(-1)?.saved, false);
  assert.equal(page.dirty, true);
});
