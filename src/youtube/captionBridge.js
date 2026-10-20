// On-demand transport in the existing YouTube page. Rust parses all captions.
(() => {
  const hosts = ['youtube.com', 'www.youtube.com', 'm.youtube.com', 'music.youtube.com'];
  if (window.__govueCaptions || location.protocol !== 'https:' || !hosts.includes(location.hostname)) return;
  const MAX_BYTES = 8_000_000;
  const TIMEOUT = 11000;
  const pending = new Map();
  let capture;
  const currentVideo = () => {
    const url = new URL(location.href);
    return url.pathname === '/watch' ? url.searchParams.get('v') : url.pathname.match(/^\/(?:shorts|live|embed)\/([\w-]{11})(?:\/|$)/)?.[1];
  };
  const playerResponse = id => {
    let data;
    try { data = document.getElementById('movie_player')?.getPlayerResponse?.(); } catch {}
    if (data?.videoDetails?.videoId === id) return data;
    return window.ytInitialPlayerResponse?.videoDetails?.videoId === id ? window.ytInitialPlayerResponse : null;
  };
  const captionAddress = address => {
    try {
      const url = new URL(address, location.href);
      return url.protocol === 'https:' && hosts.includes(url.hostname) && url.pathname === '/api/timedtext' &&
        url.searchParams.get('v') === currentVideo() ? url : null;
    } catch { return null; }
  };
  const panelAddress = address => {
    try {
      const url = new URL(address, location.href);
      return url.origin === location.origin && url.pathname === '/youtubei/v1/get_transcript';
    } catch { return false; }
  };
  const panelBody = (body, id) => {
    try {
      const params = JSON.parse(body).params;
      return atob(decodeURIComponent(params).replace(/-/g, '+').replace(/_/g, '/')).startsWith('\x0a\x0b' + id);
    } catch { return false; }
  };
  const startCapture = () => {
    if (capture) return capture;
    const originalFetch = window.fetch;
    const originalOpen = XMLHttpRequest.prototype.open;
    const originalSend = XMLHttpRequest.prototype.send;
    const requests = new WeakMap(), listeners = new Map();
    const controller = new AbortController(), sources = new Map();
    const videoId = currentVideo();
    let player, changed = false, previous;
    const deliver = (address, source, body) => {
      if (typeof source !== 'string' || !source.trim() || source.length > MAX_BYTES) return;
      for (const state of [...pending.values()]) {
        if (captionAddress(address)) state.caption(address, source);
        else if (!state.translated && panelAddress(address) && panelBody(body, state.videoId)) state.finish(source, 'panel');
      }
    };
    const captureFetch = async function (...args) {
      const address = typeof args[0] === 'string' ? args[0] : args[0]?.url || String(args[0]);
      const interested = captionAddress(address) || panelAddress(address);
      const body = panelAddress(address) ? args[1]?.body ?? args[0]?.clone?.().text() : null;
      const response = await Reflect.apply(originalFetch, this, args);
      if (interested && response.ok) Promise.resolve(body).then(async body => {
        deliver(address, await response.clone().text(), body);
      }).catch(() => {});
      return response;
    };
    const captureOpen = function (method, address, ...rest) {
      const old = listeners.get(this);
      if (old) { this.removeEventListener('load', old); listeners.delete(this); }
      requests.delete(this);
      if (captionAddress(address) || panelAddress(address)) {
        const request = { address, body: null };
        requests.set(this, request);
        const listener = () => {
          listeners.delete(this);
          if (this.status !== 200) return;
          try { deliver(request.address, this.responseType === 'json' ? JSON.stringify(this.response) : this.responseText, request.body); } catch {}
        };
        listeners.set(this, listener);
        this.addEventListener('load', listener, { once: true });
      }
      return Reflect.apply(originalOpen, this, [method, address, ...rest]);
    };
    const captureSend = function (body) {
      const request = requests.get(this);
      if (request) request.body = body;
      return Reflect.apply(originalSend, this, [body]);
    };
    window.fetch = captureFetch;
    XMLHttpRequest.prototype.open = captureOpen;
    XMLHttpRequest.prototype.send = captureSend;
    capture = {
      read(address) {
        if (!sources.has(address)) sources.set(address,
          Promise.resolve().then(() => Reflect.apply(originalFetch, window,
            [address, { credentials: 'include', signal: controller.signal }]))
            .then(response => response.ok ? response.text() : null)
            .then(source => typeof source === 'string' && source.trim() && source.length <= MAX_BYTES ? source : null)
            .catch(() => null));
        return sources.get(address);
      },
      select(track) {
        player = document.getElementById('movie_player');
        if (!changed) previous = player?.getOption?.('captions', 'track');
        player?.loadModule?.('captions');
        changed = true;
        player?.setOption?.('captions', 'track', {});
        player?.setOption?.('captions', 'track', track);
        player?.setOption?.('captions', 'reload', true);
      },
      stop() {
        controller.abort(); sources.clear();
        if (window.fetch === captureFetch) window.fetch = originalFetch;
        if (XMLHttpRequest.prototype.open === captureOpen) XMLHttpRequest.prototype.open = originalOpen;
        if (XMLHttpRequest.prototype.send === captureSend) XMLHttpRequest.prototype.send = originalSend;
        for (const [xhr, listener] of listeners) xhr.removeEventListener('load', listener);
        listeners.clear();
        if (changed && currentVideo() === videoId) {
          try { player?.setOption?.('captions', 'track', previous || {}); } catch {}
        }
        capture = null;
      },
    };
    return capture;
  };
  window.__govueCaptions = {
    cancel(requestId) { pending.get(requestId)?.finish(); },
    request(requestId, task) {
      if (currentVideo() !== task.videoId || !/^[\w-]{11}$/.test(task.videoId)) return;
      if (pending.get(requestId)?.taskId === task.taskId) return;
      pending.get(requestId)?.finish();
      for (const state of [...pending.values()]) if (state.videoId !== task.videoId) state.finish();
      const transport = startCapture();
      const sourceId = task.sourceTrackId || task.trackId;
      const translated = sourceId !== task.trackId;
      const previousPanels = new WeakSet([...document.querySelectorAll('ytd-transcript-renderer,ytm-transcript-renderer,ytm-transcript-search-panel-renderer')].map(node => node.data).filter(data => data && typeof data === 'object'));
      const fetched = new Set();
      let timer, interval;
      const valid = () => pending.get(requestId) === state && currentVideo() === task.videoId && !!playerResponse(task.videoId);
      const finish = (source, format) => {
        if (pending.get(requestId) !== state) return;
        const ok = valid() && typeof source === 'string' && source.length > 0 && source.length <= MAX_BYTES;
        pending.delete(requestId);
        clearTimeout(timer); clearInterval(interval);
        if (!pending.size) transport.stop();
        window.ReactNativeWebView?.postMessage(JSON.stringify({ type: 'captionPage', requestId, taskId: task.taskId,
          videoId: task.videoId, trackId: task.trackId, ok, ...ok && { source, format } }));
      };
      const sourceTrack = () => playerResponse(task.videoId)?.captions?.playerCaptionsTracklistRenderer?.captionTracks?.find(track => track.vssId === sourceId);
      const trackName = () => {
        try { return new URL(sourceTrack().baseUrl, location.href).searchParams.get('name') || ''; } catch { return ''; }
      };
      const matches = address => {
        const url = captionAddress(address);
        return valid() && url && url.searchParams.get('lang') === (translated ? sourceTrack()?.languageCode : task.language) &&
          (url.searchParams.get('name') || '') === trackName() &&
          (url.searchParams.get('kind') === 'asr') === task.automatic &&
          (translated ? url.searchParams.get('tlang') === task.language : !url.searchParams.has('tlang'));
      };
      const fetchSource = address => {
        if (!matches(address) || fetched.has(address) || fetched.size >= 4) return;
        fetched.add(address);
        transport.read(address).then(source => {
          if (source && matches(address)) finish(source, 'timedtext');
        });
      };
      const reuseProof = address => {
        const proof = captionAddress(address), track = sourceTrack();
        if (!valid() || !proof?.searchParams.get('pot') || !track) return;
        const target = captionAddress(track.baseUrl);
        if (!target) return;
        // Subtitle proofs bind to the video, including when the selectable player
        // tracklist hides ASR behind an uploaded track. Preserve signed bytes;
        // only replace unsigned format/client/proof fields in the requested track.
        const fields = ['pot', 'potc', 'c', 'cver', 'cplayer', 'cplatform'];
        const signed = new Set((target.searchParams.get('sparams') || '').split(','));
        const copy = fields.filter(key => !signed.has(key) && proof.searchParams.has(key));
        const query = target.search.slice(1).split('&').filter(pair => {
          const key = new URLSearchParams(pair).keys().next().value;
          return !['fmt', 'xosf', 'tlang', ...copy].includes(key);
        });
        const extra = new URLSearchParams({ fmt: 'json3' });
        if (translated) extra.set('tlang', task.language);
        for (const key of copy) extra.set(key, proof.searchParams.get(key));
        fetchSource(`${target.origin}${target.pathname}?${query.join('&')}&${extra}`);
      };
      const state = { taskId: task.taskId, videoId: task.videoId, translated, finish, caption(address, source) {
        if (matches(address)) finish(source, 'timedtext');
        else reuseProof(address);
      } };
      pending.set(requestId, state);
      timer = setTimeout(() => finish(), TIMEOUT);
      // A reader opened after captions loaded can reuse the completed page
      // request immediately. Signed URLs and proofs stay in this document only.
      let scanned = false, selected = false, opened = false, retried = false;
      const started = Date.now();
      const tick = () => {
        if (!valid()) { if (currentVideo() !== task.videoId) finish(); return; }
        if (!scanned) {
          scanned = true;
          const resources = performance.getEntriesByType?.('resource') || [];
          const addresses = resources.map(entry => entry.name).filter(address => captionAddress(address)).reverse();
          const existing = addresses.find(address => matches(address));
          if (existing) fetchSource(existing);
          else for (const address of addresses) { reuseProof(address); if (fetched.size) break; }
        }
        if (selected && !retried && Date.now() - started >= 3000) { selected = false; retried = true; }
        const player = document.getElementById('movie_player');
        if (!selected && player?.loadModule && player?.setOption && (!fetched.size || Date.now() - started >= 1500)) {
          try {
            const track = sourceTrack();
            if (track) {
              const tracks = player.getOption?.('captions', 'tracklist');
              const available = Array.isArray(tracks) && (tracks.find(t => (t.vss_id || t.vssId) === sourceId) || tracks.find(t => t.languageCode === track.languageCode) || tracks[0]);
              transport.select(available || { languageCode: track.languageCode, kind: track.kind, vss_id: track.vssId, name: trackName() });
              selected = true;
            }
          } catch {}
        }
        for (const renderer of translated ? [] : document.querySelectorAll('ytd-transcript-renderer,ytm-transcript-renderer,ytm-transcript-search-panel-renderer')) {
          if (opened && renderer.data && !previousPanels.has(renderer.data)) {
            const source = JSON.stringify(renderer.data);
            if (source.includes('transcriptSegmentRenderer')) { finish(source, 'panel'); return; }
          }
        }
        if (!translated && !opened && Date.now() - started >= 3000) {
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
