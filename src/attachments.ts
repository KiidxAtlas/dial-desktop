// Files selected, dropped or pasted into the draft. Only images get previews.
import { files, showToast } from './bridge';
import { IMAGE_FILE, imageMime, MAX_ATTACHMENTS, MAX_FILE_BYTES } from './attachmentFormats';
import type { Draft } from './drafts';
import type { Attachment } from './types';

export { IMAGE_FILE } from './attachmentFormats';

// A persisted draft key, not a credential. getRandomValues is available on older WebKit too.
function attachmentKey(): string {
  return Array.from(crypto.getRandomValues(new Uint32Array(4)), n => n.toString(16).padStart(8, '0')).join('');
}

function hasRoom(draft: Draft): boolean {
  if (draft.attachments.length < MAX_ATTACHMENTS) return true;
  showToast(`Attach at most ${MAX_ATTACHMENTS} files to one prompt`);
  return false;
}

/** Check again after async reads: another batch may have filled the draft. */
function append(draft: Draft, attachment: Attachment): boolean {
  if (attachment.path !== null && draft.attachments.some(a => a.path === attachment.path)) return true;
  if (!hasRoom(draft)) return false;
  draft.attachments.push(attachment);
  return true;
}

/** Check path-only files now for useful feedback; send still validates the current file. */
export async function addPaths(draft: Draft, paths: string[]) {
  for (const path of new Set(paths)) {
    if (draft.attachments.some(a => a.path === path)) continue;
    if (!hasRoom(draft)) return;
    try {
      const image = IMAGE_FILE.test(path);
      if (!image) await files.validateAttachment(path);
      const file = image ? await files.readImage(path) : { path, name: path.split(/[\\/]/).pop() || path, url: null };
      if (!append(draft, { ...file, key: attachmentKey(), data: null })) return;
    } catch (error) {
      showToast(String(error));
    }
  }
}

function readBase64(file: File): Promise<string> {
  if (!file.size) return Promise.resolve('');
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const url = String(reader.result);
      const comma = url.indexOf(',');
      if (comma < 0) reject(new Error('Missing file data'));
      else resolve(url.slice(comma + 1));
    };
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(file);
  });
}

/** DOM clipboard files expose bytes rather than a local path. */
export async function addBlobs(draft: Draft, blobs: File[]) {
  for (const blob of blobs) {
    const mime = imageMime(blob);
    const name = blob.name || (mime ? `Pasted image.${mime.slice(6)}` : 'Pasted file');
    if (blob.size > MAX_FILE_BYTES) {
      showToast(`${name} is too large. Attach files up to ${MAX_FILE_BYTES / (1024 * 1024)} MiB each.`);
      continue;
    }
    if (!hasRoom(draft)) return;
    try {
      const data = await readBase64(blob);
      const url = mime ? `data:${mime};base64,${data}` : null;
      if (!append(draft, { key: attachmentKey(), name, url, path: null, data })) return;
    } catch (error) {
      showToast(`Could not read ${name}: ${error}`);
    }
  }
}

/** WebKit may expose only TIFF, or no image bytes, for a copied screenshot. */
export async function addNativeClipboardImage(draft: Draft) {
  try {
    const data = await files.readClipboardImage();
    if (!data) return showToast('No clipboard image found. Copy the screenshot, or choose its file.');
    append(draft, { key: attachmentKey(), name: 'Pasted image.png', url: `data:image/png;base64,${data}`, path: null, data });
  } catch (error) {
    showToast(String(error));
  }
}

/** Native file-manager copies may not be exposed as DOM files. */
export async function addNativeClipboardFiles(draft: Draft, platform: string) {
  try {
    const paths = await files.readClipboardFilePaths();
    if (paths.length) await addPaths(draft, paths);
    else if (platform === 'macos') await addNativeClipboardImage(draft);
  } catch (error) {
    showToast(`Could not read clipboard files: ${error}`);
  }
}
