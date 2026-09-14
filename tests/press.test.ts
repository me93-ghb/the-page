import { test } from 'node:test';
import assert from 'node:assert/strict';
import { makeCard, breakLine, wrapLine } from '../src/lib/press.ts';

const archive = { date: '2026-09-12', start: '2026-09-12T08:13:00+01:00', label: 'quiet evening' };
test('press captures only selected writing and visible archive fields without mutating its source', () => {
  const source = { ...archive, content: 'secret before / selected writing / secret after', updated: 'unchanged', last_end_input: 'unchanged' };
  const before = structuredClone(source);
  const card = makeCard(['selected writing'], source)!;
  assert.equal(card.text, 'selected writing');
  assert.equal(makeCard(['**selected** [writing](https://private.example/secret) ![photo](private/image.png)'], source)!.text, 'selected writing');
  assert.equal(card.archive, 'Sat 12 Sep 2026');
  assert.equal(card.time, '08:13');
  assert.equal(card.label, 'quiet evening');
  assert.deepEqual(source, before);
  assert.doesNotMatch(JSON.stringify(card), /secret|updated|last_end_input|content/);
  assert.equal(makeCard(['  '], source), null);
  assert.deepEqual([12, 13, 14].map(day => makeCard(['writing'], { ...archive, date: `2026-09-${day}` })!.layout), [0, 1, 2]);
});
test('shortening retains complete words and Unicode graphemes, with a visible flag', () => {
  const text = 'A family 👨‍👩‍👧‍👦 remembers the café together. '.repeat(8);
  const card = makeCard([text], archive)!;
  assert.equal(card.shortened, true);
  assert.ok([...new Intl.Segmenter().segment(card.text)].length <= 160);
  assert.ok(text.startsWith(card.text.slice(0, -1)));
  assert.match(card.text, /…$/);
  assert.doesNotMatch(card.text, /\uFFFD/);
  assert.equal(makeCard(['A family 👨‍👩‍👧‍👦 remembers.'], archive)!.shortened, false);
});
test('a semantic break retains punctuation, and wrapping never loses long words', () => {
  assert.deepEqual(breakLine('Some days arrive quietly, and leave a little light behind.'), ['Some days arrive quietly,', 'and leave a little light behind.']);
  assert.deepEqual(breakLine('Wait, here comes another day.'), ['Wait, here comes another day.']);
  const lines = wrapLine('aVeryLongWord 👨‍👩‍👧‍👦 remains', s => [...new Intl.Segmenter().segment(s)].length, 5);
  assert.equal(lines.join('').replaceAll(' ', ''), 'aVeryLongWord👨‍👩‍👧‍👦remains');
  assert.ok(lines.every(line => [...new Intl.Segmenter().segment(line)].length <= 5));
});
test('partial selections retain session context while exporting only selected writing', () => {
  const source = 'Before **some days arrive quietly** after.';
  assert.equal(makeCard([source], archive, { from: 0, to: source.indexOf(' days') })!.text, 'Before some');
  assert.equal(makeCard([source], archive, { from: source.indexOf('quietly'), to: source.length })!.text, 'quietly after.');
  assert.equal(makeCard([source], archive, { from: source.indexOf('some'), to: source.indexOf(' quietly') })!.text, 'some days arrive');
  assert.equal(makeCard([source], archive, { from: 7, to: 9 }), null);
  assert.equal(makeCard(['Before **literal', 'next** after.'], archive)!.text, 'Before **literal next** after.');
  const before = 'Unselected secret';
  assert.equal(makeCard([before, source, 'Another secret'], archive, { from: before.length, to: before.length + source.indexOf(' days') })!.text, 'Before some');
  const link = 'Before [selected words](https://private.example/secret) after.';
  const url = link.indexOf('https');
  assert.equal(makeCard([link], archive, { from: 0, to: url + 10 })!.text, 'Before selected words');
  assert.equal(makeCard([link], archive, { from: url, to: url + 10 }), null);
  const escaped = String.raw`Before \*literal* after.`;
  const star = escaped.indexOf('*');
  assert.equal(makeCard([escaped], archive, { from: star, to: star + 8 })!.text, '*literal');
});
