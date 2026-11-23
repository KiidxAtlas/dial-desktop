// Clipboard and prompt formats, shared by the composer and the native bridge.
import type { Attachment } from './types';

/** Match the combined file count and per-file limit checked by the desktop backend. */
export const MAX_ATTACHMENTS = 10;
export const MAX_FILE_BYTES = 64 * 1024 * 1024;
export const IMAGE_FILE = /\.(png|jpe?g|gif|webp)$/i;

export function imageMime(file: File): string | null {
  if (!file.size) return null;
  if (/^image\/(png|jpeg|gif|webp)$/i.test(file.type)) return file.type.toLowerCase();
  const ext = !file.type && IMAGE_FILE.exec(file.name)?.[1].toLowerCase();
  return ext ? `image/${ext === 'jpg' ? 'jpeg' : ext}` : null;
}

/** A thumbnail URL also identifies existing image-only drafts saved before file support. */
export function attachmentPayload(attachments: readonly Attachment[], acceptsImages: boolean | null | undefined) {
  const images: string[] = [], inlineImages: string[] = [];
  const documents: Pick<Attachment, 'name' | 'path' | 'data'>[] = [];
  for (const { name, url, path, data } of attachments) {
    if (url && acceptsImages !== false) {
      if (path !== null) images.push(path);
      else if (data !== null) inlineImages.push(data);
    } else {
      documents.push({ name, path, data });
    }
  }
  return { images, inlineImages, documents };
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
  const hasText = c?.getData('text/plain') || Array.from(c?.types ?? []).some(type => type.startsWith('text/') && !!c?.getData(type));
  if (event.isTrusted && !hasText && (platform === 'macos' || platform === 'windows')) return { kind: 'nativeFiles' };
  return { kind: 'text' };
}
