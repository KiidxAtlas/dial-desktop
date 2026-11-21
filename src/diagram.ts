// Mermaid diagrams and charts in answers, drawn by beautiful-mermaid in Dial's palette. The SVG is
// always shown as an image, never inserted as markup, so generated content stays inert.

export type DiagramTheme = 'light' | 'dark';

/** Colours matched to the app's dark and light palettes; CSS shows the one in effect.
 * Nodes are translucent, so they read as glass over the conversation or background art. */
const THEMES = {
  dark: { bg: '#262624', fg: '#f3f2ec', line: 'rgb(222 220 209 / 0.34)', accent: '#d97757', muted: '#bfbdb4', surface: 'rgb(57 57 54 / 0.72)', border: 'rgb(222 220 209 / 0.18)' },
  light: { bg: '#faf9f5', fg: '#1f1e1d', line: 'rgb(31 30 29 / 0.3)', accent: '#c96442', muted: '#57564f', surface: 'rgb(255 255 255 / 0.82)', border: 'rgb(31 30 29 / 0.16)' },
} as const;
const LAYOUT = { padding: 14, nodeSpacing: 34, layerSpacing: 52, font: 'system-ui, -apple-system, sans-serif' };
/** A wide diagram may shrink this much to fit; past that it keeps its size and scrolls sideways,
 * so labels stay readable. */
const MIN_SCALE = 0.8;
/** Dial's touch on the renderer's output: rounded nodes, clay decisions, chip labels, edges that draw in. */
const STYLE = `
  .node rect { stroke-width: 1px; }
  .node[data-shape="diamond"] polygon, .node[data-shape="diamond"] path {
    fill: color-mix(in srgb, var(--accent) 16%, var(--_node-fill));
    stroke: color-mix(in srgb, var(--accent) 55%, transparent);
  }
  .edge { stroke-width: 1.25px; stroke-linecap: round; stroke-linejoin: round; }
  .edge-label rect { fill: var(--_node-fill); stroke: color-mix(in srgb, var(--accent) 35%, transparent); }
  .edge-label text { fill: var(--accent); font-weight: 500; }
  .xychart-bar { rx: 4px; }
  @media (prefers-reduced-motion: no-preference) {
    .edge[pathLength] { stroke-dasharray: 1; stroke-dashoffset: 1; animation: draw 0.9s 0.15s cubic-bezier(0.2, 0.8, 0.2, 1) forwards; }
    .node, .edge-label, .xychart-bar, .xychart-dot { animation: rise 0.5s cubic-bezier(0.2, 0.8, 0.2, 1) both; }
    @keyframes draw { to { stroke-dashoffset: 0; } }
    @keyframes rise { from { opacity: 0; } }
  }
`;

function style(svg: string) {
  return svg
    .replace(/@import url\([^)]*\);/, '')
    .replace('</style>', `${STYLE}</style>`)
    .replace(/(<g class="node"[^>]*>\s*<rect[^>]*?) rx="0" ry="0"/g, '$1 rx="9" ry="9"')
    .replace(/(<g class="edge-label"[^>]*>\s*<rect[^>]*?) rx="2" ry="2"/g, '$1 rx="11" ry="11"')
    .replace(/<polyline class="edge"([^>]*data-style="solid")/g, '<polyline class="edge" pathLength="1"$1');
}

const url = (svg: string) => `data:image/svg+xml;charset=utf-8,${encodeURIComponent(style(svg))}`;

/** "Chart" for Mermaid xycharts, otherwise "Diagram". */
export function diagramName(source: string) {
  return /^\s*xychart(-beta)?\b/.test(source) ? 'Chart' : 'Diagram';
}

export interface RenderedDiagram {
  /** Transparent light and dark renderings, decoded, classed `diagram-light`/`diagram-dark`. */
  images: HTMLImageElement[];
  /** The raw (dark) SVG, for reading node positions. */
  svg: string;
  /** An opaque rendering of `theme`, for the image viewer's own surface. */
  opaqueURL(theme: DiagramTheme): string;
}

/** Flowchart arrows that Mermaid reads without spaces but the renderer only reads when spaced:
 * `A-->B` parses as a node `A--`, and `A -.label.-> B` silently drops the edge and `B`. */
const TEXT_ARROW = /(?<![-.=])(<?)(--|-\.|==)(?![->.=])\s*([^|]+?)\s*(-->|---|\.->|\.-|==>|===)(?![->.=])/g;
const ARROW = /(?<![-.=<])(<?)(-->|-\.->|==>|---|-\.-|===)(?![->.=])/g;
const OPEN = '[({';
const CLOSE = ')]}';

/** `source` with flowchart arrows spaced out. Node text in brackets or quotes and `|edge labels|`
 * are left as written; other diagram types are returned unchanged. */
