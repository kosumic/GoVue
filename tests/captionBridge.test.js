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

test('ASR hidden by an uploaded track reuses the page video proof with its own signed relative URL', async t => {
  const requests = [];
  const env = setup(t, async (url, options) => {
    requests.push({ url: String(url), options });
    return { ok: true, text: async () => source };
  });
  const { window } = env;
  const raw = `/api/timedtext?v=${videoId}&lang=en&kind=asr&signature=signed%2Bbytes%2f&fmt=srv3&sparams=v%2Clang%2Ckind`;
  window.ytInitialPlayerResponse.captions.playerCaptionsTracklistRenderer.captionTracks[0].baseUrl = raw;
  const player = window.document.getElementById('movie_player');
  player.getOption = (_module, option) => option === 'tracklist'
    ? [{ languageCode: 'en', kind: '', name: 'Default', vss_id: '.en.named' }] : env.previous;
  const proof = `https://www.youtube.com/api/timedtext?v=${videoId}&lang=en&name=Default&pot=video-proof%2B%2F%3D&potc=1&c=WEB&cver=page-version`;
  window.performance.getEntriesByType = () => [
    { name: proof.replace(videoId, otherVideo) },
    { name: proof.replace('www.youtube.com', 'youtube.com.evil.test') },
    { name: proof },
  ];
  window.__govueCaptions.request('reader:hidden-asr', task);
  await settle();
  assert.equal(requests.length, 1);
  const url = new URL(requests[0].url);
  assert.equal(url.searchParams.get('kind'), 'asr');
  assert.equal(url.searchParams.get('lang'), 'en');
  assert.equal(url.searchParams.get('pot'), 'video-proof+/=');
  assert.equal(url.searchParams.get('c'), 'WEB');
  assert.equal(url.searchParams.get('fmt'), 'json3');
  assert.equal(url.searchParams.has('name'), false, 'an uploaded track name cannot leak into ASR');
  assert.ok(requests[0].url.includes('signature=signed%2Bbytes%2f'), 'signed bytes retain their encoding');
  assert.equal(requests[0].options.credentials, 'include');
  assert.equal(requests[0].options.signal.aborted, true, 'completion releases the page request');
  assert.equal(env.selections.length, 0, 'existing proof avoids changing player captions');
  assert.equal(env.messages[0].ok, true);
  assert.equal(env.messages[0].source, source);
  assert.equal(window.fetch, env.original.fetch);
});

test('a completed ASR request is replayed immediately when the reader opens later', async t => {
  const requests = [];
  const env = setup(t, async (url, options) => {
    requests.push({ url, options });
    return { ok: true, text: async () => source };
  });
  env.window.performance.getEntriesByType = () => [{ name: address }];
  env.window.__govueCaptions.request('reader:already-loaded', task);
  await settle();
  assert.equal(requests.length, 1);
  assert.equal(requests[0].url, address);
  assert.equal(env.messages[0].ok, true);
  assert.equal(env.selections.length, 0);
});

test('an uploaded-caption response supplies proof for the requested ASR track without returning its text', async t => {
  const requests = [];
  const uploaded = address.replace('&kind=asr', '') + '&name=Default';
  const env = setup(t, async url => {
    requests.push(String(url));
    const body = new URL(url).searchParams.get('kind') === 'asr' ? source : 'Uploaded captions';
    return { ok: true, text: async () => body, clone: () => ({ text: async () => body }) };
  });
  env.window.ytInitialPlayerResponse.captions.playerCaptionsTracklistRenderer.captionTracks[0].baseUrl =
    `/api/timedtext?v=${videoId}&lang=en&kind=asr&signature=asr-source`;
  env.window.__govueCaptions.request('reader:new-proof', task);
  await env.window.fetch(uploaded);
  await settle();
  assert.equal(requests.length, 2);
  assert.equal(new URL(requests[1]).searchParams.get('kind'), 'asr');
  assert.equal(env.messages.length, 1);
  assert.equal(env.messages[0].source, source);
});

test('simultaneous player and reader requests both receive ASR without cancelling each other', async t => {
  const env = setup(t);
  env.window.__govueCaptions.request('reader:parallel', task);
  env.window.__govueCaptions.request('player:parallel', { ...task, taskId: 8 });
  assert.equal(env.messages.length, 0);
  await env.window.fetch(address);
  await settle();
  assert.deepEqual(env.messages.map(reply => ({ requestId: reply.requestId, taskId: reply.taskId, ok: reply.ok })), [
    { requestId: 'reader:parallel', taskId: 7, ok: true },
    { requestId: 'player:parallel', taskId: 8, ok: true },
  ]);
  assert.equal(env.window.fetch, env.original.fetch);
  assert.equal(env.window.XMLHttpRequest.prototype.open, env.original.open);
  assert.equal(env.window.XMLHttpRequest.prototype.send, env.original.send);
  assert.equal(env.selections.at(-1).value, env.previous);
});

