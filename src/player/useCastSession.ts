import { useEffect, useRef, useState } from 'react';
import { AppState, PermissionsAndroid, Platform } from 'react-native';
import type { VideoPlayer } from 'expo-video';
import {
  coreRequest,
  castNeedsLocalPermission,
} from '../../modules/lingoplay-core';
import type { MediaCatalog } from '../core/media';

export type CastDevice = {
  id: string;
  name: string;
  host: string;
  port: number;
};
export type CastState = {
  available: boolean;
  id: string | null;
  videoId?: string;
  device?: string;
  deviceId?: string | null;
  active: boolean;
  acknowledged: boolean;
  link?: boolean;
  linkConnected?: boolean;
  url?: string | null;
  phase: string;
  position?: number;
  duration?: number;
  speed?: number;
  error?: string | null;
};
const idle: CastState = {
  available: false,
  id: null,
  active: false,
  acknowledged: false,
  phase: 'closed',
};
export async function castNetworkAccess() {
  if (Platform.OS !== 'android' || !(await castNeedsLocalPermission()))
    return true;
  const permission = 'android.permission.ACCESS_LOCAL_NETWORK' as Parameters<
    typeof PermissionsAndroid.check
  >[0];
  return (
    (await PermissionsAndroid.check(permission)) ||
    (await PermissionsAndroid.request(permission)) ===
      PermissionsAndroid.RESULTS.GRANTED
  );
}
async function permissions() {
  if (!(await castNetworkAccess()))
    throw Error('Allow local-network access to cast.');
  if (Platform.OS === 'android' && Number(Platform.Version) >= 33) {
    const permission = PermissionsAndroid.PERMISSIONS.POST_NOTIFICATIONS;
    if (
      !(await PermissionsAndroid.check(permission)) &&
      (await PermissionsAndroid.request(permission)) !==
        PermissionsAndroid.RESULTS.GRANTED
    )
      throw Error(
        'Enable notifications to keep the active Cast session visible.',
      );
  }
}
export function useCastSession(input: {
  player: VideoPlayer;
  videoId: string | null;
  catalog: MediaCatalog | null;
  resolve: () => Promise<MediaCatalog>;
  userAgent: string;
  height: number;
  fps: number;
  audioKey: string;
  onError: (message: string) => void;
}) {
  const [state, setState] = useState(idle);
  const stateRef = useRef(state);
  const current = useRef(input);
  current.current = input;
  const lifetime = useRef(0);
  const starting = useRef(false);
  const suspended = useRef<{
    videoId: string;
    position: number;
    playing: boolean;
  } | null>(null);
  const apply = (next: CastState, resume = true) => {
    const previous = stateRef.current;
    const value = current.current;
    stateRef.current = next;
    setState(next);
    const remote =
      next.active &&
      next.acknowledged &&
      !next.link &&
      next.videoId === value.videoId;
    if (
      value.player.showNowPlayingNotification !== !remote &&
      (remote || AppState.currentState === 'active')
    )
      value.player.showNowPlayingNotification = !remote;
    if (remote) {
      suspended.current = {
        videoId: next.videoId!,
        position: next.position ?? 0,
        playing: next.phase === 'playing' || next.phase === 'buffering',
      };
      value.player.pause();
    }
    const restore = suspended.current;
    if (!next.active && restore) {
      suspended.current = null;
      if (resume && restore.videoId === value.videoId) {
        value.player.currentTime = next.position ?? restore.position;
        if (restore.playing) value.player.play();
      }
    }
    if (next.error && next.error !== previous.error) value.onError(next.error);
  };
  useEffect(() => {
    let active = true,
      busy = false;
    const poll = async () => {
      if (!active || busy) return;
      busy = true;
      const generation = lifetime.current;
      try {
        const next = await coreRequest<CastState>({ op: 'cast_status' });
        if (active && generation === lifetime.current) apply(next);
      } catch {
      } finally {
        busy = false;
      }
    };
    void poll();
    const timer = setInterval(() => void poll(), 1000);
    const listener = input.player.addListener(
      'playingChange',
      ({ isPlaying }) => {
        const owner = stateRef.current;
        if (
          isPlaying &&
          owner.active &&
          owner.acknowledged &&
          !owner.link &&
          owner.videoId === current.current.videoId
        )
          input.player.pause();
      },
    );
    return () => {
      active = false;
      clearInterval(timer);
      listener.remove();
      ++lifetime.current;
    };
  }, [input.player]);
  useEffect(() => {
    ++lifetime.current;
    const generation = lifetime.current;
    void coreRequest<CastState>({ op: 'cast_sync', videoId: input.videoId })
      .then((next) => {
        if (generation === lifetime.current) apply(next);
      })
      .catch(() => {});
  }, [input.videoId]);
  async function start(
    device?: CastDevice,
    selection?: { height: number; fps: number; audioKey: string },
  ) {
    if (starting.current) return;
    starting.current = true;
    const generation = ++lifetime.current;
    const owner = current.current.videoId;
    try {
      await permissions();
      const value = current.current;
      const catalog =
        value.catalog?.videoId === owner
          ? value.catalog
          : await value.resolve();
      if (
        !owner ||
        generation !== lifetime.current ||
        current.current.videoId !== owner ||
        catalog.videoId !== owner
      )
        return;
      const next = await coreRequest<CastState>({
        op: 'cast_start',
        catalog,
        device,
        link: !device,
        userAgent: value.userAgent,
        position:
          stateRef.current.active && stateRef.current.acknowledged
            ? stateRef.current.position
            : value.player.currentTime,
        height: selection?.height ?? value.height,
        fps: selection?.fps ?? value.fps,
        audioKey: selection?.audioKey ?? value.audioKey,
        speed: value.player.playbackRate,
      });
      if (generation === lifetime.current && current.current.videoId === owner)
        apply(next);
      else if (next.id) await coreRequest({ op: 'cast_stop', id: next.id });
    } finally {
      starting.current = false;
    }
  }
  async function stop(resume = true) {
    const owner = stateRef.current;
    ++lifetime.current;
    if (!owner.id) return;
    const next = await coreRequest<CastState>({
      op: 'cast_stop',
      id: owner.id,
    });
    if (stateRef.current.id === owner.id) apply(next, resume);
  }
  async function control(action: string, values: Record<string, unknown> = {}) {
    const owner = stateRef.current;
    if (!owner.active || !owner.acknowledged || owner.link || !owner.id)
      return false;
    const generation = lifetime.current;
    const next = await coreRequest<CastState>({
      op: 'cast_control',
      id: owner.id,
      action,
      ...values,
    });
    if (stateRef.current.id === owner.id && generation === lifetime.current)
      apply(next);
    return true;
  }
  async function replace(
    catalog: MediaCatalog,
    height: number,
    fps: number,
    audioKey: string,
  ) {
    const owner = stateRef.current;
    if (!owner.active || owner.link || !owner.id) return false;
    const generation = ++lifetime.current;
    const next = await coreRequest<CastState>({
      op: 'cast_replace',
      id: owner.id,
      catalog,
      userAgent: current.current.userAgent,
      position: owner.position ?? current.current.player.currentTime,
      height,
      fps,
      audioKey,
      speed: owner.speed ?? current.current.player.playbackRate,
    });
    if (
      generation === lifetime.current &&
      current.current.videoId === catalog.videoId
    )
      apply(next);
    else if (next.id) await coreRequest({ op: 'cast_stop', id: next.id });
    return true;
  }
  return { state, stateRef, start, stop, control, replace };
}