export function spaceArrows(source: string): string {
  if (!/^\s*(flowchart|graph)\b/.test(source)) return source;
  return source.split('\n').map(line => {
    let out = '';
    let free = '';
    let depth = 0;
    let quote = false;
    let pipe = false;
    const flush = () => { out += free.replace(TEXT_ARROW, ' $1$2 $3 $4 ').replace(ARROW, ' $1$2 '); free = ''; };
    for (const ch of line) {
      if (!depth && !quote && !pipe) {
        if (ch !== '"' && ch !== '|' && !OPEN.includes(ch)) { free += ch; continue; }
        flush();
        // An edge label must follow its arrow directly: `-->|yes|`.
        if (ch === '|') { out = out.replace(/(-->|-\.->|==>|---|-\.-|===)\s+$/, '$1'); pipe = true; }
        else if (ch === '"') quote = true;
        else depth = 1;
        out += ch;
        continue;
      }
      out += ch;
      if (quote) quote = ch !== '"';
      else if (ch === '"') quote = true;
      else if (pipe) pipe = ch !== '|';
      else if (OPEN.includes(ch)) depth++;
      else if (CLOSE.includes(ch)) depth--;
    }
    flush();
    return out;
  }).join('\n');
}

/** Render `source` in both palettes. Throws when Mermaid cannot parse it. */
export async function renderDiagram(source: string, alt = diagramName(source)): Promise<RenderedDiagram> {
  const { renderMermaidSVG } = await import('beautiful-mermaid');
  const spaced = spaceArrows(source);
  const svgs = { light: '', dark: '' };
  const images = await Promise.all((['light', 'dark'] as const).map(async theme => {
    svgs[theme] = renderMermaidSVG(spaced, { ...THEMES[theme], ...LAYOUT, transparent: true });
    const img = new Image();
    img.src = url(svgs[theme]);
    img.alt = alt;
    img.className = `diagram-${theme}`;
    img.dataset.theme = theme;
    await img.decode();
    if (img.naturalWidth) img.style.minWidth = `${Math.round(img.naturalWidth * MIN_SCALE)}px`;
    return img;
  }));
  return {
    images,
    svg: svgs.dark,
    opaqueURL: theme => url(renderMermaidSVG(spaced, { ...THEMES[theme], ...LAYOUT, padding: 32 })),
  };
}

/** Which rendering is on screen: the one CSS shows for the current theme. */
export function shownTheme(images: HTMLImageElement[]): DiagramTheme {
  return ((images.find(img => img.offsetParent) ?? images[1]).dataset.theme ?? 'dark') as DiagramTheme;
}

/** A node's or subgraph's box in percent of the diagram, so an overlay scales with the image. */
export interface NodeBox { id: string; label: string; left: number; top: number; width: number; height: number }

/** Node and subgraph boxes read from rendered SVG (parsed as data, never attached to the page).
 * Subgraphs come first, in document order, so the nodes inside them sit on top. */
export function nodeBoxes(svg: string): NodeBox[] {
  const doc = new DOMParser().parseFromString(svg, 'image/svg+xml');
  const root = doc.documentElement;
  const [vx, vy, vw, vh] = (root.getAttribute('viewBox') ?? '').split(/[\s,]+/).map(Number);
  if (!(vw > 0 && vh > 0)) return [];
  const boxes: NodeBox[] = [];
  for (const node of [...root.querySelectorAll('g.subgraph[data-id]'), ...root.querySelectorAll('g.node[data-id]')]) {
    const xs: number[] = [];
    const ys: number[] = [];
    const n = (el: Element, name: string) => Number(el.getAttribute(name) ?? NaN);
    for (const el of node.querySelectorAll('rect, polygon, circle, ellipse')) {
      if (el.tagName === 'rect') { xs.push(n(el, 'x'), n(el, 'x') + n(el, 'width')); ys.push(n(el, 'y'), n(el, 'y') + n(el, 'height')); }
      else if (el.tagName === 'circle') { const r = n(el, 'r'); xs.push(n(el, 'cx') - r, n(el, 'cx') + r); ys.push(n(el, 'cy') - r, n(el, 'cy') + r); }
      else if (el.tagName === 'ellipse') { xs.push(n(el, 'cx') - n(el, 'rx'), n(el, 'cx') + n(el, 'rx')); ys.push(n(el, 'cy') - n(el, 'ry'), n(el, 'cy') + n(el, 'ry')); }
      else for (const pair of (el.getAttribute('points') ?? '').trim().split(/\s+/)) { const [x, y] = pair.split(',').map(Number); xs.push(x); ys.push(y); }
    }
    const fx = xs.filter(Number.isFinite);
    const fy = ys.filter(Number.isFinite);
    if (!fx.length || !fy.length) continue;
    const [x0, x1, y0, y1] = [Math.min(...fx), Math.max(...fx), Math.min(...fy), Math.max(...fy)];
    boxes.push({
      id: node.getAttribute('data-id')!,
      label: node.getAttribute('data-label') ?? node.getAttribute('data-id')!,
      left: ((x0 - vx) / vw) * 100,
      top: ((y0 - vy) / vh) * 100,
      width: ((x1 - x0) / vw) * 100,
      height: ((y1 - y0) / vh) * 100,
    });
  }
  return boxes;
}
