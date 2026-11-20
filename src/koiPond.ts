// "Koi Pond": a top-down pond in ASCII. Koi glide along smooth loops with bending bodies and
// beating fins under drifting lily pads, raindrop rings spread on the water and ripples shimmer.
// The scene is a character grid computed as a pure function of time (`koiFrame`), so each frame
// is deterministic and the grid can be checked without a canvas.

import type { Painter } from "./backgrounds";

/** One character cell: what to draw, in which colour, how strongly. */
export interface Frame {
  cols: number;
  rows: number;
  chars: string[];
  colors: string[];
  alpha: Float32Array;
}

/** A cell is this many times taller than wide (monospace glyph proportions). */
export const CELL_ASPECT = 1.9;
/** The pond is this many world units wide; sizes below are in these units. */
const WORLD = 150;
const TAU = Math.PI * 2;

function rng(seed: number) {
  return () => {
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** The ASCII stroke that best follows direction (dx, dy), y pointing down. */
export function strokeChar(dx: number, dy: number) {
  const a = ((Math.atan2(dy, dx) % Math.PI) + Math.PI) % Math.PI;
  if (a < Math.PI / 8 || a >= (7 * Math.PI) / 8) return "-";
  if (a < (3 * Math.PI) / 8) return "\\";
  if (a < (5 * Math.PI) / 8) return "|";
  return "/";
}

type Pattern = (f: number, side: number, d: number, seed: number) => string;

const WHITE = "#f4efe6";
const RED = "#e8462b";
const patterns: Record<string, Pattern> = {
  // White with red patches along the back.
  kohaku: (f, side, _d, s) => (Math.sin(f * 9 + s) + 0.6 * Math.sin(side * 2.2 + f * 4 + s * 2) > 0.35 ? RED : WHITE),
  // Metallic gold, brighter down the spine.
  ogon: (_f, _side, d) => (d < 0.4 ? "#ffe08a" : "#f5a524"),
  // White, red and black.
  sanke: (f, side, _d, s) => {
    if (Math.sin(f * 17 + s * 3) * Math.sin(side * 3 + s) > 0.55) return "#2b2b31";
    return Math.sin(f * 7 + s) > 0.1 ? "#df3b26" : WHITE;
  },
  // Orange with a white head.
  orange: (f, _side, d) => (f < 0.22 && d < 0.8 ? WHITE : d < 0.45 ? "#ff9a4d" : "#ff6f2a"),
  // White with one red crown.
  tancho: (f, side) => (Math.hypot((f - 0.1) * 3.2, side * 0.55) < 0.42 ? RED : WHITE),
  // Blue-grey back, orange flanks.
  asagi: (_f, _side, d) => (d > 0.72 ? "#ef7d3c" : "#9cb3c9"),
};

interface Koi {
  cx: number;
  cy: number;
  rx: number;
  ry: number;
  shape: "loop" | "eight";
  dir: 1 | -1;
  speed: number;
  phase: number;
  wob: number;
  len: number;
  width: number;
  pattern: keyof typeof patterns;
  seed: number;
  /** 1 swims at the surface; less is deeper, smaller and dimmer. */
  depth: number;
}

interface Pad {
  x: number;
  y: number;
  r: number;
  notch: number;
  sway: number;
  flower: boolean;
}

const rand = rng(108);
const KOI: Koi[] = (
  [
    ["kohaku", 0.3, 0.38, 0.22, 0.24, "loop", 1, 1],
    ["ogon", 0.66, 0.55, 0.26, 0.3, "loop", -1, 1],
    ["sanke", 0.5, 0.5, 0.36, 0.3, "eight", 1, 1],
    ["orange", 0.74, 0.3, 0.18, 0.2, "loop", 1, 0.8],
    ["tancho", 0.25, 0.68, 0.2, 0.22, "eight", -1, 0.85],
    ["asagi", 0.55, 0.72, 0.3, 0.18, "loop", -1, 0.65],
    ["kohaku", 0.45, 0.25, 0.28, 0.16, "loop", -1, 0.6],
  ] as const
).map(([pattern, cx, cy, rx, ry, shape, dir, depth]) => ({
  pattern,
  cx,
  cy,
  rx,
  ry,
  shape,
  dir,
  depth,
  speed: (0.075 + rand() * 0.05) * (shape === "eight" ? 0.8 : 1),
  phase: rand() * TAU,
  wob: rand() * TAU,
  len: (30 + rand() * 8) * (0.55 + depth * 0.45),
  width: (4.4 + rand() * 0.8) * (0.55 + depth * 0.45),
  seed: rand() * 10,
}));

const PADS: Pad[] = [
  [0.1, 0.18, 9],
  [0.2, 0.1, 5.5],
  [0.86, 0.14, 8],
  [0.94, 0.3, 5],
  [0.58, 0.12, 4.5],
  [0.12, 0.82, 7.5],
  [0.27, 0.9, 5],
  [0.82, 0.84, 10],
  [0.68, 0.94, 5.5],
  [0.47, 0.5, 4],
].map(([x, y, r], i) => ({ x, y, r, notch: rand() * TAU, sway: rand() * TAU, flower: i % 3 === 0 }));

/** Rocks around the edge: blob centres (fractions, may sit off-screen) and size. */
const ROCKS = [
  [-0.02, 0.45, 9],
  [0.03, 0.55, 6],
  [1.01, 0.6, 10],
  [0.97, 0.5, 5],
  [0.38, -0.03, 8],
  [0.45, 1.03, 9],
].map(([x, y, r]) => ({ x, y, r, seed: rand() * 10 }));

/** Raindrops: each lands at a new place every `period` seconds. */
const DROPS = Array.from({ length: 5 }, (_, i) => ({ period: 6 + i * 1.7, offset: rand() * 10, seed: i * 7.3 + 1 }));

const hash = (a: number, b: number) => {
  const x = Math.sin(a * 127.1 + b * 311.7) * 43758.5453;
  return x - Math.floor(x);
};

/** The pond at `t` seconds on a `cols` × `rows` grid. */
export function koiFrame(cols: number, rows: number, t: number): Frame {
  const n = cols * rows;
  const chars: string[] = new Array(n).fill(" ");
  const colors: string[] = new Array(n).fill("");
  const alpha = new Float32Array(n);
  const u = WORLD / cols; // world units per column
  const v = u * CELL_ASPECT; // world units per row
  const W = WORLD;
  const H = rows * v;
  const put = (c: number, r: number, ch: string, color: string, a: number) => {
    if (c < 0 || r < 0 || c >= cols || r >= rows) return;
    const i = r * cols + c;
    chars[i] = ch;
    colors[i] = color;
    alpha[i] = a;
  };
  const cellAt = (x: number, y: number): [number, number] => [Math.floor(x / u), Math.floor(y / v)];
  /** Visit every cell whose centre is within `radius` of (x, y). */
  const disk = (x: number, y: number, radius: number, visit: (c: number, r: number, dx: number, dy: number) => void) => {
    const c0 = Math.max(0, Math.floor((x - radius) / u));
    const c1 = Math.min(cols - 1, Math.floor((x + radius) / u));
    const r0 = Math.max(0, Math.floor((y - radius) / v));
    const r1 = Math.min(rows - 1, Math.floor((y + radius) / v));
    for (let r = r0; r <= r1; r++)
      for (let c = c0; c <= c1; c++) {
        const dx = (c + 0.5) * u - x;
        const dy = (r + 0.5) * v - y;
        if (dx * dx + dy * dy <= radius * radius) visit(c, r, dx, dy);
      }
  };
  const line = (x0: number, y0: number, x1: number, y1: number, color: string, a: number, only?: (i: number) => boolean) => {
    const ch = strokeChar(x1 - x0, y1 - y0);
    const steps = Math.max(1, Math.ceil(Math.hypot(x1 - x0, y1 - y0) / (u * 0.5)));
    for (let s = 0; s <= steps; s++) {
      const [c, r] = cellAt(x0 + ((x1 - x0) * s) / steps, y0 + ((y1 - y0) * s) / steps);
      if (c < 0 || r < 0 || c >= cols || r >= rows || (only && !only(r * cols + c))) continue;
      put(c, r, ch, color, a);
    }
  };

  // Water: shimmering ripple bands.
  for (let r = 0; r < rows; r++)
    for (let c = 0; c < cols; c++) {
      const x = (c + 0.5) * u;
      const y = (r + 0.5) * v;
      const w =
        Math.sin(x * 0.19 + t * 0.55 + Math.sin(y * 0.05 + t * 0.2) * 2) +
        Math.sin(y * 0.16 - t * 0.42 + x * 0.04) +
        0.8 * Math.sin((x + y) * 0.09 + t * 0.31);
      if (w > 2.15) put(c, r, "~", "#5fb6c9", 0.34);
      else if (w > 1.75) put(c, r, "-", "#3f8fa5", 0.24);
      else if (w > 1.35) put(c, r, ".", "#2f7487", 0.2);
      else if (hash(c, r) > 0.985) put(c, r, ".", "#2a6576", 0.16);
    }

  // Raindrop rings, on open water.
  for (const d of DROPS) {
    const cycle = Math.floor((t + d.offset) / d.period);
    const age = (t + d.offset) / d.period - cycle;
    const x = W * (0.1 + 0.8 * hash(cycle, d.seed));
    const y = H * (0.1 + 0.8 * hash(d.seed, cycle + 3));
    for (const lag of [0, 2.2]) {
      const radius = age * 15 - lag;
      if (radius <= 0.4) continue;
      const fade = (1 - age) * (1 - lag / 5);
      const steps = Math.ceil((TAU * radius) / (u * 0.7));
      for (let s = 0; s < steps; s++) {
        const a = (s / steps) * TAU;
        const [c, r] = cellAt(x + Math.cos(a) * radius, y + Math.sin(a) * radius);
        put(c, r, lag ? "." : strokeChar(-Math.sin(a), Math.cos(a)), "#a6dcea", 0.4 * fade);
      }
    }
    if (age < 0.06) put(...cellAt(x, y), "o", "#d6f3fa", 0.8);
  }

  // Koi, deepest first.
  for (const k of [...KOI].sort((a, b) => a.depth - b.depth)) drawKoi(k);

  function drawKoi(k: Koi) {
    const at = (th: number): [number, number] =>
      k.shape === "loop"
        ? [
            W * (k.cx + k.rx * Math.cos(th) + 0.05 * Math.sin(2.3 * th + k.wob)),
            H * (k.cy + k.ry * Math.sin(th) + 0.05 * Math.sin(1.7 * th + k.wob)),
          ]
        : [W * (k.cx + k.rx * Math.sin(th)), H * (k.cy + k.ry * Math.sin(2 * th) * 0.9)];
    const segments = 26;
    const spacing = k.len / segments;
    // Spine from head to tail, evenly spaced along the path behind the head.
    const spine: [number, number][] = [];
    let th = k.phase + k.dir * t * k.speed;
    for (let i = 0; i <= segments; i++) {
      spine.push(at(th));
      const [ax, ay] = at(th);
      const [bx, by] = at(th - k.dir * 0.001);
      const step = Math.hypot(ax - bx, ay - by) / 0.001 || 1;
      th -= (k.dir * spacing) / step;
    }
    // Swimming: a wave travels down the body, growing toward the tail.
    const beat = t * 5.5 * (k.speed / 0.1);
    const body = spine.map(([x, y], i) => {
      const f = i / segments;
      const [nx, ny] = spine[Math.min(segments, i + 1)];
      const [px, py] = spine[Math.max(0, i - 1)];
      let tx = px - nx;
      let ty = py - ny;
      const tl = Math.hypot(tx, ty) || 1;
      tx /= tl;
      ty /= tl;
      const sway = Math.sin(f * 4.2 - beat) * k.width * 0.55 * Math.pow(f, 1.3);
      const radius = k.width * (f < 0.25 ? 0.6 + 1.6 * f : Math.max(0.14, Math.pow((1 - f) / 0.75, 0.85)));
      return { x: x - ty * sway, y: y + tx * sway, tx, ty, f, radius };
    });
    const dim = 0.45 + 0.5 * k.depth;
    const paint = patterns[k.pattern];
    // Fins first, so the body covers their roots.
    const finColor = paint(0.3, 0, 0.9, k.seed) === "#2b2b31" ? WHITE : paint(0.3, 0, 0.9, k.seed);
    const pec = body[Math.round(segments * 0.2)];
    const flap = 0.5 + 0.35 * Math.sin(beat * 0.7);
    for (const side of [-1, 1]) {
      const ox = -pec.ty * side;
      const oy = pec.tx * side;
      const reach = k.width * 1.2;
      const ex = pec.x + (ox * Math.cos(flap) - pec.tx * Math.sin(flap)) * reach;
      const ey = pec.y + (oy * Math.cos(flap) - pec.ty * Math.sin(flap)) * reach;
      line(pec.x + ox * pec.radius * 0.6, pec.y + oy * pec.radius * 0.6, ex, ey, finColor, 0.55 * dim);
    }
    const tail = body[segments];
    const tailBeat = Math.sin(beat - 4.2);
    for (const side of [-1, 0, 1]) {
      const spread = side * 0.55 + tailBeat * 0.35;
      const reach = k.width * (side ? 1.5 : 1.0);
      const dx = -tail.tx * Math.cos(spread) + tail.ty * Math.sin(spread);
      const dy = -tail.ty * Math.cos(spread) - tail.tx * Math.sin(spread);
      line(tail.x, tail.y, tail.x + dx * reach, tail.y + dy * reach, finColor, (side ? 0.6 : 0.45) * dim);
    }
    // Body: each cell takes the nearest spine disk that covers it.
    const best = new Map<number, { d: number; f: number; side: number }>();
    for (const b of body) {
      disk(b.x, b.y, b.radius, (c, r, dx, dy) => {
        const d = Math.hypot(dx, dy) / b.radius;
        const i = r * cols + c;
        const prev = best.get(i);
        if (!prev || d < prev.d) best.set(i, { d, f: b.f, side: (dx * -b.ty + dy * b.tx) / b.radius });
      });
    }
    for (const [i, { d, f, side }] of best) {
      const ch = f < 0.07 ? "@" : d < 0.38 ? "#" : d < 0.75 ? "%" : "*";
      put(i % cols, Math.floor(i / cols), ch, paint(f, side, d, k.seed), (d > 0.75 ? 0.72 : 0.95) * dim);
    }
    // Eyes.
    const head = body[2];
    for (const side of [-1, 1]) {
      const [c, r] = cellAt(head.x - head.ty * side * head.radius * 0.7, head.y + head.tx * side * head.radius * 0.7);
      put(c, r, "o", "#141418", 0.9 * dim);
    }
  }

  // Lily pads float above the fish.
  for (const p of PADS) {
    const x = W * p.x + Math.sin(t * 0.07 + p.sway) * 0.8;
    const y = H * p.y + Math.cos(t * 0.06 + p.sway) * 0.8;
    const turn = p.notch + 0.18 * Math.sin(t * 0.05 + p.sway);
    disk(x, y, p.r, (c, r, dx, dy) => {
      const rho = Math.hypot(dx, dy) / p.r;
      const ang = Math.atan2(dy, dx);
      const fromNotch = Math.abs(((ang - turn + 3 * Math.PI) % TAU) - Math.PI);
      if (fromNotch < 0.3 && rho > 0.12) return;
      const vein = Math.abs(((ang - turn) * 9) / TAU - Math.round(((ang - turn) * 9) / TAU));
      if (rho > 0.82) put(c, r, strokeChar(-dy, dx), "#9ccc65", 0.92);
      else if (vein < 0.12 && rho > 0.2) put(c, r, ":", "#7fb069", 0.85);
      else put(c, r, rho < 0.2 ? "o" : "#", rho < 0.2 ? "#6aa84f" : "#3f7d3a", 0.78);
    });
    if (p.flower) {
      const fx = x - Math.cos(turn) * p.r * 0.35;
      const fy = y - Math.sin(turn) * p.r * 0.35;
      for (let i = 0; i < 8; i++) {
        const a = (i / 8) * TAU + t * 0.02;
        const [c, r] = cellAt(fx + Math.cos(a) * 1.9, fy + Math.sin(a) * 1.9);
        put(c, r, i % 2 ? "*" : "@", i % 2 ? "#f7a8c8" : "#fbd3e4", 0.95);
      }
      put(...cellAt(fx, fy), "o", "#ffd54a", 1);
    }
  }

  // Rocks frame the edge.
  for (const s of ROCKS) {
    const x = W * s.x;
    const y = H * s.y;
    disk(x, y, s.r * 1.35, (c, r, dx, dy) => {
      const ang = Math.atan2(dy, dx);
      const edge = s.r * (1 + 0.22 * Math.sin(ang * 3 + s.seed) + 0.12 * Math.sin(ang * 7 + s.seed * 2));
      const rho = Math.hypot(dx, dy) / edge;
      if (rho > 1) return;
      const moss = hash(c + s.seed, r) > 0.82 && dy < 0;
      put(c, r, rho > 0.85 ? "%" : rho > 0.5 ? "#" : "@", moss ? "#5e8c45" : rho > 0.6 ? "#575c61" : "#71767b", 0.8);
    });
  }

  return { cols, rows, chars, colors, alpha };
}

/** Columns for a pane `w` pixels wide: glyphs about 10 px wide, never too coarse or fine. */
export function columnsFor(w: number) {
  return Math.max(40, Math.min(170, Math.round(w / 10)));
}

export const koiPond: Painter = (ctx, w, h, t) => {
  const bg = ctx.createRadialGradient(w * 0.5, h * 0.48, 0, w * 0.5, h * 0.5, Math.max(w, h) * 0.75);
  bg.addColorStop(0, "#0c2c35");
  bg.addColorStop(0.6, "#071a21");
  bg.addColorStop(1, "#030a0e");
  ctx.fillStyle = bg;
  ctx.fillRect(0, 0, w, h);
  const cols = columnsFor(w);
  const cw = w / cols;
  const ch = cw * CELL_ASPECT;
  const rows = Math.ceil(h / ch);
  const frame = koiFrame(cols, rows, t);
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
