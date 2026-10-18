import { useEffect, useRef, useState } from 'react';
import { AppState } from 'react-native';
import { useVideoPlayer } from 'expo-video';
import { aiRequest } from '../../modules/lingoplay-core';
import { extractMedia } from '../core/media';

type Clip = { needsMedia: false; uri: string; token: string; start: number; end: number };
type Missing = { needsMedia: true; videoId: string };
type GameScope = { chatId: string; messageId: string; gameId: string };
type TranscriptScope = { chatId: string; videoId: string; trackId: string };
type WordScope = { videoId: string; trackId: string; segmentIndex: number; tokenIndex: number; word: string };

// Games and the transcript reader share audio focus as well as Rust's clip cache.
let stopFocusedAudio: (() => void) | null = null;

export function stopCaptionAudio() {
  stopFocusedAudio?.();
}

export function useGameAudio(scope: GameScope, beforePlay?: () => Promise<void> | void) {
  return useCaptionAudio(scope, beforePlay);
}

export function useTranscriptAudio(scope: TranscriptScope, beforePlay?: () => Promise<void> | void) {
  return useCaptionAudio(scope, beforePlay);
}

export function useWordAudio(scope: WordScope, beforePlay?: () => Promise<void> | void) {
  return useCaptionAudio(scope, beforePlay);
}

function useCaptionAudio(scope: GameScope | TranscriptScope | WordScope, beforePlay?: () => Promise<void> | void) {
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
    const cancelled = () => !mounted.current || generation.current !== version || controller.signal.aborted;
    const cancellation = Error('Audio playback cancelled');
    const check = () => {
      if (cancelled()) throw cancellation;
    };
    try {
      const command = { op: 'word' in scope ? 'ai_word_audio' : 'gameId' in scope ? 'ai_game_audio' : 'ai_chat_audio', ...scope, cueId };
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
    const subscription = AppState.addEventListener('change', state => { if (state !== 'active') stop(); });
    return () => { mounted.current = false; stop(); subscription.remove(); };
  }, [player, identity]);
  return { play, stop, phase, error };
}
