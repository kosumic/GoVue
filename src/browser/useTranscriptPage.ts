import { useEffect, useRef } from 'react';
import type { WebViewMessageEvent } from 'react-native-webview';
import { coreRequest, observeCoreUpdates } from '../../modules/govue-core';
import { CAPTION_BRIDGE } from '../youtube/generated/captionBridge';
import { youtubeVideoId } from './address';

type Task = { taskId: number; adapterId: string; videoId: string; trackId: string };
let sequence = 0;

// This adapter only moves bounded page bytes. Rust owns the extraction job.
export function useTranscriptPage(active: boolean, url: string, inject: (code: string) => void) {
  const injectRef = useRef(inject);
  injectRef.current = inject;
  const pending = useRef(new Map<string, Task>());
  const documentReady = useRef<() => void>(() => {});
  const receive = useRef<(event: WebViewMessageEvent) => boolean>(() => false);
  useEffect(() => {
    const videoId = active ? youtubeVideoId(url) : null;
    if (!videoId) return;
    const adapterId = `caption-page:${Date.now()}:${++sequence}`;
    let alive = true, polling = false, again = false;
    const deliver = (requestId: string, task: Task) => injectRef.current(`${CAPTION_BRIDGE}\nwindow.__govueCaptions?.request(${JSON.stringify(requestId)},${JSON.stringify(task)});true;`);
    const poll = async () => {
      if (!alive) return;
      if (polling) { again = true; return; }
      polling = true;
      try {
        const state = await coreRequest<{ requests: string[]; tasks: { requestId: string; task: Task }[] }>({op:'extract_page_tasks'});
        if (!alive) return;
        for (const [requestId] of pending.current) if (!state.requests.includes(requestId)) {
          pending.current.delete(requestId);
          injectRef.current(`window.__govueCaptions?.cancel(${JSON.stringify(requestId)});true;`);
        }
        for (const {requestId, task} of state.tasks) {
          if (task.adapterId !== adapterId || task.videoId !== videoId || pending.current.get(requestId)?.taskId === task.taskId) continue;
          pending.current.set(requestId, task);
          deliver(requestId, task);
        }
      } finally { polling = false; if (again) { again = false; void poll().catch(() => {}); } }
    };
    documentReady.current = () => {
      if (!alive) return;
      // A task injected into a loading document may be lost when it is replaced.
      // Re-delivery is idempotent in the page bridge.
      for (const [requestId, task] of pending.current) deliver(requestId, task);
      void poll().catch(() => {});
    };
    receive.current = event => {
      if (event.nativeEvent.data.length > 8_100_000) return false;
      let reply;
      try { reply = JSON.parse(event.nativeEvent.data); } catch { return false; }
      if (reply?.type !== 'captionPage') return false;
      const task = pending.current.get(reply.requestId);
      if (!alive || !task || reply.taskId !== task.taskId || reply.videoId !== videoId || reply.trackId !== task.trackId) return true;
      pending.current.delete(reply.requestId);
      void coreRequest({op:'extract_page',requestId:reply.requestId,origin:event.nativeEvent.url,raw:event.nativeEvent.data}).catch(() => {});
      return true;
    };
    const subscription = observeCoreUpdates('extraction-page', () => { void poll().catch(() => {}); });
    void coreRequest({op:'extract_page_register',adapterId,videoId}).then(poll).catch(() => {});
    return () => {
      alive = false;
      subscription?.remove();
      receive.current = () => false;
      documentReady.current = () => {};
      for (const [requestId] of pending.current) injectRef.current(`window.__govueCaptions?.cancel(${JSON.stringify(requestId)});true;`);
      pending.current.clear();
      void coreRequest({op:'extract_page_register',adapterId,videoId:null}).catch(() => {});
    };
  }, [active, url]);
  return { receive: (event: WebViewMessageEvent) => receive.current(event), ready: () => documentReady.current() };
}
