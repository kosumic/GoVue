import { test } from 'node:test';
import assert from 'node:assert/strict';
import { JSDOM } from 'jsdom';
import { CAPTION_BRIDGE } from '../src/youtube/generated/captionBridge.js';

const videoId = 'abcdefghijk';
const otherVideo = '0123456789a';
const task = { taskId: 7, videoId, trackId: 'a.en', language: 'en', automatic: true, label: 'English (auto-generated)' };
const address = `https://www.youtube.com/api/timedtext?v=${videoId}&lang=en&kind=asr&pot=page-proof`;
const source = JSON.stringify({ events: [{ tStartMs: 1500, dDurationMs: 2000, segs: [{ utf8: 'Hello there' }] }] });
const settle = () => new Promise(resolve => setImmediate(resolve));

function setup(t, fetch = async () => ({ ok: true, clone: () => ({ text: async () => source }) })) {
  const dom = new JSDOM('<div id="movie_player"></div>', { url: `https://m.youtube.com/watch?v=${videoId}`, runScripts: 'outside-only' });
  t.after(() => dom.window.close());
  const { window } = dom;
  const messages = [], selections = [];
  const previous = { languageCode: 'fr', kind: 'asr', vss_id: 'a.fr' };
  window.ReactNativeWebView = { postMessage: raw => messages.push(JSON.parse(raw)) };
  window.fetch = fetch;
  window.ytInitialPlayerResponse = { videoDetails: { videoId }, captions: { playerCaptionsTracklistRenderer: { captionTracks: [
    { vssId: 'a.en', languageCode: 'en', kind: 'asr' }, { vssId: 'a.fr', languageCode: 'fr', kind: 'asr' },
  ] } } };
  const player = window.document.getElementById('movie_player');
  player.getPlayerResponse = () => window.ytInitialPlayerResponse;
  player.loadModule = () => {};
  player.getOption = () => previous;
  player.setOption = (module, option, value) => { selections.push({ module, option, value }); };
  const original = { fetch: window.fetch, open: window.XMLHttpRequest.prototype.open, send: window.XMLHttpRequest.prototype.send };
  window.eval(CAPTION_BRIDGE);
  return { window, messages, selections, previous, original };
}

test('automatic-only captions use the page response and restore playback settings and request hooks', async t => {
  const env = setup(t);
  const { window, original } = env;
  assert.equal(window.fetch, original.fetch, 'loading the bridge does not intercept requests');
  window.__govueCaptions.request('reader:1', task);
  await window.fetch(address);
  await settle();
  assert.deepEqual(env.messages, [{ type: 'captionPage', requestId: 'reader:1', taskId: 7,
    videoId, trackId: 'a.en', ok: true, source, format: 'timedtext' }]);
  assert.equal(window.fetch, original.fetch);
  assert.equal(window.XMLHttpRequest.prototype.open, original.open);
  assert.equal(window.XMLHttpRequest.prototype.send, original.send);
  assert.equal(env.selections.at(-1).value, env.previous);
});

test('wrong videos, caption kinds, translations and languages cannot satisfy the requested track', async t => {
  const env = setup(t);
  env.window.__govueCaptions.request('reader:2', task);
  for (const url of [address.replace(videoId, otherVideo), address.replace('kind=asr', 'kind=manual'),
    `${address}&tlang=fr`, address.replace('lang=en', 'lang=fr'), address.replace('www.youtube.com', 'youtube.com.evil.test')]) {
    await env.window.fetch(url);
  }
  await settle();
  assert.equal(env.messages.length, 0);
  await env.window.fetch(address);
  await settle();
  assert.equal(env.messages.length, 1);
  assert.equal(env.messages[0].ok, true);
});

