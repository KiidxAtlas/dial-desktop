// "Crab Beach": a sunset beach in ASCII. The sun sinks into a striped horizon over glittering
// water, waves roll in and wash up the sand, a palm sways, gulls and a sailboat drift by, and
// crabs scuttle sideways from spot to spot, pausing to snap their claws. Like the koi pond, the
// scene is a character grid computed as a pure function of time (`beachFrame`).

import type { Painter } from "./backgrounds";
import { CELL_ASPECT, columnsFor, strokeChar, type Frame } from "./koiPond";

export interface BeachFrame extends Frame {
  /** First row of sea. */
  horizon: number;
  /** For each column, the fractional row where the washing water ends. */
  edge: Float32Array;
  /** The lowest row the swash reaches; dry sand starts below it. */
  wet: number;
  /** Sun centre (column, row) and radius in rows. */
  sun: [number, number, number];
}

const hash = (a: number, b: number) => {
  const x = Math.sin(a * 127.1 + b * 311.7) * 43758.5453;
  return x - Math.floor(x);
};
const wrap = (x: number, p: number) => ((x % p) + p) % p;
const smooth = (x: number) => {
  const c = Math.min(1, Math.max(0, x));
  return c * c * (3 - 2 * c);
};
const mix = (a: string, b: string, f: number) => {
  const pa = parseInt(a.slice(1), 16);
  const pb = parseInt(b.slice(1), 16);
  const ch = (s: number) => Math.round(((pa >> s) & 255) * (1 - f) + ((pb >> s) & 255) * f);
  return `rgb(${ch(16)} ${ch(8)} ${ch(0)})`;
};

// ---------------------------------------------------------------------------------------------
// Sprites: rows of text, spaces transparent.

// Claws raised on bent arms, eyes up on stalks, a domed shell, and legs splayed to both sides
// that alternate as the crab scuttles. Symmetric so the pincer swap for a snap reads cleanly.
const BIG_CRAB = {
  body: [
    "(\\/)    o   o    (\\/)",
    " \\ \\    |   |    / /",
    "  \\ \\.--'---'--./ /",
    "   \\(  .  .  .  )/",
    "   _/'-._____.-'\\_",
  ],
  legs: ["  / / /       \\ \\ \\", "  \\ \\ \\       / / /"],
};
const SMALL_CRAB = {
  body: ["(\\/) o o (\\/)", " \\_\\(_._)/_/"],
  legs: ["  /\\/\\ /\\/\\", "  \\/\\/ \\/\\/"],
};

const CLOUDS = [
  ["       _.--._", "   .-(        )-.", "  (___.____.____)"],
  ["     .--.", "  .-(    ).", " (___.__)__)"],
  ["   _  _", " (  `   )_", "(    )    `)", " `--'`---'"],
];

const BOAT = ["  |\\", "  |_\\", "\\___/"];

const CASTLE = ["    |>", " _  |  _", "| |_|_| |", "|  _ _  |", "|_| |_|_|"];

interface Crab {
  seed: number;
  big: boolean;
  /** Lane across the dry sand, 0 at the wet edge to 1 at the bottom. */
  lane: number;
  /** Home column (fraction) and how far either side it roams. */
  home: number;
  range: number;
  /** Seconds per hop, and the share of it spent running. */
  hop: number;
  move: number;
  offset: number;
}

// Lanes and roaming ranges are spread so crabs rarely walk through each other.
const CRABS: Crab[] = [
  { seed: 1, big: true, lane: 0.5, home: 0.3, range: 0.13, hop: 3.4, move: 0.42, offset: 0 },
  { seed: 2, big: true, lane: 0.95, home: 0.62, range: 0.15, hop: 4.1, move: 0.38, offset: 1.7 },
  { seed: 3, big: false, lane: 0.05, home: 0.5, range: 0.2, hop: 2.6, move: 0.45, offset: 0.4 },
  { seed: 4, big: false, lane: 0.3, home: 0.62, range: 0.1, hop: 2.2, move: 0.5, offset: 1.1 },
  { seed: 5, big: false, lane: 0.92, home: 0.26, range: 0.1, hop: 3, move: 0.4, offset: 2.3 },
  { seed: 6, big: false, lane: 0.1, home: 0.84, range: 0.06, hop: 3.7, move: 0.35, offset: 0.9 },
];

