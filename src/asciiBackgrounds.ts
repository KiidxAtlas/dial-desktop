import type { Painter } from "./backgrounds";

const wrap = (x: number, period: number) => ((x % period) + period) % period;
const hash = (x: number, y: number) => wrap(Math.sin(91.23 * x + 17.17 * y) * 43123.328, 1);

type Glyph = (char: string, x: number, y: number, opacity: number) => void;

/** Art is laid out on a fixed character grid and stays crisp at any pane size. */
function ascii(bg: string, color: string, draw: (glyph: Glyph, t: number) => void): Painter {
  return (ctx, w, h, t) => {
    ctx.fillStyle = bg;
    ctx.fillRect(0, 0, w, h);
    const dx = w / 80;
    const dy = h / 48;
    ctx.save();
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.font = `${Math.min(dx, dy) * 1.24}px ui-monospace, monospace`;
    ctx.fillStyle = color;
    draw((char, x, y, opacity) => {
      ctx.globalAlpha = opacity;
      ctx.fillText(char, (x + 0.5) * dx, (y + 0.5) * dy);
    }, t);
    ctx.restore();
  };
}

export const traceRoute = ascii("#080d14", "#77efeb", (glyph, t) => {
  for (let n = 0; n < 13; n++) {
    const right = n < 8;
    const j = right ? n : n - 8;
    const dir = right ? -1 : 1;
    const len = right ? 14 + j % 4 * 3 : 9 + j * 4;
    const bend = 4 + j % 4 * 2;
    const rise = j % 2 ? -1 : 1;
    for (let i = 0; i < len; i++) {
      const tail = wrap(i + j * 7 - t * (4 + j % 3), len);
      const x = (right ? 79 : 1) + i * dir;
      const y = (right ? 4 + j * 5 : 31 + j * 3) + rise * Math.max(0, Math.min(bend, i - 3));
      const char = i === len - 1 ? "o" : i === 3 || i === bend + 3 ? "+" :
        i > 3 && i < bend + 3 ? (dir === rise ? "\\" : "/") : i % 4 === 0 ? "." : "-";
      glyph(tail < 1 ? (right ? "<" : ">") : char, x, y, Math.max(0.23, 0.95 - tail * 0.17));
    }
  }
});

const points = Array.from({ length: 570 }, (_, i) => {
  const y = 1 - 2 * (i + 0.5) / 570;
  const radius = Math.sqrt(1 - y * y);
  const a = i * 2.399963229728653;
  return [radius * Math.cos(a), y, radius * Math.sin(a)] as const;
});

export const hexSwarm = ascii("#100b0d", "#ffaf6e", (glyph, t) => {
  const c = Math.cos(t * 0.38);
  const s = Math.sin(t * 0.38);
  const depth = new Float32Array(80 * 48).fill(-1);
  for (const [x, y, z] of points) {
    const spin = z * c + x * s;
    const v = y * 0.37 + spin * 0.93;
    if (v < -0.28) continue;
    const col = Math.round(61 + (x * c - z * s) * 15);
    const row = Math.round(25 + (y * 0.93 - spin * 0.37) * 17);
    const k = row * 80 + col;
    depth[k] = Math.max(depth[k], v);
  }
  for (let row = 8; row <= 42; row++) {
    for (let col = 46; col <= 76; col++) {
      const z = depth[row * 80 + col];
      if (z < -0.28) continue;
      const scan = wrap(row + 2 * Math.sin(col * 0.23) - t * 8, 13) < 1.5;
      const i = Math.min(6, Math.floor((z + 0.28) * 4 + (scan ? 2 : 0)));
      glyph(".:+*o#@"[i], col, row, Math.min(0.95, 0.22 + i * 0.105));
    }
  }
});

const streams = Array.from({ length: 80 }, (_, x) => {
  const rim = Math.min(x, 79 - x);
  return { x, rim, start: hash(x, 1) * 64, length: 6 + hash(x, 5) * 13, speed: 8 + hash(x, 2) * 11 };
}).filter(({ x, rim }) => rim <= 23 && hash(x, 3) >= (rim > 11 ? 0.64 : 0.25));

export const stackRain = ascii("#070e0b", "#9cf9ae", (glyph, t) => {
  const hex = "0123456789ABCDEF:+=*";
  for (const { x, rim, start, length, speed } of streams) {
    const pos = wrap(start + t * speed, 64);
    for (let y = 0; y < 48; y++) {
      const tail = wrap(pos - y, 64);
      if (tail > length) continue;
      const char = hex[Math.floor(hash(x + y * 81, Math.floor(t * 2 + x * 0.3)) * hex.length)];
      const opacity = tail < 1 ? 0.94 : Math.max(0.06, (1 - tail / length) * 0.59);
      glyph(tail < 1 ? "+" : char, x, y, opacity * (rim < 12 ? 1 : 0.52));
    }
  }
});
