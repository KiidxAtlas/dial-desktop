// Artwork behind the conversation: built-in canvas presets or the user's own image.
// The choice is a preference of this computer, kept in local storage.

import { computed, reactive, ref, watch } from "vue";
import { lighthouse, traceRoute } from "./asciiBackgrounds";
import { crabBeach } from "./crabBeach";
import { cloudprint } from "./cloudprint";
import { orbitalEarth } from "./orbitalEarth";
import { koiPond } from "./koiPond";
import { lanternWatch, prepareLanternWatch } from "./lanternWatch";
import { files, showToast } from "./bridge";

/**
 * Paints a whole background onto `ctx`, `w` by `h` device pixels, as it looks `time` seconds in.
 * Deterministic: a moving background is a pure function of the time, so frames need no state.
 */
export type Painter = (ctx: CanvasRenderingContext2D, w: number, h: number, time: number) => void;

export interface Preset {
  id: string;
  name: string;
  paint: Painter;
  prepare?: () => Promise<void>;
  /** Animated; still ones ignore the time. */
  moving?: boolean;
}

export type Placement = "always" | "empty";

export interface BackgroundPrefs {
  /** A preset id, `"custom"` for the user's image, or `"none"`. */
  id: string;
  /** The user's image file, kept when another background is chosen. */
  path: string | null;
  /** How much a conversation darkens the background, 0 to `MAX_DIM`. */
  dim: number;
  /** Behind every session, or only behind an empty one. */
  placement: Placement;
  /** Animated backgrounds move; off keeps them still. */
  motion: boolean;
}

export const MAX_DIM = 0.9;
export const DEFAULT_PREFS: BackgroundPrefs = { id: "lantern-watch", path: null, dim: 0.62, placement: "always", motion: true };
const STORAGE_KEY = "dial.background";

// ---------------------------------------------------------------------------------------------
// Painting helpers

