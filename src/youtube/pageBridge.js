// Kept as literal JavaScript so Babel cannot introduce helpers outside the WebView.
export const YOUTUBE_BRIDGE = String.raw`
(() => {
  if (window.__lingoplay || !['youtube.com', 'www.youtube.com', 'm.youtube.com', 'music.youtube.com'].includes(location.hostname) || location.protocol !== 'https:') return;
  const videoId = () => {
    const url = new URL(location.href);
    const id = url.pathname === '/watch' ? url.searchParams.get('v') : url.pathname.match(/^\/(?:shorts|live|embed)\/([\w-]{11})(?:\/|$)/)?.[1];
    return id && /^[\w-]{11}$/.test(id) ? id : null;
  };
  const send = value => window.ReactNativeWebView?.postMessage(JSON.stringify(value));
  const text = value => typeof value === 'string' ? value : value?.simpleText || value?.runs?.map(run => run.text || '').join('') || '';
  const clean = value => value.replace(/\s+/g, ' ').trim();
  const walk = (root, key) => {
    const found = [];
    const seen = new WeakSet();
    const queue = [root];
    let count = 0;
    while (queue.length && count++ < 100000) {
      const value = queue.pop();
      if (!value || typeof value !== 'object' || seen.has(value)) continue;
      seen.add(value);
      if (value[key]) found.push(value[key]);
      for (const child of Object.values(value)) if (child && typeof child === 'object') queue.push(child);
    }
    return found;
  };
  const config = key => window.ytcfg?.get?.(key);
  let transcriptRequest = null;
  const request = async (url, options, signal) => {
    const response = await fetch(url, { credentials: 'same-origin', ...options, signal });
    if (!response.ok) throw new Error('YouTube could not provide this transcript. Try opening the video and requesting it again.');
    return response;
  };
  const api = async (method, body, signal) => {
    if (method === 'get_transcript' && transcriptRequest?.id === videoId()) {
      const saved = transcriptRequest;
      const response = await request(saved.url, {
        method: 'POST', headers: saved.headers,
        body: JSON.stringify({ ...saved.body, ...body })
      }, signal);
      return response.json();
    }
    const context = config('INNERTUBE_CONTEXT');
    if (!context) throw new Error('YouTube is still loading. Play the video, then try again.');
    const url = new URL('/youtubei/v1/' + method, location.origin);
    const key = config('INNERTUBE_API_KEY');
    if (key) url.searchParams.set('key', key);
    const response = await request(url.href, {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ context, ...body })
    }, signal);
    return response.json();
  };
  const playerResponse = id => {
    const player = document.getElementById('movie_player');
    let current;
    try { current = player?.getPlayerResponse?.(); } catch {}
    if (current?.videoDetails?.videoId === id) return current;
    if (window.ytInitialPlayerResponse?.videoDetails?.videoId === id) return window.ytInitialPlayerResponse;
    return null;
  };
  const segmentsFromApi = data => walk(data, 'transcriptSegmentRenderer').map(segment => ({
    start: Number(segment.startMs) / 1000,
    duration: Math.max(0, (Number(segment.endMs) - Number(segment.startMs)) / 1000),
    text: clean(text(segment.snippet))
  })).filter(segment => Number.isFinite(segment.start) && segment.start >= 0 && Number.isFinite(segment.duration) && segment.text)
    .sort((a, b) => a.start - b.start);
  let cachedTranscript = null;
  const captionCache = new Map();
  let documentVideo = videoId();
  let domOwner = documentVideo;
  const parseCaptionSource = source => {
    let segments;
    if (source.trimStart().startsWith('{')) {
      const data = JSON.parse(source);
      segments = (data.events || []).filter(event => Array.isArray(event.segs)).map(event => ({
        start: Number(event.tStartMs) / 1000,
        duration: Math.max(0, Number(event.dDurationMs || 0) / 1000),
        text: clean(event.segs.map(segment => segment.utf8 || '').join(''))
      }));
    } else {
      const xml = new DOMParser().parseFromString(source, 'text/xml');
      if (xml.querySelector('parsererror')) throw new Error('YouTube returned unreadable captions.');
      segments = Array.from(xml.querySelectorAll('text, p')).map(node => {
        const legacy = node.tagName === 'text';
        const start = Number(node.getAttribute(legacy ? 'start' : 't')) / (legacy ? 1 : 1000);
        const duration = Number(node.getAttribute(legacy ? 'dur' : 'd') || 0) / (legacy ? 1 : 1000);
        const decoder = document.createElement('textarea');
        decoder.innerHTML = node.textContent || '';
        return { start, duration: Math.max(0, duration), text: clean(decoder.value) };
      });
    }
    return segments.filter(segment => segment.text && Number.isFinite(segment.start) && segment.start >= 0 && Number.isFinite(segment.duration))
      .sort((a, b) => a.start - b.start);
  };
  const rememberCaptions = (address, source, id) => {
    try {
      const url = new URL(address, location.origin);
      if (videoId() !== id || url.protocol !== 'https:' || !['youtube.com', 'www.youtube.com', 'm.youtube.com'].includes(url.hostname) ||
          url.pathname !== '/api/timedtext' || url.searchParams.get('v') !== id || url.searchParams.has('tlang') ||
          !source.trim() || source.length > 8000000) return;
      const language = url.searchParams.get('lang');
      const automatic = url.searchParams.get('kind') === 'asr';
      const tracks = playerResponse(id)?.captions?.playerCaptionsTracklistRenderer?.captionTracks || [];
      const track = tracks.find(track => track.languageCode === language && (track.kind === 'asr') === automatic);
      if (!track) return;
      const segments = parseCaptionSource(source);
      if (!segments.length) return;
      const key = String(track.vssId || track.languageCode);
      captionCache.set(key, { videoId: id, segments, source, language: text(track.name) || language, trackId: key });
      if (captionCache.size > 3) captionCache.delete(captionCache.keys().next().value);
    } catch {}
  };
  // Let YouTube add its own integrity/session data. A hand-built Innertube
  // request can return 400 even while Show transcript works on the same page.
  const originalFetch = window.fetch;
  window.fetch = async function (...args) {
    const id = videoId();
    let template = null;
    try {
      if (typeof Request !== 'undefined' && args[0] instanceof Request) template = args[0].clone();
    } catch {}
    const response = await Reflect.apply(originalFetch, this, args);
    try {
      const url = new URL(typeof args[0] === 'string' ? args[0] : args[0]?.url || String(args[0]), location.origin);
      if (id && response.ok && url.pathname === '/api/timedtext') {
        response.clone().text().then(source => rememberCaptions(url.href, source, id)).catch(() => {});
      }
      if (id && response.ok && url.origin === location.origin && url.pathname === '/youtubei/v1/get_transcript') {
        response.clone().json().then(async data => {
          if (videoId() === id && segmentsFromApi(data).length) {
            try {
              const body = template ? await template.json() : JSON.parse(args[1]?.body || 'null');
              if (body && videoId() === id) transcriptRequest = {
                id, url: url.href, body,
                headers: template ? Array.from(template.headers.entries()) : args[1]?.headers || { 'Content-Type': 'application/json' }
              };
            } catch {}
            if (videoId() !== id) return;
            cachedTranscript = { id, data };
            domOwner = id;
          }
        }).catch(() => {});
      }
    } catch {}
    return response;
  };
  // YouTube's player uses XHR on Android. Capture its signed request instead
  // of rebuilding a caption URL without the player's proof-of-origin token.
  const xhrRequests = new WeakMap();
  const xhrObserved = new WeakSet();
  const originalOpen = XMLHttpRequest.prototype.open;
  XMLHttpRequest.prototype.open = function (method, url, ...rest) {
    xhrRequests.set(this, { id: videoId(), url: String(url) });
    if (!xhrObserved.has(this)) {
      xhrObserved.add(this);
      this.addEventListener('load', () => {
        const meta = xhrRequests.get(this);
        if (!meta?.id || !meta.url.includes('/api/timedtext') || this.status !== 200) return;
        try {
          const source = this.responseType === 'json' ? JSON.stringify(this.response) : this.responseText;
          if (typeof source === 'string') rememberCaptions(this.responseURL || meta.url, source, meta.id);
        } catch {}
      });
    }
    return Reflect.apply(originalOpen, this, [method, url, ...rest]);
  };
  const captionTrackIndex = (id, tracks, requestedTrack) => {
    if (requestedTrack !== 'default') return tracks.findIndex((track, index) => String(track.vssId || track.languageCode + ':' + index) === requestedTrack);
    const selected = document.getElementById('movie_player')?.getOption?.('captions', 'track');
    const current = selected?.vss_id || selected?.vssId;
    const currentIndex = tracks.findIndex(track => track.vssId && track.vssId === current);
    if (currentIndex >= 0) return currentIndex;
    const captions = playerResponse(id)?.captions?.playerCaptionsTracklistRenderer;
    const defaultIndex = captions?.audioTracks?.[captions.defaultAudioTrackIndex || 0]?.defaultCaptionTrackIndex;
    if (Number.isInteger(defaultIndex) && defaultIndex >= 0 && defaultIndex < tracks.length) return defaultIndex;
    const language = (document.documentElement.lang || '').toLowerCase();
    const languageIndex = tracks.findIndex(track => track.languageCode?.toLowerCase() === language);
    if (languageIndex >= 0) return languageIndex;
    const baseIndex = tracks.findIndex(track => track.languageCode?.toLowerCase().split('-')[0] === language.split('-')[0]);
    return baseIndex >= 0 ? baseIndex : 0;
  };
  const playerCaptions = async (id, tracks, requestedTrack, signal) => {
    const player = document.getElementById('movie_player');
    if (!player?.loadModule || !player?.setOption || !tracks.length) return null;
    const previous = player.getOption?.('captions', 'track') || {};
    const track = tracks[captionTrackIndex(id, tracks, requestedTrack)];
    if (!track) return null;
    const key = String(track.vssId || track.languageCode);
    if (captionCache.get(key)?.videoId === id) return captionCache.get(key);
    try {
      player.loadModule('captions');
      const nativeTracks = player.getOption?.('captions', 'tracklist') || [];
      const nativeTrack = nativeTracks.find(item => (item.vss_id || item.vssId) === track.vssId);
      // A paused/unstarted player can report a selected track without having
      // fetched it. Selecting that same track or setting reload is a no-op;
      // clear it first so YouTube issues its own signed caption request.
      player.setOption('captions', 'track', {});
      player.setOption('captions', 'track', nativeTrack || { languageCode: track.languageCode, kind: track.kind, vss_id: track.vssId });
      player.setOption('captions', 'reload', true);
      const deadline = Date.now() + 12000;
      while (Date.now() < deadline && !signal.aborted && videoId() === id) {
        if (captionCache.get(key)?.videoId === id) return captionCache.get(key);
        await new Promise(resolve => setTimeout(resolve, 100));
      }
      return null;
    } finally {
      if (videoId() === id) {
        try { player.setOption('captions', 'track', previous); } catch {}
      }
    }
  };
  const renderedTranscript = id => {
    if (domOwner === id && playerResponse(id)?.videoDetails?.videoId === id) {
      const renderers = document.querySelectorAll('ytd-transcript-renderer, ytm-transcript-renderer, ytm-transcript-search-panel-renderer');
      for (const renderer of renderers) {
        const data = renderer.data;
        if (data && segmentsFromApi(data).length) return data;
      }
    }
    return cachedTranscript?.id === id ? cachedTranscript.data : null;
  };
  const pageTranscript = async (id, signal) => {
    // Android can finish loading the document before Polymer has initialized
    // YouTube's controls. Wait for the live renderer rather than falling back
    // immediately to the token-protected API when the static button is absent.
    const deadline = Date.now() + 20000;
    let clicks = 0;
    let lastClick = 0;
    let expanded = false;
    const visible = node => {
      const rect = node.getBoundingClientRect();
      return rect.width > 0 && rect.height > 0 && getComputedStyle(node).visibility !== 'hidden';
    };
    while (Date.now() < deadline && !signal.aborted && videoId() === id) {
      const data = renderedTranscript(id);
      if (data) return data;
      // YouTube renders duplicate controls in collapsed descriptions and closed
      // engagement panels. Their commands can be inert until the description opens.
      const button = Array.from(document.querySelectorAll('ytd-video-description-transcript-section-renderer button, ytm-video-description-transcript-section-renderer button')).find(visible);
      if (!button && !expanded) {
        const expand = Array.from(document.querySelectorAll('ytd-watch-metadata ytd-text-inline-expander #expand')).find(visible);
        if (expand) { expand.click(); expanded = true; }
      }
      const section = button?.closest('ytd-video-description-transcript-section-renderer, ytm-video-description-transcript-section-renderer');
      const buttonHost = button?.closest('ytd-button-renderer');
      if (clicks < 3 && Date.now() - lastClick >= 1500 && button && !button.disabled && section.data && (!buttonHost || buttonHost.data) && playerResponse(id)) {
        button.click();
        clicks++;
        lastClick = Date.now();
      }
      await new Promise(resolve => setTimeout(resolve, 100));
    }
    return null;
  };
  const selectedLanguage = data => text(walk(data, 'sortFilterSubMenuRenderer').flatMap(menu => menu.subMenuItems || []).find(item => item.selected)?.title);
  const pageLanguage = async (id, label, signal) => {
    const initial = await pageTranscript(id, signal);
    if (!initial || signal.aborted || videoId() !== id) return null;
    if (clean(selectedLanguage(initial)) === clean(label)) return initial;
    const choices = document.querySelectorAll('ytd-transcript-footer-renderer tp-yt-paper-item, ytm-transcript-footer-renderer [role="option"]');
    const choice = Array.from(choices).find(node => clean(node.textContent || '') === clean(label));
    if (!choice) return null;
    const previousSegment = walk(initial, 'transcriptSegmentRenderer')[0];
    choice.click();
    const deadline = Date.now() + 6500;
    while (Date.now() < deadline && !signal.aborted && videoId() === id) {
      const data = renderedTranscript(id);
      if (data && walk(data, 'transcriptSegmentRenderer')[0] !== previousSegment && clean(selectedLanguage(data)) === clean(label)) return data;
      await new Promise(resolve => setTimeout(resolve, 100));
    }
    return null;
  };
  const captionSegments = async (track, signal) => {
    const url = new URL(track.baseUrl, location.origin);
    if (url.protocol !== 'https:' || !['www.youtube.com', 'youtube.com', 'm.youtube.com'].includes(url.hostname) || url.pathname !== '/api/timedtext') {
      throw new Error('YouTube returned an unsupported caption address.');
    }
    url.searchParams.set('fmt', 'json3');
    const response = await request(url.href, {}, signal);
    const source = await response.text();
    if (!source.trim()) throw new Error('YouTube did not return captions for this language. Try the default transcript.');
    return parseCaptionSource(source);
  };
  let lastUrl = '';
  let active = null;
  const announce = () => {
    if (lastUrl === location.href) return;
    lastUrl = location.href;
    const nextVideo = videoId();
    if (nextVideo !== documentVideo) {
      documentVideo = nextVideo;
      cachedTranscript = null;
      captionCache.clear();
      transcriptRequest = null;
      domOwner = null;
      active?.abort();
    }
    send({ type: 'page', url: location.href, videoId: videoId(), title: document.title, userAgent: navigator.userAgent });
  };
  window.__lingoplay = {
    async extract(requestId, requestedTrack = 'default') {
      active?.abort();
      const controller = new AbortController();
      active = controller;
      const signal = controller.signal;
      const id = videoId();
      if (!id) return;
      const timeout = setTimeout(() => controller.abort(), 35000);
      try {
        const player = playerResponse(id);
        const rawTracks = player?.captions?.playerCaptionsTracklistRenderer?.captionTracks || [];
        const tracks = rawTracks.map((track, index) => ({
          id: String(track.vssId || track.languageCode + ':' + index),
          label: text(track.name) || track.languageCode,
          language: track.languageCode || '', automatic: track.kind === 'asr'
        }));
        const captured = await playerCaptions(id, rawTracks, requestedTrack, signal);
        let segments = captured?.segments || [];
        let language = captured?.language || 'YouTube default';
        let trackId = captured?.trackId || requestedTrack;
        let failure;
        if (!segments.length && requestedTrack === 'default') {
          try {
            let data = await pageTranscript(id, signal);
            if (!data) {
              // /next binds transcript parameters to this video after SPA navigation.
              const next = await api('next', { videoId: id }, signal);
              if (signal.aborted || videoId() !== id) return;
              const endpoint = walk(next, 'getTranscriptEndpoint')[0];
              if (endpoint?.params) data = await api('get_transcript', { params: endpoint.params }, signal);
            }
            if (data) {
              segments = segmentsFromApi(data);
              const menus = walk(data, 'sortFilterSubMenuRenderer');
              const selected = menus.flatMap(menu => menu.subMenuItems || []).find(item => item.selected);
              language = text(selected?.title) || language;
            }
          } catch (error) {
            if (signal.aborted) throw error;
            failure = error;
          }
        } else if (!segments.length) {
          const track = tracks.find(track => track.id === requestedTrack);
          if (track) {
            try {
              const nativeLanguage = await pageLanguage(id, track.label, signal);
              if (nativeLanguage) {
                segments = segmentsFromApi(nativeLanguage);
                language = track.label;
              }
              const initial = renderedTranscript(id);
              const menus = walk(initial, 'sortFilterSubMenuRenderer');
              const item = menus.flatMap(menu => menu.subMenuItems || []).find(item => clean(text(item.title)) === clean(track.label));
              const endpoint = walk(item, 'getTranscriptEndpoint')[0];
              const params = endpoint?.params || item?.continuation?.reloadContinuationData?.continuation;
              if (!segments.length && params) {
                const data = await api('get_transcript', { params }, signal);
                segments = segmentsFromApi(data);
                language = track.label;
              }
            } catch (error) {
              if (signal.aborted) throw error;
              failure = error;
            }
          }
        }
        if (signal.aborted || videoId() !== id) return;
        if (!segments.length) {
          const index = captionTrackIndex(id, rawTracks, requestedTrack);
          if (index < 0 || !rawTracks[index]) {
            throw failure || new Error('No transcript is available. Play the video and try again; some videos have no captions.');
          }
          segments = await captionSegments(rawTracks[index], signal);
          language = tracks[index].label;
          trackId = tracks[index].id;
        }
        if (!segments.length) throw new Error('This video has no caption text available yet. Live captions may appear later.');
        if (signal.aborted || videoId() !== id) return;
        send({ type: 'transcript', requestId, videoId: id, ok: true, transcript: {
          videoId: id, title: player?.videoDetails?.title || document.title.replace(/ - YouTube$/, ''),
          language, trackId, tracks: [{ id: 'default', label: 'YouTube default', language: '', automatic: false }, ...tracks], segments,
          ...(captured?.source ? { captionSource: captured.source } : {})
        }});
      } catch (error) {
        if (active !== controller || videoId() !== id) return;
        send({ type: 'transcript', requestId, videoId: id, ok: false,
          error: signal.aborted ? 'Transcript request timed out. Check your connection and try again.' : error.message || 'Transcript unavailable. Try again.' });
      } finally {
        clearTimeout(timeout);
        if (active === controller) active = null;
      }
    },
    cancel() { active?.abort(); active = null; },
    seek(id, seconds) {
      if (videoId() !== id || !Number.isFinite(seconds) || seconds < 0) return;
      const player = document.getElementById('movie_player');
      // Use the player API so YouTube updates its own timeline and playback
      // state, including when a native transcript tap resumes a paused video.
      if (player?.seekTo && player?.playVideo) {
        try {
          player.seekTo(seconds, true);
          player.playVideo();
          return;
        } catch {}
      }
      const video = player?.querySelector('video') || document.querySelector('video');
      if (video) {
        video.currentTime = seconds;
        try { video.play()?.catch(() => {}); } catch {}
      }
    }
  };
  document.addEventListener('yt-navigate-finish', announce);
  window.addEventListener('popstate', announce);
  // Mobile YouTube doesn't consistently dispatch the desktop navigation event.
  setInterval(announce, 750);
  announce();
})();
true;
`;

export function extractionScript(requestId, trackId = 'default') {
  return `${YOUTUBE_BRIDGE}\nwindow.__lingoplay?.extract(${JSON.stringify(requestId)}, ${JSON.stringify(trackId)}); true;`;
}

export function seekScript(videoId, seconds) {
  return `window.__lingoplay?.seek(${JSON.stringify(videoId)}, ${JSON.stringify(seconds)}); true;`;
}
