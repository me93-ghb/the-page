import { markdownLanguage } from '@codemirror/lang-markdown';

export type Card = { text: string; shortened: boolean; date: string; day: number; layout: number; archive: string; time: string; label: string };
export type Paper = { paper: string; ink: string; secondary: string; cobalt: string; stamp: string; onStamp: string };
const graphemes = (text: string) => Array.from(new Intl.Segmenter().segment(text), part => part.segment);

function plainWriting(sections: readonly string[], selection: { from: number; to: number }) {
  let offset = 0;
  return sections.map(section => {
    const from = Math.max(0, selection.from - offset), to = Math.min(section.length, selection.to - offset);
    offset += section.length;
    if (from >= to) return '';
    const cuts: { from: number; to: number }[] = [];
    const cut = (start: number, end: number) => cuts.push({ from: Math.max(from, start) - from, to: Math.min(to, end) - from });
    markdownLanguage.parser.parse(section).iterate({ from, to, enter(node) {
      if (node.name === 'Image' || ['URL', 'LinkMark', 'EmphasisMark', 'CodeMark', 'CodeInfo', 'HeaderMark', 'QuoteMark', 'ListMark'].includes(node.name)) {
        cut(node.from, node.to); return false;
      }
      if (node.name === 'Escape') cut(node.from, node.from + 1);
    } });
    let text = section.slice(from, to);
    for (const cut of cuts.reverse()) if (cut.from < cut.to) text = text.slice(0, cut.from) + text.slice(cut.to);
    return text;
  }).filter(Boolean).join(' ').replace(/\s+/gu, ' ').trim();
}

export function makeCard(sections: readonly string[], page: { date: string; start: string; label: string }, selection = { from: 0, to: sections.reduce((length, section) => length + section.length, 0) }): Card | null {
  let text = plainWriting(sections, selection);
  if (!text) return null;
  const shortened = graphemes(text).length > 160;
  if (shortened) {
    let retained = '';
    for (const { segment } of new Intl.Segmenter(undefined, { granularity: 'word' }).segment(text)) {
      if (graphemes(retained + segment).length > 159) break;
      retained += segment;
    }
    if (!retained.trim()) throw new Error('Select a shorter word or phrase for the card.');
    text = retained.trimEnd() + '…';
  }
  const date = new Date(`${page.date}T12:00:00`), day = date.getDate();
  if (!Number.isFinite(date.getTime()) || !/^\d{4}-\d\d-\d\dT\d\d:\d\d/.test(page.start)) throw new Error('This page has no usable date and time.');
  return { text, shortened, date: page.date, day, layout: day % 3,
    archive: `${date.toLocaleDateString('en-GB', { weekday: 'short' })} ${day} ${date.toLocaleDateString('en-US', { month: 'short' })} ${date.getFullYear()}`, time: page.start.slice(11, 16), label: page.label };
}

export function breakLine(text: string): string[] {
  const at = text.search(/[,;:]/u);
  if (at < 0) return [text];
  const parts = [text.slice(0, at + 1).trim(), text.slice(at + 1).trim()];
  return parts.every(part => part.split(/\s+/u).length >= 3) ? parts : [text];
}

export function wrapLine(text: string, measure: (text: string) => number, width: number): string[] {
  const lines: string[] = []; let line = '';
  for (const word of text.split(/\s+/u)) {
    if (line && measure(`${line} ${word}`) > width) { lines.push(line); line = ''; }
    if (measure(word) <= width) { line += (line ? ' ' : '') + word; continue; }
    for (const letter of graphemes(word)) {
      if (line && measure(line + letter) > width) { lines.push(line); line = ''; }
      line += letter;
    }
  }
  if (line) lines.push(line);
  return lines;
}

export function paperColours(style: CSSStyleDeclaration): Paper {
  const colour = (name: string) => style.getPropertyValue(name).trim();
  return { paper: colour('--paper'), ink: colour('--ink'), secondary: colour('--ink-2'), cobalt: colour('--wet'), stamp: colour('--block'), onStamp: colour('--on-block') };
}

let edge: Promise<HTMLImageElement> | undefined;
function tornEdge() {
  return edge ??= new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => resolve(image);
    image.onerror = () => { edge = undefined; reject(new Error('The card edge could not be rendered.')); };
    image.src = 'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="80" viewBox="0 0 1200 80"><filter id="tear" x="-10%" y="-100%" width="120%" height="300%"><feTurbulence type="fractalNoise" baseFrequency=".02 .07" numOctaves="3" seed="7" result="noise"/><feDisplacementMap in="SourceGraphic" in2="noise" scale="26" xChannelSelector="R" yChannelSelector="G"/></filter><rect x="-40" y="-80" width="1280" height="140" fill="white" filter="url(#tear)"/></svg>`);
  });
}

