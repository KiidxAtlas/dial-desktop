import type { Painter } from "./backgrounds";

// A small repeating ink screen. Vary the dot size rather than introducing
// per-frame noise: the print travels with the clouds without shimmering.
let screen: CanvasPattern | null = null;
function ink(ctx: CanvasRenderingContext2D) {
  if (screen) return screen;
  const tile = document.createElement("canvas");
  tile.width = tile.height = 32;
  const g = tile.getContext("2d")!;
  g.fillStyle = "rgba(53,81,76,.25)";
  for (let i = 0; i < 64; i++) {
    const size = 0.8 + ((i * 47 % 19) / 10);
    g.fillRect((i % 8) * 4, Math.floor(i / 8) * 4, size, size + .5);
  }
  return screen = ctx.createPattern(tile, "repeat");
}

export const cloudprint: Painter = (ctx, w, h, time) => {
  const sky = ctx.createLinearGradient(0, 0, 0, h);
  sky.addColorStop(0, "#788f92");
  sky.addColorStop(1, "#b2c2b5");
  ctx.fillStyle = sky;
  ctx.fillRect(0, 0, w, h);
  const unit = Math.max(w, h) / 1000;
  const width = w / unit, height = h / unit;
  ctx.save();
  ctx.scale(unit, unit);
  const print = ink(ctx);
  for (let layer = 0; layer < 3; layer++) {
    ctx.save();
    // Periodic silhouette lets each bank drift indefinitely without a jump.
    const period = 768, drift = (time * (1.5 + layer) + layer * 211) % period;
    ctx.translate(drift, 0);
    ctx.beginPath();
    ctx.moveTo(-period, height);
    for (let x = -period; x <= width + 4; x += 4) {
      const a = x / period * Math.PI * 2;
      const crest = .10 * Math.cos(a) + .045 * Math.sin(3*a) + .075 * Math.abs(Math.sin(5*a)) + .024 * Math.abs(Math.cos(13*a));
      ctx.lineTo(x, height * (.70 + layer * .125) - crest * Math.min(580, height));
    }
    ctx.lineTo(width + 4, height);
    ctx.closePath();
    ctx.clip();
    const wash = ctx.createLinearGradient(0, height * (.49 + layer * .125), 0, height * (.82 + layer * .125));
    wash.addColorStop(0, ["#f6dfaf", "#adc0ad", "#edd2a0"][layer]);
    wash.addColorStop(1, ["#aca995", "#638482", "#aca58a"][layer]);
    ctx.fillStyle = wash;
    ctx.fillRect(-period, 0, width + period, height);
    if (print) {
      ctx.fillStyle = print;
      ctx.fillRect(-period, 0, width + period, height);
    }
    ctx.restore();
  }
  ctx.restore();
};
