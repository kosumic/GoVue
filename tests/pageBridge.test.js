import { test } from 'node:test';
import assert from 'node:assert/strict';
import { JSDOM } from 'jsdom';
import { YOUTUBE_BRIDGE, extractionScript, playbackPositionScript, seekScript } from '../src/youtube/pageBridge.js';

const videoId = 'abcdefghijk';
const otherVideoId = '0123456789a';
const track = { vssId: '.en', name: { simpleText: 'English' }, languageCode: 'en', baseUrl: `https://www.youtube.com/api/timedtext?v=${videoId}&signature=keep-me` };
const segment = (startMs, endMs, value) => ({ transcriptSegmentRenderer: { startMs, endMs, snippet: { runs: [{ text: value }] } } });

function setup(fetch, { url = `https://www.youtube.com/watch?v=${videoId}`, tracks = [track], context = true } = {}) {
  const dom = new JSDOM('<!doctype html><title>Example - YouTube</title><video></video>', { url, runScripts: 'outside-only' });
  const { window } = dom;
  // Speed up page-readiness deadlines while keeping asynchronous DOM hydration.
  const now = window.Date.now.bind(window.Date);
  const epoch = now();
  window.Date.now = () => epoch + (now() - epoch) * 20;
  window.HTMLElement.prototype.getBoundingClientRect = function () {
    const hidden = this.hidden || this.closest('[hidden]');
    return { width: hidden ? 0 : 100, height: hidden ? 0 : 44 };
  };
  const watch = window.document.createElement('ytd-watch-flexy');
  watch.data = {};
  window.document.body.append(watch);
  const messages = [];
  const requests = [];
  window.ReactNativeWebView = { postMessage: raw => messages.push(JSON.parse(raw)) };
  window.ytInitialPlayerResponse = { videoDetails: { videoId, title: 'Example' }, captions: { playerCaptionsTracklistRenderer: { captionTracks: tracks } } };
  window.ytcfg = { get: key => context && key === 'INNERTUBE_CONTEXT' ? { client: { clientName: 'WEB', clientVersion: 'fixture' } } : null };
  window.fetch = async (url, options) => {
    requests.push({ url, options });
    return fetch(url, options);
  };
  const response = (data, ok = true) => ({ ok, json: async () => data, text: async () => typeof data === 'string' ? data : JSON.stringify(data), clone: () => response(data, ok) });
  return { dom, window, messages, requests, response, close: () => window.close() };
}

test('extracts transcript endpoint segments using this page session and current video', async () => {
  const env = setup(async (url, options) => {
    const body = JSON.parse(options.body);
    assert.equal(options.credentials, 'same-origin');
    assert.ok(options.signal);
    assert.equal(body.context.client.clientName, 'WEB');
    if (url.includes('/next')) {
      assert.equal(body.videoId, videoId);
      return env.response({ contents: [{ getTranscriptEndpoint: { params: 'video-specific-params' } }] });
    }
    assert.equal(body.params, 'video-specific-params');
    return env.response({ content: [segment('2000', '4000', 'Second line'), segment('0', '2000', 'First\nline')] });
  });
  try {
    env.window.eval(YOUTUBE_BRIDGE);
    assert.equal(env.messages[0].videoId, videoId);
    await env.window.__lingoplay.extract('test:1');
    const reply = env.messages.at(-1);
    assert.equal(reply.ok, true);
    assert.equal(reply.transcript.trackId, 'default');
    assert.deepEqual(reply.transcript.segments, [{ start: 0, duration: 2, text: 'First line' }, { start: 2, duration: 2, text: 'Second line' }]);
    assert.equal(env.requests.length, 2);
  } finally { env.close(); }
});

