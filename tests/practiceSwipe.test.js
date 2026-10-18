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
    builder.onResolve({ filter: /\/(usePractice|LearningGameCard|IconButton|theme|useReducedMotion|Haptics|PlayerSheet)$/ },
      ({ path }) => ({ path: `mock:${path.split('/').at(-1)}`, external: true }));
  } }],
});

async function harness(t, options = {}) {
  const reviews = [], animations = [], renames = [];
  let busy = false;
  let currentGame = structuredClone(game);
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
      View: 'View', Text: 'Text', TextInput: 'TextInput', Pressable: 'Pressable', ScrollView: 'ScrollView', ActivityIndicator: 'ActivityIndicator', Keyboard: { dismiss: noop },
      StyleSheet: { create: value => value, hairlineWidth: 1 },
      Animated: { Value, View: 'AnimatedView', Text: 'AnimatedText', timing: animation, spring: animation,
        parallel: jobs => ({ start: () => jobs.forEach(job => job.start()) }) },
      PanResponder: { create: panHandlers => ({ panHandlers }) },
      AppState: { currentState: 'active', addEventListener: () => ({ remove: noop }) },
      useWindowDimensions: () => ({ width: 402, height: 874 }),
    },
    'lucide-react-native': new Proxy({}, { get: (_, name) => props => h(`Icon${String(name)}`, props) }),
    usePractice: { usePractice: () => ({
      library: { games: [{ id: currentGame.id, title: currentGame.definition.title, source: null, verdict: null }],
        session: { started: options.reviewing !== false, game: options.reviewing === false ? null : currentGame, remaining: 1, total: 1, canUndo: false } },
      busy, error: '', review: async verdict => { reviews.push(verdict); }, gameAction: noop, reset: async () => {},
      rename: async (id, title) => {
        renames.push({ id, title }); await options.rename?.(id, title);
        currentGame = { ...currentGame, definition: { ...currentGame.definition, title: options.savedTitle ?? title } };
        root.update(h(module.exports.GamePractice));
      },
    }) },
    LearningGameCard: { GameSurface: props => h('GameSurface', props) },
    IconButton: { IconButton: props => h('Pressable', { ...props, accessibilityLabel: props.label }) },
    PlayerSheet: { PlayerSheet: ({ visible, children, footer }) => visible ? h('Dialog', null, children, footer) : null },
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
  return { root, card, values, reviews, renames, flush,
    button: label => root.root.findByProps({ accessibilityLabel: label }),
    input: () => root.root.findByType('TextInput'),
    labels: () => root.root.findAllByType('Text').map(node => node.children.join('')),
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

test('renaming from the card keeps the running game mounted and shows the canonical saved name', async t => {
  const env = await harness(t, { savedTitle: 'Past obligation' });
  const body = env.root.root.findByType('GameSurface');
  await act(() => env.button('Rename activity').props.onPress());
  assert.equal(env.input().props.value, 'Grammar');
  await act(() => env.input().props.onChangeText('  Past obligation  '));
  await act(async () => env.button('Save activity name').props.onPress());
  assert.deepEqual(env.renames, [{ id: 'saved-game', title: '  Past obligation  ' }]);
  assert.ok(env.labels().includes('Past obligation'));
  assert.equal(env.root.root.findAllByType('Dialog').length, 0);
  assert.equal(env.root.root.findByType('GameSurface'), body);
  assert.deepEqual(env.reviews, []);
});

test('library entries can be renamed without starting a review and blank names disable Save', async t => {
  const env = await harness(t, { reviewing: false, savedTitle: 'My saved grammar' });
  await act(() => env.button('Rename Grammar').props.onPress());
  await act(() => env.input().props.onChangeText('   '));
  assert.equal(env.button('Save activity name').props.disabled, true);
  assert.deepEqual(env.renames, []);
  await act(() => env.input().props.onChangeText('My saved grammar'));
  await act(async () => env.button('Save activity name').props.onPress());
  assert.ok(env.button('Practice My saved grammar'));
  assert.ok(env.button('Rename My saved grammar'));
  assert.equal(env.root.root.findAllByType('GameSurface').length, 0);
  assert.deepEqual(env.reviews, []);
});

test('a failed rename keeps the editor and original game open so the user can retry', async t => {
  let attempt = 0;
  const env = await harness(t, { savedTitle: 'Saved name', rename: async () => { if (++attempt === 1) throw Error('Storage temporarily unavailable'); } });
  const body = env.root.root.findByType('GameSurface');
  await act(() => env.button('Rename activity').props.onPress());
  await act(() => env.input().props.onChangeText('Saved name'));
  await act(async () => env.button('Save activity name').props.onPress());
  assert.ok(env.labels().includes('Storage temporarily unavailable'));
  assert.equal(env.input().props.value, 'Saved name');
  assert.ok(env.labels().includes('Grammar'));
  await act(async () => env.button('Save activity name').props.onPress());
  assert.equal(env.renames.length, 2);
  assert.ok(env.labels().includes('Saved name'));
  assert.equal(env.root.root.findAllByType('Dialog').length, 0);
  assert.equal(env.root.root.findByType('GameSurface'), body);
});