export async function drawCard(canvas: HTMLCanvasElement, card: Card, colours: Paper, width = 1200, preview = false) {
  const height = width * 5 / 3;
  const lineSize = Math.max(width * .054, preview ? 16 : 0), monoSize = Math.max(width * .021, preview ? 10.5 : 0);
  await Promise.all([document.fonts.load(`400 ${lineSize}px "Alegreya Variable"`, card.text + card.day), document.fonts.load(`400 ${monoSize}px "IBM Plex Mono"`, card.archive + card.time + card.label)]);
  await document.fonts.ready;
  const edge = await tornEdge();
  const ratio = preview ? window.devicePixelRatio || 1 : 1;
  canvas.width = Math.round(width * ratio); canvas.height = Math.round(height * ratio);
  const ctx = canvas.getContext('2d');
  if (!ctx) throw new Error('The card canvas is unavailable.');
  ctx.scale(ratio, ratio);
  ctx.fillStyle = colours.paper; ctx.fillRect(0, 0, width, height);
  ctx.globalCompositeOperation = 'destination-in';
  ctx.drawImage(edge, 0, height - width / 15, width, width / 15);
  ctx.globalCompositeOperation = 'source-over';
  // The mask only covers the bottom strip; restore the untouched, flat body.
  ctx.fillStyle = colours.paper; ctx.fillRect(0, 0, width, height - width / 15);
  ctx.textBaseline = 'top';
  const compact = preview && width * .054 < 16;
  const margin = width * (compact ? .06 : .12), available = width - margin * 2;
  ctx.font = `400 ${lineSize}px "Alegreya Variable", Georgia, serif`;
  const lines = breakLine(card.text).flatMap((part, index) => {
    const indent = index ? lineSize * 1.2 : 0;
    return wrapLine(part, text => ctx.measureText(text).width, available - indent).map(text => ({ text, indent, width: ctx.measureText(text).width }));
  });
  const lineHeight = lineSize * 1.28, textHeight = lines.length * lineHeight;
  ctx.font = `400 ${monoSize}px "IBM Plex Mono", Menlo, monospace`;
  ctx.letterSpacing = `${monoSize * .06}px`;
  const archive = [{ text: card.archive, stamp: false }, { text: '/', stamp: false }, { text: card.time, stamp: true }, ...(card.label ? ['/', ...card.label.split(/\s+/u)].map(text => ({ text, stamp: false })) : [])];
  const gap = ctx.measureText(' ').width, stampPadding = monoSize * .3;
  const rows: { text: string; width: number; stamp: boolean }[][] = [[]];
  let rowWidth = 0;
  for (const part of archive) {
    const stamp = part.stamp;
    for (const text of wrapLine(part.text, text => ctx.measureText(text).width, available - (stamp ? stampPadding * 2 : 0))) {
      const size = ctx.measureText(text).width + (stamp ? stampPadding * 2 : 0);
      if (rowWidth && rowWidth + gap + size > available) { rows.push([]); rowWidth = 0; }
      rows.at(-1)!.push({ text, width: size, stamp }); rowWidth += (rowWidth ? gap : 0) + size;
    }
  }
  const archiveLine = monoSize * 1.6, archiveHeight = rows.length * archiveLine;
  const archiveY = card.layout === 1 ? height * .09 : height * .9 - archiveHeight;
  const gapHeight = lineHeight * (compact ? .5 : 1);
  const numeralY = height * (compact ? .04 : .1);
  const minY = card.layout === 1 ? archiveY + archiveHeight + gapHeight : card.layout === 2 ? numeralY + width * .24 * .85 + gapHeight : height * .08;
  const maxY = card.layout === 1 ? height * .86 : archiveY - gapHeight;
  if (textHeight > maxY - minY) throw new Error('This selection needs a larger window to stay readable.');
  const textY = Math.max(minY, Math.min(height * (card.layout === 0 ? .41 : card.layout === 1 ? .58 : .46) - textHeight / 2, maxY - textHeight));
  for (let row = 0; row < rows.length; row++) {
    const parts = rows[row], total = parts.reduce((n, part) => n + part.width, 0) + (parts.length - 1) * gap;
    let x = card.layout === 0 ? (width - total) / 2 : card.layout === 1 ? width - margin - total : margin;
    const y = archiveY + row * archiveLine;
    for (const part of parts) {
      if (part.stamp) { ctx.fillStyle = colours.stamp; ctx.fillRect(x, y - monoSize * .12, part.width, monoSize * 1.4); }
      ctx.fillStyle = part.stamp ? colours.onStamp : colours.secondary;
      ctx.fillText(part.text, x + (part.stamp ? stampPadding : 0), y); x += part.width + gap;
    }
  }
  ctx.letterSpacing = '0px'; ctx.fillStyle = colours.ink;
  ctx.font = `400 ${lineSize}px "Alegreya Variable", Georgia, serif`;
  lines.forEach((line, index) => ctx.fillText(line.text, card.layout === 0 ? (width - line.width) / 2 + line.indent / 2 : margin + line.indent, textY + index * lineHeight));
  if (card.layout === 2) {
    const size = width * .24;
    ctx.fillStyle = colours.cobalt; ctx.font = `400 ${size}px "Alegreya Variable", Georgia, serif`; ctx.letterSpacing = `${size * -.03}px`;
    ctx.fillText(String(card.day), margin, numeralY); ctx.letterSpacing = '0px';
  }
  if (card.shortened) {
    ctx.font = `400 ${monoSize}px "IBM Plex Mono", Menlo, monospace`; ctx.fillStyle = colours.secondary;
    ctx.fillText('Selection shortened', margin, height * .95 - monoSize);
  }
}

export async function cardPng(card: Card, colours: Paper): Promise<number[]> {
  const canvas = document.createElement('canvas');
  await drawCard(canvas, card, colours);
  const blob = await new Promise<Blob>((resolve, reject) => canvas.toBlob(blob => blob ? resolve(blob) : reject(new Error('The PNG could not be rendered.')), 'image/png'));
  return Array.from(new Uint8Array(await blob.arrayBuffer()));
}
