// The image viewer: generated images and image paths in answers open here, larger, with ways to
// open the file in the system viewer, show it in Finder or copy its path.

import { shallowRef } from "vue";
import { snapshot } from "./bridge";

export interface ViewedImage {
  /** The path as the conversation named it (relative to the project, or absolute). */
  path: string;
  /** The same file as an absolute path, for Finder and copying. */
  absolute: string;
  /** The project the path belongs to. */
  root: string;
}

export const viewedImage = shallowRef<ViewedImage | null>(null);

/** Raster image file names the app can preview (the backend checks the real file too). */
export const IMAGE_EXT = /\.(png|jpe?g|gif|webp)$/i;

/** `path` against the project `root`: `./a.png` and `a.png` → `root/a.png`; absolute stays. */
export function absoluteIn(root: string, path: string): string {
  if (path.startsWith("/")) return path;
  return `${root.replace(/\/+$/, "")}/${path.replace(/^(\.\/)+/, "")}`;
}

export function viewImage(path: string) {
  const root = snapshot.value?.workspace?.root;
  if (!root) return;
  const absolute = absoluteIn(root, path);
  const prefix = `${root.replace(/\/+$/, "")}/`;
  // Name it as the project does: `images/a.png`, not the whole path.
  const shown = absolute.startsWith(prefix) ? absolute.slice(prefix.length) : path;
  viewedImage.value = { path: shown, absolute, root };
}
