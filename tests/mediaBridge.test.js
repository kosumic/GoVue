import { test } from 'node:test';
import assert from 'node:assert/strict';
import { JSDOM } from 'jsdom';
import { MEDIA_BRIDGE } from '../src/youtube/mediaBridge.js';

const videoId = 'abcdefghijk';
const otherVideoId = '0123456789a';

function setup() {
  const dom = new JSDOM('<!doctype html><div id="movie_player"><video></video></div><section id="comments"></section>', {
    url: `https://m.youtube.com/watch?v=${videoId}`, runScripts: 'outside-only', pretendToBeVisual: true,
  });
  const { window } = dom;
  const messages = [];
  const player = window.document.getElementById('movie_player');
  const comments = window.document.getElementById('comments');
  const rect = { left: 0, top: 48, width: 400, height: 225 };
  let occluded = false;
  Object.defineProperties(window, { innerWidth: { value: 400 }, innerHeight: { value: 700 } });
  window.ReactNativeWebView = { postMessage: raw => messages.push(JSON.parse(raw)) };
  window.fetch = async () => { throw Error('Layout tracking must not request media'); };
  window.HTMLMediaElement.prototype.pause = () => {};
  player.pauseVideo = () => {};
  player.getPlayerResponse = () => ({ videoDetails: { videoId } });
  player.getBoundingClientRect = () => rect;
  window.document.elementFromPoint = () => occluded ? comments : player;
  window.eval(MEDIA_BRIDGE);
  return { window, player, rect, messages, occlude: value => { occluded = value; },
    layout: () => messages.filter(m => m.type === 'playerLayout').at(-1), close: () => window.close() };
}

test('reports the inline video slot and follows scrolling without resizing the page player', async () => {
  const env = setup();
  try {
    assert.equal(env.window.__govueMedia.resolve, undefined, 'page media extraction is retired');
    env.window.__govueMedia.native(videoId, true);
    assert.deepEqual(env.layout(), { type: 'playerLayout', videoId, layout: { ...env.rect, viewportWidth: 400, viewportHeight: 700, visible: true } });
    assert.equal(env.window.getComputedStyle(env.player).opacity, '0');
    env.rect.top = -60;
    env.window.dispatchEvent(new env.window.Event('scroll'));
    await new Promise(resolve => env.window.requestAnimationFrame(resolve));
    assert.equal(env.layout().layout.top, -60);
    assert.equal(env.layout().layout.visible, true);
    assert.equal(env.player.getBoundingClientRect().height, 225);
    env.rect.top = -300;
    env.window.__govueMedia.configure({});
    assert.equal(env.layout().layout.visible, false);
  } finally { env.close(); }
});

test('yields the native surface to an overlapping comments panel and restores it on dismissal', () => {
  const env = setup();
  try {
    env.window.__govueMedia.native(videoId, true);
    env.occlude(true); env.window.__govueMedia.configure({});
    assert.equal(env.layout().layout.visible, false);
    env.occlude(false); env.window.__govueMedia.configure({});
    assert.equal(env.layout().layout.visible, true);
    env.window.__govueMedia.native(videoId, false);
    assert.equal(env.window.document.documentElement.dataset.govueNativePlayer, 'false');
    assert.notEqual(env.window.getComputedStyle(env.player).opacity, '0');
  } finally { env.close(); }
});

test('releases the old video slot on in-page navigation and ignores stale native ownership', () => {
  const env = setup();
  try {
    env.window.__govueMedia.native(videoId, true);
    env.window.history.pushState({}, '', `/watch?v=${otherVideoId}`);
    env.window.__govueMedia.configure({});
    env.window.__govueMedia.native(videoId, true);
    assert.equal(env.window.document.documentElement.dataset.govueNativePlayer, 'false');
    env.window.__govueMedia.native(otherVideoId, true);
    assert.equal(env.layout().videoId, otherVideoId);
    assert.equal(env.window.document.documentElement.dataset.govueNativePlayer, 'true');
  } finally { env.close(); }
});
