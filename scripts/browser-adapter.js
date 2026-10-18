// LiTube's injected UI talks only to this bounded, origin-checked message adapter.
(() => {
  if (window.__govueBrowser) return;
  let preferences = {};
  let documentGeneration = 0;
  let lastTransient = '';
  const post = (action, payload = {}) =>
    window.ReactNativeWebView?.postMessage(
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
  window.addEventListener('tabVisibilityChanged', () => {
    lastTransient = '';
  });
  window.__govueBrowser = {
    setBottomInset(bottom) {
      if (!Number.isFinite(bottom) || bottom < 0 || bottom > 200) return;
      document.documentElement.style.setProperty('--govue-bottom-inset', `${bottom}px`);
      document.documentElement.toggleAttribute('data-govue-bottom-inset', bottom > 0);
    },
    setDocumentGeneration(value) {
      if (!Number.isSafeInteger(value) || value <= documentGeneration) return;
      documentGeneration = value;
      window.dispatchEvent(new CustomEvent('govueDocumentChanged'));
    },
    configure(settings) {
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