const SHELLS = [
  [0.16, 0.25, "@", "#f2d3b3"],
  [0.47, 0.9, "*", "#ff8a65"],
  [0.58, 0.18, "@", "#efd9c4"],
  [0.33, 0.78, "*", "#ffb07a"],
  [0.78, 0.55, "@", "#e9c4a4"],
  [0.93, 0.92, "*", "#ff7f5c"],
  [0.24, 0.55, "o", "#f6e6d6"],
] as const;

/** Where crab `c` is at `t`: column, lane fraction, whether running and claw pose. */
export function crabAt(c: Crab, cols: number, t: number) {
  const s = (t + c.offset) / c.hop;
  const k = Math.floor(s);
  const p = s - k;
  const q = smooth(p / c.move);
  const wx = (n: number) => cols * (c.home + c.range * (2 * hash(c.seed, n) - 1));
  const wy = (n: number) => Math.min(1, Math.max(0, c.lane + 0.05 * (2 * hash(c.seed + 0.37, n) - 1)));
  return {
    x: wx(k) + (wx(k + 1) - wx(k)) * q,
    y: wy(k) + (wy(k + 1) - wy(k)) * q,
    running: p < c.move,
    snap: p >= c.move && Math.sin(t * 7 + c.seed * 5) > 0.55,
  };
}