test('captures the Android player caption XHR, restores its caption selection, and keeps tokens in the page', async () => {
  const env = setup(async () => { throw new Error('Direct caption or transcript fetch should not be used'); });
  const previous = {};
  const selections = [];
  try {
    class CaptionRequest extends env.window.EventTarget {
      open(method, url) { this.url = url; }
      send() {
        this.status = 200;
        this.responseType = '';
        this.responseURL = this.url;
        this.responseText = JSON.stringify({ events: [{ tStartMs: 4000, dDurationMs: 2000, segs: [{ utf8: 'Player captions' }] }] });
        this.dispatchEvent(new env.window.Event('load'));
      }
    }
    env.window.XMLHttpRequest = CaptionRequest;
    const player = env.window.document.createElement('div');
    player.id = 'movie_player';
    player.getPlayerResponse = () => env.window.ytInitialPlayerResponse;
    player.loadModule = () => {};
    player.getOption = (module, option) => option === 'tracklist' ? [{ languageCode: 'en', vss_id: '.en' }] : previous;
    player.setOption = (module, option, value) => {
      if (option === 'track') selections.push(value);
      if (option === 'reload') {
        const xhr = new env.window.XMLHttpRequest();
        xhr.open('GET', `https://www.youtube.com/api/timedtext?v=${videoId}&lang=en&fmt=json3&pot=private-player-token`);
        xhr.send();
      }
    };
    env.window.document.body.append(player);
    env.window.eval(YOUTUBE_BRIDGE);
    await env.window.__lingoplay.extract('android-player');
    const reply = env.messages.at(-1);
    assert.equal(reply.ok, true);
    assert.deepEqual(reply.transcript.segments, [{ start: 4, duration: 2, text: 'Player captions' }]);
    assert.equal(reply.transcript.trackId, '.en');
    assert.equal(selections.at(-1), previous);
    assert.equal(env.requests.length, 0);
    assert.ok(!JSON.stringify(env.messages).includes('private-player-token'));
  } finally { env.close(); }
});

test('caption fallback preserves signed URL parameters, filters empty events, and identifies automatic tracks', async () => {
  const env = setup(async url => {
    if (url.includes('/next')) return env.response({});
    const parsed = new URL(url);
    assert.equal(parsed.searchParams.get('signature'), 'keep-me');
    assert.equal(parsed.searchParams.get('fmt'), 'json3');
    return env.response({ events: [{ tStartMs: 1250, dDurationMs: 3000, segs: [{ utf8: 'Hello ' }, { utf8: 'world' }] }, { tStartMs: 0, segs: [{ utf8: '\n' }] }] });
  }, { tracks: [{ ...track, kind: 'asr' }] });
  try {
    env.window.eval(YOUTUBE_BRIDGE);
    await env.window.__lingoplay.extract('fallback');
    const reply = env.messages.at(-1);
    assert.equal(reply.ok, true);
    assert.equal(reply.transcript.tracks[1].automatic, true);
    assert.equal(reply.transcript.language, 'English');
    assert.deepEqual(reply.transcript.segments, [{ start: 1.25, duration: 3, text: 'Hello world' }]);
  } finally { env.close(); }
});

test('reselects a caption track on a paused player when reload alone does not fetch its text', async () => {
  const env = setup(async () => { throw Error('The player must issue its own caption request'); });
  const preferred = { languageCode: 'en', vss_id: '.en' };
  let selected = preferred;
  let requests = 0;
  try {
    class CaptionRequest extends env.window.EventTarget {
      open(method, url) { this.responseURL = url; }
      send() {
        this.status = 200;
        this.responseType = '';
        this.responseText = JSON.stringify({ events: [
          { tStartMs: 240, dDurationMs: 4000, segs: [{ utf8: 'First caption' }] },
          { tStartMs: 8420000, dDurationMs: 1500, segs: [{ utf8: 'Final caption' }] },
        ] });
        this.dispatchEvent(new env.window.Event('load'));
      }
    }
    env.window.XMLHttpRequest = CaptionRequest;
    const player = env.window.document.createElement('div');
    player.id = 'movie_player';
    player.getPlayerResponse = () => env.window.ytInitialPlayerResponse;
    player.loadModule = () => {};
    player.playVideo = () => { throw Error('Extraction must keep the web video paused'); };
    player.getOption = (_, option) => option === 'tracklist' ? [preferred] : selected || {};
    player.setOption = (_, option, value) => {
      // Match the live player: reload and reselecting its current track do
      // nothing. Only clearing that selection allows a fresh signed request.
      if (option !== 'track' || (value.vss_id || value.vssId) === selected?.vss_id) return;
      selected = value.vss_id ? value : null;
      if (!selected) return;
      requests++;
      const xhr = new env.window.XMLHttpRequest();
      xhr.open('GET', `https://www.youtube.com/api/timedtext?v=${videoId}&lang=en&fmt=json3&pot=private-fixture-token`);
      xhr.send();
    };
    env.window.document.body.append(player);
    env.window.eval(YOUTUBE_BRIDGE);
    await env.window.__lingoplay.extract('paused-selected-track');
    const reply = env.messages.at(-1);
    assert.equal(reply.ok, true);
    assert.equal(reply.transcript.language, 'English');
    assert.equal(reply.transcript.segments.length, 2);
    assert.equal(reply.transcript.segments.at(-1).start, 8420);
    assert.equal(requests, 1);
    assert.equal(selected, preferred);
    assert.equal(env.requests.length, 0);
    assert.ok(!JSON.stringify(env.messages).includes('private-fixture-token'));
  } finally { env.close(); }
});

