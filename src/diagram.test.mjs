import assert from 'node:assert/strict';
import test from 'node:test';
import { spaceArrows } from './diagram.ts';

test('compact flowchart arrows are spaced without changing labels or splitting longer operators', () => {
  for (const [line, expected] of [
    ['A-->B-->C', 'A --> B --> C'],
    ['A-.yes.->B', 'A -. yes .-> B'],
    ['A["x-->y"]-->|go-->now|B<-->C', 'A["x-->y"] -->|go-->now|B <--> C'],
    ['A----->B', 'A----->B'],
  ]) assert.equal(spaceArrows(`graph TD\n${line}`), `graph TD\n${expected}`);
});
