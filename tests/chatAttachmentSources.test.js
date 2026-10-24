import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { build } from 'esbuild';

const require = createRequire(import.meta.url);
const bundle = await build({ entryPoints: ['src/ai/chatAttachmentSources.ts'], bundle: true, write: false, platform: 'node', format: 'cjs', packages: 'external',
  plugins: [{ name: 'native-file-boundary', setup(builder) { builder.onResolve({ filter: /modules\/govue-core$/ }, () => ({ path: 'mock-core', external: true })); } }],
});
function harness({ granted = true, canceled = false, clipboard = null } = {}) {
  const files = new Map(), copies = [], deleted = [], calls = [];
  const asset = { uri: 'content://photos/selected-image', fileName: 'photo.png', mimeType: 'image/png' };
  const metadata = { size: 1024, marker: 'original native bytes' }; files.set(asset.uri, metadata);
  const uri = (parent, name) => `${typeof parent === 'string' ? parent : parent.uri}${name ? `/${name}` : ''}`;
  const fs = { Paths: { cache: { uri: 'file:///cache' } },
    File: class {
      constructor(parent, name) { this.uri = uri(parent, name); }
      get exists() { return files.has(this.uri); }
      get size() { return files.get(this.uri)?.size ?? 0; }
      async copy(target) { copies.push({ from: this.uri, to: target.uri }); files.set(target.uri, files.get(this.uri)); }
      delete() { deleted.push(this.uri); files.delete(this.uri); }
    },
    Directory: class {
      constructor(parent, name) { this.uri = uri(parent, name); this.exists = false; }
      create() { this.exists = true; }
      delete() { this.exists = false; for (const path of files.keys()) if (path.startsWith(`${this.uri}/`)) files.delete(path); }
    },
  };
  const images = { UIImagePickerPreferredAssetRepresentationMode: { Compatible: 'compatible' },
    async requestCameraPermissionsAsync() { calls.push({ op: 'permission' }); return { granted }; },
    async launchCameraAsync(options) { calls.push({ op: 'camera', options }); return { canceled, assets: [asset] }; },
    async launchImageLibraryAsync(options) { calls.push({ op: 'photos', options }); return { canceled, assets: [asset] }; },
  };
  const module = { exports: {} };
  Function('require', 'module', 'exports', bundle.outputFiles[0].text)(name => name === 'expo-file-system' ? fs : name === 'expo-image-picker' ? images
    : name === 'expo-document-picker' ? { getDocumentAsync: async options => { calls.push({ op: 'files', options }); return { canceled, assets: [{ uri: asset.uri, name: 'source.txt', mimeType: 'text/plain' }] }; } }
    : name === 'mock-core' ? { copyChatClipboardAttachment: async () => { calls.push({ op: 'clipboard' }); return clipboard; } } : require(name), module, module.exports);
  return { ...module.exports, files, copies, deleted, calls, asset, metadata };
}
test('photo staging copies bytes natively into disposable cache files and preserves the original library asset', async () => {
  const env = harness();
  const [source] = await env.pickChatAttachment('photo');
  assert.equal(source.name, 'photo.png');
  assert.equal(env.calls[0].options.preferredAssetRepresentationMode, 'compatible');
  const staged = await env.stageChatAttachment(source, 12 * 1024 * 1024);
  assert.match(staged.source.uri, /^file:\/\/\/cache\/chat-inputs\//);
  assert.deepEqual(env.copies, [{ from: source.uri, to: staged.source.uri }]);
  assert.equal(env.files.get(staged.source.uri), env.metadata, 'native copy carries the original bytes');
  staged.dispose(); env.discardChatAttachment(source);
  assert.equal(env.files.has(staged.source.uri), false);
  assert.equal(env.files.has(source.uri), true, 'a user library asset is never deleted');
  assert.equal(env.deleted.length, 0);
});
test('camera permission denial and picker cancellation do not create attachment copies', async () => {
  const denied = harness({ granted: false });
  await assert.rejects(denied.pickChatAttachment('camera'), /camera access/);
  assert.deepEqual(denied.calls.map(c => c.op), ['permission']);
  const camera = harness(); await camera.pickChatAttachment('camera');
  assert.deepEqual(camera.calls.map(c => c.op), ['permission', 'camera']);
  assert.deepEqual(camera.calls[1].options.mediaTypes, ['images']);
  const canceled = harness({ canceled: true });
  assert.deepEqual(await canceled.pickChatAttachment('file'), []);
  assert.deepEqual(await canceled.pickChatAttachment('photo'), []);
  assert.equal(canceled.copies.length, 0);
});
test('oversized files are rejected before native copying and clipboard contents are requested only on Paste', async () => {
  const env = harness({ clipboard: { uri: 'file:///cache/chat-inputs/pasted', name: 'pasted.pdf' } });
  env.metadata.size = 13 * 1024 * 1024;
  await assert.rejects(env.stageChatAttachment({ uri: env.asset.uri, name: 'large.png' }, 12 * 1024 * 1024), /12 MiB/);
  assert.equal(env.copies.length, 0);
  await env.pickChatAttachment('photo');
  assert.equal(env.calls.some(c => c.op === 'clipboard'), false);
  assert.equal((await env.pickChatAttachment('clipboard'))[0].name, 'pasted.pdf');
  assert.equal(env.calls.filter(c => c.op === 'clipboard').length, 1);
  await assert.rejects(harness().pickChatAttachment('clipboard'), /no file or image/);
});
