import type { Painter } from "./backgrounds";

const photoUrl = new URL('./assets/quay-market-painted.webp', import.meta.url).href;
let photo: HTMLCanvasElement | undefined;
let loading: Promise<void> | undefined;
export function loadMarketPhoto() {
  if (!loading) {
    const image = new Image();
    image.src = photoUrl;
    loading = image.decode().then(() => { photo = screenPrint(image); });
  }
  return loading;
}

// Screen the color plate once. Small gaps in the ink let the page's own grain
// and fold show through, rather than baking a second paper color into the art.
function screenPrint(image: HTMLImageElement) {
  const plate = document.createElement('canvas');
  plate.width = 960;
  plate.height = Math.round(plate.width * image.naturalHeight / image.naturalWidth);
  const g = plate.getContext('2d')!;
  g.drawImage(image, 0, 0, plate.width, plate.height);
  const { data } = g.getImageData(0, 0, plate.width, plate.height);
  const pitch = 4.5;
  g.globalCompositeOperation = 'destination-out';
  g.fillStyle = 'rgba(0,0,0,.46)';
  g.beginPath();
  for (let row = 0, y = 2; y < plate.height; row++, y += pitch) {
    for (let x = row % 2 === 0 ? 2 : 2 + pitch/2; x < plate.width; x += pitch) {
      const i = (Math.floor(y) * plate.width + Math.floor(x)) * 4;
      const light = (data[i]*.2126 + data[i+1]*.7152 + data[i+2]*.0722) / 255;
      const radius = .55 + 1.25*light;
      g.moveTo(x + radius, y);
      g.arc(x, y, radius, 0, 2*Math.PI);
    }
  }
  g.fill();
  return plate;
}

// Gray strokes stand in for body type: justified rows, indented paragraph starts, short last lines.
const LINE = 9;
function textLines(g: CanvasRenderingContext2D, x: number, y: number, width: number, bottom: number, seed: number) {
  g.fillStyle = '#a0957f';
  for (let row = 0; y + LINE <= bottom; y += LINE, row++) {
    const n = row + seed * 7;
    const indent = n % 9 === 0 ? 10 : 0;
    const end = n % 9 === 8 ? width * (.35 + (n * 37 % 40) / 100) : width;
    for (let left = indent; left < end;) {
      const word = 9 + Math.floor(n * 13 + left * 7) % 23;
      g.fillRect(x + left, y, Math.min(word, end - left), 2.4);
      left += word + 3.2;
    }
  }
}

/** The comic's stage: a table, two cats and the ball, in its own coordinates. */
function rally(g: CanvasRenderingContext2D, t: number) {
  const angle = t * .38 * Math.PI * 2;
  const swing = Math.cos(angle), travel = Math.sin(angle), progress = (1 - swing) / 2;
  const left = Math.max(0, swing) ** 6, right = Math.max(0, -swing) ** 6;
  const ballX = 85 + 118 * progress;
  const ballY = 255 - 23 * Math.abs(travel) - left * 7 - right * 9;

  g.fillStyle = '#8d826b';
  g.fillRect(85, 279, 3, 20); g.fillRect(199, 279, 3, 20);
  g.fillStyle = '#86907d'; g.fillRect(79, 273, 130, 5);
  g.fillStyle = '#554b3e'; g.fillRect(79, 278, 130, 2);
  g.strokeStyle = '#756c59'; g.lineWidth = .7; g.strokeRect(133, 263, 22, 10);
  for (let x = 135; x < 155; x += 3) g.fillRect(x, 263, .55, 10);
  for (let y = 265; y < 273; y += 2) g.fillRect(133, y, 22, .55);
  cat(g, 50 + 2*left, 1, left, left * 7); cat(g, 238 - 2*right, -1, right, right * 9);

  g.strokeStyle = '#956750'; g.lineWidth = 1.2;
  g.beginPath();
  if (left + right > .72) {
    // Fleeting marks where the ball meets a paddle.
    const side = left > right ? 1 : -1;
    g.moveTo(ballX + 5*side, ballY-4); g.lineTo(ballX + 10*side, ballY-8);
    g.moveTo(ballX + 6*side, ballY+1); g.lineTo(ballX + 12*side, ballY+2);
  } else {
    const trail = Math.sign(travel);
    g.moveTo(ballX-6*trail, ballY+1); g.lineTo(ballX-16*trail, ballY+3);
    g.moveTo(ballX-10*trail, ballY+5); g.lineTo(ballX-20*trail, ballY+7);
  }
  g.stroke();
  g.fillStyle = '#554334'; g.beginPath();
  g.arc(ballX, ballY, 3.2, 0, 2*Math.PI); g.fill();
  g.fillStyle = '#dfc397'; g.fillRect(ballX-1.2, ballY-1.6, 1.3, 1.3);
}

