import assert from 'node:assert/strict';
import test from 'node:test';
import { createServer } from 'vite';

test('first-save choices accrue only for the matching type and never invent decisions', async () => {
  const vite = await createServer({ configFile: false, optimizeDeps: { noDiscovery: true }, server: { middlewareMode: true, hmr: false, watch: null } });
  try {
    const { applyTypeChange, typeState, types } = await vite.ssrLoadModule('/src/savedTypes.ts');
    const saves = typeState.firstSaves;
    saves.set('book', {});
    applyTypeChange(null);
    applyTypeChange('book'); // A list or item refresh is not a design choice.
    applyTypeChange({ id: 'other', choices: { fields: ['Name'] } });
    assert.deepEqual(saves.get('book'), {});
    assert.equal(saves.has('other'), false);
    const layout = { kind: 'book', style: 'cookbook' };
    applyTypeChange({ id: 'book', choices: { layout } });
    assert.deepEqual(saves.get('book'), { layout });
    const look = { font: 'serif', texture: 'linen', accent: '#985341' };
    applyTypeChange({ id: 'book', choices: { look, fields: ['Title', 'Ingredients'] } });
    assert.deepEqual(saves.get('book'), { layout, look, fields: ['Title', 'Ingredients'] });
    assert.equal(types.revision.book, 3);
    saves.delete('book');
    applyTypeChange({ id: 'book', choices: { look } });
    assert.equal(saves.size, 0);
  } finally {
    await vite.close();
  }
});
