import type { Painter } from "./backgrounds";
import artworkUrl from "./assets/lantern-watch.webp";

let print: HTMLImageElement | undefined;
let loading: Promise<void> | undefined;

export function prepareLanternWatch() {
  if (!loading) {
    const image = new Image();
    image.src = artworkUrl;
    loading = image.decode().then(() => { print = image; }).catch((error) => {
      loading = undefined;
      throw error;
    });
  }
  return loading;
}

// Star and lantern positions are pixels in the 1672 × 941 landscape illustration.
const stars = [[64, 33], [237, 17], [659, 44], [129, 118], [744, 174], [279, 309], [426, 376], [263, 465]];

export const lanternWatch: Painter = (ctx, w, h, t) => {
  if (!print) {
    ctx.fillStyle = "#0b1126";
    ctx.fillRect(0, 0, w, h);
    return;
  }

  // Cover the pane; bias any crop toward the cat and the lantern at its feet.
  const scale = Math.max(w / print.naturalWidth, h / print.naturalHeight);
  ctx.save();
  ctx.translate((w - print.naturalWidth * scale) * 0.59, (h - print.naturalHeight * scale) * 0.72);
  ctx.scale(scale, scale);
  ctx.drawImage(print, 0, 0);

  ctx.fillStyle = "#fff0c5";
  for (const [x, y] of stars) {
    const glint = (1 + Math.sin(t * (0.7 + x / 420) + y)) / 2;
    ctx.globalAlpha = 0.08 + 0.72 * glint ** 3;
    ctx.beginPath();
    ctx.arc(x, y, 1 + glint * 0.9, 0, 2 * Math.PI);
    ctx.fill();
  }

  const x = 897, y = 804;
  const flame = 0.64 + 0.19 * Math.sin(2.1 * t) + 0.11 * Math.sin(4.7 * t + 1);
  const glow = ctx.createRadialGradient(x, y, 0, x, y, 95);
  glow.addColorStop(0, "#fff1af");
  glow.addColorStop(0.13, "rgba(255, 180, 63, 0.72)");
  glow.addColorStop(0.48, "rgba(255, 159, 53, 0.28)");
  glow.addColorStop(1, "rgba(255, 155, 50, 0)");
  ctx.globalAlpha = flame;
  ctx.fillStyle = glow;
  ctx.fillRect(x - 95, y - 95, 190, 190);

  // A handful of illuminated dust motes rise and fade inside the pool of light.
  ctx.fillStyle = "#ffe7a5";
  for (let i = 0; i < 7; i++) {
    const rise = (t / (6 + i / 2) + i * 0.37) % 1;
    ctx.globalAlpha = 0.7 * Math.sin(Math.PI * rise) ** 2;
    ctx.beginPath();
    ctx.arc(x - 18 + i * 5 + 9 * Math.sin(5 * rise + i), y - 12 - 82 * rise, 1.6, 0, 2 * Math.PI);
    ctx.fill();
  }
  ctx.restore();
};
