import type { Painter } from "./backgrounds";

const wrap = (x: number, period: number) => ((x % period) + period) % period;

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

// ---------------------------------------------------------------------------------------------
// Lighthouse: a red lighthouse on a sea cliff at sunset, drawn in coloured characters. The cell
// grid keeps the characters' proportions, so the scene adds columns as the pane widens.

type RGB = readonly [number, number, number];

const clamp01 = (x: number) => Math.min(1, Math.max(0, x));
const smooth = (a: number, b: number, x: number) => {
  const k = clamp01((x - a) / (b - a));
  return k * k * (3 - 2 * k);
};
const mix = (a: RGB, b: RGB, k: number): RGB => [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k, a[2] + (b[2] - a[2]) * k];

function hash(x: number, y: number) {
  let h = Math.imul(x | 0, 374761393) ^ Math.imul(y | 0, 668265263);
  h = Math.imul(h ^ (h >>> 13), 1274126177);
  return ((h ^ (h >>> 16)) >>> 0) / 4294967296;
}

/** Smooth value noise, 0 to 1. */
function noise(x: number, y: number) {
  const xi = Math.floor(x);
  const yi = Math.floor(y);
  const xf = x - xi;
  const yf = y - yi;
  const u = xf * xf * (3 - 2 * xf);
  const v = yf * yf * (3 - 2 * yf);
  const a = hash(xi, yi);
  const b = hash(xi + 1, yi);
  const c = hash(xi, yi + 1);
  const d = hash(xi + 1, yi + 1);
  return a + (b - a) * u + (c - a) * v + (a - b - c + d) * u * v;
}
const fbm = (x: number, y: number) => noise(x, y) * 0.5 + noise(x * 2.03 + 17, y * 2.03) * 0.3 + noise(x * 4.1 + 5, y * 4.1 + 9) * 0.2;

function ramp(stops: [number, RGB][], x: number): RGB {
  for (let i = 1; i < stops.length; i++) {
    if (x <= stops[i][0]) {
      const [a, from] = stops[i - 1];
      const [b, to] = stops[i];
      return mix(from, to, clamp01((x - a) / (b - a)));
    }
  }
  return stops[stops.length - 1][1];
}

const SKY: [number, RGB][] = [
  [0, [52, 18, 70]],
  [0.38, [150, 38, 92]],
  [0.7, [236, 86, 66]],
  [0.9, [255, 148, 78]],
  [1, [255, 204, 122]],
];
const CLOUD = " .:-=+*#";
const FOAM = ".'~o*";
const SPRAY = "'.*o`";
const WHITE: RGB = [240, 238, 236];
const GOLD: RGB = [255, 226, 160];
/** A character cell is this much wider than it is tall. */
const ASPECT = 0.6;

