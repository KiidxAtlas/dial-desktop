// The right-click menu of an image shown in the app (conversation thumbnails and the viewer):
// copy the image, save it, and the file actions when it is a file.

import { files, showToast } from "./bridge";
import { copyText } from "./sessionActions";
import { openMenu, type MenuItem } from "./ui";

export interface ShownImage {
  /** File name, or a label such as "Flowchart". */
  name: string;
  /** `data:` URL of the image as shown. */
  url: string;
  /** The image's file, when it is one: Show in Finder and Copy path use it. */
  absolute?: string;
  /** Open it larger (a thumbnail) or in the system viewer (the viewer). */
  open?: { label: string; run: () => void };
}

/** Raster data URLs keep their bytes; anything else (diagrams are SVG) is saved as PNG. */
const RASTER = /^data:image\/(png|jpeg|gif|webp);base64,/;
const EXTENSION: Record<string, string> = { png: "png", jpeg: "jpg", gif: "gif", webp: "webp" };

/** The image as a PNG blob, for the clipboard, which takes PNG everywhere. */
async function pngBlob(url: string): Promise<Blob> {
  if (url.startsWith("data:image/png;")) return (await fetch(url)).blob();
  const img = new Image();
  img.src = url;
  await img.decode();
  const canvas = document.createElement("canvas");
  canvas.width = img.naturalWidth || 1;
  canvas.height = img.naturalHeight || 1;
  canvas.getContext("2d")!.drawImage(img, 0, 0);
  return new Promise((resolve, reject) =>
    canvas.toBlob((blob) => (blob ? resolve(blob) : reject(new Error("could not encode"))), "image/png"),
  );
}

function base64(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result).replace(/^data:[^,]*,/, ""));
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(blob);
  });
}

/** Put the image on the clipboard. Call it straight from the click: the clipboard item is made
 * synchronously (WebKit requires that) and filled once the PNG is ready. */
export function copyImage(url: string) {
  try {
    const item = new ClipboardItem({ "image/png": pngBlob(url) });
    navigator.clipboard.write([item]).then(
      () => showToast("Image copied"),
      () => showToast("Could not copy the image"),
    );
  } catch {
    showToast("Could not copy the image");
  }
}

/** Ask where to save the image and write it there, in its own format (PNG for diagrams). */
export async function saveImage({ name, url }: Pick<ShownImage, "name" | "url">) {
  const raster = RASTER.exec(url);
  const ext = raster ? EXTENSION[raster[1]] : "png";
  let data: string;
  try {
    data = raster ? url.slice(raster[0].length) : await base64(await pngBlob(url));
  } catch {
    return showToast("Could not save the image");
  }
  const base = name.replace(/\.[a-z0-9]+$/i, "").replace(/[/\\:]+/g, "-").trim() || "image";
  await files.saveImage(`${base}.${ext}`, ext === "jpg" ? ["jpg", "jpeg"] : [ext], data);
}

export function imageMenuItems(image: ShownImage): MenuItem[] {
  const items: MenuItem[] = [];
  if (image.open) items.push({ label: image.open.label, icon: "external", run: image.open.run }, { divider: true });
  items.push(
    { label: "Copy image", icon: "copy", run: () => copyImage(image.url) },
    { label: "Save image as…", icon: "download", run: () => void saveImage(image) },
  );
  const absolute = image.absolute;
  if (absolute) {
    items.push(
      { divider: true },
      { label: "Show in Finder", icon: "folder", run: () => void files.reveal(absolute) },
      { label: "Copy path", icon: "file", run: () => void copyText(absolute, "Path copied") },
    );
  }
  return items;
}

/** Open the image menu at a right-click. */
export function openImageMenu(event: MouseEvent, image: ShownImage) {
  openMenu(event, imageMenuItems(image));
}
