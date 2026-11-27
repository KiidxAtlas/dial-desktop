// Clipboard and prompt formats, shared by the composer and the native bridge.
import type { Attachment } from './types';

/** Match the combined file count and per-file limit checked by the desktop backend. */
export const MAX_ATTACHMENTS = 10;
export const MAX_FILE_BYTES = 64 * 1024 * 1024;
export const IMAGE_FILE = /\.(png|jpe?g|gif|webp)$/i;
const imageDataPrefix = /^data:image\/(?:png|jpeg|gif|webp);base64,/i;

/** Picker, drop and paste previews contain the full image selected by the user. */
function previewBytes(url: string): string | null {
  const prefix = url.match(imageDataPrefix);
  return prefix ? url.slice(prefix[0].length) || null : null;
}

export function imageMime(file: File): string | null {
  if (!file.size) return null;
  if (/^image\/(png|jpeg|gif|webp)$/i.test(file.type)) return file.type.toLowerCase();
  const ext = !file.type && IMAGE_FILE.exec(file.name)?.[1].toLowerCase();
  return ext ? `image/${ext === 'jpg' ? 'jpeg' : ext}` : null;
}

/** A thumbnail URL also identifies existing image-only drafts saved before file support. */
export function attachmentPayload(attachments: readonly Attachment[], acceptsImages: boolean | null | undefined) {
  const inlineImages: string[] = [];
  const documents: Pick<Attachment, 'name' | 'path' | 'data'>[] = [];
  for (const { name, url, path, data } of attachments) {
    if (url) {
      if (acceptsImages === false) throw new Error('This model cannot view images. Choose a vision-capable model or remove the image before sending.');
      const bytes = previewBytes(url);
      if (!bytes) throw new Error(`Could not read the attached image ${name}. Attach it again before sending.`);
      inlineImages.push(bytes);
    } else {
      documents.push({ name, path, data });
    }
  }
  return { images: [], inlineImages, documents };
}

type PasteAction = { kind: 'files'; files: File[]; hasDirectory: boolean } | { kind: 'nativeFiles' | 'macImage' | 'text' };

/** Read event-scoped data synchronously. Never inspect native clipboard contents on synthetic events or normal text pastes. */
export function clipboardAction(event: ClipboardEvent, platform: string): PasteAction {
  const c = event.clipboardData;
  const items = Array.from(c?.items ?? []).filter(item => item.kind === 'file');
  const fileItems = items.filter(item => !item.webkitGetAsEntry?.()?.isDirectory);
  const hasDirectory = items.length !== fileItems.length;
  let files = Array.from(c?.files ?? []);
  if (!files.length || hasDirectory) files = fileItems.flatMap(item => item.getAsFile() ?? []);
  if (files.length || hasDirectory) {
    if (!hasDirectory && event.isTrusted && platform === 'macos' && files.every(f => /^image\/(?:x-)?tiff?$/i.test(f.type))) return { kind: 'macImage' };
    return { kind: 'files', files, hasDirectory };
  }
  if (!event.isTrusted) return { kind: 'text' };
  const hasText = c?.getData('text/plain') || Array.from(c?.types ?? []).some(type => {
    if (!type.startsWith('text/')) return false;
    const data = c?.getData(type);
    // A Linux file-manager URI list is a file format, not editable text. Never interpret
    // clipboard text as paths: the native reader requests only the file-list target.
    if (data && type === 'text/uri-list' && platform === 'linux') {
      const uris = data.split(/\r?\n/).map(s => s.trim()).filter(s => s && !s.startsWith('#'));
      if (uris.length && uris.every(uri => /^file:\/\/(?:localhost)?\//i.test(uri))) return false;
    }
    return !!data;
  });
  if (!hasText && ['linux', 'macos', 'windows'].includes(platform)) return { kind: 'nativeFiles' };
  return { kind: 'text' };
}
