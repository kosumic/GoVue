import { test } from 'node:test';
import assert from 'node:assert/strict';
import { embeddedPlayerFrame } from '../src/media/playerLayout.ts';

const layout = { left: 10, top: 48, width: 380, height: 214, viewportWidth: 400, viewportHeight: 700, visible: true };

test('maps CSS video bounds into the app viewport, preserving scroll offsets', () => {
  assert.deepEqual(embeddedPlayerFrame(layout, 360, 600), { left: 9, top: 43.2, width: 342, height: 192.6 });
  assert.deepEqual(embeddedPlayerFrame({ ...layout, top: -80 }, 360, 600), { left: 9, top: -72, width: 342, height: 192.6 });
  assert.deepEqual(embeddedPlayerFrame(null, 400, 700), { left: 0, top: 0, width: 400, height: 225 });
  assert.deepEqual(embeddedPlayerFrame(null, 700, 160), { left: 0, top: 0, width: 700, height: 160 });
});
