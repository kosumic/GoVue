import { forwardRef, useEffect, useImperativeHandle, useRef } from 'react';
import { StyleSheet, View } from 'react-native';
import WebView from 'react-native-webview';

export type PlayerRuntimeHandle = { evaluate: (code: string) => Promise<Record<string, string>> };
type Job = { resolve: (value: Record<string, string>) => void; reject: (error: Error) => void; timer: ReturnType<typeof setTimeout> };
// This renderer has no YouTube cookies, page CSP, native APIs, or network access.
const source = { html: `<!doctype html><meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline' 'unsafe-eval'; connect-src 'none'"><title>Player script runtime</title><script>
window.runPlayer = (id, code) => {
  try {
    const result = Function(code)();
    const values = {};
    for (const key of ['n', 'sig']) if (typeof result?.[key] === 'string' && result[key].length <= 10000) values[key] = result[key];
    window.ReactNativeWebView.postMessage(JSON.stringify({id, ok:true, values}));
  } catch { window.ReactNativeWebView.postMessage(JSON.stringify({id, ok:false})); }
};
</script>` };

export const PlayerRuntime = forwardRef<PlayerRuntimeHandle>(function PlayerRuntime(_, ref) {
  const view = useRef<WebView>(null);
  const jobs = useRef(new Map<string, Job>());
  const sequence = useRef(0);
  function failAll() {
    for (const job of jobs.current.values()) { clearTimeout(job.timer); job.reject(Error('The player script runtime stopped. Reload the video and try again.')); }
    jobs.current.clear();
  }
  useEffect(() => failAll, []);
  useImperativeHandle(ref, () => ({
    evaluate(code) {
      if (code.length > 2_000_000 || jobs.current.size >= 4) return Promise.reject(Error('Player script is too large or busy.'));
      return new Promise((resolve, reject) => {
        const id = String(++sequence.current);
        const timer = setTimeout(() => { jobs.current.delete(id); reject(Error('Player script timed out.')); }, 10000);
        jobs.current.set(id, { resolve, reject, timer });
        view.current?.injectJavaScript(`window.runPlayer(${JSON.stringify(id)},${JSON.stringify(code)}); true;`);
      });
    },
  }), []);
  return <View pointerEvents="none" accessibilityElementsHidden importantForAccessibility="no-hide-descendants" style={styles.runtime}>
    <WebView ref={view} source={source} originWhitelist={['about:blank']} javaScriptEnabled cacheEnabled={false}
      domStorageEnabled={false} sharedCookiesEnabled={false} thirdPartyCookiesEnabled={false}
      allowFileAccess={false} allowFileAccessFromFileURLs={false} allowUniversalAccessFromFileURLs={false}
      javaScriptCanOpenWindowsAutomatically={false} setSupportMultipleWindows={false}
      onShouldStartLoadWithRequest={request => request.url === 'about:blank'}
      onOpenWindow={() => {}} onRenderProcessGone={failAll} onContentProcessDidTerminate={failAll}
      onMessage={event => {
        if (event.nativeEvent.data.length > 30000) return;
        try {
          const reply = JSON.parse(event.nativeEvent.data);
          const job = jobs.current.get(reply.id);
          if (!job) return;
          clearTimeout(job.timer); jobs.current.delete(reply.id);
          if (reply.ok !== true || !reply.values || typeof reply.values !== 'object') { job.reject(Error('Could not solve the YouTube player signature.')); return; }
          const values: Record<string, string> = {};
          for (const key of ['n', 'sig']) if (typeof reply.values[key] === 'string' && reply.values[key].length <= 10000) values[key] = reply.values[key];
          job.resolve(values);
        } catch {}
      }} />
  </View>;
});
const styles = StyleSheet.create({ runtime: { position: 'absolute', width: 1, height: 1, opacity: 0, overflow: 'hidden' } });
