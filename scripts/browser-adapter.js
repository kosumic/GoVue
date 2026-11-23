// LiTube's injected UI talks only to this bounded, origin-checked message adapter.
(() => {
  if (window.__govueBrowser) return;
  let preferences = {};
  let documentGeneration = 0;
  let lastTransient = '';
  let active = false;
  let nativePlayback = false;
  let preparationTimer;
  let lastCandidate = '';
  const post = (action, payload = {}) =>
    window.__govuePageTransport?.postMessage(
      JSON.stringify({
        type: 'litube',
        action,
        payload,
        documentUrl: location.href,
        documentGeneration,
        videoId: new URL(location.href).searchParams.get('v'),
      }),
    );
  const parse = (value) => {
    try {
      return typeof value === 'string' && value.length < 100000
        ? JSON.parse(value)
        : null;
    } catch {
      return null;
    }
  };
  const isFeed = () => ['/', '/results', '/feed/subscriptions'].includes(location.pathname);
  function schedulePreparation(restart = false) {
    if (restart) {
      clearTimeout(preparationTimer);
      preparationTimer = undefined;
    }
    if (preparationTimer !== undefined || !active || !nativePlayback || !documentGeneration || !isFeed()) return;
    preparationTimer = setTimeout(() => {
      preparationTimer = undefined;
      if (document.hidden || !isFeed()) return;
      const selector = 'a[href*="/watch?"]';
      for (const y of [0.35, 0.55, 0.72]) {
        const item = document.elementFromPoint?.(innerWidth / 2, innerHeight * y);
        const link = item?.closest(selector) ?? item?.closest(
          'ytm-video-with-context-renderer, ytm-compact-video-renderer, ytm-rich-item-renderer, ytm-media-item, yt-lockup-view-model'
        )?.querySelector(selector);
        if (!link) continue;
        const target = new URL(link.href);
        if (target.origin !== location.origin || target.pathname !== '/watch'
            || !/^[\w-]{11}$/.test(target.searchParams.get('v') ?? '')) continue;
        if (target.href !== lastCandidate) {
          lastCandidate = target.href;
          post('prefetch', { url: lastCandidate });
        }
        return;
      }
    }, 500);
  }
  window.Bridge = {
    getPreferences: () => JSON.stringify(preferences),
    prepare: (url) => post('prepare', { url }),
    extension: () => post('settings'),
    download: () => post('library'),
    chat: () => post('chat'),
    about: () => post('about'),
    openWith: (url) => post('share', { url }),
    addToQueue: (value) => post('queueAdd', parse(value)),
    showMediaItemMenu: (value) => post('card', parse(value)),
    reportQueueAddFailed: () => post('queueError'),
    setPageHasPlaylist: (enabled) =>
      post('playlist', { enabled: enabled === true }),
    gallery: (value, index) => post('gallery', { urls: parse(value), index }),
    reportAppearance: (appearance) => post('appearance', { appearance }),
    setPageAppearance: (url, dark) =>
      post('appearance', { url, dark: dark === true }),
    openTab: (url) => post('openTab', { url }),
    goBack: () => post('back'),
    setTransientUi: (kind, generation) => {
      const stamp = `${generation}:${kind}`;
      if (stamp === lastTransient) return;
      lastTransient = stamp;
      post('transient', { kind, generation });
    },
    shortsAudioReady: (url) => post('shortsReady', { url }),
    shortsAutoplayBlocked: (url, reason) =>
      post('shortsBlocked', { url, reason }),
    currentDocumentGeneration: () => documentGeneration,
  };
  window.Download = { postMessage: (value) => post('download', parse(value)) };
  window.addEventListener('tabVisibilityChanged', event => {
    lastTransient = '';
    active = event.detail?.active === true;
    if (!active) lastCandidate = '';
    schedulePreparation(true);
  });
  window.addEventListener('scroll', () => schedulePreparation(true), { capture: true, passive: true });
  window.addEventListener('resize', () => schedulePreparation(true), { passive: true });
  document.addEventListener('visibilitychange', () => schedulePreparation(true));
  new MutationObserver(() => schedulePreparation()).observe(document, { subtree: true, childList: true });
  window.__govueBrowser = {
    observeFirstContent(callback) {
      if (!globalThis.PerformanceObserver?.supportedEntryTypes?.includes('paint')) return;
      const observer = new PerformanceObserver((entries) => {
        if (!entries.getEntriesByName('first-contentful-paint').length) return;
        observer.disconnect();
        callback();
      });
      observer.observe({ type: 'paint', buffered: true });
    },
    setBottomInset(bottom) {
      if (!Number.isFinite(bottom) || bottom < 0 || bottom > 200) return;
      document.documentElement.style.setProperty('--govue-bottom-inset', `${bottom}px`);
      document.documentElement.toggleAttribute('data-govue-bottom-inset', bottom > 0);
    },
    setDocumentGeneration(value) {
      if (!Number.isSafeInteger(value) || value <= documentGeneration) return;
      documentGeneration = value;
      lastCandidate = '';
      schedulePreparation(true);
      window.dispatchEvent(new CustomEvent('govueDocumentChanged'));
    },
    configure(settings) {
      nativePlayback = Boolean(settings.adBlock || settings.backgroundPlayback || settings.autoPip);
      schedulePreparation(true);
      preferences = {
        enable_display_dislikes: settings.showDislikes,
        enable_show_likes: settings.showLikes,
        enable_hide_shorts: settings.hideShorts,
      };
      window.dispatchEvent(
        new CustomEvent('preferencesChanged', { detail: { key: '*' } }),
      );
    },
  };
  let lastContext = '';
  let shortHold = null,
    shortTimer = null,
    shortPoint = null,
    suppressClickUntil = 0;
  const finishShortHold = () => {
    clearTimeout(shortTimer);
    shortTimer = null;
    if (shortHold) {
      shortHold.video.playbackRate = shortHold.rate;
      shortHold = null;
      suppressClickUntil = Date.now() + 600;
    }
  };
  document.addEventListener(
    'pointerdown',
    (event) => {
      if (!window.Lite?.isShorts()) return;
      const video = [...document.querySelectorAll('video')].find((v) => {
        const r = v.getBoundingClientRect();
        return (
          r.width > 40 && r.height > 40 && r.top < innerHeight && r.bottom > 0
        );
      });
      if (!video || event.target.closest('button,a,[role="button"]')) return;
      shortPoint = { x: event.clientX, y: event.clientY };
      shortTimer = setTimeout(() => {
        shortHold = { video, rate: video.playbackRate };
        video.playbackRate = 2;
      }, 500);
    },
    true,
  );
  document.addEventListener(
    'pointermove',
    (event) => {
      if (
        shortPoint &&
        Math.hypot(event.clientX - shortPoint.x, event.clientY - shortPoint.y) >
          12
      )
        finishShortHold();
    },
    true,
  );
  document.addEventListener('pointerup', finishShortHold, true);
  document.addEventListener('pointercancel', finishShortHold, true);
  document.addEventListener(
    'click',
    (event) => {
      if (Date.now() < suppressClickUntil) {
        event.preventDefault();
        event.stopImmediatePropagation();
      }
    },
    true,
  );
  setInterval(() => {
    const videoId = window.Lite?.id();
    if (!videoId) return;
    const data =
      window.ytInitialData?.contents?.singleColumnWatchNextResults?.playlist
        ?.playlist;
    const entries = (data?.contents || [])
      .map((v) => v.playlistPanelVideoRenderer)
      .filter(Boolean);
    const index = entries.findIndex((v) => v.videoId === videoId);
    const playlistId = new URL(location.href).searchParams.get('list');
    const item = (v) =>
      v && /^[\w-]{11}$/.test(v.videoId)
        ? {
            videoId: v.videoId,
            title:
              v.title?.simpleText ||
              v.title?.runs?.map((r) => r.text).join('') ||
              'Video',
            author: v.shortBylineText?.runs?.map((r) => r.text).join('') || '',
            playlistId,
          }
        : null;
    const autoplay = window.__watchAutoplayNext?.();
    let next = index >= 0 ? item(entries[index + 1]) : null;
    if (!next && index < 0 && autoplay?.url) {
      try {
        const url = new URL(autoplay.url);
        const id = url.searchParams.get('v');
        if (
          url.protocol === 'https:' &&
          ['m.youtube.com', 'www.youtube.com', 'youtube.com'].includes(
            url.hostname,
          ) &&
          /^[\w-]{11}$/.test(id)
        )
          next = { videoId: id, title: 'Next video', author: '' };
      } catch {}
    }
    const context = {
      videoId,
      next,
      previous: index > 0 ? item(entries[index - 1]) : null,
      autoplay: index >= 0 || autoplay?.enabled === true,
    };
    const stamp = JSON.stringify(context);
    if (stamp === lastContext) return;
    lastContext = stamp;
    post('context', context);
  }, 1000);
})();
