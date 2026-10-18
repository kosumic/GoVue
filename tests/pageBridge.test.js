import { test } from 'node:test';
import assert from 'node:assert/strict';
import { JSDOM } from 'jsdom';
import { YOUTUBE_BRIDGE, cancelPositionScript, pauseScript, playbackPositionScript, seekScript } from '../src/youtube/pageScripts.js';

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

test('bridge is idempotent, stays off other origins, and seeks only the current video', () => {
  const env = setup(async () => env.response({}));
  const other = setup(async () => other.response({}), { url: 'https://youtube.com.evil.test/watch?v=abcdefghijk' });
  try {
    const fetch = env.window.fetch;
    const open = env.window.XMLHttpRequest.prototype.open;
    env.window.eval(YOUTUBE_BRIDGE);
    const bridge = env.window.__govue;
    assert.equal(bridge.extract, undefined);
    assert.equal(env.window.fetch, fetch, 'playback bridge must not capture caption requests');
    assert.equal(env.window.XMLHttpRequest.prototype.open, open);
    env.window.eval(YOUTUBE_BRIDGE);
    assert.equal(env.window.__govue, bridge);
    let plays = 0;
    env.window.document.querySelector('video').play = () => { plays++; return Promise.resolve(); };
    env.window.eval(seekScript(videoId, 42.5));
    assert.equal(env.window.document.querySelector('video').currentTime, 42.5);
    assert.equal(plays, 1);
    env.window.eval(seekScript(otherVideoId, 10));
    assert.equal(env.window.document.querySelector('video').currentTime, 42.5);
    assert.equal(plays, 1);
    other.window.eval(YOUTUBE_BRIDGE);
    assert.equal(other.window.__govue, undefined);
    assert.doesNotThrow(() => new Function(playbackPositionScript('quote"\nrequest', videoId)));
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
    for (const seconds of [-1, NaN, Infinity]) env.window.__govue.seek(videoId, seconds);
    env.window.__govue.seek(otherVideoId, 10);
    env.window.history.pushState({}, '', `/watch?v=${otherVideoId}`);
    env.window.__govue.seek(videoId, 10);
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

test('line pauses require the current request, player, and timeline and can be cancelled', () => {
  const env = setup(async () => env.response({}));
  try {
    const player = env.window.document.createElement('div');
    player.id = 'movie_player';
    let owner = videoId;
    player.getVideoData = () => ({ video_id: owner });
    let pauses = 0;
    player.pauseVideo = () => { pauses++; };
    env.window.document.body.append(player);
    const video = env.window.document.querySelector('video');
    player.append(video);
    video.currentTime = 2;
    video.playbackRate = 1.5;
    Object.defineProperty(video, 'paused', { value: false });
    env.window.eval(YOUTUBE_BRIDGE);
    env.window.eval(playbackPositionScript('current', videoId, true));
    assert.deepEqual(env.messages.at(-1), { type: 'playbackPosition', requestId: 'current', videoId,
      seconds: 2, playing: true, seeking: false, rate: 1.5 });
    env.window.eval(pauseScript('old', videoId, 2));
    env.window.eval(pauseScript('current', otherVideoId, 2));
    env.window.eval(pauseScript('current', videoId, 20));
    player.classList.add('ad-showing');
    env.window.eval(pauseScript('current', videoId, 2));
    player.classList.remove('ad-showing');
    owner = otherVideoId;
    env.window.eval(pauseScript('current', videoId, 2));
    owner = videoId;
    assert.equal(pauses, 0);
    env.window.eval(pauseScript('current', videoId, 2));
    assert.equal(pauses, 1);
    env.window.eval(cancelPositionScript('current'));
    env.window.eval(pauseScript('current', videoId, 2));
    assert.equal(pauses, 1, 'closing the reader cancels queued pauses');
    env.window.eval(playbackPositionScript('next', videoId, true));
    Object.defineProperty(video, 'seeking', { value: true });
    env.window.eval(pauseScript('next', videoId, 2));
    assert.equal(pauses, 1, 'a seek invalidates the sampled timeline');
  } finally { env.close(); }
});
