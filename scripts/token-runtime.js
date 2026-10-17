import { BotGuardClient } from 'bgutils-js/botguard';
import { WebPoMinter } from 'bgutils-js/webpo';

// Network and native APIs are unavailable in this dedicated renderer. Rust
// fetches challenge bytes and owns each job; this adapter only executes them.
const signals = new Map();
const release = requestId => {
  const state = signals.get(requestId);
  signals.delete(requestId);
  if (state) void state.client.shutdown().catch(() => {});
};
window.runExtractionToken = async (requestId, task) => {
  let value;
  try {
    if (task.stage === 'snapshot') {
      const script = document.createElement('script');
      script.textContent = task.interpreter;
      document.head.appendChild(script);
      script.remove();
      const client = await BotGuardClient.create({ program: task.program, globalName: task.globalName, globalObject: window });
      const output = [];
      signals.set(requestId, { output, client });
      value = await client.snapshot({ webPoSignalOutput: output });
    } else if (task.stage === 'mint') {
      const state = signals.get(requestId);
      if (!state) throw Error();
      const minter = await WebPoMinter.create(task, state.output);
      value = await minter.mintAsWebsafeString(task.videoId);
      release(requestId);
    } else throw Error();
    if (typeof value !== 'string' || !value || value.length > 2_000_000) throw Error();
    window.ReactNativeWebView.postMessage(JSON.stringify({ requestId, taskId: task.taskId, value }));
  } catch {
    release(requestId);
    window.ReactNativeWebView.postMessage(JSON.stringify({ requestId, taskId: task.taskId, error: true }));
  }
};
window.releaseExtractionToken = release;
window.ReactNativeWebView.postMessage(JSON.stringify({ ready: true }));
