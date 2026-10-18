import { useEffect, useRef, useState } from 'react';
import {
  coreRequest,
  observePhysicalOrientation,
  setOrientationObservation,
} from '../../modules/govue-core';

type Context = {
  visible: boolean;
  mini: boolean;
  locked: boolean;
  pip: boolean;
  casting?: boolean;
};
type Sample = { degrees: number; systemAutoRotate: boolean };
type Surface = { fullscreen: boolean };

export function useSurfaceOrientation(context: Context) {
  const [fullscreen, updateFullscreen] = useState(false);
  const facts = useRef(context);
  facts.current = context;
  const sample = useRef<Sample>({ degrees: -1, systemAutoRotate: false });
  const mounted = useRef(true);
  const manual = useRef(0);
  const writes = useRef(Promise.resolve());
  const submit = (op: string, values: Record<string, unknown> = {}) => {
    const revision = manual.current;
    writes.current = writes.current
      .catch(() => {})
      .then(async () => {
        if (!mounted.current) return;
        const state = await coreRequest<Surface>({
          op,
          ...values,
          ...(op === 'player_orientation'
            ? { ...sample.current, ...facts.current }
            : {}),
        });
        if (mounted.current && revision === manual.current)
          updateFullscreen(state.fullscreen);
      })
      .catch(() => {});
  };
  useEffect(() => {
    mounted.current = true;
    submit('player_surface_reset');
    const listener = observePhysicalOrientation((value) => {
      sample.current = value;
      submit('player_orientation');
    });
    void setOrientationObservation(true).catch(() => {});
    return () => {
      mounted.current = false;
      listener?.remove();
      void setOrientationObservation(false).catch(() => {});
    };
  }, []);
  useEffect(() => {
    submit('player_orientation');
  }, [
    context.visible,
    context.mini,
    context.locked,
    context.pip,
    context.casting,
    fullscreen,
  ]);
  return {
    fullscreen,
    setFullscreen(enabled: boolean) {
      ++manual.current;
      submit('player_fullscreen', { enabled });
      updateFullscreen(enabled);
    },
  };
}
