import assert from 'node:assert/strict';
import test from 'node:test';
import { createServer } from 'vite';

test('formula parser evaluates arithmetic and rejects prototype names or deep unary/power chains', async () => {
  const vite = await createServer({
    configFile: false,
    optimizeDeps: { noDiscovery: true },
    server: { middlewareMode: true, hmr: false, watch: null },
  });
  try {
    const { parseFormula, evaluate } = await vite.ssrLoadModule('/src/formula.ts');
    assert.equal(evaluate(parseFormula('2 + 3 * 4'), {}), 14);
    assert.equal(evaluate(parseFormula('-2 ^ 2'), {}), -4);
    assert.equal(evaluate(parseFormula('clamp(x, 0, 10)'), { x: 42 }), 10);

    // Prototype properties must be rejected cleanly as unknown functions, never throwing TypeError.
    for (const bad of ['toString()', 'constructor()', 'valueOf()', '__proto__()']) {
      assert.throws(() => parseFormula(bad), /unknown function/);
    }

    // Deep unary `+` and right-associative `^` chains respect MAX_DEPTH.
    assert.throws(() => parseFormula('+'.repeat(50) + '1'), /nested too deeply/);
    assert.throws(() => parseFormula('2^'.repeat(50) + '2'), /nested too deeply/);
  } finally {
    await vite.close();
  }
});
