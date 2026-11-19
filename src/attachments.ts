// The draft's image attachments: chosen in the dialog, dropped on the window or pasted.

import { files, showToast } from "./bridge";
import { ui } from "./ui";

/** Matches `MAX_IMAGES` in `src-tauri/src/attachments.rs`. */
export const MAX_IMAGES = 10;
const IMAGE_FILE = /\.(png|jpe?g|gif|webp)$/i;
let next = 0;

function room(): number {
  const left = MAX_IMAGES - ui.attachments.length;
  if (left <= 0) showToast(`Attach at most ${MAX_IMAGES} images to one prompt`);
  return Math.max(left, 0);
}

/** Attach image files by path; other files are skipped with a message. */
export async function addPaths(paths: string[]) {
  const images = paths.filter((p) => IMAGE_FILE.test(p));
  if (images.length < paths.length) showToast("Only PNG, JPEG, GIF and WebP images can be attached");
  for (const path of images.slice(0, room())) {
    if (ui.attachments.some((a) => a.path === path)) continue;
    try {
      const image = await files.readImage(path);
      if (room() === 0) return;
      ui.attachments.push({ key: `a${++next}`, name: image.name, url: image.url, path: image.path, data: null });
    } catch (error) {
      showToast(String(error));
    }
  }
}

/** Attach pasted image data (clipboard files have no path). */
export async function addBlobs(blobs: File[]) {
  for (const blob of blobs.slice(0, room())) {
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
    ui.attachments.push({ key: `a${++next}`, name: blob.name || "Pasted image", url, path: null, data });
  }
}

export function remove(key: string) {
  ui.attachments = ui.attachments.filter((a) => a.key !== key);
}

/** The attachments to send, leaving the draft without any. */
export function take() {
  const taken = ui.attachments;
  ui.attachments = [];
  return taken;
}