/** A seeded random source (mulberry32), so a preset looks the same every time. */
function rng(seed: number): () => number {
  return () => {
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** One "pixel" of the design, which was drawn for a 1600-pixel-wide pane. */
const unitOf = (w: number, h: number) => Math.max(w, h) / 1600;
const TAU = Math.PI * 2;

/** Star field: `density` stars per 1600×1000 design pixels, above `maxY`. */
function stars(ctx: CanvasRenderingContext2D, w: number, h: number, rand: () => number, density: number, maxY = h) {
  const u = unitOf(w, h);
  const count = Math.round((density * (w * maxY)) / (u * u * 1_600_000));
  ctx.fillStyle = "#ffffff";
  for (let i = 0; i < count; i++) {
    const big = rand() > 0.96;
    ctx.globalAlpha = (big ? 0.6 : 0.15) + rand() * 0.5;
    ctx.beginPath();
    ctx.arc(rand() * w, rand() * maxY, Math.max(0.35, (big ? 1.1 + rand() * 0.6 : 0.45 + rand() * 0.5) * u), 0, TAU);
    ctx.fill();
  }
  ctx.globalAlpha = 1;
}

function vertical(ctx: CanvasRenderingContext2D, h: number, stops: [number, string][]) {
  const g = ctx.createLinearGradient(0, 0, 0, h);
  for (const [at, color] of stops) g.addColorStop(at, color);
  return g;
}

/** A soft round light, transparent at its edge. */
function glow(ctx: CanvasRenderingContext2D, x: number, y: number, r: number, rgb: string, alpha: number) {
  const g = ctx.createRadialGradient(x, y, 0, x, y, r);
  g.addColorStop(0, `rgb(${rgb} / ${alpha})`);
  g.addColorStop(1, `rgb(${rgb} / 0)`);
  ctx.fillStyle = g;
  ctx.fillRect(x - r, y - r, r * 2, r * 2);
}

/** A filled ridge line: the sum of sines `waves` ([cycles across, phase, share]) around `base`. */
function ridge(
  ctx: CanvasRenderingContext2D,
  w: number,
  h: number,
  base: number,
  amp: number,
  waves: [number, number, number][],
  fill: string | CanvasGradient,
) {
  const step = Math.max(1, unitOf(w, h) * 3);
  ctx.beginPath();
  ctx.moveTo(0, h);
  for (let x = 0; x <= w + step; x += step) {
    let y = 0;
    for (const [cycles, phase, share] of waves) y += Math.sin((x / w) * cycles * TAU + phase) * share;
    ctx.lineTo(x, base + y * amp);
  }
  ctx.lineTo(w, h);
  ctx.closePath();
  ctx.fillStyle = fill;
  ctx.fill();
}

// ---------------------------------------------------------------------------------------------
// The presets

/** Light streaks sweeping around a far horizon, under a field of stars. */
const warp: Painter = (ctx, w, h) => {
  const rand = rng(7);
  const u = unitOf(w, h);
  ctx.fillStyle = "#030308";
  ctx.fillRect(0, 0, w, h);
  stars(ctx, w, h, rand, 420);
  const cx = w * 0.5;
  const cy = h * 0.6;
  ctx.globalCompositeOperation = "lighter";
  glow(ctx, cx, cy + h * 0.06, w * 0.45, "90 120 255", 0.1);
  ctx.lineCap = "round";
  const paths = (count: number, draw: (t: number, rx: number, ry: number, a: number, span: number) => void) => {
    for (let i = 0; i < count; i++) {
      const t = rand();
      const r = (0.16 + t * t * 1.3) * w;
      // The near half of each ring only: the far half would cross the horizon.
      const a = -0.12 + rand() * (Math.PI + 0.24);
      draw(t, r * 1.2, r * 0.38, a, 0.05 + rand() * 0.8);
    }
  };
  const color = (t: number) => {
    const p = rand();
    if (t < 0.16) return p < 0.7 ? "235 240 255" : "150 185 255";
    if (t < 0.3) return p < 0.75 ? "230 50 62" : "255 140 120";
    if (t < 0.55) return p < 0.6 ? "120 150 235" : "210 220 255";
    return p < 0.7 ? "70 90 190" : "150 120 230";
  };
  paths(1600, (t, rx, ry, a, span) => {
    ctx.strokeStyle = `rgb(${color(t)})`;
    ctx.globalAlpha = (0.06 + rand() * 0.45) * (1 - t * 0.55);
    ctx.lineWidth = (0.4 + rand() * 1.3) * u * (1 + t * 1.5);
    ctx.beginPath();
    ctx.ellipse(cx, cy, rx, ry, -0.05, a, a + span);
    ctx.stroke();
  });
  // Sparks along the streaks.
  paths(3200, (t, rx, ry, a) => {
    ctx.fillStyle = `rgb(${color(t)})`;
    ctx.globalAlpha = 0.2 + rand() * 0.7;
    const x = cx + Math.cos(a) * rx;
    const y = cy + Math.sin(a) * ry;
    ctx.beginPath();
    ctx.arc(x, y, (0.5 + rand() * 1.1) * u * (1 + t), 0, TAU);
    ctx.fill();
  });
  ctx.globalAlpha = 1;
  ctx.globalCompositeOperation = "source-over";
};

/** Desert dunes at dusk, in the clay of Dial's accent. */
const dune: Painter = (ctx, w, h) => {
  const rand = rng(11);
  ctx.fillStyle = vertical(ctx, h, [
    [0, "#0f0b14"],
    [0.38, "#2c1823"],
    [0.56, "#7e3f35"],
    [0.66, "#d98a63"],
  ]);
  ctx.fillRect(0, 0, w, h);
  stars(ctx, w, h, rand, 260, h * 0.4);
  ctx.globalCompositeOperation = "lighter";
  glow(ctx, w * 0.64, h * 0.62, w * 0.42, "242 150 110", 0.32);
  glow(ctx, w * 0.64, h * 0.62, w * 0.06, "255 214 180", 0.9);
  ctx.globalCompositeOperation = "source-over";
  const layers: [string, string][] = [
    ["#a35540", "#6e3530"],
    ["#7a3a31", "#4a2324"],
    ["#4f2422", "#2e1618"],
    ["#2e1515", "#1a0e0f"],
    ["#160b0b", "#0b0607"],
  ];
  layers.forEach(([top, bottom], k) => {
    const base = h * (0.6 + k * 0.085);
    const g = ctx.createLinearGradient(0, base - h * 0.08, 0, h);
    g.addColorStop(0, top);
    g.addColorStop(1, bottom);
    ridge(
      ctx,
      w,
      h,
      base,
      h * (0.035 + k * 0.012),
      [
        [0.8 + rand() * 0.8, rand() * TAU, 0.6],
        [2 + rand() * 2, rand() * TAU, 0.3],
        [6 + rand() * 4, rand() * TAU, 0.1],
      ],
      g,
    );
  });
};

// ---------------------------------------------------------------------------------------------
// Animated presets

/** Warm lights drifting and blinking over dark hills at night. */
const fireflies: Painter = (ctx, w, h, time) => {
  const rand = rng(53);
  const u = unitOf(w, h);
  ctx.fillStyle = vertical(ctx, h, [
    [0, "#04060a"],
    [0.7, "#0a100f"],
    [1, "#0c1210"],
  ]);
  ctx.fillRect(0, 0, w, h);
  stars(ctx, w, h, rand, 160, h * 0.5);
  ridge(ctx, w, h, h * 0.8, h * 0.06, [
    [1.1, 0.6, 0.6],
    [3.4, 2.1, 0.3],
    [9, 1.2, 0.1],
  ], "#060a09");
  ridge(ctx, w, h, h * 0.9, h * 0.05, [
    [0.8, 2.4, 0.6],
    [4.6, 0.9, 0.3],
    [13, 0.2, 0.1],
  ], "#030505");
  ctx.globalCompositeOperation = "lighter";
  for (let i = 0; i < 120; i++) {
    const [bx, by] = [rand(), 0.35 + rand() * 0.6];
    const [fx, fy, fp] = [0.05 + rand() * 0.12, 0.04 + rand() * 0.1, 0.4 + rand() * 0.9];
    const [px, py, pp] = [rand() * TAU, rand() * TAU, rand() * TAU];
    const warm = rand() < 0.7 ? "255 196 110" : "217 119 87";
    const x = w * (bx + 0.05 * Math.sin(time * fx + px) + 0.02 * Math.sin(time * fx * 3.1 + py));
    const y = h * (by + 0.04 * Math.cos(time * fy + py));
    const light = Math.pow(Math.max(0, Math.sin(time * fp + pp)), 3);
    if (light < 0.02) continue;
    const size = (0.6 + by) * u;
    glow(ctx, x, y, 16 * size, warm, 0.35 * light);
    ctx.fillStyle = `rgb(255 236 190 / ${light})`;
    ctx.beginPath();
    ctx.arc(x, y, 1.4 * size, 0, TAU);
    ctx.fill();
  }
  ctx.globalCompositeOperation = "source-over";
};

export const PRESETS: Preset[] = [
  { id: "warp", name: "Warp", paint: warp },
  { id: "dune", name: "Dune", paint: dune },
  { id: "fireflies", name: "Fireflies", paint: fireflies, moving: true },
  { id: "trace-route", name: "Trace Route", paint: traceRoute, moving: true },
  { id: "lighthouse", name: "Lighthouse", paint: lighthouse, moving: true },
  { id: "koi-pond", name: "Koi Pond", paint: koiPond, moving: true },
  { id: "crab-beach", name: "Crab Beach", paint: crabBeach, moving: true },
  { id: "cloudprint", name: "Cloudprint", paint: cloudprint, moving: true },
  { id: "orbital-earth", name: "Orbital Earth", paint: orbitalEarth, moving: true },
  { id: "lantern-watch", name: "Lantern Watch", paint: lanternWatch, prepare: prepareLanternWatch, moving: true },
];

// ---------------------------------------------------------------------------------------------
// The preference

/** Saved preferences, with anything missing or unknown replaced by its default. */
export function parsePrefs(text: string | null): BackgroundPrefs {
  let raw: Partial<Record<keyof BackgroundPrefs, unknown>> = {};
  try {
    const parsed = text ? JSON.parse(text) : null;
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) raw = parsed;
  } catch {
    // A damaged value falls back to the defaults.
  }
  const path = typeof raw.path === "string" && raw.path ? raw.path : null;
  if (raw.id === "newsprint") raw.id = "orbital-earth";
  const known = typeof raw.id === "string" && (raw.id === "none" || PRESETS.some((p) => p.id === raw.id));
  const id = raw.id === "custom" && path ? "custom" : known ? (raw.id as string) : DEFAULT_PREFS.id;
  const dim = typeof raw.dim === "number" && Number.isFinite(raw.dim) ? Math.min(MAX_DIM, Math.max(0, raw.dim)) : DEFAULT_PREFS.dim;
  const placement = raw.placement === "empty" || raw.placement === "always" ? raw.placement : DEFAULT_PREFS.placement;
  const motion = typeof raw.motion === "boolean" ? raw.motion : DEFAULT_PREFS.motion;
  return { id, path, dim, placement, motion };
}

function stored(): string | null {
  try {
    return localStorage.getItem(STORAGE_KEY);
  } catch {
    return null;
  }
}

export const background = reactive<BackgroundPrefs>(parsePrefs(stored()));
watch(background, (prefs) => {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(prefs));
  } catch {
    // Without storage the choice lasts until the app quits.
  }
});

