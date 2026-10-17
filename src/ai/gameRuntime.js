// The outer document keeps the native session private. Generated code runs in
// an opaque-origin iframe; native requests must still pass host correlation.
export function scriptJSON(value) {
  return JSON.stringify(value).replace(/</g, '\\u003c').replace(/\u2028/g, '\\u2028').replace(/\u2029/g, '\\u2029');
}

export function gameDocument({ definition, state, cues, session, colors }) {
  const theme = `:root{color-scheme:${colors.dark ? 'dark' : 'light'};--background:${colors.background};--ink:${colors.ink};--muted:${colors.muted};--accent:${colors.accent};--raised:${colors.raised}}`;
  const bootstrap = `(() => {
    const spec = ${scriptJSON({ html: definition.html, css: definition.css, script: definition.script, state, cues })};
    let state = spec.state, sequence = 0;
    const pending = new Map(), listeners = new Set();
    const request = (method, values) => new Promise((resolve, reject) => {
      if (pending.size >= 8) return reject(Error('The game is busy. Please wait.'));
      const id = String(++sequence);
      const timer = setTimeout(() => { pending.delete(id); reject(Error('The game request timed out. Try again.')); }, 90000);
      pending.set(id, { resolve, reject, timer });
      parent.postMessage({ v: 1, id, method, ...values }, '*');
    });
    addEventListener('message', event => {
      if (event.source !== parent || event.data?.v !== 1) return;
      const reply = event.data;
      if (reply.state && typeof reply.state === 'object') {
        state = reply.state;
        for (const listener of listeners) { try { listener(state); } catch {} }
      }
      const job = pending.get(reply.id);
      if (!job) return;
      pending.delete(reply.id); clearTimeout(job.timer);
      if (reply.error) job.reject(Error(reply.error)); else job.resolve(reply.state ?? null);
    });
    Object.defineProperty(window, 'game', { value: Object.freeze({
      cues: Object.freeze(spec.cues),
      dispatch: (action, input = {}) => request('action', { action, input }),
      playAudio: ({ cueId }) => request('audio', { cueId }),
      onState: listener => { listeners.add(listener); listener(state); return () => listeners.delete(listener); }
    }) });
    // WebView implementations may inject messaging objects into child frames.
    // Hide those too; the host also rejects messages without its private session.
    for (const name of ['ReactNativeWebView', 'webkit', 'RTCPeerConnection', 'webkitRTCPeerConnection', 'Worker', 'SharedWorker']) {
      try { Object.defineProperty(window, name, { value: undefined, writable: false, configurable: false }); } catch {}
    }
    addEventListener('click', event => { if (event.target.closest?.('a')) event.preventDefault(); }, true);
    addEventListener('submit', event => event.preventDefault(), true);
    addEventListener('error', () => parent.postMessage({ v: 1, id: 'error', method: 'error' }, '*'));
    const style = document.createElement('style'); style.textContent = spec.css; document.head.append(style);
    document.body.innerHTML = spec.html;
    const script = document.createElement('script'); script.textContent = spec.script; document.body.append(script);
    parent.postMessage({ v: 1, id: 'ready', method: 'ready' }, '*');
  })();`;
  const inner = `<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data:; connect-src 'none'; media-src 'none'; frame-src 'none'; worker-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'"><style>${theme}*{box-sizing:border-box}body{margin:0;padding:16px;background:var(--background);color:var(--ink);font:16px system-ui,sans-serif;overflow-wrap:anywhere}button,input,select{font:inherit}button{min-height:44px;cursor:pointer}button:focus-visible,input:focus-visible{outline:2px solid var(--accent);outline-offset:3px}@media(prefers-reduced-motion:reduce){*,*::before,*::after{animation:none!important;transition:none!important}}</style></head><body><script>${bootstrap.replace(/<\/script/gi, '<\\/script')}</script></body></html>`;
  return `<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; frame-src about:; connect-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'"><style>html,body{margin:0;height:100%;overflow:hidden;background:${colors.background}}iframe{border:0;width:100%;height:100%;display:block}</style></head><body><iframe id="surface" title="Interactive learning game" sandbox="allow-scripts" referrerpolicy="no-referrer"></iframe><script>(()=>{
    const frame=document.getElementById('surface');
    const session=${scriptJSON(session)};
    addEventListener('message',event=>{
      if(event.source!==frame.contentWindow||event.origin!=='null')return;
      const message=event.data;
      if(!message||message.v!==1||typeof message.id!=='string'||message.id.length>80||!['ready','error','action','audio'].includes(message.method))return;
      const raw=JSON.stringify({session,request:message});
      if(raw.length<=10000)window.ReactNativeWebView.postMessage(raw);
    });
    window.__lingoGameReply=reply=>frame.contentWindow.postMessage({v:1,...reply},'*');
    frame.srcdoc=${scriptJSON(inner)};
  })();</script></body></html>`;
}
