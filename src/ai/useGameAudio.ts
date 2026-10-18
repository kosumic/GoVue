import { useEffect, useMemo, useRef, useState } from 'react';
import { AppState } from 'react-native';
import { useVideoPlayer } from 'expo-video';
import { aiRequest } from '../../modules/govue-core';
import { captionAudioPreparation, requestCaptionAudio, type Clip, type CaptionScope, type GameScope, type TranscriptScope, type WordScope } from './captionAudio';

// Games and the transcript reader share audio focus as well as Rust's clip cache.
let stopFocusedAudio: (() => void) | null = null;

export function stopCaptionAudio() {
  stopFocusedAudio?.();
}

export function useGameAudio(scope: GameScope, beforePlay?: () => Promise<void> | void, cueIds: string[] = []) {
  return useCaptionAudio(scope, beforePlay, cueIds);
}

export function useTranscriptAudio(scope: TranscriptScope, beforePlay?: () => Promise<void> | void) {
  return useCaptionAudio(scope, beforePlay);
}

export function useWordAudio(scope: WordScope, beforePlay?: () => Promise<void> | void) {
  return useCaptionAudio(scope, beforePlay);
}

function useCaptionAudio(scope: CaptionScope, beforePlay?: () => Promise<void> | void, cueIds: string[] = []) {
  const player = useVideoPlayer(null, player => {
    player.audioMixingMode = 'doNotMix';
    player.timeUpdateEventInterval = 0.05;
    player.staysActiveInBackground = false;
    player.showNowPlayingNotification = false;
    player.seekTolerance = { toleranceBefore: 0, toleranceAfter: 0 };
  });
  const [phase, setPhase] = useState<'idle' | 'loading' | 'playing'>('idle');
  const [error, setError] = useState('');
  const generation = useRef(0);
  const mounted = useRef(true);
  const active = useRef<{ cancel: () => void } | null>(null);
  const extraction = useRef<AbortController | null>(null);
  const replacements = useRef(Promise.resolve());
  const focus = useRef<(() => void) | null>(null);
  const identity = JSON.stringify(scope);
  const cueIdentity = JSON.stringify(cueIds);
  const preparation = useMemo(() => captionAudioPreparation(scope), [identity]);
  const [cachePhase, setCachePhase] = useState<'idle' | 'loading' | 'ready' | 'error'>('idle');
  const [cacheError, setCacheError] = useState('');
  const [cacheRetry, setCacheRetry] = useState(0);
  function replace(source: { uri: string; contentType: 'hls' } | null) {
    const next = replacements.current.catch(() => {}).then(() => player.replaceAsync(source));
    replacements.current = next;
    return next;
  }
  function stop() {
    ++generation.current;
    extraction.current?.abort();
    active.current?.cancel();
    active.current = null;
    if (stopFocusedAudio === focus.current) stopFocusedAudio = null;
    try { player.pause(); } catch {}
    if (mounted.current) { setPhase('idle'); setError(''); }
  }
  async function play(cueId: string, onStarted?: () => void) {
    stopFocusedAudio?.();
    stop();
    focus.current = stop;
    stopFocusedAudio = stop;
    const version = generation.current;
    const controller = new AbortController();
    extraction.current = controller;
    setPhase('loading');
    let clip: Clip | null = null;
    const cancelled = () => !mounted.current || generation.current !== version;
    const cancellation = Error('Audio playback cancelled');
    const check = () => {
      if (cancelled()) throw cancellation;
    };
    try {
      if ('gameId' in scope && cueIds.includes(cueId)) {
        await preparation.prepare(cueId);
        check();
      }
      clip = await requestCaptionAudio(scope, cueId, controller.signal);
      check();
      if (!clip) throw Error('Caption audio could not be prepared.');
      await beforePlay?.();
      check();
      await replace({ uri: clip.uri, contentType: 'hls' });
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
        const status = player.addListener('statusChange', ({ status }) => { if (status === 'error') finish(Error('Caption audio could not be played.')); });
        const timeout = setTimeout(() => finish(Error('Audio playback timed out. Try replaying the clip.')), 60000);
        active.current = { cancel: () => finish(cancellation) };
        player.play();
        setPhase('playing');
        onStarted?.();
      });
    } catch (error) {
      if (cancelled()) return;
      setError(error instanceof Error ? error.message : 'Could not play this caption.');
      throw error;
    } finally {
      if (mounted.current && generation.current === version) {
        player.pause();
        await replace(null).catch(() => {});
        if (mounted.current && generation.current === version) {
          setPhase('idle');
          if (stopFocusedAudio === focus.current) stopFocusedAudio = null;
        }
      }
      if (clip) await aiRequest({ op: 'ai_game_audio_release', token: clip.token }).catch(() => {});
    }
  }
  useEffect(() => {
    mounted.current = true;
    let cacheGeneration = 0;
    async function preload() {
      const version = ++cacheGeneration;
      const ids: string[] = JSON.parse(cueIdentity);
      if (!ids.length) { setCachePhase('idle'); setCacheError(''); return; }
      setCachePhase('loading'); setCacheError('');
      try {
        for (const id of ids) {
          if (!mounted.current || version !== cacheGeneration) return;
          await preparation.prepare(id);
        }
        if (mounted.current && version === cacheGeneration) setCachePhase('ready');
      } catch (reason) {
        if (mounted.current && version === cacheGeneration) {
          setCachePhase('error');
          setCacheError(reason instanceof Error ? reason.message : 'Could not cache activity audio.');
        }
      }
    }
    if (AppState.currentState === 'active') void preload();
    const subscription = AppState.addEventListener('change', state => {
      if (state === 'active') void preload();
      else { ++cacheGeneration; preparation.cancel(); stop(); }
    });
    return () => { mounted.current = false; ++cacheGeneration; preparation.cancel(); stop(); subscription.remove(); };
  }, [player, identity, preparation, cueIdentity, cacheRetry]);
  return { play, stop, phase, error, cachePhase, cacheError, retryCache: () => setCacheRetry(value => value + 1) };
}