/** The user's image as a `data:` URL, once read. */
export const customUrl = ref<string | null>(null);

/** The chosen preset; `null` for none or the user's image. */
export const preset = computed(() => PRESETS.find((p) => p.id === background.id) ?? null);

/** Whether there is anything to show. */
export const hasBackground = computed(() => (background.id === "custom" ? customUrl.value !== null : preset.value !== null));

/** Use the image at `path`; throws a readable message when it cannot be read. */
export async function useImage(path: string) {
  const image = await files.readImage(path);
  customUrl.value = image.url;
  background.path = path;
  background.id = "custom";
}

/** Forget the user's image; its background falls back to the default artwork. */
export function forgetImage() {
  if (background.id === "custom") background.id = DEFAULT_PREFS.id;
  background.path = null;
  customUrl.value = null;
}

/** Read the saved image again at start-up. */
export async function restoreImage() {
  if (!background.path) return;
  try {
    customUrl.value = (await files.readImage(background.path)).url;
  } catch (error) {
    if (background.id === "custom") showToast(`Background image unavailable: ${error}`);
  }
}

/** A moving background shows this moment when it is held still. */
const STILL_TIME = 40;
/** Animation frame budget: about 30 frames a second is smooth for slow motion. */
const FRAME_MS = 1000 / 30;

