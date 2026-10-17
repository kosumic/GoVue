(() => {
  'use strict';
  if (window.__nav) return;
  window.__nav = true;

  // YouTube can replace its bottom bar during SPA navigation.
  function ensureChat() {
    const bar = document.querySelector('ytm-pivot-bar-renderer');
    if (!bar || bar.querySelector('[data-lingoplay-chat]')) return;
    const item = document.createElement('ytm-pivot-bar-item-renderer');
    item.setAttribute('data-lingoplay-chat', '');
    item.style.cssText = 'flex:1;min-width:0;display:flex;align-items:center;justify-content:center';
    const button = document.createElement('button');
    button.type = 'button';
    button.setAttribute('aria-label', 'Chat');
    button.style.cssText = 'color:inherit;background:none;border:0;font:inherit;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:3px;min-height:48px;width:100%;padding:4px';
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    for (const [name, value] of Object.entries({ width: '24', height: '24', viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', 'stroke-width': '1.8', 'stroke-linecap': 'round', 'stroke-linejoin': 'round', 'aria-hidden': 'true' })) svg.setAttribute(name, value);
    const path = document.createElementNS('http://www.w3.org/2000/svg', 'path');
    path.setAttribute('d', 'M21 11.5a8.4 8.4 0 0 1-.9 3.8 8.5 8.5 0 0 1-7.6 4.7 8.4 8.4 0 0 1-3.8-.9L3 21l1.9-5.7a8.4 8.4 0 0 1-.9-3.8 8.5 8.5 0 0 1 4.7-7.6 8.4 8.4 0 0 1 3.8-.9h.5a8.5 8.5 0 0 1 8 8z');
    svg.appendChild(path);
    const label = document.createElement('span');
    label.style.cssText = 'font-size:10px;line-height:14px';
    label.textContent = 'Chat';
    button.append(svg, label);
    button.addEventListener('click', (event) => {
      event.preventDefault();
      event.stopImmediatePropagation();
      window.Bridge?.chat?.();
    }, true);
    item.appendChild(button);
    const last = bar.querySelector('ytm-pivot-bar-item-renderer:last-child');
    if (last?.parentNode === bar) bar.insertBefore(item, last);
    else bar.appendChild(item);
  }
  let chatFrame = 0;
  new MutationObserver(() => {
    if (chatFrame) return;
    chatFrame = requestAnimationFrame(() => { chatFrame = 0; ensureChat(); });
  }).observe(document.documentElement, { childList: true, subtree: true });
  ensureChat();

  function isYoutubeHost(hostname) {
    // Strict suffix match mirroring Kotlin UrlPolicy: a substring test
    // would route lookalike hosts cross-tab while native rejects them.
    return hostname === 'youtube.com' ||
      hostname.endsWith('.youtube.com') ||
      hostname === 'youtu.be';
  }

  function kind(url) {
    try {
      const u = new URL(String(url).toLowerCase(), location.href);
      if (!isYoutubeHost(u.hostname)) return 'unknown';
      if (u.hostname === 'youtu.be') {
        const segs = u.pathname.split('/').filter(Boolean);
        return segs.length ? 'watch' : 'unknown';
      }
      const segments = u.pathname.split('/').filter(Boolean);
      if (segments.length === 0) return 'home';
      const first = segments[0];
      if (first === 'shorts') return 'shorts';
      if (first === 'watch') return 'watch';
      if (first === 'live') return 'watch';
      if (first === 'embed') return 'watch';
      if (first === 'channel') return 'channel';
      if (first === 'gaming') return 'gaming';
      if (first === 'select_site') return 'select_site';
      if (first === 'results') return 'searching';
      if (first.startsWith('@')) return '@';
      if (first === 'feed' && segments.length > 1) {
        const second = segments[1];
        if (second === 'subscriptions') return 'subscriptions';
        if (second === 'library') return 'library';
        if (second === 'history') return 'history';
        if (second === 'channels') return 'channels';
        if (second === 'playlists') return 'playlists';
        return segments.join('/');
      }
      return segments.join('/');
    } catch (e) {
      return 'unknown';
    }
  }

  function absolute(href) {
    try {
      return new URL(href, location.href).toString();
    } catch (e) {
      return href;
    }
  }

  function openTab(url) {
    const bridge = window.Bridge;
    if (!bridge || typeof bridge.openTab !== 'function') return;
    // The page may push a transient hash (e.g. #searching) on the source page
    // before cross-tab navigation; strip it so back lands on a clean page.
    cleanSourceHash();
    bridge.openTab(url);
  }

  function cleanSourceHash() {
    if (!location.hash) return;
    const oldURL = location.href;
    const clean = location.href.split('#')[0];
    if (clean === location.href) return;
    try {
      originalReplaceState.call(history, null, '', clean);
      // replaceState does not notify hash-driven overlays that their route ended.
      window.dispatchEvent(new HashChangeEvent('hashchange', { oldURL, newURL: clean }));
    } catch (e) {
      // Best-effort: replaceState can throw on cross-origin documents.
    }
  }

  // One of two independent history.pushState/replaceState wrappers
  // (display_dislikes.js stacks its own); kept separate on purpose —
  // merging would couple script injection order.
  const originalPushState = history.pushState;
  const originalReplaceState = history.replaceState;
  const originalBack = history.back;
  const originalGo = history.go;

  // Each native tab has its own WebView history. The page header calls
  // history.back(), but a newly opened tab has no entry for its source page.
  // The host walks local history or pops the tab, matching system Back.
  // Sheets, dialogs and search pop their own #hash entry in-page.
  function nativeBack() {
    if (location.hash) return false;
    const bridge = window.Bridge;
    if (!bridge || typeof bridge.goBack !== 'function') return false;
    bridge.goBack();
    return true;
  }

  history.back = function () {
    if (window.Lite?.ui && Lite.ui.kind() !== 'none') {
      return originalBack.apply(this, arguments);
    }
    if (!nativeBack()) return originalBack.apply(this, arguments);
  };

  history.go = function (delta) {
    if (Number(delta) === -1 && window.Lite?.ui && Lite.ui.kind() !== 'none') {
      return originalGo.apply(this, arguments);
    }
    if (Number(delta) === -1 && nativeBack()) return;
    return originalGo.apply(this, arguments);
  };

  // Opens [url] in a new tab unless it matches [location.href]'s kind.
  // Returns true when routed cross-tab, false to let navigation proceed in-page.
  function maybeOpen(url) {
    if (typeof url !== 'string') return false;
    const nextUrl = absolute(url);
    const path = new URL(nextUrl).pathname;
    if (/^\/signin(?:\/|$)|^\/accounts\/|^\/(?:check_connection|set_setting)$/i.test(path)) return false;
    const nextKind = kind(nextUrl);
    const bridge = window.Bridge;
    if (nextKind === 'shorts' && bridge && typeof bridge.shouldInheritShorts === 'function' && bridge.shouldInheritShorts()) return false;
    if (nextKind === 'unknown' || nextKind === kind(location.href)) return false;
    openTab(nextUrl);
    return true;
  }

  history.pushState = function (data, title, url) {
    if (url != null && maybeOpen(url)) return;
    return originalPushState.apply(this, arguments);
  };

  history.replaceState = function (data, title, url) {
    if (url != null && maybeOpen(url)) return;
    return originalReplaceState.apply(this, arguments);
  };

  document.addEventListener('click', (event) => {
    try {
      const target = event.target;
      const anchor = target && target.closest && target.closest('a');
      const logo = target && target.closest && target.closest('ytm-home-logo');
      const nav = target && target.closest && target.closest('ytm-pivot-bar-item-renderer');

      let href;
      // Bottom-bar internals are Polymer data; guard the read so a
      // reshaped object never breaks clicks.
      const endpoint = nav && nav.data && nav.data.navigationEndpoint;
      if (endpoint) {
        const metadata = endpoint.commandMetadata;
        href = metadata && metadata.webCommandMetadata && metadata.webCommandMetadata.url;
      } else if (anchor && anchor.href) {
        href = anchor.getAttribute('href');
      } else if (logo) {
        href = '/';
      }
      if (!href) return;

      if (maybeOpen(href)) {
        event.preventDefault();
        event.stopImmediatePropagation();
      }
    } catch (e) {
      // Routing is best-effort; let the page handle the click instead.
    }
  }, true);
})();
