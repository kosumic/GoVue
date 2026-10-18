import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { createElement } from 'react';
import { act, create } from 'react-test-renderer';
import { build } from 'esbuild';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const require = createRequire(import.meta.url);
const placement = {
  x: 12, y: -185, width: 190, height: 106, right: 12, bottom: 68,
  closeArmed: false, dismiss: false,
  close: { left: 150, top: 700, width: 96, height: 72 },
};

async function harness(initial = {}) {
  const calls = [], animations = [], dismissals = [], errors = [];
  class Value {
    constructor(value) { this.value = value; }
    setValue(value) { this.value = value; }
    stopAnimation(callback) {
      for (const job of animations) if (job.target === this) job.cancelled = true;
      callback?.(this.value);
    }
  }
  class ValueXY extends Value {
    constructor() { super(); this.x = new Value(0); this.y = new Value(0); }
    setValue({ x, y }) { this.x.setValue(x); this.y.setValue(y); }
    stopAnimation(callback) {
      super.stopAnimation();
      callback?.({ x: this.x.value, y: this.y.value });
    }
  }
  const native = {
    Animated: {
      Value, ValueXY,
      timing: (target, { toValue }) => ({
        start: callback => animations.push({ target, toValue, callback }),
      }),
      parallel: children => ({ start(callback) {
        let remaining = children.length;
        for (const child of children) child.start(() => {
          if (--remaining === 0) callback?.({ finished: true });
        });
      } }),
    },
    Easing: { bezier: () => null },
    PanResponder: { create: panHandlers => ({ panHandlers }) },
    StyleSheet: { create: styles => styles, absoluteFill: {} },
    View: 'View',
  };
  const result = await build({
    entryPoints: ['src/player/useMiniPlayer.tsx'], bundle: true, write: false,
    platform: 'node', format: 'cjs', packages: 'external', jsx: 'automatic',
    plugins: [{ name: 'mini-player-edges', setup(builder) {
      builder.onResolve({ filter: /modules\/govue-core$|^\.\/Haptics$|^\.\/icons$/ },
        ({ path }) => ({ path, external: true }));
    } }],
  });
  const module = { exports: {} };
  Function('require', 'module', 'exports', result.outputFiles[0].text)(name => {
    if (name === 'react-native') return native;
    if (name === 'expo-linear-gradient') return { LinearGradient: 'Gradient' };
    if (name.endsWith('modules/govue-core')) return {
      coreRequest: request => new Promise(resolve => calls.push({ request, resolve })),
    };
    if (name === './Haptics') return { haptic() {} };
    if (name === './icons') return { Delete: 'Delete' };
    return require(name);
  }, module, module.exports);
  let mini, root;
  let props = {
    size: { width: 426, height: 860 }, bottomInset: 48, active: false, strength: 0,
    onDismiss: () => dismissals.push(true), onError: error => errors.push(error),
    ...initial,
  };
  function Player() { mini = module.exports.useMiniPlayer(props); return null; }
  await act(() => { root = create(createElement(Player)); });
  return {
    calls, dismissals, errors, mini: () => mini,
    values: () => [
      ...mini.motionStyle.transform.map(transform => Object.values(transform)[0].value),
      mini.motionStyle.opacity.value,
    ],
    render: changes => act(() => { props = { ...props, ...changes }; root.update(createElement(Player)); }),
    reply: (index, value = placement) => act(async () => { calls[index].resolve(value); }),
    flush: () => act(() => {
      for (const job of animations.splice(0)) {
        if (job.cancelled) continue;
        job.target.setValue(job.toValue);
        job.callback?.({ finished: true });
      }
    }),
    unmount: () => act(() => root.unmount()),
  };
}

test('expanded playback keeps neutral animation nodes across viewport changes', async t => {
  const env = await harness();
  t.after(env.unmount);
  const nodes = env.mini().motionStyle;
  await env.render({ size: { width: 240, height: 135 } });
  await env.render({ size: { width: 426, height: 860 } });
  assert.equal(env.calls.length, 0, 'PiP resizing must not settle an inactive mini player');
  assert.deepEqual(env.values(), [0, 0, 1, 1]);
  assert.equal(env.mini().motionStyle.transform[0].translateX, nodes.transform[0].translateX);
  assert.equal(env.mini().motionStyle.opacity, nodes.opacity);
  assert.equal(env.mini().style.transform, undefined, 'geometry can change without detaching animated nodes');
});

test('expanding stops queued native motion and resets the same mounted player', async t => {
  const env = await harness({ active: true });
  t.after(env.unmount);
  const x = env.mini().motionStyle.transform[0].translateX;
  await env.reply(0);
  await env.flush();
  assert.deepEqual(env.values(), [12, -185, 1, 1]);
  await env.render({ size: { width: 426, height: 952 } });
  await env.reply(1, { ...placement, y: -277 });
  await env.render({ active: false });
  await env.flush();
  assert.deepEqual(env.values(), [0, 0, 1, 1], 'a late native animation tick must not move the expanded player');
  assert.equal(env.mini().motionStyle.transform[0].translateX, x);
  await env.render({ size: { width: 426, height: 860 } });
  assert.equal(env.calls.length, 2);
  assert.deepEqual(env.values(), [0, 0, 1, 1]);
});

test('late Rust placement replies cannot overwrite expansion or a newer mini session', async t => {
  const env = await harness({ active: true });
  t.after(env.unmount);
  await env.render({ active: false });
  await env.reply(0);
  await env.flush();
  assert.deepEqual(env.values(), [0, 0, 1, 1]);
  await env.render({ active: true });
  await env.render({ active: false });
  await env.render({ active: true, size: { width: 500, height: 900 } });
  await env.reply(1);
  await env.flush();
  assert.deepEqual(env.values(), [0, 0, 1, 1]);
  assert.equal(env.calls[2].request.width, 500);
  await env.reply(2, { ...placement, x: -20, y: -200 });
  await env.flush();
  assert.deepEqual(env.values(), [-20, -200, 1, 1]);
  assert.deepEqual(env.errors, []);
});

test('expanding during dismissal cancels native fade and its close callback', async t => {
  const env = await harness({ active: true });
  t.after(env.unmount);
  await env.reply(0);
  await env.flush();
  await act(() => env.mini().dismiss());
  await env.render({ active: false });
  await env.flush();
  assert.deepEqual(env.values(), [0, 0, 1, 1]);
  assert.deepEqual(env.dismissals, []);
});