const reducedMotion = typeof matchMedia === "function" ? matchMedia("(prefers-reduced-motion: reduce)") : null;

/** Whether moving backgrounds may move: the user's switch and the system's "reduce motion". */
export function motionAllowed() {
  return background.motion && !reducedMotion?.matches;
}

/**
 * Paint `p` onto `canvas` at its displayed size, as at `time`. Moving backgrounds render at one
 * pixel per point (they are soft, and repainted every frame); still ones at full sharpness.
 */
export function paintCanvas(canvas: HTMLCanvasElement, p: Preset, time = STILL_TIME) {
  const scale = p.moving ? 1 : Math.min(window.devicePixelRatio || 1, 2);
  const w = Math.round(canvas.clientWidth * scale);
  const h = Math.round(canvas.clientHeight * scale);
  if (!w || !h) return;
  // Resizing clears the canvas, so only when the size changed.
  if (canvas.width !== w) canvas.width = w;
  if (canvas.height !== h) canvas.height = h;
  const ctx = canvas.getContext("2d");
  if (ctx) p.paint(ctx, w, h, time);
}

/**
 * Keep `canvas` showing `p`: animated at about 30 frames a second when it moves and motion is
 * allowed, else painted once and again after resizing. Returns the function that stops it.
 */
export function show(canvas: HTMLCanvasElement, p: Preset): () => void {
  const ready = p.prepare?.().catch(() => { /* Keep the printed layout if an asset cannot load. */ });
  if (p.moving && motionAllowed()) {
    const start = performance.now() - STILL_TIME * 1000;
    let last = -Infinity;
    let frame = 0;
    const tick = (now: number) => {
      frame = requestAnimationFrame(tick);
      if (now - last < FRAME_MS || document.hidden) return;
      last = now;
      paintCanvas(canvas, p, (now - start) / 1000);
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }
  let timer: ReturnType<typeof setTimeout> | undefined;
  const repaint = () => paintCanvas(canvas, p);
  // While resizing the old picture stretches; it is repainted once resizing pauses.
  const observer = new ResizeObserver(() => {
    clearTimeout(timer);
    timer = setTimeout(repaint, 140);
  });
  observer.observe(canvas);
  repaint();
  let active = true;
  ready?.then(() => { if (active) repaint(); });
  return () => {
    active = false;
    observer.disconnect();
    clearTimeout(timer);
  };
}

/** Bumped when the system's "reduce motion" changes, so shown backgrounds restart. */
export const motionChanged = ref(0);
reducedMotion?.addEventListener("change", () => motionChanged.value++);
