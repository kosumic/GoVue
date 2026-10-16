import { useRef, useState } from 'react';
import type WebView from 'react-native-webview';

// Native renderer lifetime belongs to the view. Retire synchronously so queued
// events and ref cleanups from an old instance cannot reach its replacement.
export function useWebViewRenderer() {
  const view = useRef<WebView | null>(null);
  const [renderer, setRenderer] = useState({ generation: 0, mounted: true });
  const current = useRef(renderer);
  const isCurrent = () => current.current === renderer && renderer.mounted;
  function replace(mounted: boolean) {
    const next = { generation: current.current.generation + 1, mounted };
    view.current = null;
    current.current = next;
    setRenderer(next);
  }
  return {
    view,
    ...renderer,
    isCurrent,
    attach: (handle: WebView | null) => { if (isCurrent()) view.current = handle; },
    retire: () => { if (isCurrent()) replace(false); },
    recreate: () => replace(true),
  };
}
