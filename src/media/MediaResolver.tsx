import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
} from 'react';
import { StyleSheet, useWindowDimensions, View } from 'react-native';
import WebView, { type WebViewMessageEvent } from 'react-native-webview';
import { coreRequest } from '../../modules/lingoplay-core';
import { isYouTubeUrl, youtubeVideoId } from '../browser/address';
import {
  acceptMedia,
  defaultMediaSettings,
  type MediaCatalog,
} from '../core/media';
import type { DownloadConfig, DownloadPlan } from '../core/downloads';
import { acceptTranscript } from '../core/transcripts';
import { PLAYER_DECODER } from '../youtube/generated/playerDecoder';
import { YOUTUBE_BRIDGE, extractionScript } from '../youtube/pageBridge';
import { mediaSettingsScript, mediaNativeScript } from '../youtube/mediaBridge';
import type { PlayerRuntimeHandle } from './PlayerRuntime';
import { useBrowserIdentity } from '../browser/useBrowserIdentity';

type Resolved = { catalog: MediaCatalog; srt?: string };
export type MediaResolverHandle = {
  resolve: (
    videoId: string,
    config?: DownloadConfig,
    signal?: AbortSignal,
  ) => Promise<Resolved>;
};
type Job = {
  id: string;
  requestId: string;
  document: number;
  videoId: string;
  config?: DownloadConfig;
  resolve: (value: Resolved) => void;
  reject: (error: Error) => void;
  timer?: ReturnType<typeof setTimeout>;
  started: boolean;
  processing?: boolean;
  catalog?: MediaCatalog;
  captionId?: string;
  cleanup?: () => void;
};
// A separate page resolves a card or playlist without changing the retained watch tab.
// It shares the browser's login session; signature evaluation stays in PlayerRuntime.
// Allow YouTube's muted startup to initialize its player/session before pausing.
// Replacing play() with a no-op can leave the controller waiting forever.
const silence = `(()=>{if(window.__lingoplayResolverMuted)return;window.__lingoplayResolverMuted=true;const play=HTMLMediaElement.prototype.play;HTMLMediaElement.prototype.play=function(...args){this.muted=true;return Reflect.apply(play,this,args);};setInterval(()=>document.querySelectorAll('video,audio').forEach(v=>{v.muted=true;}),250);})();true;`;
export const MediaResolver = forwardRef<
  MediaResolverHandle,
  { runtime: React.RefObject<PlayerRuntimeHandle | null> }