export const lighthouse: Painter = (ctx, w, h, t) => {
  const rows = Math.max(36, Math.min(66, Math.round(h / 15)));
  const cellH = h / rows;
  const cellW = cellH * ASPECT;
  const cols = Math.ceil(w / cellW);
  const horizon = Math.round(rows * 0.6);
  const n = cols * rows;
  const chars: string[] = new Array(n).fill(" ");
  const color = new Float32Array(n * 3);
  const alpha = new Float32Array(n);
  const kind = new Uint8Array(n); // 0 sky, 1 sea, 2 rock, 3 lighthouse
  const put = (c: number, r: number, ch: string, rgb: RGB, a = 1) => {
    if (c < 0 || c >= cols || r < 0 || r >= rows) return;
    const i = r * cols + c;
    chars[i] = ch;
    color[i * 3] = rgb[0];
    color[i * 3 + 1] = rgb[1];
    color[i * 3 + 2] = rgb[2];
    alpha[i] = a;
  };

  // The land: a steep cliff rising to a plateau on the right, and rocks along the bottom.
  const top = new Float64Array(cols);
  for (let c = 0; c < cols; c++) {
    const nx = (c + 0.5) / cols;
    let edge = Infinity;
    if (nx > 0.48) {
      const face = smooth(0.48, 0.66, nx);
      const jag = (noise(c * 0.45, 3.7) - 0.5) * 2.4 * (1 - smooth(0.63, 0.69, nx));
      edge = rows * (1.02 - face * 0.55 - smooth(0.66, 1, nx) * 0.03) + jag;
    }
    top[c] = Math.min(edge, rows * (0.88 + 0.2 * noise(c * 0.09, 9.1)));
  }
  const rock = (c: number, r: number) => c >= 0 && c < cols && r + 0.5 >= top[c];

  const sunX = cols * 0.2;
  const sunY = horizon - 0.6;
  const sunR = rows * 0.05;

  // The lighthouse, centred on a cell boundary so it is symmetric.
  const lx = Math.round(cols * 0.8);
  const base = Math.floor(Math.min(top[lx - 6] ?? rows, top[lx + 5] ?? rows));
  const tower = Math.round(rows * 0.27);
  const gallery = base - tower - 1;
  const lampY = gallery - 2;
  const spin = t * 0.55;
  const sweep = Math.cos(spin);
  const lamp = 0.55 + 0.45 * Math.pow(Math.max(0, Math.sin(spin)), 6);
  const beamLength = cols * 0.8 * Math.abs(sweep);

  const beam = (c: number, r: number) => {
    const dx = c + 0.5 - lx;
    if (dx * sweep <= 0 || Math.abs(dx) > beamLength) return 0;
    const along = Math.abs(dx);
    const across = Math.abs(r - lampY) / (0.5 + along * ASPECT * 0.1);
    if (across >= 1) return 0;
    return (1 - across * across) * Math.pow(1 - along / beamLength, 0.7) * (0.35 + 0.65 * Math.abs(sweep));
  };

  for (let r = 0; r < rows; r++) {
    for (let c = 0; c < cols; c++) {
      const i = r * cols + c;
      if (rock(c, r)) {
        kind[i] = 2;
        continue;
      }
      const glint = beam(c, r);
      if (r < horizon) {
        // Sky: gradient, sun and halo, drifting clouds lit from below, stars at the top.
        const s = (r + 0.5) / horizon;
        const d = Math.hypot((c + 0.5 - sunX) * ASPECT, r + 0.5 - sunY) / sunR;
        let sky = ramp(SKY, s);
        if (d > 1) sky = mix(sky, [255, 214, 150], 0.55 * Math.exp(-(d - 1) * 0.32));
        if (d < 1) {
          put(c, r, d < 0.72 ? "@" : "#", d < 0.72 ? [255, 240, 190] : [255, 202, 112]);
        } else {
          const band = smooth(0.04, 0.3, s) * (1 - smooth(0.8, 0.97, s));
          const cloud = smooth(0.42, 0.8, fbm(c * ASPECT * 0.05 + t * 0.035, r * 0.27)) * band;
          if (cloud > 0.07) {
            const lit = mix([110, 34, 86], [255, 158, 108], clamp01(s * 1.1 - 0.1 + Math.exp(-d * 0.2) * 0.5));
            put(c, r, CLOUD[Math.min(CLOUD.length - 1, 1 + Math.floor(cloud * 0.9 * (CLOUD.length - 1)))], mix(sky, lit, 0.8), 0.4 + 0.5 * cloud);
          } else {
            const k = hash(c, r);
            if (s < 0.5 && k < 0.03) {
              const twinkle = 0.5 + 0.5 * Math.sin(t * (1 + k * 60) + k * 300);
              put(c, r, k < 0.006 ? "+" : ".", [255, 232, 240], (0.2 + 0.7 * twinkle) * (1 - s * 1.7));
            } else if (s > 0.86 && k < 0.3) {
              put(c, r, "-", sky, 0.35);
            } else if (k < 0.1) {
              put(c, r, ".", sky, 0.3);
            }
          }
        }
      } else {
        // Sea: waves that flatten toward the horizon, and a glittering path under the sun.
        kind[i] = 1;
        const depth = (r + 0.5 - horizon) / (rows - horizon);
        const water = mix(mix([226, 120, 82], [18, 46, 66], smooth(0, 0.5, depth)), [8, 22, 36], smooth(0.5, 1, depth));
        const wave = Math.sin(c * ASPECT * (0.32 / (0.15 + depth)) + t * 1.3 + r * 2.1 + noise(c * 0.08, r * 0.5 + t * 0.2) * 5);
        const width = sunR * (0.9 + depth * 2.6);
        const path = Math.exp(-((((c + 0.5 - sunX) * ASPECT) / width) ** 2));
        const shimmer = noise(c * 0.6, r * 1.3 + t * 2.2);
        if (path * shimmer > 0.3) {
          put(c, r, shimmer > 0.72 ? "=" : shimmer > 0.55 ? "*" : "-", mix(water, GOLD, Math.min(1, path * 1.1)), 0.6 + 0.4 * path);
        } else if (depth < 0.08) {
          put(c, r, wave > 0 ? "-" : "_", mix(water, [255, 170, 110], 0.4), 0.7);
        } else if (wave > -0.45) {
          const crest = wave > 0.6;
          put(c, r, crest ? "~" : wave > 0.1 ? "-" : ".", mix(water, [255, 168, 116], crest ? 0.3 : 0.08), crest ? 0.85 : 0.55);
        }
        if (rock(c + 1, r) || rock(c - 1, r) || rock(c, r + 1)) {
          const foam = 0.5 + 0.5 * Math.sin(t * 2.4 + c * 1.3 + r * 0.7 + hash(c, r) * 6);
          put(c, r, FOAM[Math.min(FOAM.length - 1, Math.floor(foam * FOAM.length))], mix(WHITE, [255, 190, 150], 0.2), 0.5 + 0.5 * foam);
        }
      }
      if (glint > 0.08) {
        const lit = mix(chars[i] === " " ? (r < horizon ? ramp(SKY, (r + 0.5) / horizon) : [40, 60, 80]) : [color[i * 3], color[i * 3 + 1], color[i * 3 + 2]], [255, 240, 200], glint * 0.9);
        put(c, r, glint > 0.55 ? "=" : glint > 0.25 ? "-" : ".", lit, Math.max(alpha[i], 0.3 + glint * 0.7));
      }
    }
  }

  // Rock: cracked stone, lit orange where it faces the sun, grass swaying on the plateau.
  for (let c = 0; c < cols; c++) {
    for (let r = Math.max(0, Math.floor(top[c])); r < rows; r++) {
      if (!rock(c, r)) continue;
      const depth = r + 0.5 - top[c];
      const slope = (top[Math.min(cols - 1, c + 1)] - top[Math.max(0, c - 1)]) / 2;
      const shade = 1 - 0.4 * smooth(horizon, rows, r);
      let open = 0;
      for (let k = 1; k <= 8; k++) if (!rock(c - k, r)) { open = ((9 - k) / 8) ** 1.5; break; }
      const light = Math.max(open, depth < 3 ? 0.6 - depth * 0.15 : 0);
      const tone = fbm(c * 0.18, r * 0.3);
      const stone = mix([58, 40, 50], [136, 96, 86], tone);
      const rgb = mix(stone, [222, 124, 64], light * 0.85).map((v) => v * shade) as unknown as RGB;
      if (depth < 1) {
        if (Math.abs(slope) < 0.6 && (c + 0.5) / cols > 0.64) {
          const sway = Math.sin(t * 1.6 + c * 0.45);
          put(c, r, sway > 0.35 ? "/" : sway < -0.35 ? "\\" : "|", [196, 160, 72]);
        } else {
          put(c, r, slope < -0.6 ? "/" : slope > 0.6 ? "\\" : "_", mix(rgb, [255, 170, 100], 0.4));
        }
      } else {
        const crack = noise(c * 0.5, r * 0.9);
        put(c, r, crack > 0.74 ? "/" : crack < 0.22 ? "\\" : tone > 0.6 ? "#" : tone > 0.45 ? "%" : ":", rgb, 0.9);
      }
    }
  }

  // A fence along the plateau edge.
  for (let c = Math.round(cols * 0.7); c < cols; c++) {
    const r = Math.floor(top[c]) - 1;
    if (Math.abs(c + 0.5 - lx) > 8 && kind[r * cols + c] === 0) put(c, r, c % 4 === 0 ? "+" : "-", [110, 60, 44]);
  }

  // The tower: red, lit from the sun's side, with a white door and warm windows.
  for (let r = gallery + 1; r <= base; r++) {
    const k = (base - r) / tower;
    const half = 5.2 - 1.8 * k;
    for (let c = Math.floor(lx - half); c <= Math.ceil(lx + half); c++) {
      const u = (c + 0.5 - lx) / half;
      if (Math.abs(u) >= 1) continue;
      kind[r * cols + c] = 3;
      const red = mix([84, 10, 18], [236, 44, 36], clamp01(0.55 - 0.55 * u));
      const rgb = u < -0.55 ? mix(red, [255, 140, 80], 0.45) : red;
      const middle = c === lx - 1 || c === lx;
      if (middle && r >= base - 2) put(c, r, "|", WHITE);
      else if (middle && r === base - 3) put(c, r, c < lx ? "/" : "\\", WHITE);
      else if (middle && [0.38, 0.62, 0.84].some((f) => r === base - Math.round(tower * f))) put(c, r, c < lx ? "[" : "]", [255, 206, 120]);
      else if (Math.abs(u) > 1 - 1 / half) put(c, r, "|", mix(rgb, [255, 200, 160], 0.25));
      else put(c, r, r % 3 === 0 ? "=" : "#", rgb);
    }
  }
  const span = (r: number, from: number, to: number, draw: (c: number) => [string, RGB]) => {
    for (let c = from; c <= to; c++) {
      const [ch, rgb] = draw(c);
      put(c, r, ch, rgb);
      if (r >= 0 && r < rows && c >= 0 && c < cols) kind[r * cols + c] = 3;
    }
  };
  const bright = mix([255, 190, 90], [255, 252, 230], lamp);
  span(gallery, lx - 6, lx + 5, (c) => [c === lx - 6 ? "[" : c === lx + 5 ? "]" : "=", [86, 26, 30]]);
  span(gallery - 1, lx - 6, lx + 5, (c) => [c % 2 ? "|" : "+", [70, 26, 32]]);
  for (const r of [gallery - 2, gallery - 3]) {
    span(r, lx - 3, lx + 2, (c) => (c === lx - 3 || c === lx + 2 ? ["|", [90, 28, 32]] : [lamp > 0.8 ? "@" : "*", bright]));
  }
  span(gallery - 4, lx - 3, lx + 2, (c) => [c === lx - 3 ? "/" : c === lx + 2 ? "\\" : "=", [214, 38, 36]]);
  span(gallery - 5, lx - 2, lx + 1, (c) => [c === lx - 2 ? "/" : c === lx + 1 ? "\\" : "=", [190, 34, 34]]);
  span(gallery - 6, lx - 1, lx, (c) => [c < lx ? "/" : "\\", [170, 30, 32]]);

  // Gulls drifting across the sky.
  for (let g = 0; g < 4; g++) {
    const x = Math.round(((t * (1.1 + g * 0.35) + g * 37) % (cols + 30)) - 15);
    const y = Math.round(rows * (0.12 + g * 0.06) + Math.sin(t * 0.6 + g * 2) * 1.2);
    const wings = Math.sin(t * 5 + g * 1.7) > 0 ? "\\v/" : "-v-";
    for (let k = 0; k < 3; k++) {
      if (y >= 0 && y < rows && x + k >= 0 && x + k < cols && kind[y * cols + x + k] === 0) put(x + k, y, wings[k], [44, 14, 32], 0.9);
    }
  }

  // Spray thrown up where waves break on the cliff, every few seconds.
  let foot = Math.round(cols * 0.46);
  while (foot < cols && top[foot] > rows * 0.8) foot++;
  for (const [ox, oy, period, offset, count] of [[foot - 1, rows * 0.8, 7, 0, 70], [Math.round(cols * 0.34), rows * 0.93, 5.3, 2.1, 30]]) {
    const age = (t + offset) % period;
    if (age > 3) continue;
    for (let j = 0; j < count; j++) {
      const vx = -(0.4 + hash(j, 1) * 6) * (hash(j, 4) < 0.25 ? -0.5 : 1);
      const vy = -(3 + hash(j, 2) * (count > 40 ? 12 : 7));
      const c = Math.round(ox + vx * age * 1.3);
      const r = Math.round(oy + vy * age + 5 * age * age);
      if (r < 0 || r >= rows || c < 0 || c >= cols || kind[r * cols + c] === 3) continue;
      put(c, r, SPRAY[j % SPRAY.length], mix(WHITE, [255, 200, 160], hash(j, 3) * 0.4), (1 - age / 3) * (0.5 + 0.5 * hash(j, 5)));
    }
  }

  // Draw: a dim gradient underneath, the characters, then glows and a vignette.
  const back = ctx.createLinearGradient(0, 0, 0, h);
  back.addColorStop(0, "#14061c");
  back.addColorStop(0.42 * (horizon / rows) / 0.56, "#3c0f24");
  back.addColorStop(horizon / rows, "#6a2a18");
  back.addColorStop(Math.min(1, horizon / rows + 0.002), "#0a1420");
  back.addColorStop(1, "#03060b");
  ctx.fillStyle = back;
  ctx.fillRect(0, 0, w, h);
  // The land as a solid dark mass, so the rock characters read as texture on stone.
  const land = ctx.createLinearGradient(0, h * 0.4, 0, h);
  land.addColorStop(0, "#2a1a1e");
  land.addColorStop(1, "#0c0a0e");
  ctx.fillStyle = land;
  for (let c = 0; c < cols; c++) {
    const from = Math.max(0, Math.ceil(top[c] - 0.5));
    if (from < rows) ctx.fillRect(c * cellW, from * cellH, cellW + 1, h - from * cellH);
  }
  ctx.save();
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.font = `${cellH * 1.02}px ui-monospace, Menlo, monospace`;
  for (let i = 0; i < n; i++) {
    if (chars[i] === " " || alpha[i] <= 0) continue;
    ctx.globalAlpha = Math.min(1, alpha[i]);
    ctx.fillStyle = `rgb(${color[i * 3] | 0} ${color[i * 3 + 1] | 0} ${color[i * 3 + 2] | 0})`;
    ctx.fillText(chars[i], ((i % cols) + 0.5) * cellW, (Math.floor(i / cols) + 0.5) * cellH);
  }
  ctx.globalAlpha = 1;
  ctx.globalCompositeOperation = "lighter";
  const bloom = (x: number, y: number, radius: number, rgb: string, a: number) => {
    const g = ctx.createRadialGradient(x, y, 0, x, y, radius);
    g.addColorStop(0, `rgb(${rgb} / ${a})`);
    g.addColorStop(1, `rgb(${rgb} / 0)`);
    ctx.fillStyle = g;
    ctx.fillRect(x - radius, y - radius, radius * 2, radius * 2);
  };
  bloom(sunX * cellW, sunY * cellH, h * 0.42, "255 140 80", 0.2);
  bloom(sunX * cellW, sunY * cellH, h * 0.1, "255 220 160", 0.25);
  bloom(lx * cellW, (lampY + 0.5) * cellH, h * (0.04 + 0.12 * lamp), "255 230 170", 0.15 + 0.35 * lamp);
  ctx.globalCompositeOperation = "source-over";
  const vignette = ctx.createRadialGradient(w / 2, h / 2, Math.min(w, h) * 0.35, w / 2, h / 2, Math.max(w, h) * 0.75);
  vignette.addColorStop(0, "rgb(0 0 0 / 0)");
  vignette.addColorStop(1, "rgb(0 0 0 / 0.45)");
  ctx.fillStyle = vignette;
  ctx.fillRect(0, 0, w, h);
  ctx.restore();
};
