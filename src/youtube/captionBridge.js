// On-demand transport in the existing YouTube page. Rust parses all captions.
(() => {
  if (window.__govueCaptions || location.protocol !== 'https:' ||
      !['youtube.com', 'www.youtube.com', 'm.youtube.com', 'music.youtube.com'].includes(location.hostname)) return;
  const MAX_BYTES = 8_000_000;
  const TIMEOUT = 11000;
  let pending = null;
  const currentVideo = () => {
    const url = new URL(location.href);
    return url.pathname === '/watch' ? url.searchParams.get('v') : url.pathname.match(/^\/(?:shorts|live|embed)\/([\w-]{11})(?:\/|$)/)?.[1];
  };
  const playerResponse = id => {
    const player = document.getElementById('movie_player');
    let data;
    try { data = player?.getPlayerResponse?.(); } catch {}
    if (data?.videoDetails?.videoId === id) return data;
    return window.ytInitialPlayerResponse?.videoDetails?.videoId === id ? window.ytInitialPlayerResponse : null;
  };
  window.__govueCaptions = {
    cancel(requestId) { if (pending?.requestId === requestId) pending.finish(); },
    request(requestId, task) {
      if (currentVideo() !== task.videoId || !/^[\w-]{11}$/.test(task.videoId)) return;
      if (pending?.requestId === requestId && pending.taskId === task.taskId) return;
      pending?.finish();
      const originalFetch = window.fetch;
      const originalOpen = XMLHttpRequest.prototype.open;
      const originalSend = XMLHttpRequest.prototype.send;
      const panelRequests = new WeakSet();
      const previousPanels = new WeakSet([...document.querySelectorAll('ytd-transcript-renderer,ytm-transcript-renderer,ytm-transcript-search-panel-renderer')].map(node => node.data).filter(data => data && typeof data === 'object'));
      const listeners = new Map();
      let timer, interval, changed = false, previous;
      let player = document.getElementById('movie_player');
      const valid = () => pending === state && currentVideo() === task.videoId && !!playerResponse(task.videoId);
      const finish = (source, format) => {
        if (pending !== state) return;
        const ok = valid() && typeof source === 'string' && source.length > 0 && source.length <= MAX_BYTES;
        pending = null;
        clearTimeout(timer); clearInterval(interval);
        if (window.fetch === captureFetch) window.fetch = originalFetch;
        if (XMLHttpRequest.prototype.open === captureOpen) XMLHttpRequest.prototype.open = originalOpen;
        if (XMLHttpRequest.prototype.send === captureSend) XMLHttpRequest.prototype.send = originalSend;
        for (const [xhr, listener] of listeners) xhr.removeEventListener('load', listener);
        listeners.clear();
        if (changed && currentVideo() === task.videoId) {
          try { player.setOption('captions', 'track', previous || {}); } catch {}
        }
        window.ReactNativeWebView?.postMessage(JSON.stringify({type:'captionPage',requestId,taskId:task.taskId,
          videoId:task.videoId,trackId:task.trackId,ok,...ok && {source,format}}));
      };
      const state = { requestId, taskId: task.taskId, finish };
      const trackName = () => {
        try {
          const list = player?.getOption?.('captions', 'tracklist');
          const track = Array.isArray(list) && list.find(track => (track.vss_id || track.vssId) === task.trackId);
          if (track) return track.name || '';
          const source = playerResponse(task.videoId)?.captions?.playerCaptionsTracklistRenderer?.captionTracks?.find(track => track.vssId === task.trackId);
          return new URL(source.baseUrl).searchParams.get('name') || '';
        } catch { return ''; }
      };
      const matches = address => {
        try {
          const url = new URL(address, location.href);
          return valid() && url.protocol === 'https:' &&
            ['youtube.com','www.youtube.com','m.youtube.com'].includes(url.hostname) &&
            url.pathname === '/api/timedtext' && url.searchParams.get('v') === task.videoId &&
            url.searchParams.get('lang') === task.language &&
            (url.searchParams.get('name') || '') === trackName() &&
            (url.searchParams.get('kind') === 'asr') === task.automatic && !url.searchParams.has('tlang');
        } catch { return false; }
      };
      const panelAddress = address => {
        try {
          const url = new URL(address, location.href);
          return valid() && url.origin === location.origin && url.pathname === '/youtubei/v1/get_transcript';
        } catch { return false; }
      };
      const panelBody = body => {
        try {
          const params = JSON.parse(body).params;
          const decoded = atob(decodeURIComponent(params).replace(/-/g, '+').replace(/_/g, '/'));
          return decoded.startsWith('\x0a\x0b' + task.videoId);
        } catch { return false; }
      };
      const captureFetch = async function (...args) {
        const body = args[1]?.body ?? args[0]?.clone?.().text();
        const response = await Reflect.apply(originalFetch, this, args);
        const address = typeof args[0] === 'string' ? args[0] : args[0]?.url || String(args[0]);
        if (response.ok && panelAddress(address)) Promise.resolve(body).then(body => {
          if (panelBody(body)) return response.clone().text().then(source => { if (valid()) finish(source, 'panel'); });
        }).catch(() => {});
        if (response.ok && matches(address)) response.clone().text().then(source => {
          if (source.trim() && matches(address)) finish(source, 'timedtext');
        }).catch(() => {});
        return response;
      };
      const captureOpen = function (method, address, ...rest) {
        if (matches(address) || panelAddress(address)) {
          const listener = () => {
            listeners.delete(this);
            const panel = panelAddress(address) && panelRequests.has(this);
            if (this.status !== 200 || !matches(address) && !panel) return;
            try {
              const source = this.responseType === 'json' ? JSON.stringify(this.response) : this.responseText;
              if (source?.trim()) finish(source, panel ? 'panel' : 'timedtext');
            } catch {}
          };
          listeners.set(this, listener);
          this.addEventListener('load', listener, {once:true});
        }
        return Reflect.apply(originalOpen, this, [method, address, ...rest]);
      };
      const captureSend = function (body) {
        if (panelBody(body)) panelRequests.add(this);
        return Reflect.apply(originalSend, this, [body]);
      };
      pending = state;
      window.fetch = captureFetch;
      XMLHttpRequest.prototype.open = captureOpen;
      XMLHttpRequest.prototype.send = captureSend;
      timer = setTimeout(() => finish(), TIMEOUT);
      let selected = false, opened = false, retried = false;
      const started = Date.now();
      const tick = () => {
        player ||= document.getElementById('movie_player');
        if (!valid()) { if (currentVideo() !== task.videoId) finish(); return; }
        if (selected && !retried && Date.now() - started >= 3000) { selected = false; retried = true; }
        if (!selected && player?.loadModule && player?.setOption) {
          try {
            const tracks = playerResponse(task.videoId)?.captions?.playerCaptionsTracklistRenderer?.captionTracks || [];
            const track = tracks.find(t => t.vssId === task.trackId);
            if (track) {
              if (!changed) previous = player.getOption?.('captions', 'track');
              player.loadModule('captions');
              changed = true;
              player.setOption('captions', 'track', {});
              player.setOption('captions', 'track', {languageCode:track.languageCode,kind:track.kind,vss_id:track.vssId,name:trackName()});
              player.setOption('captions', 'reload', true);
              selected = true;
            }
          } catch {}
        }
        for (const renderer of document.querySelectorAll('ytd-transcript-renderer,ytm-transcript-renderer,ytm-transcript-search-panel-renderer')) {
          if (opened && renderer.data && !previousPanels.has(renderer.data)) {
            const source = JSON.stringify(renderer.data);
            if (source.includes('transcriptSegmentRenderer')) { finish(source, 'panel'); return; }
          }
        }
        if (!opened && Date.now() - started >= 1000) {
          const section = document.querySelector('ytd-video-description-transcript-section-renderer,ytm-video-description-transcript-section-renderer');
          const button = section?.querySelector('button');
          if (button && !button.disabled && section.data) { opened = true; button.click(); }
          else document.querySelector('ytd-watch-metadata ytd-text-inline-expander #expand')?.click();
        }
      };
      interval = setInterval(tick, 100);
      tick();
    },
  };
})();
