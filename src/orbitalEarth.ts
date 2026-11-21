import type { Painter } from './backgrounds';

const rad = Math.PI / 180;
// Simplified coastlines in longitude/latitude. Sample once, not while animating.
const coasts = [
  '-168,67 -154,71 -141,69 -126,72 -108,69 -96,73 -80,69 -85,55 -78,53 -65,61 -57,52 -67,44 -75,39 -81,30 -81,25 -85,30 -96,28 -98,22 -91,18 -87,22 -86,16 -78,9 -82,8 -91,15 -104,20 -115,31 -124,41 -125,49 -138,59 -149,60 -162,55 -158,61',
  '-81,12 -72,11 -62,10 -50,1 -35,-5 -39,-18 -43,-23 -49,-28 -53,-34 -59,-39 -66,-55 -72,-51 -75,-38 -70,-19 -77,-12 -81,-4',
  '-73,78 -61,82 -30,83 -17,76 -24,68 -44,60 -52,65 -56,73',
  '-17,15 -16,22 -9,31 -5,36 10,37 13,33 23,32 32,31 35,23 43,12 51,12 49,3 42,-3 40,-16 34,-21 32,-29 21,-35 17,-29 12,-18 12,-7 8,4 1,5 -7,4 -15,10',
  '-10,36 -9,43 -2,44 -5,48 4,51 9,55 6,59 15,67 28,71 31,66 23,59 31,60 42,68 66,69 93,77 112,73 139,72 170,68 179,65 166,60 161,51 155,50 158,60 140,54 140,47 132,43 129,35 125,38 122,30 114,22 108,21 109,12 104,9 100,14 104,2 99,7 94,18 89,22 85,20 78,8 73,16 68,24 57,26 57,21 52,17 45,13 42,17 35,29 28,37 23,39 24,35 20,39 16,41 16,38 12,42 8,44 4,43 0,38',
  '113,-22 115,-34 124,-33 132,-32 140,-38 148,-39 154,-28 150,-21 143,-11 141,-17 136,-12 130,-12 123,-17 118,-20',
  '43,-24 46,-25 51,-13 48,-13', '95,5 106,-6 102,-5', '108,1 116,7 119,2 115,-4 110,-3', '131,-2 150,-6 147,-10 134,-5', '130,32 141,41 145,45 141,36', '-10,51 -5,59 1,52',
].map(shape => shape.split(' ').map(pair => pair.split(',').map(Number)));

function isLand(lon: number, lat: number) {
  return lat < -72 || coasts.some(poly => {
    let inside = false;
    for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
      const [x, y] = poly[i], [u, v] = poly[j];
      if ((y > lat) !== (v > lat) && lon < x + (u - x) * (lat - y) / (v - y)) inside = !inside;
    }
    return inside;
  });
}

function surface() {
  const dots = [];
  for (let lat = -88; lat <= 88; lat += 2) {
    const c = Math.cos(lat * rad), n = Math.round(180 * c);
    for (let j = 0; j < n; j++) {
      const lon = 360 * (j + (lat % 4 ? 0.5 : 0)) / n - 180;
      const land = isLand(lon, lat);
      if (land || (lat % 6 === 0 && j % 3 === 0)) dots.push({ x: c * Math.sin(lon * rad), y: Math.sin(lat * rad), z: c * Math.cos(lon * rad), land });
    }
  }
  return dots;
}
let dots: ReturnType<typeof surface>;

// Nominal circular approximations from the ILRS satellite mission tables:
// https://ilrs.gsfc.nasa.gov/missions/satellite_missions/current_missions/lag1_general.html
// See also ajis_general.html and star_general.html in the same catalog. Phases
// and ascending nodes are chosen for the composition, not from tracking data.
const spacecraft = [
  { name: 'LAGEOS-1', km: 5900, inclination: 109.84 },
  { name: 'Ajisai', km: 1490, inclination: 50 },
  { name: 'Starlette', km: 963, inclination: 48.83 },
  { name: 'LAGEOS-2', km: 5800, inclination: 52.64 },
  { name: 'Stella', km: 808, inclination: 98.6 },
];

