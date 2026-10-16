import { PLAYER_DECODER } from './generated/playerDecoder.js';

// Runs in YouTube's page runtime. Keep helpers inside this literal.
export const MEDIA_BRIDGE = String.raw`
(() => {
  if (window.__lingoplayMedia || location.protocol !== 'https:' || !['youtube.com','www.youtube.com','m.youtube.com','music.youtube.com'].includes(location.hostname)) return;
  const id = () => {
    const url = new URL(location.href);
    const value = url.pathname === '/watch' ? url.searchParams.get('v') : url.pathname.match(/^\/(?:shorts|live|embed)\/([\w-]{11})/)?.[1];
    return /^[\w-]{11}$/.test(value || '') ? value : null;
  };
  const player = () => document.getElementById('movie_player');
  const post = value => window.ReactNativeWebView?.postMessage(JSON.stringify(value));
  let settings = { adBlock:true, sponsorBlock:true, categories:['sponsor','selfpromo','interaction'] };
  let owner = id();
  let segments = [];
  let nativeOwner = null;
  let lastSkip = '';
  let lastSkipAt = 0;
  let lastReadyAt = 0;
  let lastLayout = '';
  let layoutFrame = 0;
  let observedPlayer = null;
  const resizeObserver = typeof ResizeObserver === 'function' ? new ResizeObserver(scheduleLayout) : null;
  function reportLayout() {
    const videoId = id();
    if (!videoId || nativeOwner !== videoId || document.visibilityState === 'hidden') return;
    const p = player();
    if (p !== observedPlayer) { resizeObserver?.disconnect(); if (p) resizeObserver?.observe(p); observedPlayer = p; }
    const viewport = window.visualViewport;
    const viewportWidth = viewport?.width || window.innerWidth;
    const viewportHeight = viewport?.height || window.innerHeight;
    const offsetLeft = viewport?.offsetLeft || 0;
    const offsetTop = viewport?.offsetTop || 0;
    const rect = p?.getBoundingClientRect();
    const left = rect ? rect.left - offsetLeft : 0;
    const top = rect ? rect.top - offsetTop : 0;
    const width = rect?.width || 0;
    const height = rect?.height || 0;
    const x1 = Math.max(0, left), y1 = Math.max(0, top);
    const x2 = Math.min(viewportWidth, left + width), y2 = Math.min(viewportHeight, top + height);
    let visible = x2 > x1 && y2 > y1;
    // A native view cannot be covered by a page drawer. Yield its touches and
    // surface when a YouTube comment panel/menu overlaps the video's slot.
    if (visible && document.elementFromPoint) {
      const dx = Math.min(12, (x2 - x1) / 4), dy = Math.min(12, (y2 - y1) / 4);
      const points = [[(x1+x2)/2,(y1+y2)/2],[x1+dx,y1+dy],[x2-dx,y1+dy],[x1+dx,y2-dy],[x2-dx,y2-dy]];
      visible = points.every(([x,y]) => {
        const hit = document.elementFromPoint(x + offsetLeft, y + offsetTop);
        return !hit || p.contains(hit) || hit.contains(p);
      });
    }
    const layout = { left, top, width, height, viewportWidth, viewportHeight, visible };
    const key = JSON.stringify(layout);
    if (key !== lastLayout) { lastLayout = key; post({type:'playerLayout',videoId,layout}); }
  }
  function scheduleLayout() {
    if (!layoutFrame) layoutFrame = requestAnimationFrame(() => { layoutFrame = 0; reportLayout(); });
  }
  const blocked = address => {
    if (!settings.adBlock) return false;
    try {
      const url = new URL(address, location.href);
      return ['doubleclick.net','googleadservices.com','googlesyndication.com'].some(host => url.hostname === host || url.hostname.endsWith('.'+host)) ||
        (url.hostname.endsWith('youtube.com') && /^\/(?:api\/stats\/ads|pagead\/|ptracking)/.test(url.pathname));
    } catch { return false; }
  };
  const originalFetch = window.fetch;
  window.fetch = async function (...args) {
    const address = typeof args[0] === 'string' ? args[0] : args[0]?.url || String(args[0]);
    if (blocked(address)) return new Response(null, {status:204});
    return Reflect.apply(originalFetch, this, args);
  };
  const open = XMLHttpRequest.prototype.open;
  const send = XMLHttpRequest.prototype.send;
  const requests = new WeakMap();
  XMLHttpRequest.prototype.open = function(method,address,...rest) {
    requests.set(this, {address:String(address)});
    return Reflect.apply(open,this,[method,address,...rest]);
  };
  XMLHttpRequest.prototype.send = function(...args) {
    if (blocked(requests.get(this)?.address)) { this.abort(); return; }
    return Reflect.apply(send,this,args);
  };
  const adStyle = 'html[data-lingoplay-adblock="true"] :is(#masthead-ad,#player-ads,ytd-ad-slot-renderer,ytm-ad-slot-renderer,ad-slot-renderer,ytd-display-ad-renderer,ytd-promoted-sparkles-web-renderer,ytm-promoted-sparkles-web-renderer,ytm-companion-ad-renderer,ytm-app-promo-renderer,ytm-app-install-ad-renderer,yt-mealbar-promo-renderer,ytm-open-in-app-renderer,.ytp-ad-overlay-container,.video-ads.ytp-ad-module,[data-lingoplay-app-promo]){display:none!important}';
  function tick() {
    const videoId = id();
    if (owner !== videoId) { owner = videoId; segments = []; nativeOwner = null; lastSkip = ''; lastLayout = ''; }
    const root = document.documentElement;
    if (root) {
      root.dataset.lingoplayAdblock = String(settings.adBlock);
      root.dataset.lingoplayNativePlayer = String(!!videoId && nativeOwner === videoId);
      if (!document.getElementById('lingoplay-ad-rules')) {
        const style = document.createElement('style'); style.id = 'lingoplay-ad-rules'; style.textContent = adStyle + '\nhtml[data-lingoplay-native-player="true"] #movie_player{opacity:0!important}'; root.appendChild(style);
      }
      for (const control of document.querySelectorAll('ytm-mobile-topbar-renderer :is(a,button,[role="button"]),ytm-mobile-topbar :is(a,button,[role="button"]),ytm-mobile-app-bar :is(a,button,[role="button"])')) {
        const label=(control.getAttribute('aria-label')||control.textContent||'').trim().toLowerCase();
        const app=['open app','open in app','open in the app','打开应用','在应用中打开','開啟應用程式'].includes(label);
        if(app) control.setAttribute('data-lingoplay-app-promo',''); else control.removeAttribute('data-lingoplay-app-promo');
      }
    }
    const p = player();
    const video = p?.querySelector('video');
    reportLayout();
    if (!p || !video) return;
    if (videoId && Date.now()-lastReadyAt>1000 && p.getPlayerResponse?.()?.videoDetails?.videoId===videoId) {
      lastReadyAt=Date.now();post({type:'mediaReady',videoId});
    }
    if (nativeOwner === videoId) { if (!video.paused) { p.pauseVideo?.(); video.pause(); } return; }
    const adPlayer = document.querySelector('.ad-showing,.ad-interrupting');
    const advertising = !!adPlayer;
    if (settings.adBlock && advertising) {
      document.querySelector('.ytp-ad-skip-button,.ytp-skip-ad-button,.ytp-ad-skip-button-modern')?.click();
      const advert=adPlayer.querySelector('video')||video;
      if (Number.isFinite(advert.duration) && advert.duration > 0 && advert.currentTime < advert.duration - 0.05) advert.currentTime = advert.duration;
      return;
    }
    if (settings.sponsorBlock && !advertising && !video.paused) {
      const segment = segments.find(s => settings.categories.includes(s.category) && video.currentTime >= s.start && video.currentTime < s.end - 0.05);
      if (segment) {
        const key = videoId+':'+segment.start+':'+segment.end;
        if (key === lastSkip && Date.now()-lastSkipAt < 1500) return;
        lastSkip=key; lastSkipAt=Date.now();
        if (p.seekTo) p.seekTo(segment.end,true); else video.currentTime=segment.end;
        post({type:'sponsorSkipped',videoId,start:segment.start,end:segment.end,category:segment.category});
      }
    }
  }
  window.__lingoplayMedia = {
    configure(value) { settings = {...settings,...value}; tick(); },
    sponsors(videoId,value) { if (videoId===id()) segments=value; },
    native(videoId,enabled) {
      if (videoId!==id()) return;
      nativeOwner=enabled?videoId:null; lastLayout='';
      if(enabled) { player()?.pauseVideo?.(); player()?.querySelector('video')?.pause(); }
      tick();
    },
    async resolve(requestId) {
      const videoId=id();
      if (!videoId) return;
      try {
        let media;
        for(let attempt=0;attempt<30;attempt++) {
          if(id()!==videoId) return;
          const p=player();
          const response=p?.getPlayerResponse?.() || window.ytInitialPlayerResponse;
          if(response?.videoDetails?.videoId===videoId) {
            let streaming;
            try { streaming=p?.getStreamingData?.(); if(typeof streaming==='string') streaming=JSON.parse(streaming); } catch {}
            streaming=streaming?.streamingData || streaming || response.streamingData || {};
            const details=response.videoDetails;
            const fallback=window.__lingoplayMediaInfo ? await window.__lingoplayMediaInfo(videoId).catch(()=>null) : null;
            const rawFormats=[...(response.streamingData?.formats||[]),...(response.streamingData?.adaptiveFormats||[]),...(streaming.formats||[]),...(streaming.adaptiveFormats||[]),...(fallback?.formats||[]),...(fallback?.adaptiveFormats||[])];
            const byItag=new Map();
            for(const format of rawFormats) {
              const previous=byItag.get(format.itag);
              const usable=!!(format.url||format.signatureCipher||format.cipher);
              const previousUsable=!!(previous?.url||previous?.signatureCipher||previous?.cipher);
              if(!previous || (usable&&!previousUsable) || (usable===previousUsable && format.audioTrack?.audioIsDefault && !previous.audioTrack?.audioIsDefault)) byItag.set(format.itag,format);
            }
            const unique=[...byItag.values()];
            if(!window.__lingoplayDecode) throw Error('The media decoder could not initialize. Reload the video and try again.');
            // Only decoded URLs can cross into native memory. Never substitute
            // captured player requests, which can belong to prefetched adverts.
            const formats=await window.__lingoplayDecode(unique,videoId,requestId);
            media={title:details.title,author:details.author,duration:Number(details.lengthSeconds)||0,position:Number(p?.getCurrentTime?.())||0,live:!!details.isLiveContent,formats,hlsManifestUrl:fallback?.hlsManifestUrl||streaming.hlsManifestUrl};
            if(formats.some(f=>f.url) || media.hlsManifestUrl) break;
          }
          if(attempt===0 && p && !nativeOwner) p.playVideo?.();
          await new Promise(r=>setTimeout(r,250));
        }
        if(id()!==videoId) return;
        if(!media) throw Error('YouTube is still loading. Play the video and try again.');
        post({type:'media',requestId,videoId,ok:true,media});
      } catch(error) { if(id()===videoId) post({type:'media',requestId,videoId,ok:false,error:error.message||'Media unavailable.'}); }
    }
  };
  setInterval(tick,250);
  window.addEventListener('scroll',scheduleLayout,{capture:true,passive:true});
  window.addEventListener('resize',scheduleLayout,{passive:true});
  window.visualViewport?.addEventListener('resize',scheduleLayout,{passive:true});
  window.visualViewport?.addEventListener('scroll',scheduleLayout,{passive:true});
  document.addEventListener('yt-navigate-finish',tick);
  tick();
})();
true;
`;

export const mediaSettingsScript = settings => `${MEDIA_BRIDGE}\nwindow.__lingoplayMedia?.configure(${JSON.stringify(settings)}); true;`;
export const mediaResolveScript = requestId => `${PLAYER_DECODER}\n${MEDIA_BRIDGE}\nwindow.__lingoplayMedia?.resolve(${JSON.stringify(requestId)}); true;`;
export const mediaNativeScript = (videoId, enabled) => `window.__lingoplayMedia?.native(${JSON.stringify(videoId)},${JSON.stringify(enabled)}); true;`;
export const mediaSponsorsScript = (videoId, segments) => `window.__lingoplayMedia?.sponsors(${JSON.stringify(videoId)},${JSON.stringify(segments)}); true;`;
