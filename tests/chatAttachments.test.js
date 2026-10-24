import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { createElement } from 'react';
import { act, create } from 'react-test-renderer';
import { build } from 'esbuild';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const require = createRequire(import.meta.url);
const bundle = await build({ entryPoints: ['src/ai/useChatAttachments.ts'], bundle: true, write: false, platform: 'node', format: 'cjs', packages: 'external',
  plugins: [{ name: 'attachment-native-boundary', setup(builder) {
    builder.onResolve({ filter: /modules\/govue-core$/ }, () => ({ path: 'mock-core', external: true }));
    builder.onResolve({ filter: /\/chatAttachmentSources$/ }, () => ({ path: 'mock-sources', external: true }));
  } }],
});
const file = (id = 'file', kind = 'text') => ({ id, kind, name: `${id}.ts`, mimeType: 'text/plain', uri: `file:///private/${id}.txt`, width: 0, height: 0, bytes: 20 });
const asset = name => ({ name, uri: `file:///picker-cache/${name}`, mimeType: 'application/octet-stream' });
const deferred = () => { let resolve; const promise = new Promise(done => { resolve = done; }); return { promise, resolve }; };
async function harness({ pick = async () => ({ canceled: true }), info = async () => ({ exists: true, isDirectory: false, size: 20 }), save = async command => file(command.name) } = {}) {
  const calls = [], reads = [], deleted = [];
  let current, root;
  const core = { async aiRequest(command) {
    calls.push(command);
    if (command.op === 'ai_chat_attachment_limits') return { maxAttachments: 4, maxSourceBytes: 12 * 1024 * 1024 };
    if (command.op === 'ai_chat_attachment_import') return save(command);
    if (command.op === 'ai_chat_attachment_release') return null;
    throw Error(`Unexpected operation ${command.op}`);
  } };
  core.importChatAttachment = async command => { calls.push({ ...command, op: 'ai_chat_attachment_import' }); return save(command); };
  const fs = { EncodingType: { Base64: 'base64' }, getInfoAsync: info,
    async readAsStringAsync(uri, options) { reads.push({ uri, options }); return 'ZmlsZQ=='; },
    async deleteAsync(uri, options) { deleted.push({ uri, options }); },
  };
  const module = { exports: {} };
  const sources = {
    async pickChatAttachment() { const result = await pick({ type: '*/*', multiple: true, copyToCacheDirectory: true }); return result.canceled ? [] : result.assets; },
    discardChatAttachment: source => { deleted.push({ uri: source.uri, options: { idempotent: true } }); },
    async stageChatAttachment(source, maxBytes) {
      const metadata = await info(source.uri);
      if (metadata.size > maxBytes) throw Error(`Choose a file smaller than ${maxBytes / 1024 / 1024} MiB.`);
      return { source, dispose() {} };
    },
  };
  Function('require', 'module', 'exports', bundle.outputFiles[0].text)(name => name === 'mock-core' ? core
    : name === 'mock-sources' ? sources : name === 'expo-document-picker' ? { getDocumentAsync: pick } : name === 'expo-file-system/legacy' ? fs : require(name), module, module.exports);
  function Harness({ context }) { current = module.exports.useChatAttachments(context); return null; }
  await act(async () => { root = create(createElement(Harness, { context: 'first' })); });
  return { calls, reads, deleted, state: () => current,
    pick: () => act(async () => { await current.pick(); }),
    render: context => act(async () => { root.update(createElement(Harness, { context })); }),
    unmount: () => act(async () => { root.unmount(); }),
  };
}
test('picker imports names and MIME hints, retains successful files on a partial failure, and clears temporary copies', async t => {
  let round = 0;
  const env = await harness({
    pick: async options => { assert.deepEqual(options, { type: '*/*', multiple: true, copyToCacheDirectory: true }); return { canceled: false, assets: [asset('source.ts'), asset('binary.exe')] }; },
    save: async command => { if (++round === 2) throw Error('Choose a UTF-8 text or code file.'); return file(command.name); },
  });
  t.after(env.unmount);
  await env.pick();
  assert.deepEqual(env.state().attachments.map(f => f.id), ['source.ts']);
  assert.match(env.state().error, /UTF-8/);
  assert.equal(env.state().busy, false);
  assert.equal(env.calls.find(c => c.op === 'ai_chat_attachment_import').mimeType, 'application/octet-stream');
  assert.deepEqual(env.deleted.map(f => f.uri), [asset('source.ts').uri, asset('binary.exe').uri]);
  await env.render('next');
  assert.deepEqual(env.state().attachments, []);
  assert.deepEqual(env.calls.at(-1), { op: 'ai_chat_attachment_release', attachments: ['source.ts'] });
});
test('oversized picks are rejected before reading bytes and a five-file selection is rejected before import', async t => {
  const large = await harness({ pick: async () => ({ canceled: false, assets: [asset('large.pdf')] }), info: async () => ({ exists: true, isDirectory: false, size: 13 * 1024 * 1024 }) });
  t.after(large.unmount);
  await large.pick();
  assert.equal(large.reads.length, 0); assert.match(large.state().error, /12 MiB/); assert.equal(large.deleted.length, 1);
  const many = await harness({ pick: async () => ({ canceled: false, assets: Array.from({ length: 5 }, (_, i) => asset(`${i}.txt`)) }) });
  t.after(many.unmount);
  await many.pick();
  assert.equal(many.reads.length, 0); assert.match(many.state().error, /up to 4/); assert.equal(many.deleted.length, 5);
});
test('a picker returning after a chat switch cannot attach files to the new chat', async t => {
  const picker = deferred();
  const env = await harness({ pick: () => picker.promise }); t.after(env.unmount);
  let pending;
  await act(async () => { pending = env.state().pick(); });
  await env.render('second');
  await act(async () => { picker.resolve({ canceled: false, assets: [asset('late.txt')] }); await pending; });
  assert.deepEqual(env.state().attachments, []); assert.equal(env.state().busy, false); assert.equal(env.reads.length, 0);
  assert.deepEqual(env.deleted.map(f => f.uri), [asset('late.txt').uri]);
});
test('an import finishing after a chat switch releases its private copy without altering the new draft', async t => {
  const imported = deferred();
  const env = await harness({ pick: async () => ({ canceled: false, assets: [asset('late.txt')] }), save: () => imported.promise }); t.after(env.unmount);
  let pending;
  await act(async () => { pending = env.state().pick(); });
  assert.equal(env.calls.at(-1).op, 'ai_chat_attachment_import');
  await env.render('second');
  await act(async () => { imported.resolve(file('late')); await pending; });
  assert.deepEqual(env.state().attachments, []); assert.equal(env.state().busy, false);
  assert.deepEqual(env.calls.at(-1), { op: 'ai_chat_attachment_release', attachments: ['late'] });
  assert.equal(env.deleted.length, 1);
});

test('a new chat importing the same file waits for the old import and its release', async t => {
  const old = deferred(); let saves = 0;
  const env = await harness({ pick: async () => ({ canceled: false, assets: [asset('same.txt')] }), save: () => ++saves === 1 ? old.promise : Promise.resolve(file('same')) });
  t.after(env.unmount);
  let first, second;
  await act(async () => { first = env.state().pick(); });
  await env.render('second');
  await act(async () => { second = env.state().pick(); });
  assert.equal(saves, 1, 'the old import still owns this canonical file ID');
  await act(async () => { old.resolve(file('same')); await first; await second; });
  assert.deepEqual(env.calls.filter(c => c.op !== 'ai_chat_attachment_limits').map(c => c.op), ['ai_chat_attachment_import', 'ai_chat_attachment_release', 'ai_chat_attachment_import']);
  assert.deepEqual(env.state().attachments.map(f => f.id), ['same']);
});
