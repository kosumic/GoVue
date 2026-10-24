import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { createElement as h, createRef } from 'react';
import { act, create } from 'react-test-renderer';
import { build } from 'esbuild';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const require = createRequire(import.meta.url);
async function load(entry, mocks) {
  const result = await build({ entryPoints: [entry], bundle: true, write: false, platform: 'node', format: 'cjs', packages: 'external',
    plugins: [{ name: 'image-input-edges', setup(builder) {
      builder.onResolve({ filter: /\/(theme|govue-core|chatAttachmentSources)$/ }, args => ({ path: `mock:${args.path.split('/').at(-1)}`, external: true }));
    } }],
  });
  const module = { exports: {} };
  Function('require', 'module', 'exports', result.outputFiles[0].text)(name => mocks[name] ?? require(name), module, module.exports);
  return module.exports;
}
test('image gestures keep zoom and pan within the image, re-clamp after rotation, and double-tap resets', async t => {
  const { ZoomableChatImage } = await load('src/components/ZoomableChatImage.tsx', {
    'react-native': { View: 'View', Image: 'Image', Pressable: 'Pressable', Text: 'Text', StyleSheet: { create: styles => styles }, PanResponder: { create: handlers => ({ panHandlers: handlers }) } },
    'mock:theme': { useThemeStyles: factory => factory({}) },
  });
  let root;
  await act(() => { root = create(h(ZoomableChatImage, { uri: 'file:///photo.jpg', name: 'Photo', width: 1000, height: 1000 }), { createNodeMock: () => ({ measureInWindow: callback => callback(0, 100) }) }); });
  t.after(() => act(() => root.unmount()));
  const view = () => root.root.findByProps({ accessibilityRole: 'image' });
  const transform = () => root.root.findByType('Image').props.style[1].transform;
  await act(() => view().props.onLayout({ nativeEvent: { layout: { width: 400, height: 600 } } }));
  const event = touches => ({ nativeEvent: { touches: touches.map(([pageX, pageY]) => ({ pageX, pageY })) } });
  await act(() => view().props.onPanResponderGrant(event([[100, 400], [300, 400]])));
  await act(() => view().props.onPanResponderMove(event([[-100, 400], [500, 400]]), { dx: 0, dy: 0 }));
  assert.equal(view().props.accessibilityValue.text, '3.0 times zoom');
  await act(() => view().props.onPanResponderRelease(event([]), { dx: 0, dy: 0 }));
  await act(() => view().props.onPanResponderGrant(event([[200, 400]])));
  await act(() => view().props.onPanResponderMove(event([[1200, 1400]]), { dx: 1000, dy: 1000 }));
  assert.deepEqual(transform(), [{ translateX: 400 }, { translateY: 300 }, { scale: 3 }]);
  await act(() => view().props.onPanResponderRelease(event([]), { dx: 1000, dy: 1000 }));
  await act(() => view().props.onLayout({ nativeEvent: { layout: { width: 600, height: 400 } } }));
  assert.deepEqual(transform(), [{ translateX: 300 }, { translateY: 300 }, { scale: 3 }]);
  for (let i = 0; i < 2; i++) {
    await act(() => { view().props.onPanResponderGrant(event([[200, 400]])); view().props.onPanResponderRelease(event([]), { dx: 0, dy: 0 }); });
  }
  assert.deepEqual(transform(), [{ translateX: 0 }, { translateY: 0 }, { scale: 1 }]);
  await act(() => view().props.onAccessibilityAction({ nativeEvent: { actionName: 'activate' } }));
  assert.equal(view().props.accessibilityValue.text, '2.0 times zoom');
  await act(() => root.root.findByProps({ accessibilityLabel: 'Reset image zoom' }).props.onPress());
  assert.equal(view().props.accessibilityValue.text, '1.0 times zoom');
});

const deferred = () => { let resolve; const promise = new Promise(done => { resolve = done; }); return { resolve, promise }; };
async function pasteHarness({ take = async () => true, paste = () => {} } = {}) {
  const bound = [], claimed = [], unbound = [], discarded = [], errors = [];
  let listener;
  const native = {
    observeChatImagePaste: callback => { listener = callback; return { remove() {} }; },
    async bindChatImagePaste(tag, token) { bound.push({ tag, token }); },
    async unbindChatImagePaste(token) { unbound.push(token); },
    async takeChatImagePaste(token, uri) { claimed.push({ token, uri }); return take(); },
  };
  const { useChatImagePaste } = await load('src/ai/useChatImagePaste.ts', {
    'react-native': { findNodeHandle: () => 42 }, 'mock:govue-core': native,
    'mock:chatAttachmentSources': { discardChatAttachment: source => discarded.push(source) },
  });
  const input = createRef(); input.current = {};
  function Harness({ enabled, context }) { useChatImagePaste(input, enabled, context, paste, error => errors.push(error)); return null; }
  let root;
  await act(async () => { root = create(h(Harness, { enabled: true, context: 'first' })); });
  return { bound, claimed, unbound, discarded, errors,
    emit: event => act(async () => { listener(event); }),
    render: (enabled, context = 'first') => act(async () => { root.update(h(Harness, { enabled, context })); }),
    unmount: () => act(async () => root.unmount()),
  };
}
test('image paste claims only its bound input token and passes file metadata to the current draft', async t => {
  const pasted = [];
  const env = await pasteHarness({ paste: source => pasted.push(source) }); t.after(env.unmount);
  const source = { uri: 'file:///cache/chat-inputs/paste.png', name: 'paste.png', mimeType: 'image/png' };
  await env.emit({ ...source, token: 'another-input' }); assert.equal(env.claimed.length, 0);
  await env.emit({ ...source, token: env.bound[0].token });
  assert.deepEqual(pasted, [source]); assert.equal(env.claimed.length, 1); assert.equal(env.discarded.length, 0);
});
test('a paste claimed after hiding the chat is discarded and a rejected draft import releases its claimed file', async t => {
  const take = deferred(), pasted = [];
  const env = await pasteHarness({ take: () => take.promise, paste: source => pasted.push(source) }); t.after(env.unmount);
  const source = { uri: 'file:///cache/chat-inputs/late.png', name: 'late.png', mimeType: 'image/png' };
  await env.emit({ ...source, token: env.bound[0].token });
  await env.render(false);
  await act(async () => { take.resolve(true); });
  assert.deepEqual(pasted, []); assert.deepEqual(env.discarded, [source]); assert.deepEqual(env.unbound, [env.bound[0].token]);
  const failed = await pasteHarness({ paste: async () => { throw Error('Import failed'); } }); t.after(failed.unmount);
  await failed.emit({ ...source, token: failed.bound[0].token });
  assert.deepEqual(failed.discarded, [source]); assert.deepEqual(failed.errors, ['Could not paste this image.']);
});