test('default caption fallback honors the selected track, YouTube default, and regional page language', async () => {
  const choices = [['ar', 'Arabic'], ['zh', 'Chinese'], ['en', 'English']].map(([language, label]) => ({
    ...track, vssId: '.' + language, languageCode: language, name: { simpleText: label }, baseUrl: track.baseUrl + '&lang=' + language,
  }));
  for (const scenario of [
    { current: null, defaultIndex: 2, pageLanguage: 'ar', expected: 'en' },
    { current: '.ar', defaultIndex: 2, pageLanguage: 'en', expected: 'ar' },
    { current: null, defaultIndex: undefined, pageLanguage: 'en-US', expected: 'en' },
  ]) {
    const env = setup(async url => {
      if (url.includes('/next')) return env.response({});
      assert.equal(new URL(url).searchParams.get('lang'), scenario.expected);
      return env.response({ events: [{ tStartMs: 0, dDurationMs: 2000, segs: [{ utf8: 'Preferred captions' }] }] });
    }, { tracks: choices });
    try {
      env.window.document.documentElement.lang = scenario.pageLanguage;
      env.window.ytInitialPlayerResponse.captions.playerCaptionsTracklistRenderer.audioTracks = [{ defaultCaptionTrackIndex: scenario.defaultIndex }];
      const player = env.window.document.createElement('div');
      player.id = 'movie_player';
      player.getPlayerResponse = () => env.window.ytInitialPlayerResponse;
      player.getOption = () => ({ vss_id: scenario.current });
      env.window.document.body.append(player);
      env.window.eval(YOUTUBE_BRIDGE);
      await env.window.__lingoplay.extract('preferred-fallback');
      const reply = env.messages.at(-1);
      assert.equal(reply.ok, true);
      assert.equal(reply.transcript.trackId, '.' + scenario.expected);
    } finally { env.close(); }
  }
});

test('supports XML captions and explicit language selection without a transcript API call', async () => {
  const env = setup(async url => {
    assert.ok(url.includes('/api/timedtext'));
    return env.response('<transcript><text start="1.5" dur="2">Tom &amp;amp; Jerry</text></transcript>');
  });
  try {
    env.window.eval(YOUTUBE_BRIDGE);
    await env.window.__lingoplay.extract('xml', '.en');
    assert.equal(env.messages.at(-1).transcript.segments[0].text, 'Tom & Jerry');
    assert.equal(env.requests.length, 1);
  } finally { env.close(); }
});

test('no captions and blocked caption requests produce actionable errors', async () => {
  for (const tracks of [[], [track]]) {
    const env = setup(async url => url.includes('/next') ? env.response({}) : env.response('', false), { tracks });
    try {
      env.window.eval(YOUTUBE_BRIDGE);
      await env.window.__lingoplay.extract('failure');
      assert.equal(env.messages.at(-1).ok, false);
      assert.ok(env.messages.at(-1).error.length > 10);
    } finally { env.close(); }
  }
});