let sheet: HTMLCanvasElement | undefined;
function frontPage() {
  if (sheet) return sheet;
  const c = document.createElement('canvas');
  c.width = 1440; c.height = 1920;
  const g = c.getContext('2d')!;
  g.scale(1.5, 1.5);
  const paper = g.createLinearGradient(0, 0, 960, 28);
  for (const [at, color] of [[0,'#cfc4aa'], [.46,'#e4dac1'], [.495,'#d4c9ac'], [.502,'#e7ddc4'], [1,'#d8ccb1']] as const) paper.addColorStop(at, color);
  g.fillStyle = paper; g.fillRect(0, 0, 960, 1280);
  let seed = 27;
  const random = () => ((seed = (Math.imul(1664525, seed) + 1013904223) >>> 0) / 4294967296);
  g.fillStyle = 'rgba(84,66,40,.08)';
  for (let i = 0; i < 70000; i++) g.fillRect(random()*960, random()*1280, random()*2+.5, .8);
  // Aged edges: newsprint yellows from the outside in.
  const age = g.createRadialGradient(480, 640, 300, 480, 640, 900);
  age.addColorStop(0, 'rgba(120,90,50,0)'); age.addColorStop(1, 'rgba(120,90,50,.22)');
  g.fillStyle = age; g.fillRect(0, 0, 960, 1280);
  sheet=c; return c;
}

function cat(g: CanvasRenderingContext2D, x: number, side: number, reach: number, jump: number) {
  g.save(); g.translate(x, 294-jump); g.scale(side,1);
  g.strokeStyle='#494139'; g.lineWidth=2.4; g.lineCap='round';
  g.beginPath(); g.moveTo(-7,0); g.bezierCurveTo(-32,-3,-28,-28,-34,-19); g.stroke();
  const fur=side===1 ? '#a47c58' : '#f0e1c3'; g.fillStyle=fur;
  g.beginPath(); g.ellipse(0,-7,12,15,-.18,0,Math.PI*2); g.fill(); g.stroke();
  g.beginPath(); g.moveTo(-12,-29); g.lineTo(-13,-43); g.lineTo(-3,-36); g.lineTo(4,-36);
  g.lineTo(13,-43); g.lineTo(14,-27); g.bezierCurveTo(18,-12,-15,-12,-12,-29); g.fill(); g.stroke();
  g.fillStyle='#494139'; g.fillRect(2,-30,2,3); g.fillRect(9,-30,2,3); g.fillRect(6,-25,3,2);
  g.fillStyle = '#bd8f74'; g.fillRect(-9, -38, 2.5, 5); g.fillRect(9, -38, 2.5, 5);
  g.strokeStyle = '#5c4c3d'; g.lineWidth = .8;
  g.beginPath(); g.moveTo(-5,-34); g.lineTo(-4,-30); g.moveTo(-1,-34); g.lineTo(0,-31);
  g.moveTo(1,-25); g.lineTo(-5,-26); g.moveTo(1,-23); g.lineTo(-5,-22);
  g.moveTo(11,-25); g.lineTo(17,-26); g.moveTo(11,-23); g.lineTo(17,-22); g.stroke();
  g.strokeStyle = '#494139'; g.lineWidth = 2;
  g.beginPath(); g.moveTo(-6,5); g.lineTo(-10,10); g.lineTo(-4,10); g.moveTo(5,5); g.lineTo(9,10); g.lineTo(14,10); g.stroke();
  g.translate(6,-12); g.rotate(-reach*.45);
  g.beginPath(); g.moveTo(0,0); g.quadraticCurveTo(10,6,19,-4); g.lineTo(26,-13); g.stroke();
  g.fillStyle='#945e4e'; g.beginPath(); g.ellipse(28,-16,6,9,.45,0,2*Math.PI); g.fill(); g.stroke();
  g.restore();
}