/** The beach at `t` seconds on a `cols` × `rows` grid. */
export function beachFrame(cols: number, rows: number, t: number): BeachFrame {
  const n = cols * rows;
  const chars: string[] = new Array(n).fill(" ");
  const colors: string[] = new Array(n).fill("");
  const alpha = new Float32Array(n);
  const put = (c: number, r: number, ch: string, color: string, a: number) => {
    c = Math.round(c);
    r = Math.round(r);
    if (c < 0 || r < 0 || c >= cols || r >= rows) return;
    const i = r * cols + c;
    chars[i] = ch;
    colors[i] = color;
    alpha[i] = a;
  };
  /** Draw `rows` of text with its top-left at (c, r); inner spaces are opaque when `solid`. */
  const sprite = (lines: readonly string[], c: number, r: number, color: (ch: string, row: number) => string, a: number, solid = false) => {
    c = Math.round(c);
    r = Math.round(r);
    lines.forEach((line, dy) => {
      const first = line.search(/\S/);
      const last = line.trimEnd().length - 1;
      for (let dx = 0; dx < line.length; dx++) {
        const ch = line[dx];
        if (ch === " " && !(solid && dx > first && dx < last)) continue;
        put(c + dx, r + dy, ch, color(ch, dy), ch === " " ? 0 : a);
      }
    });
  };

  const horizon = Math.round(rows * 0.38);
  const shore = horizon + Math.max(5, Math.round(rows * 0.17));
  const wet = shore + 4;
  const sandTop = wet + 1;
  const sandRows = Math.max(1, rows - sandTop - 2);
  const edge = new Float32Array(cols);
  for (let c = 0; c < cols; c++)
    edge[c] = shore + 0.6 + 1.6 * Math.sin(t * 0.5 + c * 0.03) + 0.9 * Math.sin(t * 0.83 + c * 0.071 + 1.3);

  // Stars fading in at the top of the dusk sky.
  for (let r = 0; r < horizon * 0.55; r++)
    for (let c = 0; c < cols; c++) {
      const h = hash(c, r);
      if (h < 0.988) continue;
      const twinkle = 0.5 + 0.5 * Math.sin(t * (1 + h * 2) + h * 40);
      put(c, r, h > 0.997 ? "+" : ".", "#fdf3e0", (0.15 + 0.45 * twinkle) * (1 - r / (horizon * 0.55)));
    }

  // The sun, half set, with stripes cut through its lower part.
  const sunR = Math.max(3, rows * 0.13);
  const sunX = cols * 0.7;
  const sunY = horizon - sunR * 0.25;
  for (let r = Math.floor(sunY - sunR); r < horizon; r++)
    for (let c = Math.floor(sunX - sunR * CELL_ASPECT); c <= sunX + sunR * CELL_ASPECT; c++) {
      const d = Math.hypot((c + 0.5 - sunX) / (sunR * CELL_ASPECT), (r + 0.5 - sunY) / sunR);
      if (d > 1) continue;
      const below = r + 0.5 - (sunY - sunR * 0.15);
      if (below > 0 && wrap(horizon - r, 3) === 2 - (Math.floor(below / 3) % 2)) continue;
      const f = (r + 0.5 - (sunY - sunR)) / (horizon - (sunY - sunR));
      put(c, r, d < 0.55 ? "@" : d < 0.85 ? "#" : "*", mix("#ffe48a", "#ff6a3d", f), d < 0.85 ? 0.95 : 0.7);
    }

  // Clouds drifting in front of the sky, lit pink from below.
  CLOUDS.forEach((cloud, i) => {
    const width = Math.max(...cloud.map((l) => l.length));
    const span = cols + width + 10;
    const x = wrap(cols * (0.15 + i * 0.37) + t * (0.35 + i * 0.12), span) - width - 5;
    const y = horizon * (0.12 + i * 0.22);
    sprite(cloud, x, y, (_ch, dy) => mix("#f6c7c0", "#f08a7a", dy / cloud.length), 0.55, true);
  });

  // Gulls gliding and flapping.
  for (let i = 0; i < 4; i++) {
    const span = cols + 20;
    const x = wrap(cols * hash(i, 9) + t * (2 + i * 0.7), span) - 10;
    const y = horizon * (0.25 + 0.15 * i) + 1.2 * Math.sin(t * 0.6 + i * 2);
    const wing = Math.sin(t * (5 + i) + i * 3);
    sprite([wing > 0.3 ? "\\v/" : wing < -0.3 ? "_v_" : "~v~"], x, y, () => "#f9eadc", 0.75);
  }

  // The open sea: rippling bands, and the sun's glitter path.
  for (let r = horizon; r < shore - 5; r++) {
    const f = (r - horizon) / Math.max(1, shore - 5 - horizon);
    for (let c = 0; c < cols; c++) {
      const half = sunR * CELL_ASPECT * (0.45 + 0.9 * f) * (0.75 + 0.25 * Math.sin(r * 1.7 + t * 0.8));
      const off = Math.abs(c + 0.5 - sunX);
      if (off < half && hash(c, r * 31 + Math.floor(t * 5 + c * 0.13)) > 0.45) {
        const core = 1 - off / half;
        put(c, r, core > 0.5 ? "=" : "-", mix("#ffd27a", "#ff8a4c", f), 0.35 + 0.55 * core);
        continue;
      }
      const w = Math.sin(c * 0.23 - t * 0.6 + r * 1.3) + Math.sin(c * 0.11 + t * 0.35 - r * 0.6);
      const fleck = hash(c, r * 13 + Math.floor(t * 0.7 + c * 0.05));
      if (w > 1.5 && fleck > 0.35) put(c, r, "~", "#8fc3d6", 0.18 + 0.3 * f);
      else if (w > 1.25 && fleck > 0.6) put(c, r, "-", "#6aa3b8", 0.12 + 0.2 * f);
    }
  }

  // A sailboat on the horizon.
  {
    const span = cols + 12;
    const x = wrap(cols * 0.2 + t * 0.4, span) - 6;
    sprite(BOAT, x, horizon - 2, () => "#f3e3cf", 0.7);
  }

  // Waves rolling in, the swash, and its foam edge.
  for (let c = 0; c < cols; c++) {
    const e = edge[c];
    for (let r = shore - 5; r < e - 0.5; r++) {
      const w = Math.sin(c * 0.19 + t * 0.5 + r * 0.9) + Math.sin(c * 0.05 - t * 0.3);
      if (w > 1.6) put(c, r, "~", "#9fd3df", 0.3);
      else if (hash(c, r + Math.floor(t * 3)) > 0.93) put(c, r, ".", "#cfeef2", 0.35);
    }
    for (let k = 0; k < 2; k++) {
      const front = shore - 5 + wrap(t * 0.9 + k * 2.5, 5) + 0.6 * Math.sin(c * 0.09 + k);
      const gap = Math.sin(c * 0.17 + k * 3 + t * 0.2) + 0.5 * Math.sin(c * 0.41 + k);
      if (front >= e - 0.8 || gap < -0.3) continue;
      const near = (front - (shore - 5)) / 5;
      put(c, Math.floor(front), near > 0.55 ? "~" : "-", "#dff5f7", 0.25 + 0.55 * near);
    }
    const fr = Math.floor(e);
    put(c, fr, "~", "#f4fbfc", 0.9);
    if (hash(c, Math.floor(t * 4)) > 0.55) put(c, fr - 1, hash(c, 3) > 0.5 ? "o" : ".", "#e6f7f9", 0.55);
    // Wet sand left behind by the retreating water.
    for (let r = fr + 1; r <= wet; r++) if (hash(c, r * 7) > 0.84) put(c, r, ",", "#b08a63", 0.3);
  }

  // Dry sand, a little denser nearer the viewer.
  for (let r = sandTop; r < rows; r++) {
    const f = (r - sandTop) / sandRows;
    for (let c = 0; c < cols; c++) {
      const h = hash(c * 1.3, r * 2.1);
      if (h > 0.985) put(c, r, "'", "#f0caa0", 0.35 + 0.2 * f);
      else if (h > 0.95) put(c, r, ":", "#e0b583", 0.22 + 0.15 * f);
      else if (h > 0.86) put(c, r, ".", "#e0b583", 0.16 + 0.16 * f);
    }
  }
  for (const [x, y, ch, color] of SHELLS) put(cols * x, sandTop + y * sandRows, ch, color, 0.85);

  // A sandcastle with a fluttering flag.
  {
    const x = cols * 0.88 - 4;
    const y = sandTop + sandRows * 0.42 - 2;
    const flag = [Math.sin(t * 4) > 0 ? "    |>" : "    |~", ...CASTLE.slice(1)];
    sprite(flag, x, y, (ch, dy) => (dy === 0 && ch !== "|" ? "#ff6b4a" : "#e6b980"), 0.85);
  }

  // A palm leaning over the beach from the left.
  {
    const baseX = cols * 0.07;
    const baseY = sandTop + sandRows * 0.55;
    const topY = horizon * 0.3;
    const sway = Math.sin(t * 0.7) * 0.8;
    const at = (p: number) => baseX + cols * 0.1 * Math.pow(p, 1.5) + sway * p * p;
    // A ringed trunk three cells wide: shaded edges that follow the lean, lit from the sun's side.
    for (let r = Math.round(baseY); r >= topY + 1; r--) {
      const p = (baseY - r) / (baseY - topY);
      const x = at(p);
      const slope = at(p + 0.02) - x;
      const edge = strokeChar(slope / (0.02 * (baseY - topY)), -1 / CELL_ASPECT);
      put(x - 1, r, edge, "#6e4a2e", 0.92);
      put(x, r, r % 2 ? "=" : "-", "#a8784f", 0.92);
      put(x + 1, r, edge, "#d39d68", 0.92);
    }
    // A little mound of sand where it roots.
    sprite([".-'^'-."], at(0) - 3, baseY + 1, () => "#d8a978", 0.55);
    const cx = at(1);
    const cy = topY;
    const fronds = [0.05, 0.5, 1.05, 2.1, 2.65, 3.1];
    fronds.forEach((a0, i) => {
      const a = a0 + 0.07 * Math.sin(t * 0.9 + i * 1.3);
      const len = cols * (0.08 + 0.025 * hash(i, 4)) * (a0 > 1 && a0 < 2.2 ? 0.75 : 1);
      const droop = len * 0.9;
      const side = Math.cos(a0) >= 0 ? 1 : -1;
      let px = cx;
      let py = cy;
      for (let s = 1; s <= 32; s++) {
        const u = s / 32;
        const x = cx + Math.cos(a) * len * u;
        const yIso = -Math.sin(a) * len * u * 0.6 + droop * u * u;
        const y = cy + yIso / CELL_ASPECT;
        if (Math.round(x) !== Math.round(px) || Math.round(y) !== Math.round(py)) {
          // Leaflets hang off the underside of each frond, slanting outwards, thinning to the tip.
          if (u > 0.15 && s % 2 === 0) {
            const leaf = u > 0.8 ? (u > 0.9 ? "," : "'") : side > 0 ? "\\" : "/";
            put(x, y + 1, leaf, u < 0.55 ? "#4f9a47" : "#3f8a3e", 0.85 - 0.3 * u);
          }
          put(x, y, strokeChar(x - px, (y - py) * CELL_ASPECT), u < 0.5 ? "#8bd873" : "#64b056", 0.95);
        }
        px = x;
        py = y;
      }
    });
    sprite(["o@o"], cx - 1, cy + 1, (ch) => (ch === "@" ? "#7a4a22" : "#9a6634"), 0.95);
  }

  // Crabs, farthest first, scuttling sideways from spot to spot.
  const placed = CRABS.map((c) => ({ c, ...crabAt(c, cols, t) })).sort((a, b) => a.y - b.y);
  for (const { c, x, y, running, snap } of placed) {
    const art = c.big ? BIG_CRAB : SMALL_CRAB;
    const step = running ? Math.floor(t * 14 + c.seed) % 2 : 0;
    const body = art.body.map((l) => (snap ? l.replaceAll("(\\/)", "(||)") : l));
    const lines = [...body, art.legs[step]];
    const width = lines[0].length;
    const top = sandTop + y * sandRows - lines.length + 1;
    sprite(
      lines,
      x - width / 2,
      top,
      (ch, dy) => (ch === "o" ? "#fff6e8" : ch === "," ? "#ffd9c9" : dy === lines.length - 1 ? "#d9432b" : "#ff6242"),
      0.97,
      true,
    );
  }

  return { cols, rows, chars, colors, alpha, horizon, edge, wet, sun: [sunX, sunY, sunR] };
}