test('SPA navigation discards an old response and rebinds requests to the new video', async () => {
  let resolveFirst;
  let startedResolve;
  const started = new Promise(resolve => { startedResolve = resolve; });
  let counter = 0;
  const env = setup(async () => {
    if (++counter === 1) return new Promise(resolve => { resolveFirst = resolve; startedResolve(); });
    return env.response({});
  });
  try {
    env.window.eval(YOUTUBE_BRIDGE);
    const task = env.window.__lingoplay.extract('old-video');
    await started;
    env.window.history.pushState({}, '', `/watch?v=${otherVideoId}`);
    env.window.document.dispatchEvent(new env.window.Event('yt-navigate-finish'));
    resolveFirst(env.response({}));
    await task;
    assert.equal(env.messages.at(-1).type, 'page');
    assert.equal(env.messages.at(-1).videoId, otherVideoId);
    assert.equal(env.messages.filter(message => message.type === 'transcript').length, 0);
    await env.window.__lingoplay.extract('new-video');
    assert.equal(JSON.parse(env.requests[1].options.body).videoId, otherVideoId);
    assert.equal(env.messages.at(-1).ok, false); // Old video's caption tracks must not be reused.
  } finally { env.close(); }
});

test('uses Show transcript and captures YouTube session requests when direct requests would fail', async () => {
  const env = setup(async url => {
    assert.ok(url.includes('/get_transcript'));
    return env.response({ actions: [segment('1000', '3000', 'Native session captions')] });
  });
  try {
    const section = env.window.document.createElement('ytd-video-description-transcript-section-renderer');
    section.data = { button: {} };
    const button = env.window.document.createElement('button');
    button.onclick = () => { void env.window.fetch('/youtubei/v1/get_transcript', { method: 'POST' }); };
    section.append(button);
    env.window.document.body.append(section);
    env.window.eval(YOUTUBE_BRIDGE);
    await env.window.__lingoplay.extract('native-session');
    assert.equal(env.messages.at(-1).ok, true);
    assert.equal(env.messages.at(-1).transcript.segments[0].text, 'Native session captions');
    assert.equal(env.requests.length, 1);
  } finally { env.close(); }
});

test('waits for YouTube controls to hydrate after the document loads', async () => {
  const env = setup(async url => {
    assert.ok(url.includes('/get_transcript'), 'Should use the page control instead of the direct fallback');
    return env.response({ actions: [segment('0', '2000', 'Hydrated transcript')] });
  });
  try {
    const watch = env.window.document.querySelector('ytd-watch-flexy');
    watch.data = { getTranscriptEndpoint: { params: 'waiting-for-controls' } };
    const section = env.window.document.createElement('ytd-video-description-transcript-section-renderer');
    const button = env.window.document.createElement('button');
    let clicks = 0;
    button.onclick = () => {
      clicks++;
      assert.ok(section.data);
      void env.window.fetch('/youtubei/v1/get_transcript', { method: 'POST' });
    };
    section.append(button);
    env.window.setTimeout(() => env.window.document.body.append(section), 20);
    env.window.setTimeout(() => { section.data = { primaryButton: {} }; }, 160);
    env.window.eval(YOUTUBE_BRIDGE);
    await env.window.__lingoplay.extract('delayed-hydration');
    assert.equal(clicks, 1);
    assert.equal(env.messages.at(-1).transcript.segments[0].text, 'Hydrated transcript');
    assert.equal(env.requests.length, 1);
  } finally { env.close(); }
});

test('language continuations reuse YouTube native request context inside the page', async () => {
  const menu = { sortFilterSubMenuRenderer: { subMenuItems: [
    { title: 'English', selected: true },
    { title: 'Spanish', continuation: { reloadContinuationData: { continuation: 'spanish-params' } } },
  ] } };
  const env = setup(async (url, options) => {
    const body = JSON.parse(options.body);
    assert.equal(body.serviceIntegrityDimensions.poToken, 'fixture-integrity');
    if (body.params === 'spanish-params') return env.response({ content: [segment('0', '2000', 'Hola mundo')], menu });
    return env.response({ content: [segment('0', '2000', 'Hello world')], menu });
  }, { tracks: [track, { ...track, vssId: '.es', name: { simpleText: 'Spanish' }, languageCode: 'es' }] });
  try {
    const section = env.window.document.createElement('ytd-video-description-transcript-section-renderer');
    section.data = { button: {} };
    const button = env.window.document.createElement('button');
    button.onclick = () => { void env.window.fetch('/youtubei/v1/get_transcript', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ params: 'default-params', serviceIntegrityDimensions: { poToken: 'fixture-integrity' } }),
    }); };
    section.append(button);
    env.window.document.body.append(section);
    env.window.eval(YOUTUBE_BRIDGE);
    await env.window.__lingoplay.extract('default-language');
    await env.window.__lingoplay.extract('spanish-language', '.es');
    const reply = env.messages.at(-1);
    assert.equal(reply.ok, true);
    assert.equal(reply.transcript.language, 'Spanish');
    assert.equal(reply.transcript.segments[0].text, 'Hola mundo');
    assert.equal(env.requests.length, 2);
    assert.ok(!JSON.stringify(env.messages).includes('fixture-integrity'));
  } finally { env.close(); }
});

