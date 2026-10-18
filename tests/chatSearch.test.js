import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import React from 'react';
import { act, create } from 'react-test-renderer';
import { build } from 'esbuild';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const require = createRequire(import.meta.url);
const bundle = await build({ entryPoints: ['src/ai/useChatFind.ts'], bundle: true, write: false, platform: 'node', format: 'cjs', packages: 'external',
  plugins: [{ name: 'core', setup(builder) { builder.onResolve({ filter: /modules\/lingoplay-core$/ }, () => ({ path: 'mock-core', external: true })); } }],
});
const hit = (messageId, offset = 0) => ({ messageId, offset, role: 'assistant', before: '', matched: 'word', after: '', leading: false, trailing: false });
const result = (query, matches, selected = 0) => ({ query, matches, selected: matches.length ? selected : null, truncated: false });

async function harness(t, request) {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const module = { exports: {} }, calls = [];
  Function('require', 'module', 'exports', bundle.outputFiles[0].text)(name => name === 'mock-core' ? { aiRequest(command) { calls.push(command); return request(command, calls.length); } } : require(name), module, module.exports);
  let search, root;
  let props = { id: 'first', messages: [{ id: 'one', content: '' }], enabled: true, initialQuery: '' };
  function Probe() { search = module.exports.useChatFind(props.id, props.messages, props.enabled, props.initialQuery); return null; }
  await act(() => { root = create(React.createElement(Probe)); });
  t.after(() => act(() => root.unmount()));
  return { calls, search: () => search,
    query: text => act(() => search.setQuery(text)),
    tick: ms => act(async () => t.mock.timers.tick(ms)),
    render: values => act(() => { props = { ...props, ...values }; root.update(React.createElement(Probe)); }),
  };
}

test('a late search cannot replace the current query or a different conversation', async t => {
  const pending = [];
  const env = await harness(t, command => new Promise(resolve => pending.push({ command, resolve })));
  await env.query('old'); await env.tick(150);
  await env.query('new'); await env.tick(150);
  await act(async () => pending[0].resolve(result('old', [hit('stale')])));
  assert.equal(env.search().hit, undefined);
  await act(async () => pending[1].resolve(result('new', [hit('current')])));
  assert.equal(env.search().hit.messageId, 'current');
  await env.query('later'); await env.tick(150);
  await env.render({ id: 'second' });
  await act(async () => pending[2].resolve(result('later', [hit('wrong-chat')])));
  assert.equal(env.search().query, '');
  assert.equal(env.search().hit, undefined);
});

test('streaming cannot starve search or overwrite explicit match navigation', async t => {
  let finishNavigation;
  const matches = [hit('first'), hit('second', 10)];
  const env = await harness(t, command => command.direction ? new Promise(resolve => { finishNavigation = resolve; }) : Promise.resolve(result('word', matches, command.previous?.messageId === 'second' ? 1 : 0)));
  await env.query('word');
  for (let i = 0; i < 3; i++) { await env.tick(40); await env.render({ messages: [{ id: 'stream', content: 'word'.repeat(i) }] }); }
  await env.tick(30);
  assert.equal(env.calls.length, 1, 'constant content updates still reach native search');
  assert.equal(env.search().hit.messageId, 'first');
  await env.render({ messages: [{ id: 'stream', content: 'word growing' }] });
  await act(() => env.search().navigate('next'));
  assert.deepEqual(env.calls.at(-1).previous, { query: 'word', messageId: 'first', offset: 0 });
  await env.render({ messages: [{ id: 'stream', content: 'word growing again' }] });
  await env.tick(150);
  assert.equal(env.calls.length, 2, 'live refresh waits for explicit navigation');
  await act(async () => finishNavigation(result('word', matches, 1)));
  assert.equal(env.search().hit.messageId, 'second');
  await env.tick(150);
  assert.equal(env.search().hit.messageId, 'second', 'refresh reconciles from the newly selected occurrence');
  await env.render({ enabled: false });
  const count = env.calls.length;
  await env.tick(1000);
  assert.equal(env.calls.length, count);
  assert.equal(env.search().hit, undefined);
});
