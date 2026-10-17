import { test } from 'node:test';
import assert from 'node:assert/strict';
import { JSDOM } from 'jsdom';
import { gameDocument, scriptJSON } from '../src/ai/gameRuntime.js';

const colors = { dark: true, background: '#0F0F0F', ink: '#FFFFFF', muted: '#AAAAAA', accent: '#FFFFFF', raised: '#272727' };

function runtime(t, extra = {}) {
  const requests = [];
  const definition = {
    html: '<button id="choose">Choose</button><p id="answer"></p><p id="error"></p>', css: '',
    script: `game.onState(state => document.querySelector('#answer').textContent = state.answer);
      document.querySelector('#choose').onclick = () => game.dispatch('choose', { id: 'a' }).then(state => window.result = state.answer).catch(error => document.querySelector('#error').textContent = error.message);`,
    ...extra,
  };
  const host = new JSDOM(gameDocument({ definition, state: { answer: 'Ready' }, cues: [], session: 'fixture', colors }), {
    runScripts: 'dangerously', beforeParse(window) { window.ReactNativeWebView = { postMessage() {} }; },
  });
  host.window.postMessage = message => requests.push(message);
  const frame = new JSDOM(host.window.document.querySelector('iframe').srcdoc, {
    runScripts: 'dangerously', beforeParse(window) {
      Object.defineProperty(window, 'parent', { value: host.window });
      window.ReactNativeWebView = { postMessage() { throw Error('Direct native bridge used'); } };
      window.webkit = { messageHandlers: { ReactNativeWebView: {} } };
    },
  });
  const reply = (value, source = host.window) => frame.window.dispatchEvent(new frame.window.MessageEvent('message', { source, data: { v: 1, ...value } }));
  t.after(() => { frame.window.close(); host.window.close(); });
  return { host, frame, requests, reply };
}

test('generated UI dispatches actions, accepts correlated host state, and displays host errors', async t => {
  const { frame, requests, reply } = runtime(t);
  const document = frame.window.document;
  assert.equal(document.querySelector('#answer').textContent, 'Ready');
  assert.equal(Object.isFrozen(frame.window.game), true);
  assert.equal(frame.window.ReactNativeWebView, undefined);
  assert.equal(frame.window.webkit, undefined);
  document.querySelector('#choose').click();
  const action = requests.find(message => message.method === 'action');
  assert.equal(action.action, 'choose');
  assert.deepEqual(JSON.parse(JSON.stringify(action.input)), { id: 'a' });
  reply({ id: action.id, state: { answer: 'Forged' } }, frame.window);
  assert.equal(document.querySelector('#answer').textContent, 'Ready', 'another frame cannot change state');
  reply({ id: action.id, state: { answer: 'Saved in Rust' } });
  await Promise.resolve();
  assert.equal(document.querySelector('#answer').textContent, 'Saved in Rust');
  assert.equal(frame.window.result, 'Saved in Rust');
  document.querySelector('#choose').click();
  const second = requests.filter(message => message.method === 'action').at(-1);
  reply({ id: second.id, error: 'That choice is unavailable.' });
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(document.querySelector('#error').textContent, 'That choice is unavailable.');
});

test('caption and script delimiters remain data and cannot inject a host script', t => {
  const caption = '</script><script>window.injected = true</script>\u2028你好';
  assert.equal(Function(`return ${scriptJSON(caption)}`)(), caption);
  const { host, frame } = runtime(t, {
    html: '<p id="caption"></p>',
    script: `document.querySelector('#caption').textContent = ${scriptJSON(caption)};`,
  });
  assert.equal(frame.window.document.querySelector('#caption').textContent, caption);
  assert.equal(host.window.injected, undefined);
  assert.equal(frame.window.injected, undefined);
});

test('audio crosses the capability bridge and request pressure is bounded', async t => {
  const { frame, requests, reply } = runtime(t);
  const audio = frame.window.game.playAudio({ cueId: 'cue-42' });
  const request = requests.find(message => message.method === 'audio');
  assert.equal(request.cueId, 'cue-42');
  assert.equal(Object.hasOwn(request, 'url'), false);
  reply({ id: request.id });
  assert.equal(await audio, null);
  const pending = Array.from({ length: 9 }, () => frame.window.game.dispatch('choose', {}).catch(error => error.message));
  assert.equal(await pending.at(-1), 'The game is busy. Please wait.');
  for (const message of requests.filter(message => message.method === 'action')) reply({ id: message.id, error: 'cancelled' });
  assert.deepEqual(await Promise.all(pending.slice(0, 8)), Array(8).fill('cancelled'));
});