test('reads an already-open transcript and never reuses its DOM data after SPA navigation', async () => {
  const env = setup(async () => env.response({}));
  try {
    const renderer = env.window.document.createElement('ytd-transcript-renderer');
    renderer.data = { content: [segment('0', '2000', 'Current video captions')] };
    env.window.document.body.append(renderer);
    env.window.eval(YOUTUBE_BRIDGE);
    await env.window.__lingoplay.extract('existing');
    assert.equal(env.messages.at(-1).ok, true);
    assert.equal(env.requests.length, 0);
    env.window.history.pushState({}, '', `/watch?v=${otherVideoId}`);
    env.window.ytInitialPlayerResponse.videoDetails.videoId = otherVideoId;
    env.window.document.dispatchEvent(new env.window.Event('yt-navigate-finish'));
    await env.window.__lingoplay.extract('after-navigation');
    assert.equal(env.messages.at(-1).ok, false);
  } finally { env.close(); }
});

test('language menu selection waits for new caption rows before returning the selected language', async () => {
  const env = setup(async () => env.response({}), { tracks: [track, { ...track, vssId: '.es', name: { simpleText: 'Spanish' }, languageCode: 'es' }] });
  const makeData = (value, spanish) => ({
    content: [segment('0', '2000', value)],
    footer: { sortFilterSubMenuRenderer: { subMenuItems: [{ title: 'English', selected: !spanish }, { title: 'Spanish', selected: spanish }] } },
  });
  try {
    const renderer = env.window.document.createElement('ytd-transcript-renderer');
    renderer.data = makeData('Hello', false);
    const footer = env.window.document.createElement('ytd-transcript-footer-renderer');
    const choice = env.window.document.createElement('tp-yt-paper-item');
    choice.textContent = 'Spanish';
    choice.onclick = () => {
      renderer.data.footer.sortFilterSubMenuRenderer.subMenuItems[1].selected = true;
      env.window.setTimeout(() => { renderer.data = makeData('Hola', true); }, 20);
    };
    footer.append(choice);
    renderer.append(footer);
    env.window.document.body.append(renderer);
    env.window.eval(YOUTUBE_BRIDGE);
    await env.window.__lingoplay.extract('spanish-menu', '.es');
    assert.equal(env.messages.at(-1).transcript.segments[0].text, 'Hola');
    assert.equal(env.messages.at(-1).transcript.language, 'Spanish');
    assert.equal(env.requests.length, 0);
  } finally { env.close(); }
});

test('bridge is idempotent, stays off other origins, and seeks only the current video', () => {
  const env = setup(async () => env.response({}));
  const other = setup(async () => other.response({}), { url: 'https://youtube.com.evil.test/watch?v=abcdefghijk' });
  try {
    env.window.eval(YOUTUBE_BRIDGE);
    const bridge = env.window.__lingoplay;
    env.window.eval(YOUTUBE_BRIDGE);
    assert.equal(env.window.__lingoplay, bridge);
    let plays = 0;
    env.window.document.querySelector('video').play = () => { plays++; return Promise.resolve(); };
    env.window.eval(seekScript(videoId, 42.5));
    assert.equal(env.window.document.querySelector('video').currentTime, 42.5);
    assert.equal(plays, 1);
    env.window.eval(seekScript(otherVideoId, 10));
    assert.equal(env.window.document.querySelector('video').currentTime, 42.5);
    assert.equal(plays, 1);
    other.window.eval(YOUTUBE_BRIDGE);
    assert.equal(other.window.__lingoplay, undefined);
    assert.doesNotThrow(() => new Function(extractionScript('quote"\nrequest', '.en')));
  } finally { env.close(); other.close(); }
});

