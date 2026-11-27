import assert from 'node:assert/strict';
import test from 'node:test';
import { attachmentPayload, clipboardAction, imageMime } from './attachmentFormats.ts';

const file = (name, type, bytes = 'contents') => new File([bytes], name, { type });
const pdf = file('report.pdf', 'application/pdf');
const png = file('plot.PNG', 'image/png');
const tiff = file('shot.tif', 'image/tiff');
const item = (f, isDirectory = false) => ({ kind: 'file', getAsFile: () => f, webkitGetAsEntry: () => ({ isDirectory }) });
const paste = ({ files = [], items = [], text = '', formats = {}, isTrusted = true } = {}) => ({
  isTrusted, clipboardData: { files, items, types: Object.keys(formats), getData: type => type === 'text/plain' ? text : formats[type] ?? '' },
});

test('only raster image files get a preview MIME', () => {
  for (const [f, mime] of [[pdf, null], [png, 'image/png'], [file('p.JPG', ''), 'image/jpeg'], [tiff, null], [file('drawing.svg', 'image/svg+xml'), null], [file('log', ''), null], [file('empty.png', '', ''), null]]) {
    assert.equal(imageMime(f), mime, f.name);
  }
});

test('file paste takes the DOM files or file items once; a folder never becomes a zero-byte file', () => {
  for (const event of [paste({ files: [pdf, png], items: [item(pdf), item(png)], text: 'report.pdf' }), paste({ items: [item(pdf), item(png)] })]) {
    assert.deepEqual(clipboardAction(event, 'windows'), { kind: 'files', files: [pdf, png], hasDirectory: false });
  }
  const folder = file('folder', '', '');
  assert.deepEqual(clipboardAction(paste({ files: [folder, pdf], items: [item(folder, true), item(pdf)] }), 'windows'), { kind: 'files', files: [pdf], hasDirectory: true });
});

test('only trusted text-free pastes use native fallbacks; macOS TIFF screenshots still use PNG conversion', () => {
  for (const [event, platform, kind] of [
    [paste(), 'windows', 'nativeFiles'], [paste(), 'macos', 'nativeFiles'], [paste(), 'linux', 'nativeFiles'],
    [paste({ formats: { 'text/uri-list': '# files\r\nfile:///local.pdf\r\nfile://localhost/tmp/x' } }), 'linux', 'nativeFiles'],
    [paste({ formats: { 'text/uri-list': 'https://example.com/' } }), 'linux', 'text'],
    [paste({ formats: { 'text/uri-list': 'file://server/share/foo' } }), 'linux', 'text'],
    [paste({ text: 'file:///local.pdf', formats: { 'text/uri-list': 'file:///local.pdf' } }), 'linux', 'text'],
    [paste({ formats: { 'text/html': '<p>hello</p>' } }), 'linux', 'text'],
    [paste({ isTrusted: false }), 'linux', 'text'],
    [paste({ text: 'C:\\copied.pdf' }), 'windows', 'text'], [paste({ text: ' ' }), 'macos', 'text'],
    [paste({ isTrusted: false }), 'windows', 'text'], [paste({ isTrusted: false }), 'macos', 'text'],
    [paste({ files: [tiff] }), 'macos', 'macImage'], [paste({ files: [tiff, pdf] }), 'macos', 'files'],
    [paste({ files: [tiff], isTrusted: false }), 'macos', 'files'],
  ]) assert.equal(clipboardAction(event, platform).kind, kind, platform);
});

test('picked and pasted images send their preview bytes in order; only documents use workspace copies', () => {
  const documents = [{ name: 'report.pdf', path: '/report.pdf', data: null }, { name: 'data.bin', path: null, data: 'AP8=' }, { name: 'empty', path: null, data: '' }];
  const images = [{ name: 'photo.png', path: '/photo.png', data: null }, { name: 'Paste.png', path: null, data: 'aW1n' }];
  const draft = [...images, ...documents].map((a, i) => ({ ...a, key: String(i), url: ['data:image/png;base64,cGljaw==', 'data:image/png;base64,aW1n'][i] ?? null }));
  for (const vision of [true, null, undefined]) {
    assert.deepEqual(attachmentPayload(draft, vision), { documents, images: [], inlineImages: ['cGljaw==', 'aW1n'] });
  }
  assert.throws(() => attachmentPayload(draft, false), /choose a vision-capable model/i);
  assert.deepEqual(attachmentPayload(draft.slice(2), false), { documents, images: [], inlineImages: [] });
  for (const url of ['file:///photo.png', 'data:image/png;base64,']) {
    assert.throws(() => attachmentPayload([{ ...draft[0], url }], true), /attach it again/i);
  }
});