test('cancelling one caption consumer preserves the other capture and its cleanup', async t => {
  const env = setup(t);
  env.window.__govueCaptions.request('reader:cancel', task);
  env.window.__govueCaptions.request('player:continue', { ...task, taskId: 8 });
  env.window.__govueCaptions.cancel('reader:cancel');
  assert.equal(env.messages.length, 1);
  assert.equal(env.messages[0].ok, false);
  assert.notEqual(env.window.fetch, env.original.fetch);
  await env.window.fetch(address);
  await settle();
  assert.equal(env.messages[1].requestId, 'player:continue');
  assert.equal(env.messages[1].ok, true);
  assert.equal(env.window.fetch, env.original.fetch);
});

test('parallel consumers share one replay and cancelling one does not abort the other', async t => {
  let resolve, signal;
  const requests = [];
  const env = setup(t, (url, options) => {
    requests.push(url); signal = options.signal;
    return new Promise(done => { resolve = done; });
  });
  env.window.performance.getEntriesByType = () => [{ name: address }];
  env.window.__govueCaptions.request('reader:shared', task);
  env.window.__govueCaptions.request('player:shared', { ...task, taskId: 8 });
  await settle();
  assert.equal(requests.length, 1);
  env.window.__govueCaptions.cancel('reader:shared');
  assert.equal(signal.aborted, false);
  resolve({ ok: true, text: async () => source });
  await settle();
  assert.equal(env.messages[1].requestId, 'player:shared');
  assert.equal(env.messages[1].ok, true);
  assert.equal(signal.aborted, true);
  assert.equal(env.window.fetch, env.original.fetch);
});

test('a second language uses the ASR source with the requested translation and the active video proof', async t => {
  const requests = [];
  const french = JSON.stringify({ events: [{ tStartMs: 1500, dDurationMs: 2000, segs: [{ utf8: 'Bonjour' }] }] });
  const env = setup(t, async url => {
    requests.push(String(url));
    return { ok: true, text: async () => new URL(url).searchParams.get('tlang') === 'fr' ? french : source };
  });
  env.window.ytInitialPlayerResponse.captions.playerCaptionsTracklistRenderer.captionTracks[0].baseUrl =
    `/api/timedtext?v=${videoId}&lang=en&kind=asr&signature=source%2Bsignature`;
  env.window.performance.getEntriesByType = () => [{ name: address }];
  env.window.__govueCaptions.request('reader:primary', task);
  env.window.__govueCaptions.request('reader:translation', {
    ...task, taskId: 8, trackId: 'translate:fr:a.en', sourceTrackId: 'a.en', language: 'fr',
  });
  await settle();
  assert.equal(requests.length, 2);
  const translated = new URL(requests.find(url => new URL(url).searchParams.has('tlang')));
  assert.equal(translated.searchParams.get('lang'), 'en');
  assert.equal(translated.searchParams.get('tlang'), 'fr');
  assert.equal(translated.searchParams.get('kind'), 'asr');
  assert.equal(translated.searchParams.get('pot'), 'page-proof');
  assert.ok(translated.href.includes('signature=source%2Bsignature'));
  assert.deepEqual(env.messages.map(reply => ({ id: reply.requestId, track: reply.trackId, source: reply.source })), [
    { id: 'reader:primary', track: 'a.en', source },
    { id: 'reader:translation', track: 'translate:fr:a.en', source: french },
  ]);
});

test('original captions and other translations cannot complete a pending second-language request', async t => {
  let resolveTranslation;
  const french = JSON.stringify({ events: [{ tStartMs: 1500, segs: [{ utf8: 'Bonjour' }] }] });
  const env = setup(t, async url => {
    const target = new URL(url).searchParams.get('tlang');
    if (target === 'fr') return new Promise(resolve => { resolveTranslation = resolve; });
    return { ok: true, clone: () => ({ text: async () => source }) };
  });
  env.window.ytInitialPlayerResponse.captions.playerCaptionsTracklistRenderer.captionTracks[0].baseUrl =
    `/api/timedtext?v=${videoId}&lang=en&kind=asr&signature=source`;
  env.window.__govueCaptions.request('reader:only-french', {
    ...task, trackId: 'translate:fr:a.en', sourceTrackId: 'a.en', language: 'fr',
  });
  await env.window.fetch(address);
  await settle();
  await env.window.fetch(address + '&tlang=es');
  await settle();
  assert.equal(env.messages.length, 0);
  resolveTranslation({ ok: true, text: async () => french });
  await settle();
  assert.equal(env.messages.length, 1);
  assert.equal(env.messages[0].trackId, 'translate:fr:a.en');
  assert.equal(env.messages[0].source, french);
});
