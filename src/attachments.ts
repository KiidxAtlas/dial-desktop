// The draft's image attachments: chosen in the dialog, dropped on the window or pasted.

import { files, showToast } from "./bridge";
import type { Draft } from "./drafts";

/** Matches `MAX_IMAGES` in `src-tauri/src/attachments.rs`. */
export const MAX_IMAGES = 10;
export const IMAGE_FILE = /\.(png|jpe?g|gif|webp)$/i;
const IMAGE_MIME = /^image\/(png|jpeg|gif|webp)$/i;
/** Matches `MAX_IMAGE_BYTES` in the desktop backend. */
const MAX_IMAGE_BYTES = 64 * 1024 * 1024;

function hasRoom(draft: Draft): boolean {
  if (draft.attachments.length < MAX_IMAGES) return true;
  showToast(`Attach at most ${MAX_IMAGES} images to one prompt`);
  return false;
}

/** Attach image files by path; other files are skipped with a message. */
export async function addPaths(draft: Draft, paths: string[]) {
  const images = [...new Set(paths.filter((p) => IMAGE_FILE.test(p)))];
  if (paths.some((p) => !IMAGE_FILE.test(p))) showToast("Only PNG, JPEG, GIF and WebP images can be attached");
  for (const path of images) {
    if (draft.attachments.some((a) => a.path === path)) continue;
    if (!hasRoom(draft)) return;
    try {
      const image = await files.readImage(path);
      if (draft.attachments.some((a) => a.path === image.path)) continue;
      if (!hasRoom(draft)) return;
      draft.attachments.push({ key: crypto.randomUUID(), name: image.name, url: image.url, path: image.path, data: null });
    } catch (error) {
      showToast(String(error));
    }
  }
}

/** Attach pasted image data (clipboard files have no path). */
export async function addBlobs(draft: Draft, blobs: File[]) {
  for (const blob of blobs) {
    if (!IMAGE_MIME.test(blob.type)) {
      showToast("Only PNG, JPEG, GIF and WebP images can be attached");
      continue;
    }
    if (blob.size > MAX_IMAGE_BYTES) {
      showToast(`Pasted images can be at most ${MAX_IMAGE_BYTES / (1024 * 1024)} MiB`);
      continue;
    }
    if (!hasRoom(draft)) return;
    const url = await new Promise<string>((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => resolve(String(reader.result));
      reader.onerror = () => reject(reader.error);
      reader.readAsDataURL(blob);
    }).catch(() => null);
    const data = url?.split(",", 2)[1];
    if (!url || !data) {
      showToast("The pasted image could not be read");
      continue;
    }
    if (!hasRoom(draft)) return;
    draft.attachments.push({ key: crypto.randomUUID(), name: blob.name || "Pasted image", url, path: null, data });
  }
}

/** Fallback for WebKit paste events that do not expose the clipboard's image bytes. */
export async function addNativeClipboardImage(draft: Draft) {
  try {
    const data = await files.readClipboardImage();
    if (!data) return showToast('No clipboard image found. Copy the screenshot, or choose its file.');
    if (!hasRoom(draft)) return;
    draft.attachments.push({ key: crypto.randomUUID(), name: "Pasted image", url: `data:image/png;base64,${data}`, path: null, data });
  } catch (error) {
    showToast(String(error));
  }
}
