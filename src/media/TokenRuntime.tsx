import { useEffect, useRef, useState } from 'react';
import { Platform, StyleSheet, View } from 'react-native';
import WebView from 'react-native-webview';
import { useWebViewRenderer } from '../browser/useWebViewRenderer';
import { registerTokenRuntime, type TokenTask } from '../core/extraction';
import { TOKEN_RUNTIME } from '../youtube/generated/tokenRuntime';

type Job = { requestId: string; task: TokenTask; sent: boolean;
  resolve: (value: string) => void; reject: (error: Error) => void;
  timer: ReturnType<typeof setTimeout> };
const source = { baseUrl: 'https://www.youtube.com', html: `<!doctype html><meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline' 'unsafe-eval'; connect-src 'none'"><script>Object.defineProperty(Document.prototype,'cookie',{get:()=>'',set:()=>{},configurable:false});${TOKEN_RUNTIME.replace(/<\/script/gi, '<\\/script')}</script>` };

// A local, network-disabled renderer executes BotGuard only. It never loads
// YouTube, receives cookies/signed URLs, or evaluates player signature code.
export function TokenRuntime() {
  const renderer = useWebViewRenderer();
  const current = useRef(renderer);
  current.current = renderer;
  const ready = useRef(false);
  const initialLoad = useRef(true);
  const jobs = useRef(new Map<string, Job>());
  const [generation, setGeneration] = useState(0);
  const key = (requestId: string, taskId: number) => `${requestId}/${taskId}`;
  const dispatch = (job: Job) => {
    if (job.sent || !ready.current || !current.current.view.current) return;
    job.sent = true;
    current.current.view.current.injectJavaScript(
      `window.runExtractionToken(${JSON.stringify(job.requestId)},${JSON.stringify(job.task)});true;`,
    );
  };
  const fail = () => {
    ready.current = false;
    for (const job of jobs.current.values()) {
      clearTimeout(job.timer);
      job.reject(Error('YouTube verification runtime stopped.'));
    }
    jobs.current.clear();
  };
  const restart = () => {
    fail();
    initialLoad.current = true;
    if (Platform.OS === 'android') current.current.retire();
    setGeneration(value => value + 1);
  };
  const recover = () => { if (renderer.isCurrent()) restart(); };
  useEffect(() => {
    if (!current.current.mounted) current.current.recreate();
  }, [generation]);
  useEffect(() => {
    const unregister = registerTokenRuntime({
      evaluate(requestId, task) {
        return new Promise((resolve, reject) => {
          const identity = key(requestId, task.taskId);
          const job: Job = { requestId, task, sent: false, resolve, reject,
            timer: setTimeout(() => { jobs.current.delete(identity); reject(Error('YouTube verification timed out.')); restart(); }, 20000) };
          jobs.current.set(identity, job);
          dispatch(job);
        });
      },
      release(requestId) {
        for (const [identity, job] of jobs.current) {
          if (job.requestId !== requestId) continue;
          jobs.current.delete(identity); clearTimeout(job.timer);
          job.reject(Object.assign(Error('Extraction cancelled.'), { name: 'AbortError' }));
        }
        if (ready.current) current.current.view.current?.injectJavaScript(
          `window.releaseExtractionToken(${JSON.stringify(requestId)});true;`,
        );
      },
    });
    return () => { unregister(); fail(); };
  }, []);
  return <View pointerEvents="none" accessibilityElementsHidden importantForAccessibility="no-hide-descendants" style={styles.hidden}>
    {renderer.mounted && <WebView key={`${renderer.generation}:${generation}`} ref={renderer.attach} source={source}
      javaScriptEnabled domStorageEnabled={false} cacheEnabled={false} sharedCookiesEnabled={false}
      thirdPartyCookiesEnabled={false} allowFileAccess={false} allowFileAccessFromFileURLs={false}
      allowUniversalAccessFromFileURLs={false} mixedContentMode="never" setSupportMultipleWindows={false}
      onShouldStartLoadWithRequest={request => {
        if (request.url === 'about:blank') return true;
        // WKWebView reports loadHTMLString's base URL for its initial local
        // document. Permit that one load, then reject every HTTPS navigation.
        if (Platform.OS === 'ios' && initialLoad.current && request.isTopFrame &&
          request.navigationType === 'other' && request.url === `${source.baseUrl}/`) {
          initialLoad.current = false;
          return true;
        }
        return false;
      }}
      onRenderProcessGone={recover} onContentProcessDidTerminate={recover}
      onMessage={event => {
        if (!renderer.isCurrent() || event.nativeEvent.data.length > 2_100_000) return;
        try {
          const reply = JSON.parse(event.nativeEvent.data);
          if (reply.ready === true) { initialLoad.current = false; ready.current = true; for (const job of jobs.current.values()) dispatch(job); return; }
          const identity = key(reply.requestId, reply.taskId);
          const job = jobs.current.get(identity);
          if (!job) return;
          jobs.current.delete(identity); clearTimeout(job.timer);
          if (reply.error || typeof reply.value !== 'string' || !reply.value || reply.value.length > 2_000_000)
            job.reject(Error('YouTube verification failed.'));
          else job.resolve(reply.value);
        } catch {}
      }} />}
  </View>;
}
const styles = StyleSheet.create({ hidden: { position: 'absolute', width: 1, height: 1, opacity: 0, overflow: 'hidden' } });
