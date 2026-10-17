import { useEffect, useRef, useState } from 'react';
import { AppState } from 'react-native';
import { useVideoPlayer } from 'expo-video';
import { aiRequest } from '../../modules/lingoplay-core';
import { extractMedia } from '../core/media';

type Clip = { needsMedia: false; uri: string; token: string; start: number; end: number };
type Missing = { needsMedia: true; videoId: string };

export function useGameAudio(scope: { chatId: string; messageId: string; gameId: string }, beforePlay?: () => Promise<void> | void) {
  const player = useVideoPlayer(null, player => {
    player.audioMixingMode = 'doNotMix';
    player.timeUpdateEventInterval = 0.05;
    player.staysActiveInBackground = false;
    player.showNowPlayingNotification = false;
    player.seekTolerance = { toleranceBefore: 0, toleranceAfter: 0 };
  });
  const [phase, setPhase] = useState<'idle' | 'loading' | 'playing'>('idle');
  const generation = useRef(0);
  const mounted = useRef(true);
  const active = useRef<{ cancel: () => void } | null>(null);
  const extraction = useRef<AbortController | null>(null);
  function stop() {
    ++generation.current;
    extraction.current?.abort();
    active.current?.cancel();
    active.current = null;
    try { player.pause(); } catch {}
    if (mounted.current) setPhase('idle');
  }
  async function play(cueId: string) {
    stop();
    const version = generation.current;
    const controller = new AbortController();
    extraction.current = controller;
    setPhase('loading');
    let clip: Clip | null = null;
    const check = () => {
      if (!mounted.current || generation.current !== version || controller.signal.aborted) throw Error('Audio playback was stopped.');
    };
    try {
      const command = { op: 'ai_game_audio', ...scope, cueId };
      let source = await aiRequest<Clip | Missing>(command);
      if (!source.needsMedia) clip = source;
      check();
      if (source.needsMedia) {
        const catalog = await extractMedia(source.videoId, controller.signal);
        check();
        try { source = await aiRequest<Clip>({ ...command, catalog }); }
        catch (error) {
          check();
          const fresh = await extractMedia(catalog.videoId, controller.signal, true);
          check();
          source = await aiRequest<Clip>({ ...command, catalog: fresh });
        }
      }
      // Retain the route before checking cancellation so even a late response is revoked.
      if (!source.needsMedia) clip = source;
      check();
      if (!clip) throw Error('Game audio could not be prepared.');
      await beforePlay?.();
      check();
      await player.replaceAsync({ uri: clip.uri, contentType: 'hls' });
      check();
      player.currentTime = clip.start;
      const end = clip.end;
      await new Promise<void>((resolve, reject) => {
        let done = false;
        const finish = (error?: Error) => {
          if (done) return;
          done = true;
          clearTimeout(timeout);
          tick.remove(); ended.remove(); status.remove();
          try { player.pause(); } catch {}
          active.current = null;
          if (error) reject(error); else resolve();
        };
        const tick = player.addListener('timeUpdate', ({ currentTime }) => { if (currentTime >= end) finish(); });
        const ended = player.addListener('playToEnd', () => finish());
        const status = player.addListener('statusChange', ({ status }) => { if (status === 'error') finish(Error('Game audio could not be played.')); });
        const timeout = setTimeout(() => finish(Error('Audio playback timed out. Try replaying the clip.')), 60000);
        active.current = { cancel: () => finish(Error('Audio playback was stopped.')) };
        player.play();
        setPhase('playing');
      });
    } finally {
      if (mounted.current && generation.current === version) {
        player.pause();
        await player.replaceAsync(null).catch(() => {});
        if (mounted.current) setPhase('idle');
      }
      if (clip) await aiRequest({ op: 'ai_game_audio_release', token: clip.token }).catch(() => {});
    }
  }
  useEffect(() => {
    mounted.current = true;
    const subscription = AppState.addEventListener('change', state => { if (state !== 'active') stop(); });
    return () => { mounted.current = false; stop(); subscription.remove(); };
  }, [player]);
  return { play, stop, phase };
}
