// The image viewer: generated images and image paths in answers open here, larger, with ways to
// open the file in the system viewer, show it in Finder or copy its path.

import { shallowRef } from "vue";
import { snapshot } from "./bridge";
import type { Attachment } from "./types";

export interface ViewedImage {
  /** The path as the conversation named it (relative to the project, or absolute). */
  path: string;
  /** The same file as an absolute path, for Finder and copying. */
  absolute: string;
  /** The project the path belongs to; `null` for a saved Type asset image. */
  root: string | null;
  /** Whether this image lives in a Type's assets folder rather than the project workspace. */
  asset?: boolean;
}

type AttachmentPreview = Pick<Attachment, "name" | "url">;
export const viewedImage = shallowRef<ViewedImage | { attachment: AttachmentPreview } | null>(null);

export function viewAttachment({ name, url }: AttachmentPreview) {
  viewedImage.value = { attachment: { name, url } };
}

/** Raster image file names the app can preview (the backend checks the real file too). */
export const IMAGE_EXT = /\.(png|jpe?g|gif|webp)$/i;

/** Whether `path` is already an absolute POSIX or Windows path. */
export function isAbsolutePath(path: string): boolean {
  return path.startsWith("/") || /^[a-z]:[\\/]/i.test(path) || path.startsWith("\\\\");
}

/** `path` against the project `root`: `./a.png` and `a.png` → `root/a.png`; absolute stays. */
export function absoluteIn(root: string, path: string): string {
  if (isAbsolutePath(path)) return path;
  return `${root.replace(/[\\/]+$/, "")}/${path.replace(/^(\.\/)+/, "")}`;
}

export function viewImage(path: string) {
  const root = snapshot.value?.workspace?.root;
  if (!root) return;
  const absolute = absoluteIn(root, path);
  const normalizedRoot = root.replace(/[\\/]+$/, "");
  const prefix = `${normalizedRoot}/`;
  const winPrefix = `${normalizedRoot}\\`;
  // Name it as the project does: `images/a.png`, not the whole path.
  const shown = absolute.startsWith(prefix)
    ? absolute.slice(prefix.length)
    : absolute.startsWith(winPrefix)
      ? absolute.slice(winPrefix.length)
      : path;
  viewedImage.value = { path: shown, absolute, root };
}

/** Open a saved Type asset image in the lightbox (works for both app-scoped and project-scoped Types). */
export function viewTypeImage(absolute: string, label?: string) {
  const name = label?.trim() || absolute.split(/[\\/]/).pop() || absolute;
  viewedImage.value = { path: name, absolute, root: null, asset: true };
}
