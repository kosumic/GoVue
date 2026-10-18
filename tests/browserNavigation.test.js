import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { JSDOM } from 'jsdom';

test('Chat survives bottom bar replacement and works when Trusted Types blocks HTML insertion', async () => {
  const dom = new JSDOM('<ytm-pivot-bar-renderer><ytm-pivot-bar-item-renderer>Home</ytm-pivot-bar-item-renderer><ytm-pivot-bar-item-renderer>You</ytm-pivot-bar-item-renderer></ytm-pivot-bar-renderer>', {
    url: 'https://m.youtube.com/watch?v=abcdefghijk', runScripts: 'outside-only', pretendToBeVisual: true,
  });
  const { window } = dom;
  const messages = [];
  window.ReactNativeWebView = { postMessage: raw => messages.push(JSON.parse(raw)) };
  const html = Object.getOwnPropertyDescriptor(window.Element.prototype, 'innerHTML');
  Object.defineProperty(window.Element.prototype, 'innerHTML', { get: html.get, set() { throw Error('TrustedHTML required'); }, configurable: true });
  try {
    window.eval(readFileSync('scripts/browser-adapter.js', 'utf8'));
    window.__govueBrowser.setDocumentGeneration(42);
    const nav = readFileSync('src/youtube/litube/nav.js', 'utf8');
    assert.doesNotThrow(() => window.eval(nav));
    window.eval(nav);
    assert.equal(window.document.querySelectorAll('[data-govue-chat]').length, 1);
    const button = window.document.querySelector('[aria-label="Chat"]');
    assert.equal(button.textContent, 'Chat');
    assert.equal(button.closest('ytm-pivot-bar-item-renderer').nextElementSibling.textContent, 'You');
    button.click();
    assert.deepEqual(messages.at(-1), { type: 'litube', action: 'chat', payload: {}, documentUrl: window.location.href, documentGeneration: 42, videoId: 'abcdefghijk' });

    const bar = window.document.createElement('ytm-pivot-bar-renderer');
    bar.appendChild(window.document.createElement('ytm-pivot-bar-item-renderer'));
    window.document.body.replaceChildren(bar);
    window.history.pushState(null, '', '/watch?v=0123456789a');
    await new Promise(resolve => window.requestAnimationFrame(() => window.requestAnimationFrame(resolve)));
    assert.equal(bar.querySelectorAll('[data-govue-chat]').length, 1);
    bar.querySelector('button').click();
    assert.equal(messages.at(-1).action, 'chat');
    assert.equal(messages.at(-1).videoId, '0123456789a');
  } finally { window.close(); }
});
