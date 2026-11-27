// Only explicit numeric key=value samples, not incidental numbers in boot logs.
function seriesIn(text: string) {
  const series = new Map<string, number[]>();
  for (const m of text.matchAll(/(?:^|[\s,;])([a-z][\w.-]{0,31})\s*=\s*([-+]?(?:\d+(?:\.\d*)?|\.\d+)(?:e[-+]?\d+)?)(?=$|[\s,;])/gi)) {
    const value = Number(m[2]);
    if (!Number.isFinite(value)) continue;
    const values = series.get(m[1]) ?? [];
    values.push(value);
    series.set(m[1], values);
  }
  return [...series].filter(([, values]) => values.length >= 2);
}

function points(values: number[], min: number, max: number, count: number) {
  // Scale before subtracting so opposite, near-limit floats do not overflow.
  const scale = Math.max(Math.abs(min), Math.abs(max)) || 1;
  const lo = min / scale, span = max / scale - lo;
  return values.map((v, i) => `${+(100 * i / (count - 1)).toFixed(2)},${+(span ? 27 - 24 * (v / scale - lo) / span : 15).toFixed(2)}`).join(' ');
}

export function boardSignals(text: string) {
  return seriesIn(text).slice(0, 3).map(([name, values]) => {
    const min = Math.min(...values), max = Math.max(...values), count = values.length;
    return { name, min, max, count, points: points(values, min, max, count) };
  });
}

export function compareCaptures(before: string, after: string) {
  const lines = (s: string) => s ? s.replace(/\n$/, '').split('\n') : [];
  const a = lines(before), b = lines(after);
  let i = 0;
  while (i < a.length && i < b.length && a[i] === b[i]) i++;
  const difference = i === a.length && i === b.length ? null : { line: i + 1, before: a[i], after: b[i] };
  const prior = new Map(seriesIn(before));
  const traces = seriesIn(after).filter(([name]) => prior.has(name)).slice(0, 3).map(([name, values]) => {
    const base = prior.get(name)!;
    const min = Math.min(...base, ...values), max = Math.max(...base, ...values);
    const count = Math.max(base.length, values.length);
    return { name, min, max, before: points(base, min, max, count), after: points(values, min, max, count) };
  });
  return { difference, traces };
}
