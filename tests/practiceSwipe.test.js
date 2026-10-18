import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { createElement as h } from 'react';
import { act, create } from 'react-test-renderer';
import { build } from 'esbuild';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const require = createRequire(import.meta.url);
const noop = () => {};
const game = { id: 'saved-game', definition: { title: 'Grammar' }, state: {}, revision: 0, cues: [] };
const bundled = await build({ entryPoints: ['src/components/GamePractice.tsx'], bundle: true, write: false, platform: 'node', format: 'cjs', packages: 'external',
  plugins: [{ name: 'practice-edges', setup(builder) {
    builder.onResolve({ filter: /\/(usePractice|LearningGameCard|IconButton|theme|useReducedMotion|Haptics)$/ },
      ({ path }) => ({ path: `mock:${path.split('/').at(-1)}`, external: true }));
  } }],
});

async function harness(t) {
  const reviews = [], animations = [];
  let busy = false;
  class Value {
    constructor(value) { this.value = value; }
    setValue(value) { this.value = value; }
    interpolate() { return {}; }
    stopAnimation() { for (const job of animations) if (job.target === this) job.cancelled = true; }
  }
  const animation = (target, { toValue }) => ({
    start(callback) { animations.push({ target, toValue, callback }); },
    stop() { target.stopAnimation(); },
  });
  const colors = { background: '#fff', ink: '#000', muted: '#777', raised: '#eee', line: '#ddd', accent: '#000', inverted: '#fff' };
  const mocks = {
    'react-native': {
      View: 'View', Text: 'Text', Pressable: 'Pressable', ScrollView: 'ScrollView', ActivityIndicator: 'ActivityIndicator',
      StyleSheet: { create: value => value, hairlineWidth: 1 },
      Animated: { Value, View: 'AnimatedView', Text: 'AnimatedText', timing: animation, spring: animation,
        parallel: jobs => ({ start: () => jobs.forEach(job => job.start()) }) },
      PanResponder: { create: panHandlers => ({ panHandlers }) },
      AppState: { currentState: 'active', addEventListener: () => ({ remove: noop }) },
      useWindowDimensions: () => ({ width: 402, height: 874 }),
    },
    'lucide-react-native': new Proxy({}, { get: (_, name) => props => h(`Icon${String(name)}`, props) }),
    usePractice: { usePractice: () => ({
      library: { games: [], session: { started: true, game, remaining: 1, total: 1, canUndo: false } },
      busy, error: '', review: async verdict => { reviews.push(verdict); }, gameAction: noop, reset: async () => {},
    }) },
    LearningGameCard: { GameSurface: props => h('GameSurface', props) },
    IconButton: { IconButton: props => h('Pressable', { ...props, accessibilityLabel: props.label }) },
    theme: { useThemeColors: () => colors, useThemeStyles: factory => factory(colors) },
    useReducedMotion: { useReducedMotion: () => false },
    Haptics: { useHaptic: () => noop },
  };
  const module = { exports: {} };
  Function('require', 'module', 'exports', bundled.outputFiles[0].text)(name => name.startsWith('mock:') ? mocks[name.slice(5)] : mocks[name] ?? require(name), module, module.exports);
  let root;
  await act(() => { root = create(h(module.exports.GamePractice)); });
  t.after(() => act(() => root.unmount()));
  const card = () => root.root.findByType('AnimatedView');
  const values = () => card().props.style[1].transform.slice(0, 2).map(transform => Object.values(transform)[0].value);
  const flush = () => act(async () => {
    for (const job of animations.splice(0)) if (!job.cancelled) { job.target.setValue(job.toValue); job.callback?.({ finished: true }); }
  });
  await flush();
  return { root, card, values, reviews, flush,
    busy: value => act(() => { busy = value; root.update(h(module.exports.GamePractice)); }),
    gesture: (method, values = {}) => act(() => card().props[method]({}, { dx: 0, dy: 0, vx: 0, numberActiveTouches: 1, ...values })),
  };
}

test('the entire game lives inside the drag responder, which leaves taps, vertical scrolls and multiple fingers alone', async t => {
  const env = await harness(t);
  const body = env.root.root.findByType('GameSurface');
  let ancestor = body.parent;
  while (ancestor && ancestor !== env.card()) ancestor = ancestor.parent;
  assert.equal(ancestor, env.card(), 'touches over game content must reach the card capture handler');
  const capture = env.card().props.onMoveShouldSetPanResponderCapture;
  for (const gesture of [{ dx: 2, dy: 1 }, { dx: 12, dy: 100 }, { dx: 80, dy: 0, numberActiveTouches: 2 }]) {
    assert.equal(capture({}, { numberActiveTouches: 1, ...gesture }), false);
  }
  assert.equal(capture({}, { dx: 30, dy: 4, numberActiveTouches: 1 }), true);
  assert.equal(capture({}, { dx: -30, dy: -4, numberActiveTouches: 1 }), true);
  assert.deepEqual(env.reviews, []);
});

test('a card follows both finger coordinates and a short drag springs back without grading', async t => {
  const env = await harness(t);
  await env.gesture('onPanResponderGrant');
  await env.gesture('onPanResponderMove', { dx: 30, dy: 12 });
  assert.deepEqual(env.values(), [30, 12]);
  await env.gesture('onPanResponderRelease', { dx: 30, dy: 12 });
  await env.flush();
  assert.deepEqual(env.values(), [0, 0]);
  assert.deepEqual(env.reviews, []);
});

for (const [dx, verdict] of [[-160, 'forgot'], [160, 'know']]) {
  test(`a drag to ${dx} commits ${verdict} once after release`, async t => {
    const env = await harness(t);
    await env.gesture('onPanResponderGrant');
    await env.gesture('onPanResponderMove', { dx, dy: 16 });
    assert.deepEqual(env.reviews, [], 'moving past the threshold does not grade before release');
    await env.gesture('onPanResponderRelease', { dx, dy: 16 });
    assert.equal(env.root.root.findAllByType('GameSurface').length, 1, 'the game remains visible while the card flies out');
    await env.gesture('onPanResponderRelease', { dx, dy: 16 });
    await env.flush();
    assert.deepEqual(env.reviews, [verdict]);
  });
}

test('rerenders keep one responder, use current busy state and stop a returning spring when grabbed again', async t => {
  const env = await harness(t);
  const capture = env.card().props.onMoveShouldSetPanResponderCapture;
  await env.busy(true);
  assert.equal(env.card().props.onMoveShouldSetPanResponderCapture, capture);
  assert.equal(capture({}, { dx: 100, dy: 0, numberActiveTouches: 1 }), false);
  await env.busy(false);
  assert.equal(capture({}, { dx: 100, dy: 0, numberActiveTouches: 1 }), true);
  await env.gesture('onPanResponderMove', { dx: 25, dy: 5 });
  await env.gesture('onPanResponderRelease', { dx: 25, dy: 5 });
  // Re-grabbing during snap-back must cancel that spring before following the finger.
  await env.gesture('onPanResponderGrant');
  await env.gesture('onPanResponderMove', { dx: 60, dy: 20 });
  await env.flush();
  assert.deepEqual(env.values(), [60, 20]);
  await env.gesture('onPanResponderTerminate');
  await env.flush();
  assert.deepEqual(env.values(), [0, 0]);
  assert.deepEqual(env.reviews, []);
});
