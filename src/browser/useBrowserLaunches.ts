import { useEffect, useRef } from 'react';
import { Linking, Platform } from 'react-native';
import {
  coreRequest,
  observeBrowserLaunches,
  takeBrowserLaunches,
} from '../../modules/govue-core';

// Platform adapters supply shared text; core selects the first routable link.
export function useBrowserLaunches(
  navigate: (url: string) => void,
  notice: (message: string) => void,
) {
  const callbacks = useRef({ navigate, notice });
  callbacks.current = { navigate, notice };
  useEffect(() => {
    let alive = true;
    let draining = Promise.resolve();
    const open = async (text: string) => {
      try {
        const url = await coreRequest<string>({
          op: 'browser_shared_url',
          text,
        });
        if (alive) callbacks.current.navigate(url);
      } catch (error) {
        if (alive)
          callbacks.current.notice(
            error instanceof Error
              ? error.message
              : 'No supported YouTube link was found.',
          );
      }
    };
    if (Platform.OS === 'android') {
      const drain = () => {
        draining = draining
          .catch(() => {})
          .then(async () => {
            for (const text of await takeBrowserLaunches()) {
              if (!alive) return;
              await open(text);
            }
          })
          .catch(() => {});
      };
      const subscription = observeBrowserLaunches(drain);
      drain();
      return () => {
        alive = false;
        subscription?.remove();
      };
    }
    const subscription = Linking.addEventListener('url', ({ url }) => {
      void open(url);
    });
    void Linking.getInitialURL().then((url) => {
      if (url && alive) void open(url);
    });
    return () => {
      alive = false;
      subscription.remove();
    };
  }, []);
}