>(({ runtime }, ref) => {
  const browserIdentity = useBrowserIdentity();
  const viewport = useWindowDimensions();
  const view = useRef<WebView | null>(null);
  const [active, setActive] = useState<Job | null>(null);
  const current = useRef<Job | null>(null);
  const jobs = useRef<Job[]>([]);
  const sequence = useRef(0);
  const mounted = useRef(true);
  function pump() {
    if (current.current || !mounted.current) return;
    const job = jobs.current.shift();
    if (job) {
      current.current = job;
      job.timer = setTimeout(
        () =>
          finish(
            job,
            undefined,
            Error(
              job.started
                ? 'YouTube could not prepare this download. Open the video and try again.'
                : 'YouTube did not finish loading. Try again.',
            ),
          ),
        120000,
      );
      setActive(job);
    }
  }
  function finish(job: Job, result?: Resolved, error?: Error) {
    if (current.current !== job) return;
    view.current?.injectJavaScript(
      `window.__lingoplayMedia?.cancel(${JSON.stringify(job.requestId)});true;`,
    );
    clearTimeout(job.timer);
    job.cleanup?.();
    current.current = null;
    setActive(null);
    if (error) job.reject(error);
    else if (result) job.resolve(result);
    queueMicrotask(pump);
  }
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      for (const job of [current.current, ...jobs.current])
        if (job) {
          clearTimeout(job.timer);
          job.cleanup?.();
          job.reject(Error('Media resolver closed.'));
        }
      jobs.current = [];
      current.current = null;
    };
  }, []);
  useImperativeHandle(ref, () => ({
    async resolve(videoId, config, signal) {
      if (signal?.aborted)
        return Promise.reject(Error('Media request cancelled.'));
      if (!/^[\w-]{11}$/.test(videoId))
        return Promise.reject(Error('Invalid video identity.'));
      if (!config?.includeSubtitle) {
        const catalog = await coreRequest<MediaCatalog | null>({
          op: 'media_catalog_cached',
          videoId,
        });
        if (signal?.aborted) throw Error('Media request cancelled.');
        if (catalog) return { catalog };
      }
      if (jobs.current.length >= 500)
        return Promise.reject(Error('Too many pending media requests.'));
      return new Promise((resolve, reject) => {
        const id = `download:${Date.now()}:${++sequence.current}`;
        const job: Job = {
          id,
          requestId: id,
          document: 0,
          videoId,
          config,
          resolve,
          reject,
          started: false,
        };
        const abort = () => {
          if (current.current === job)
            finish(job, undefined, Error('Media request cancelled.'));
          else {
            jobs.current = jobs.current.filter((j) => j !== job);
            job.cleanup?.();
            job.reject(Error('Media request cancelled.'));
          }
        };
        signal?.addEventListener('abort', abort, { once: true });
        job.cleanup = () => signal?.removeEventListener('abort', abort);
        jobs.current.push(job);
        pump();
      });
    },
  }));
  function ready(url?: string, finalDocument = false) {
    const job = current.current;
    if (!job || (job.started && !finalDocument) || !view.current) return;
    if (url && youtubeVideoId(url) !== job.videoId) return;
    job.started = true;
    const requestId = JSON.stringify(job.requestId);
    view.current?.injectJavaScript(
      `if(window.__lingoplayDownloadRequest!==${requestId}){let stage='decoder';try{\n` +
        silence +
        '\n' +
        PLAYER_DECODER +
        "\nstage='player';\n" +
        mediaSettingsScript(defaultMediaSettings) +
        `\nstage='request';window.__lingoplayDownloadRequest=${requestId};window.__lingoplayMedia.resolve(${requestId});\n}catch{console.warn('LingoPlay download bootstrap',stage);window.ReactNativeWebView?.postMessage(JSON.stringify({type:'media',requestId:${requestId},videoId:${JSON.stringify(job.videoId)},ok:false,error:'The download page could not initialize. Open the video and try again.'}));}}true;`,
    );
  }
  async function onMessage(event: WebViewMessageEvent) {
    const job = current.current;
    const { data: raw, url: origin } = event.nativeEvent;
    if (!job || !isYouTubeUrl(origin) || raw.length > 8_000_000) return;
    let message: Record<string, unknown>;
    try {
      message = JSON.parse(raw);
    } catch {
      return;
    }
    if (!message || message.videoId !== job.videoId) return;
    const requestId = job.requestId;
    const isCurrent = () =>
      current.current === job && job.requestId === requestId;
    // Android's WebMessageListener supplies sourceOrigin; the legacy Android
    // bridge and iOS may supply a full document URL. Rust validates both forms.
    const trusted = await coreRequest<boolean>({
      op: 'media_message_origin',
      origin,
      videoId: job.videoId,
    }).catch(() => false);
    if (!trusted || !isCurrent()) return;
    if (message.type === 'mediaReady') {
      ready();
      return;
    }
    if (message.requestId !== requestId && message.requestId !== job.captionId)
      return;
    try {
      if (message.type === 'mediaEval' && message.requestId === requestId) {
        if (
          typeof message.code !== 'string' ||
          message.code.length > 2_000_000 ||
          typeof message.evalId !== 'string' ||
          !/^\d{1,12}$/.test(message.evalId)
        )
          return;
        let values: Record<string, string> | null = null;
        try {
          values = (await runtime.current?.evaluate(message.code)) ?? null;
        } catch {}
        if (isCurrent())
          view.current?.injectJavaScript(
            `window.__lingoplayEvalReply?.(${JSON.stringify(message.evalId)},${JSON.stringify(values)});true;`,
          );
      } else if (message.type === 'media' && message.requestId === requestId) {
        if (job.processing) return;
        job.processing = true;
        const catalog = await acceptMedia(raw, origin, requestId, job.videoId);
        if (!isCurrent()) return;
        job.catalog = catalog;
        view.current?.injectJavaScript(mediaNativeScript(job.videoId, true));
        if (job.config?.includeSubtitle) {
          const plan = await coreRequest<DownloadPlan>({
            op: 'downloads_plan',
            catalog,
            config: job.config,
          });
          if (!isCurrent()) return;
          if (!plan.subtitle)
            throw Error('The selected subtitles are unavailable.');
          job.captionId = requestId + ':captions';
          view.current?.injectJavaScript(
            extractionScript(job.captionId, plan.subtitle.id, false),
          );
        } else finish(job, { catalog });
      } else if (
        message.type === 'transcript' &&
        message.requestId === job.captionId &&
        job.catalog
      ) {
        const result = await acceptTranscript(
          raw,
          origin,
          job.captionId!,
          job.videoId,
        );
        if (!result.ok) throw Error(result.error);
        const srt = await coreRequest<string>({
          op: 'downloads_subtitles',
          videoId: job.videoId,
          trackId: result.transcript.trackId,
        });
        if (isCurrent()) finish(job, { catalog: job.catalog, srt });
      }
    } catch (error) {
      if (!isCurrent()) return;
      finish(
        job,
        undefined,
        error instanceof Error ? error : Error('Media could not be resolved.'),
      );
    }
  }
  const source = useMemo(
    () => ({ uri: `https://m.youtube.com/watch?v=${active?.videoId || ''}` }),
    [active?.id],
  );
  return (
    <View
      pointerEvents="none"
      accessibilityElementsHidden
      importantForAccessibility="no-hide-descendants"
      style={[
        styles.hidden,
        { width: viewport.width, height: viewport.height },
      ]}
    >
      {active && browserIdentity.ready && (
        <WebView
          key={active.id}
          ref={view}
          style={styles.page}
          source={source}
          userAgent={browserIdentity.userAgent}
          javaScriptEnabled
          domStorageEnabled
          sharedCookiesEnabled
          allowsInlineMediaPlayback
          mediaPlaybackRequiresUserAction
          allowFileAccess={false}
          allowFileAccessFromFileURLs={false}
          allowUniversalAccessFromFileURLs={false}
          mixedContentMode="never"
          setSupportMultipleWindows={false}
          injectedJavaScriptBeforeContentLoaded={silence}
          injectedJavaScript={
            silence +
            '\n' +
            PLAYER_DECODER +
            '\n' +
            YOUTUBE_BRIDGE +
            '\n' +
            mediaSettingsScript(defaultMediaSettings)
          }
          onShouldStartLoadWithRequest={(request) => isYouTubeUrl(request.url)}
          onMessage={(event) => {
            if (current.current === active) void onMessage(event);
          }}
          onLoadStart={() => {
            if (current.current !== active) return;
            active.requestId = `${active.id}:page:${++active.document}`;
            active.started = false;
            active.processing = false;
            active.catalog = undefined;
            active.captionId = undefined;
          }}
          onLoadProgress={(event) => {
            if (current.current === active && event.nativeEvent.progress >= 0.5)
              ready(event.nativeEvent.url);
          }}
          onLoadEnd={(event) => {
            if (current.current === active) {
              if (youtubeVideoId(event.nativeEvent.url) !== active.videoId)
                finish(
                  active,
                  undefined,
                  Error(
                    'YouTube redirected this video. Open it in the browser and try again.',
                  ),
                );
              else ready(event.nativeEvent.url, true);
            }
          }}
          onRenderProcessGone={() => {
            if (current.current === active)
              finish(
                active,
                undefined,
                Error('The download browser stopped. Try again.'),
              );
          }}
          onContentProcessDidTerminate={() => {
            if (current.current === active)
              finish(
                active,
                undefined,
                Error('The download browser stopped. Try again.'),
              );
          }}
          onError={() => {
            if (current.current === active)
              finish(
                active,
                undefined,
                Error('YouTube could not load this video.'),
              );
          }}
        />
      )}
    </View>
  );
});
const styles = StyleSheet.create({
  hidden: {
    position: 'absolute',
    top: 0,
    left: 0,
    opacity: 0,
    overflow: 'hidden',
  },
  page: { flex: 1 },
});