test('caption taps seek and resume through the current YouTube player, rejecting stale or invalid targets', () => {
  const env = setup(async () => env.response({}));
  try {
    const player = env.window.document.createElement('div');
    player.id = 'movie_player';
    const actions = [];
    player.seekTo = (seconds, allowSeekAhead) => actions.push(['seek', seconds, allowSeekAhead]);
    player.playVideo = () => actions.push(['play']);
    env.window.document.body.append(player);
    env.window.eval(YOUTUBE_BRIDGE);
    env.window.eval(seekScript(videoId, 24.8));
    assert.deepEqual(actions, [['seek', 24.8, true], ['play']]);
    assert.equal(env.window.document.querySelector('video').currentTime, 0);
    for (const seconds of [-1, NaN, Infinity]) env.window.__lingoplay.seek(videoId, seconds);
    env.window.__lingoplay.seek(otherVideoId, 10);
    env.window.history.pushState({}, '', `/watch?v=${otherVideoId}`);
    env.window.__lingoplay.seek(videoId, 10);
    assert.equal(actions.length, 2);
  } finally { env.close(); }
});

test('playback positions follow the web video across seeks and retain request correlation', () => {
  const env = setup(async () => env.response({}));
  try {
    const video = env.window.document.querySelector('video');
    env.window.eval(YOUTUBE_BRIDGE);
    video.currentTime = 42.5;
    env.window.eval(playbackPositionScript('position:1', videoId));
    assert.deepEqual(env.messages.at(-1), { type: 'playbackPosition', requestId: 'position:1', videoId, seconds: 42.5 });
    video.currentTime = 3;
    env.window.eval(playbackPositionScript('position:2', videoId));
    assert.deepEqual(env.messages.at(-1), { type: 'playbackPosition', requestId: 'position:2', videoId, seconds: 3 });
    const count = env.messages.length;
    env.window.eval(playbackPositionScript('stale', otherVideoId));
    assert.equal(env.messages.length, count);
    const quoted = 'quote"\nrequest';
    env.window.eval(playbackPositionScript(quoted, videoId));
    assert.equal(env.messages.at(-1).requestId, quoted);
  } finally { env.close(); }
});

test('playback positions reject a stale SPA player and hide ad or unavailable timelines', () => {
  const env = setup(async () => env.response({}));
  try {
    const player = env.window.document.createElement('div');
    player.id = 'movie_player';
    player.getVideoData = () => ({ video_id: videoId });
    env.window.document.body.append(player);
    const video = env.window.document.querySelector('video');
    player.append(video);
    video.currentTime = 12;
    env.window.eval(YOUTUBE_BRIDGE);
    player.classList.add('ad-showing');
    env.window.eval(playbackPositionScript('position:1', videoId));
    assert.equal(env.messages.at(-1).seconds, null);
    player.classList.remove('ad-showing');
    env.window.eval(playbackPositionScript('position:1', videoId));
    assert.equal(env.messages.at(-1).seconds, 12);
    video.remove();
    player.getCurrentTime = () => NaN;
    env.window.eval(playbackPositionScript('position:1', videoId));
    assert.equal(env.messages.at(-1).seconds, null);
    player.getCurrentTime = () => 31.25;
    env.window.eval(playbackPositionScript('position:1', videoId));
    assert.equal(env.messages.at(-1).seconds, 31.25);
    env.window.history.pushState({}, '', `/watch?v=${otherVideoId}`);
    env.window.ytInitialPlayerResponse.videoDetails.videoId = otherVideoId;
    const count = env.messages.length;
    env.window.eval(playbackPositionScript('position:2', otherVideoId));
    assert.equal(env.messages.length, count, 'The URL changed but the old player still owns the video');
    player.getVideoData = () => ({ video_id: otherVideoId });
    env.window.eval(playbackPositionScript('position:2', otherVideoId));
    assert.equal(env.messages.at(-1).videoId, otherVideoId);
    assert.equal(env.messages.at(-1).seconds, 31.25);
  } finally { env.close(); }
});
