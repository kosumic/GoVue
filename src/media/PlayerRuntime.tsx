import { forwardRef, useEffect, useImperativeHandle, useRef, useState } from 'react';
import { Platform, StyleSheet, View } from 'react-native';
import WebView from 'react-native-webview';
import { useWebViewRenderer } from '../browser/useWebViewRenderer';

export type PlayerRuntimeHandle = { evaluate: (code: string) => Promise<Record<string, string>> };
type Job = { code: string | null; resolve: (value: Record<string, string>) => void; reject: (error: Error) => void; timer: ReturnType<typeof setTimeout> };
// This renderer has no YouTube cookies, page CSP, native APIs, or network access.
const runtimeSource = (generation: number) => ({ html: `<!doctype html><meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline' 'unsafe-eval'; connect-src 'none'"><title>Player script runtime</title><script>
window.runPlayer = (id, code) => {
  try {
    const result = Function(code)();
    const values = {};
    for (const key of ['n', 'sig']) if (typeof result?.[key] === 'string' && result[key].length <= 10000) values[key] = result[key];
    window.ReactNativeWebView.postMessage(JSON.stringify({generation:${generation}, id, ok:true, values}));
  } catch { window.ReactNativeWebView.postMessage(JSON.stringify({generation:${generation}, id, ok:false})); }
};
window.ReactNativeWebView.postMessage(JSON.stringify({generation:${generation}, type:'ready'}));
</script>` });

export const PlayerRuntime = forwardRef<PlayerRuntimeHandle>(function PlayerRuntime(_, ref) {
  const renderer = useWebViewRenderer();
  const view = renderer.view;
  const ready = useRef(false);
  const needsRecovery = useRef(false);
  const generation = useRef(0);
  const [document, setDocument] = useState(() => ({ generation: 0, source: runtimeSource(0) }));
  const isCurrent = () => renderer.isCurrent() && document.generation === generation.current;
  const jobs = useRef(new Map<string, Job>());
  const sequence = useRef(0);
  function failAll() {
    for (const job of jobs.current.values()) { clearTimeout(job.timer); job.reject(Error('The player script runtime stopped. Reload the video and try again.')); }
    jobs.current.clear();
  }
  function terminate() {
    if (!isCurrent() || needsRecovery.current) return;
    ready.current = false; needsRecovery.current = true;
    generation.current++;
    if (Platform.OS === 'android') renderer.retire();
    failAll();
  }
  function dispatch(id: string, job: Job) {
    if (job.code === null || !ready.current || !isCurrent() || !view.current) return;
    view.current.injectJavaScript(`window.runPlayer(${JSON.stringify(id)},${JSON.stringify(job.code)}); true;`);
    job.code = null;
  }
  useEffect(() => failAll, []);
  useImperativeHandle(ref, () => ({
    evaluate(code) {
      if (code.length > 2_000_000 || jobs.current.size >= 4) return Promise.reject(Error('Player script is too large or busy.'));
      return new Promise((resolve, reject) => {
        const id = String(++sequence.current);
        const timer = setTimeout(() => { jobs.current.delete(id); reject(Error('Player script timed out.')); }, 10000);
        const job = { code, resolve, reject, timer };
        jobs.current.set(id, job);
        if (needsRecovery.current) {
          needsRecovery.current = false;
          // Replacing only the HTML reloads the isolated document on iOS while
          // retaining WKWebView. Its epoch rejects messages queued before death.
          setDocument({ generation: generation.current, source: runtimeSource(generation.current) });
          if (Platform.OS === 'android') renderer.recreate();
        } else dispatch(id, job);
      });
    },
  }));
  return <View pointerEvents="none" accessibilityElementsHidden importantForAccessibility="no-hide-descendants" style={styles.runtime}>
    {renderer.mounted && <WebView key={renderer.generation} ref={renderer.attach} source={document.source} originWhitelist={['about:blank']} javaScriptEnabled cacheEnabled={false}
      webviewDebuggingEnabled={process.env.EXPO_PUBLIC_LINGOPLAY_DEBUG === '1'}
      domStorageEnabled={false} sharedCookiesEnabled={false} thirdPartyCookiesEnabled={false}
      allowFileAccess={false} allowFileAccessFromFileURLs={false} allowUniversalAccessFromFileURLs={false}
      javaScriptCanOpenWindowsAutomatically={false} setSupportMultipleWindows={false}
      onShouldStartLoadWithRequest={request => isCurrent() && request.url === 'about:blank'}
      onOpenWindow={() => {}} onRenderProcessGone={terminate} onContentProcessDidTerminate={terminate} onError={terminate}
      onMessage={event => {
        if (!isCurrent() || needsRecovery.current) return;
        if (event.nativeEvent.data.length > 30000) return;
        try {
          const reply = JSON.parse(event.nativeEvent.data);
          if (reply?.generation !== generation.current) return;
          if (reply.type === 'ready') {
            ready.current = true;
            for (const [id, job] of jobs.current) dispatch(id, job);
            return;
          }
          const job = jobs.current.get(reply.id);
          if (!job) return;
          clearTimeout(job.timer); jobs.current.delete(reply.id);
          if (reply.ok !== true || !reply.values || typeof reply.values !== 'object') { job.reject(Error('Could not solve the YouTube player signature.')); return; }
          const values: Record<string, string> = {};
          for (const key of ['n', 'sig']) if (typeof reply.values[key] === 'string' && reply.values[key].length <= 10000) values[key] = reply.values[key];
          job.resolve(values);
        } catch {}
      }} />}
  </View>;
});
const styles = StyleSheet.create({ runtime: { position: 'absolute', width: 1, height: 1, opacity: 0, overflow: 'hidden' } });
