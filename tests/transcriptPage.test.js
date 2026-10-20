import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { createContext, runInContext } from 'node:vm';
import { createElement } from 'react';
import { act, create } from 'react-test-renderer';
import { build } from 'esbuild';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const require = createRequire(import.meta.url);
const videoId = 'abcdefghijk';

test('a caption task lost during document loading is delivered when ready and accepts only its matching reply', async t => {
  const compiled = await build({ entryPoints: ['src/browser/useTranscriptPage.ts'], bundle: true, write: false, platform: 'node', format: 'cjs', packages: 'external',
    plugins: [{ name: 'caption-page-edge', setup(builder) {
      builder.onResolve({ filter: /modules\/govue-core$/ }, () => ({ path: 'test-native', external: true }));
      builder.onResolve({ filter: /\/address$/ }, () => ({ path: 'test-address', external: true }));
    } }],
  });
  const commands = [], deliveries = [];
  let adapterId, available = true, documentReady = false;
  const task = () => ({ adapterId, taskId: 2, videoId, trackId: 'a.en', language: 'en', automatic: true });
  const context = createContext({ window: { __govueCaptions: {
    request: (requestId, task) => deliveries.push({ requestId, task }), cancel() {},
  } } });
  const module = { exports: {} };
  Function('require', 'module', 'exports', compiled.outputFiles[0].text)(name => {
    if (name === 'test-address') return { youtubeVideoId: () => videoId };
    if (name === 'test-native') return {
      observeCoreUpdates: () => ({ remove() {} }),
      coreRequest: async command => {
        commands.push(command);
        if (command.op === 'extract_page_register') adapterId = command.adapterId;
        if (command.op === 'extract_page') available = false;
        return command.op === 'extract_page_tasks' ? { requests: available ? ['worker:ready'] : [], tasks: available ? [{ requestId: 'worker:ready', task: task() }] : [] } : null;
      },
    };
    return require(name);
  }, module, module.exports);
  const inject = script => { if (documentReady) runInContext(script, context); };
  let page, root;
  function Page() { page = module.exports.useTranscriptPage(true, `https://m.youtube.com/watch?v=${videoId}`, inject); return null; }
  await act(async () => { root = create(createElement(Page)); });
  t.after(() => act(() => root.unmount()));
  assert.equal(deliveries.length, 0, 'the initial injection was lost before the document became ready');
  documentReady = true;
  await act(async () => page.ready());
  assert.deepEqual(JSON.parse(JSON.stringify(deliveries)), [{ requestId: 'worker:ready', task: task() }]);
  const payload = { type: 'captionPage', requestId: 'worker:ready', taskId: 2, videoId, trackId: 'a.en', ok: true, format: 'timedtext', source: '{"events":[]}' };
  const event = data => ({ nativeEvent: { url: 'https://m.youtube.com', data: JSON.stringify(data) } });
  await act(async () => page.receive(event({ ...payload, taskId: 1 })));
  assert.ok(!commands.some(command => command.op === 'extract_page'));
  await act(async () => page.receive(event(payload)));
  assert.deepEqual(commands.find(command => command.op === 'extract_page'), { op: 'extract_page', requestId: 'worker:ready', origin: 'https://m.youtube.com', raw: JSON.stringify(payload) });
});