export const newsprint: Painter = (g,w,h,t) => {
  const page=frontPage();
  // Overscan the small pan so a page edge never reveals a backdrop.
  const scale=Math.max(w/(page.width-64), h/(page.height-64));
  g.save();
  g.translate((w-page.width*scale)/2+16*scale*Math.sin(t*.055),(-26+12*Math.cos(t*.043))*scale);
  g.drawImage(page, 0, 0, page.width*scale, page.height*scale);
  g.restore();

  // Laid out in the visible pane, so every section stays on screen at any window shape.
  const stacked = w/h < .95;
  const unit = Math.min(w/(stacked ? 460 : 960), h/(stacked ? 900 : 560));
  g.save();
  g.translate(8*unit*Math.sin(t*.055), 5*unit*Math.cos(t*.043));
  g.scale(unit, unit);
  const width = w/unit, height = h/unit;
  const margin = Math.max(24, width*.045), pageWidth = width-2*margin, bottom = height-22;
  const ink = '#3f3a32', rule = '#9a8f7a';

  // Masthead and dateline bar.
  g.fillStyle = ink; g.textAlign = 'center';
  g.font = `bold ${stacked ? 40 : 58}px Georgia, serif`;
  g.fillText('The Evening Ledger', width/2, stacked ? 52 : 66, pageWidth);
  const bar = stacked ? 64 : 80, dateline = bar+19;
  g.fillRect(margin, bar, pageWidth, 2.2); g.fillRect(margin, bar+4, pageWidth, .8);
  g.font = '11px Georgia, serif';
  g.textAlign = 'left'; g.fillText('VOL. XVIII · No. 42', margin, dateline);
  g.textAlign = 'center'; g.fillText('SATURDAY, OCTOBER 12', width/2, dateline);
  g.textAlign = 'right'; g.fillText('FIVE CENTS', width-margin, dateline);
  g.textAlign = 'left';
  g.fillRect(margin, bar+27, pageWidth, .8); g.fillRect(margin, bar+30, pageWidth, 2.2);
  const top = bar+48, gutter = 26;
  const leftWidth = stacked ? pageWidth : pageWidth*.62;
  const rightX = stacked ? margin : margin+leftWidth+gutter;
  const rightWidth = stacked ? pageWidth : pageWidth-leftWidth-gutter;

  // Left: the lead photo, its headline, and two columns of story.
  const photoHeight = stacked ? leftWidth*.5 : Math.min(leftWidth*.5, (bottom-top)*.5);
  if (photo) {
    const cover = Math.max(leftWidth/photo.width, photoHeight/photo.height);
    const sw = leftWidth/cover, sh = photoHeight/cover;
    g.save(); g.globalCompositeOperation = 'multiply'; g.globalAlpha = .88;
    g.drawImage(photo, (photo.width-sw)/2, (photo.height-sh)*.61, sw, sh, margin, top, leftWidth, photoHeight);
    g.restore();
  }
  g.strokeStyle = rule; g.lineWidth = .6; g.strokeRect(margin, top, leftWidth, photoHeight);
  g.fillStyle = ink; g.font = `bold ${stacked ? 30 : 38}px Georgia, serif`;
  g.fillText('Market day on the quay', margin, top+photoHeight+(stacked ? 38 : 46), leftWidth);
  const leadText = top+photoHeight+(stacked ? 56 : 66);
  const leadBottom = stacked ? leadText+9*LINE : bottom;
  const columnWidth = (leftWidth-gutter)/2;
  textLines(g, margin, leadText, columnWidth, leadBottom, 1);
  textLines(g, margin+columnWidth+gutter, leadText, columnWidth, leadBottom, 4);
  g.fillStyle = rule;
  g.fillRect(margin+columnWidth+gutter/2-.4, leadText-4, .8, leadBottom-leadText);
  if (!stacked) g.fillRect(margin+leftWidth+gutter/2-.4, top, .8, bottom-top);

  // Right: a second story above the comic strip.
  const storyTop = stacked ? leadBottom+30 : top;
  if (stacked) g.fillRect(margin, storyTop-16, pageWidth, .8);
  g.fillStyle = ink; g.font = 'bold 28px Georgia, serif';
  g.fillText('The ferry waits', rightX, storyTop+22, rightWidth);
  g.fillText('a minute', rightX, storyTop+54, rightWidth);
  const stripScale = rightWidth/290, stripTop = bottom-96*stripScale;
  textLines(g, rightX, storyTop+74, rightWidth, stripTop-44, 2);
  g.fillStyle = ink; g.fillRect(rightX, stripTop-30, rightWidth, 1.4);
  g.font = 'bold 11px Georgia, serif'; g.fillText('THE ALLEY LEAGUE', rightX, stripTop-12);

  // Three panels cut from one scene, so the ball crosses the gutters mid-rally.
  g.translate(rightX, stripTop); g.scale(stripScale, stripScale);
  for (const x of [0, 99, 198]) {
    g.save(); g.beginPath(); g.rect(x, 0, 92, 96); g.clip();
    g.fillStyle = 'rgba(157,137,104,.13)'; g.fillRect(x, 0, 92, 96);
    g.fillStyle = 'rgba(95,77,54,.19)';
    for (let row = 0; row < 7; row++) {
      for (let dx = 4 + (row % 2)*2; dx < 90; dx += 4) g.fillRect(x+dx, 67+row*4, 1, 1);
    }
    g.fillStyle = 'rgba(95,77,54,.23)'; g.fillRect(x, 77, 92, .7);
    g.translate(0, -214); rally(g, t);
    g.restore();
    g.strokeStyle = ink; g.lineWidth = 1.6; g.strokeRect(x, 0, 92, 96);
  }
  g.restore();
};