test('navigation cancels capture, restores hooks, and discards a late caption response', async t => {
  let finish;
  const env = setup(t, () => new Promise(resolve => { finish = resolve; }));
  env.window.__govueCaptions.request('reader:3', task);
  const pending = env.window.fetch(address);
  env.window.history.pushState({}, '', `/watch?v=${otherVideo}`);
  env.window.__govueCaptions.cancel('reader:3');
  finish({ ok: true, clone: () => ({ text: async () => source }) });
  await pending;
  await settle();
  assert.equal(env.messages.length, 1);
  assert.equal(env.messages[0].ok, false);
  assert.equal(env.window.fetch, env.original.fetch);
  assert.notEqual(env.selections.at(-1).value, env.previous, 'the preceding video must not change the new video’s caption settings');
});

test('the transcript button response is captured through XHR only for the requested video parameters', async t => {
  const env = setup(t);
  const { window } = env;
  const panel = JSON.stringify({ actions: [{ transcriptRenderer: { content: { body: { transcriptSegmentListRenderer: {
    initialSegments: [{ transcriptSegmentRenderer: { startMs: '1500', endMs: '3500', snippet: { runs: [{ text: 'Hello there' }] } } }],
  } } } } }] });
  window.XMLHttpRequest.prototype.send = function () {
    Object.defineProperty(this, 'status', { value: 200 });
    Object.defineProperty(this, 'responseText', { value: panel });
    queueMicrotask(() => this.dispatchEvent(new window.Event('load')));
  };
  const send = window.XMLHttpRequest.prototype.send;
  window.__govueCaptions.request('reader:4', task);
  for (const id of [otherVideo, videoId]) {
    const xhr = new window.XMLHttpRequest();
    xhr.open('POST', '/youtubei/v1/get_transcript');
    xhr.send(JSON.stringify({ params: encodeURIComponent(window.btoa('\x0a\x0b' + id)) }));
    await settle();
    if (id === otherVideo) assert.equal(env.messages.length, 0);
  }
  assert.equal(env.messages.length, 1);
  assert.equal(env.messages[0].format, 'panel');
  assert.equal(env.messages[0].source, panel);
  assert.equal(window.XMLHttpRequest.prototype.send, send);
});

test('document readiness can repeat an in-flight task without cancelling its capture', async t => {
  const env = setup(t);
  env.window.__govueCaptions.request('reader:ready', task);
  const selections = env.selections.length;
  const capture = env.window.fetch;
  for (let i = 0; i < 3; i++) env.window.__govueCaptions.request('reader:ready', task);
  assert.equal(env.messages.length, 0);
  assert.equal(env.selections.length, selections);
  assert.equal(env.window.fetch, capture);
  await env.window.fetch(address);
  await settle();
  assert.equal(env.messages.length, 1);
  assert.equal(env.messages[0].ok, true);
});

test('uploaded captions capture the requested named English track without accepting another English track', async t => {
  const env = setup(t);
  const { window } = env;
  window.ytInitialPlayerResponse.captions.playerCaptionsTracklistRenderer.captionTracks.push({
    vssId: '.en.named', languageCode: 'en', baseUrl: address.replace('&kind=asr', '') + '&name=Default',
  });
  window.document.getElementById('movie_player').getOption = (_module, option) => option === 'tracklist'
    ? [{ vss_id: '.en', languageCode: 'en', name: '' }, { vss_id: '.en.named', languageCode: 'en', name: 'Default' }]
    : env.previous;
  window.__govueCaptions.request('reader:uploaded', { ...task, trackId: '.en.named', automatic: false });
  const uploaded = address.replace('&kind=asr', '');
  await window.fetch(uploaded);
  await settle();
  assert.equal(env.messages.length, 0, 'the unnamed English track cannot satisfy the named track');
  await window.fetch(uploaded + '&name=Default');
  await settle();
  assert.equal(env.messages.length, 1);
  assert.equal(env.messages[0].trackId, '.en.named');
  assert.equal(env.messages[0].ok, true);
  assert.equal(env.selections.find(selection => selection.value?.vss_id === '.en.named').value.name, 'Default');
  assert.equal(window.fetch, env.original.fetch);
});
