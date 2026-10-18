import { useEffect, useState } from 'react';
import { Platform } from 'react-native';
import { coreRequest, getBrowserUserAgent } from '../../modules/govue-core';

// LiTube removes WebView-specific UA tokens before the first request. Keeping
// one identity for the document also keeps player extraction tied to that UA.
export function useBrowserIdentity() {
  const [identity, setIdentity] = useState<{
    ready: boolean;
    userAgent?: string;
  }>({ ready: Platform.OS !== 'android' });
  useEffect(() => {
    if (Platform.OS !== 'android') return;
    let active = true;
    void getBrowserUserAgent()
      .then((agent) =>
        agent
          ? coreRequest<string>({ op: 'browser_user_agent', agent })
          : undefined,
      )
      .then((userAgent) => {
        if (active) setIdentity({ ready: true, userAgent });
      })
      .catch(() => {
        if (active) setIdentity({ ready: true });
      });
    return () => {
      active = false;
    };
  }, []);
  return identity;
}
