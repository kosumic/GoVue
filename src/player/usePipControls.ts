import { useEffect, useRef, useState } from 'react';
import { AppState, Platform } from 'react-native';
import { isPictureInPictureSupported, type VideoPlayer } from 'expo-video';
import {
  configurePipControls,
  coreRequest,
  observePipActions,
  pipBackgroundAudio,
} from '../../modules/govue-core';
import type { useMediaBrowser } from '../media/useMediaBrowser';

type PipState = {
  videoId: string;
  available: boolean;
  autoEnter: boolean;
  audioOnly: boolean;
  actions: { id: string; label: string; icon: string; enabled: boolean }[];
  perform?: string;
};
const idle: PipState = {
  videoId: '',
  available: false,
  autoEnter: false,
  audioOnly: false,
  actions: [],
};
export function usePipControls(input: {
  media: ReturnType<typeof useMediaBrowser>;
  pip: boolean;
  hasPrevious: boolean;
  hasNext: boolean;
  advance: (direction: 'next' | 'previous' | 'ended') => Promise<void>;
}) {
  const [state, setState] = useState(idle);
  const current = useRef(input);
  current.current = input;
  const active = useRef(true);
  const writes = useRef(Promise.resolve());
  const handling = useRef(false);
  async function request(action = 'snapshot', handoff = false) {
    const owner = current.current.media.playback?.videoId ?? '';
    let result: PipState | null = null;
    const work = writes.current
      .catch(() => {})
      .then(async () => {
        const value = current.current;
        if (!active.current || (value.media.playback?.videoId ?? '') !== owner)
          return;
        const player = value.media.player;
        const next = await coreRequest<PipState>({
          op: 'player_pip',
          action,
          handoff,
          videoId: owner,
          active: !!value.media.playback,
          pip: value.pip,
          playing: value.media.isPlaying(),
          ended:
            player.duration > 0 && player.currentTime >= player.duration - 0.5,
        });
        if (
          !active.current ||
          (current.current.media.playback?.videoId ?? '') !== owner
        )
          return;
        const supported = isPictureInPictureSupported();
        result = {
          ...next,
          available: next.available && supported,
          autoEnter: next.autoEnter && supported,
        };
        setState(result);
        const renderer = player as VideoPlayer & {
          govueSetVideoEnabled?: (
            expectedUri: string,
            enabled: boolean,
          ) => Promise<boolean>;
        };
        const source = value.media.playback?.uri;
        if (source)
          await renderer.govueSetVideoEnabled?.(source, !next.audioOnly);
        await configurePipControls(result);
      });
    writes.current = work.catch(() => {});
    await work;
    return result as PipState | null;
  }
  async function perform(action: string, videoId: string, handoff = false) {
    if (handling.current || videoId !== current.current.media.playback?.videoId)
      return;
    handling.current = true;
    try {
      const next = await request(
        action === 'audio_complete' ? 'snapshot' : action,
        handoff,
      );
      const value = current.current;
      if (!next || next.videoId !== value.media.playback?.videoId) return;
      switch (next.perform) {
        case 'pause':
          await value.media.pause();
          break;
        case 'play':
          await value.media.play();
          break;
        case 'replay':
          await value.media.replay();
          break;
        case 'previous':
          await value.advance('previous');
          break;
        case 'next':
          await value.advance('next');
          break;
        case 'backgroundAudio':
          if (!(await pipBackgroundAudio(videoId))) {
            await request('audio_failed');
            throw Error('Could not switch to background audio.');
          }
          break;
      }
      if (action === 'audio_failed')
        value.media.showNotice('Could not switch to background audio.');
    } catch (error) {
      current.current.media.showNotice(
        error instanceof Error
          ? error.message
          : 'Could not change picture-in-picture playback.',
      );
    } finally {
      handling.current = false;
    }
  }
  useEffect(() => {
    active.current = true;
    const subscription = observePipActions((sample) => {
      // Foreground restoration must survive an in-flight PiP action. Dropping
      // it in perform's duplicate-action guard can leave video disabled after
      // the headphones handoff. request serializes it behind pending work.
      if (sample.action === 'foreground') {
        void request('foreground', sample.handoff).catch(() => {});
        return;
      }
      void perform(sample.action, sample.videoId, sample.handoff);
    });
    const player = input.media.player;
    const playing = player.addListener('playingChange', () => {
      void request().catch(() => {});
    });
    const ended = player.addListener('playToEnd', () => {
      void request().catch(() => {});
    });
    const app = AppState.addEventListener('change', (value) => {
      if (value === 'active' && Platform.OS !== 'android')
        void request('foreground').catch(() => {});
    });
    void request(
      AppState.currentState === 'active' ? 'foreground' : 'snapshot',
    ).catch(() => {});
    return () => {
      active.current = false;
      subscription?.remove();
      playing.remove();
      ended.remove();
      app.remove();
    };
  }, [input.media.player]);
  useEffect(() => {
    void request().catch(() => {});
  }, [
    input.media.playback?.videoId,
    input.media.playback?.uri,
    input.media.settings.autoPip,
    input.media.settings.backgroundPlayback,
    input.media.cast.state.active,
    input.media.cast.state.link,
    input.hasPrevious,
    input.hasNext,
    input.pip,
  ]);
  return { ...state, perform };
}
