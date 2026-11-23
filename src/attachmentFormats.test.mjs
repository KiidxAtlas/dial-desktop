import assert from 'node:assert/strict';
import test from 'node:test';
import { attachmentPayload, clipboardAction, imageMime } from './attachmentFormats.ts';

const file = (name, type, bytes = 'contents') => new File([bytes], name, { type });
const pdf = file('report.pdf', 'application/pdf');
const png = file('plot.PNG', 'image/png');
const tiff = file('shot.tif', 'image/tiff');
const item = (f, isDirectory = false) => ({ kind: 'file', getAsFile: () => f, webkitGetAsEntry: () => ({ isDirectory }) });
const paste = ({ files = [], items = [], text = '', isTrusted = true } = {}) => ({
  isTrusted, clipboardData: { files, items, getData: type => type === 'text/plain' ? text : '' },
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
    [paste(), 'windows', 'nativeFiles'], [paste(), 'macos', 'nativeFiles'], [paste(), 'linux', 'text'],
    [paste({ text: 'C:\\copied.pdf' }), 'windows', 'text'], [paste({ text: ' ' }), 'macos', 'text'],
    [paste({ isTrusted: false }), 'windows', 'text'], [paste({ isTrusted: false }), 'macos', 'text'],
    [paste({ files: [tiff] }), 'macos', 'macImage'], [paste({ files: [tiff, pdf] }), 'macos', 'files'],
    [paste({ files: [tiff], isTrusted: false }), 'macos', 'files'],
  ]) assert.equal(clipboardAction(event, platform).kind, kind, platform);
});

test('mixed sources marshal separately, including empty pasted files; text models get files instead of image inputs', () => {
  const documents = [{ name: 'report.pdf', path: '/report.pdf', data: null }, { name: 'data.bin', path: null, data: 'AP8=' }, { name: 'empty', path: null, data: '' }];
  const images = [{ name: 'photo.png', path: '/photo.png', data: null }, { name: 'Paste.png', path: null, data: 'aW1n' }];
  const draft = [...images, ...documents].map((a, i) => ({ ...a, key: String(i), url: i < images.length ? 'data:image/png;base64,aW1n' : null }));
  for (const vision of [true, null, undefined]) {
    assert.deepEqual(attachmentPayload(draft, vision), { documents, images: ['/photo.png'], inlineImages: ['aW1n'] });
  }
  assert.deepEqual(attachmentPayload(draft, false), { documents: [...images, ...documents], images: [], inlineImages: [] });
});