export const crabBeach: Painter = (ctx, w, h, t) => {
  const cols = columnsFor(w);
  const cw = w / cols;
  const ch = cw * CELL_ASPECT;
  const rows = Math.ceil(h / ch);
  const frame = beachFrame(cols, rows, t);
  const hy = frame.horizon * ch;

  const sky = ctx.createLinearGradient(0, 0, 0, hy);
  sky.addColorStop(0, "#121634");
  sky.addColorStop(0.45, "#3a2550");
  sky.addColorStop(0.8, "#8c3f5e");
  sky.addColorStop(1, "#d8704f");
  ctx.fillStyle = sky;
  ctx.fillRect(0, 0, w, hy);

  const sandY = (frame.wet - 6) * ch;
  const sea = ctx.createLinearGradient(0, hy, 0, sandY);
  sea.addColorStop(0, "#5a3a5c");
  sea.addColorStop(0.35, "#1f3552");
  sea.addColorStop(1, "#174a5a");
  ctx.fillStyle = sea;
  ctx.fillRect(0, hy, w, sandY - hy);

  // Sand, warm near the water where it catches the sunset and darkening towards the viewer.
  const sand = ctx.createLinearGradient(0, sandY, 0, h);
  sand.addColorStop(0, "#5e412d");
  sand.addColorStop(0.45, "#3d2a1f");
  sand.addColorStop(1, "#211611");
  ctx.fillStyle = sand;
  ctx.fillRect(0, sandY, w, h - sandY);

  // Water washing up the sand, then the damp band it has just left, fading as it dries.
  const wetY = (frame.wet + 1) * ch;
  const damp = ctx.createLinearGradient(0, sandY, 0, wetY);
  damp.addColorStop(0, "rgb(20 12 8 / 0.4)");
  damp.addColorStop(1, "rgb(20 12 8 / 0)");
  for (let c = 0; c < cols; c++) {
    const e = (frame.edge[c] + 0.5) * ch;
    ctx.fillStyle = "#1d5a66";
    ctx.fillRect(c * cw, sandY, cw + 0.5, e - sandY);
    ctx.fillStyle = damp;
    ctx.fillRect(c * cw, e, cw + 0.5, wetY - e);
  }

  const [sx, sy, sr] = frame.sun;
  const gx = (sx + 0.5) * cw;
  const gy = (sy + 0.5) * ch;
  const glow = ctx.createRadialGradient(gx, gy, 0, gx, gy, sr * ch * 3.2);
  glow.addColorStop(0, "rgb(255 170 100 / 0.45)");
  glow.addColorStop(1, "rgb(255 170 100 / 0)");
  ctx.fillStyle = glow;
  ctx.fillRect(0, 0, w, sandY);

  // The sunset reflected faintly on the sand below the sun.
  const sheen = ctx.createRadialGradient(gx, sandY, 0, gx, sandY, w * 0.4);
  sheen.addColorStop(0, "rgb(255 150 90 / 0.14)");
  sheen.addColorStop(1, "rgb(255 150 90 / 0)");
  ctx.fillStyle = sheen;
  ctx.fillRect(0, sandY, w, h - sandY);

  ctx.save();
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.font = `600 ${ch * 0.92}px ui-monospace, "SF Mono", Menlo, monospace`;
  let color = "";
  for (let i = 0; i < frame.chars.length; i++) {
    const a = frame.alpha[i];
    if (a <= 0 || frame.chars[i] === " ") continue;
    if (frame.colors[i] !== color) ctx.fillStyle = color = frame.colors[i];
    ctx.globalAlpha = a;
    ctx.fillText(frame.chars[i], ((i % cols) + 0.5) * cw, (Math.floor(i / cols) + 0.5) * ch);
  }
  ctx.restore();
};