export const orbitalEarth: Painter = (g, w, h, t) => {
  const r = Math.min(w, h) * .205;
  g.save();
  g.fillStyle = '#040914'; g.fillRect(0, 0, w, h);
  for (let i = 1; i <= 190; i++) {
    g.fillStyle = i % 7 ? '#35435b' : '#8193ab';
    const s = Math.max(.7, r * (i % 7 ? .003 : .005));
    g.fillRect(((i * .618034) % 1) * w, ((i * i * .414214) % 1) * h, s, s);
  }
  g.translate(w / 2, h / 2); g.scale(r, r);
  const halo = g.createRadialGradient(0, 0, .98, 0, 0, 1.36);
  halo.addColorStop(0, '#47baff85'); halo.addColorStop(.13, '#2478ab42'); halo.addColorStop(1, '#10345100');
  g.fillStyle = halo; g.fillRect(-1.36, -1.36, 2.72, 2.72);
  const sea = g.createRadialGradient(-.42, -.5, .08, 0, 0, 1);
  sea.addColorStop(0, '#204a61'); sea.addColorStop(.82, '#0a1a32'); sea.addColorStop(.98, '#163952'); sea.addColorStop(1, '#6295ab');
  g.fillStyle = sea; g.beginPath(); g.arc(0, 0, 1, 0, 2 * Math.PI); g.fill();
  const angle = -.35 + t * .008, ca = Math.cos(angle), sa = Math.sin(angle);
  dots ??= surface();
  for (const p of dots) {
    const x = p.x * ca + p.z * sa, z = p.z * ca - p.x * sa;
    const depth = z * .96 + p.y * .28;
    if (depth <= 0) continue;
    const y = z * .28 - p.y * .96;
    g.globalAlpha = p.land ? .42 + .48 * Math.max(0, depth * .7 - x * .5 - y * .4) : .32;
    g.fillStyle = p.land ? '#a6ffed' : '#539ac7';
    const s = p.land ? .011 : .007;
    g.fillRect(x, y, s * (.4 + .6 * depth), s);
  }
  // Project tilted circular orbits. Sample earlier phases for the fading wake;
  // points on the far side disappear when the globe blocks the line of sight.
  g.lineCap = 'round';
  const labels: { name: string; x: number; y: number; tint: string }[] = [];
  for (const [i, sat] of spacecraft.entries()) {
    const plane = i, node = plane * 2.4, inc = sat.inclination * rad;
    const radius = 1 + sat.km / 6371, phase = i * 2.39996 + t * (.13 / radius ** 1.5);
    const tint = ['#7ddcfc', '#e4ae77', '#9cacf9'][plane % 3];
    let lastX = 0, lastY = 0, lastVisible = false;
    for (let j = 0; j <= 32; j++) {
      const f = j / 32, theta = phase - (1 - f) * 1.65;
      const along = radius * Math.sin(theta), u = radius * Math.cos(theta);
      const north = along * Math.sin(inc), east = along * Math.cos(inc);
      const x = u * Math.sin(node) + east * Math.cos(node);
      const z = u * Math.cos(node) - east * Math.sin(node);
      const y = z * .28 - north * .96;
      const visible = z * .96 + north * .28 >= 0 || x*x + y*y > 1;
      if (visible && lastVisible) {
        g.globalAlpha = .025 + .64 * f*f; g.strokeStyle = tint; g.lineWidth = .0035;
        g.beginPath(); g.moveTo(lastX, lastY); g.lineTo(x, y); g.stroke();
      }
      if (visible && j === 32) {
        labels.push({ name: sat.name, x: w / 2 + r*x, y: h / 2 + r*y, tint });
        for (const [size, alpha] of [[.027, .10], [.016, .32], [.008, 1]]) {
          g.globalAlpha = alpha; g.fillStyle = alpha === 1 ? '#fff6df' : tint;
          g.beginPath(); g.arc(x, y, size, 0, Math.PI * 2); g.fill();
        }
      }
      lastX = x; lastY = y; lastVisible = visible;
    }
  }
  g.restore();
  // Paint cards in display pixels, not globe units; omit text on picker thumbnails.
  if (Math.min(w, h) < 180) return;
  g.save();
  g.font = '11px ui-monospace, monospace';
  g.textBaseline = 'middle';
  const placed: { left: number; top: number; right: number }[] = [];
  for (const { name, x, y, tint } of labels) {
    const width = Math.ceil(g.measureText(name).width) + 16;
    const left = Math.max(3, Math.min(w - width - 3, x - width / 2));
    let top = y - 30;
    while (placed.some(p => left < p.right + 4 && left + width + 4 > p.left && top < p.top + 22 && top + 22 > p.top)) top -= 22;
    if (top < 3) continue;
    placed.push({ left, top, right: left + width });
    g.strokeStyle = tint; g.globalAlpha = .55;
    g.beginPath(); g.moveTo(x, y - 3); g.lineTo(x, top + 18); g.stroke();
    g.globalAlpha = .9; g.fillStyle = '#091522';
    g.beginPath(); g.roundRect(left, top, width, 18, 4); g.fill();
    g.strokeStyle = '#3c5268'; g.stroke();
    g.fillStyle = tint; g.fillText(name, left + 8, top + 9);
  }
  g.restore();
};
