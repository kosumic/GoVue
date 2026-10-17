// Runs only inside YouTube's WebView. npm run page:build bundles this script.
(() => {
  if (window.__lingoplay || !['youtube.com', 'www.youtube.com', 'm.youtube.com', 'music.youtube.com'].includes(location.hostname) || location.protocol !== 'https:') return;
  const videoId = () => {
    const url = new URL(location.href);
    const id = url.pathname === '/watch' ? url.searchParams.get('v') : url.pathname.match(/^\/(?:shorts|live|embed)\/([\w-]{11})(?:\/|$)/)?.[1];
    return id && /^[\w-]{11}$/.test(id) ? id : null;
  };
  const send = value => window.ReactNativeWebView?.postMessage(JSON.stringify(value));
  let lastUrl = '';
  let documentVideo = videoId();
  let positionRequest = null;
  const announce = () => {
    if (lastUrl === location.href) return;
    lastUrl = location.href;
    const nextVideo = videoId();
    if (nextVideo !== documentVideo) { documentVideo = nextVideo; positionRequest = null; }
    send({ type: 'page', url: location.href, videoId: videoId(), title: document.title, userAgent: navigator.userAgent });
  };
  window.__lingoplay = {
    position(requestId, id, playback = false) {
      if (videoId() !== id) return;
      const player = document.getElementById('movie_player');
      const owner = player?.getVideoData?.()?.video_id || player?.getPlayerResponse?.()?.videoDetails?.videoId || window.ytInitialPlayerResponse?.videoDetails?.videoId;
      // SPA navigation can change the URL before the old player is replaced.
      if (owner !== id) return;
      positionRequest = requestId;
      const video = player?.querySelector('video') || document.querySelector('video');
      const seconds = player?.classList.contains('ad-showing') || player?.classList.contains('ad-interrupting')
        ? null : video ? video.currentTime : player?.getCurrentTime?.();
      send({ type: 'playbackPosition', requestId, videoId: id,
        seconds: Number.isFinite(seconds) && seconds >= 0 ? seconds : null,
        ...(playback ? { playing: video ? !video.paused && !video.ended : player?.getPlayerState?.() === 1,
          seeking: video?.seeking === true, rate: video?.playbackRate || player?.getPlaybackRate?.() || 1 } : {}) });
    },
    cancelPosition(requestId) { if (positionRequest === requestId) positionRequest = null; },
    pause(requestId, id, expectedPosition) {
      if (positionRequest !== requestId || videoId() !== id || !Number.isFinite(expectedPosition)) return;
      const player = document.getElementById('movie_player');
      const owner = player?.getVideoData?.()?.video_id || player?.getPlayerResponse?.()?.videoDetails?.videoId || window.ytInitialPlayerResponse?.videoDetails?.videoId;
      if (owner !== id || player?.classList.contains('ad-showing') || player?.classList.contains('ad-interrupting')) return;
      const video = player?.querySelector('video') || document.querySelector('video');
      const seconds = video ? video.currentTime : player?.getCurrentTime?.();
      if (video?.seeking || !Number.isFinite(seconds) || Math.abs(seconds - expectedPosition) > 0.5) return;
      if (player?.pauseVideo) player.pauseVideo();
      else video?.pause();
    },
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
