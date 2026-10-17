import { useEffect, useRef, useState } from 'react';
import { AppState } from 'react-native';
import { ExpoSpeechRecognitionModule as speech, useSpeechRecognitionEvent } from 'expo-speech-recognition';
import { aiRequest } from '../../modules/lingoplay-core';
import type { ChatTranscript } from './useChats';

type Session = { base: string; draft: string; started: boolean; awaitingPermission: boolean };
type Phase = 'idle' | 'preparing' | 'listening' | 'stopping';

// Native events have no request ID. Drain an aborted session across composer
// unmounts before allowing a replacement composer to claim the microphone.
let nativeEnd: { promise: Promise<void>; release: () => void } | null = null;
function trackNativeEnd() {
  let resolve!: () => void;
  const pending = { promise: new Promise<void>(done => { resolve = done; }), release: () => {} };
  const listener = speech.addListener('end', () => pending.release());
  pending.release = () => {
    listener.remove();
    if (nativeEnd === pending) nativeEnd = null;
    resolve();
  };
  nativeEnd = pending;
}

async function drainNativeEnd() {
  const pending = nativeEnd;
  if (!pending) return;
  let timeout: ReturnType<typeof setTimeout> | undefined;
  try {
    await Promise.race([pending.promise, new Promise<void>(resolve => { timeout = setTimeout(resolve, 1500); })]);
    if (nativeEnd === pending) {
      if (await speech.getStateAsync() !== 'inactive') throw Error('Speech recognition is finishing. Tap the microphone to try again.');
      pending.release();
    }
  } finally { globalThis.clearTimeout(timeout); }
}

async function foregroundAfterPermission() {
  if (AppState.currentState === 'active') return true;
  return new Promise<boolean>(resolve => {
    const subscription = AppState.addEventListener('change', state => { if (state === 'active') done(true); });
    const timeout = setTimeout(() => done(false), 1500);
    function done(active: boolean) { subscription.remove(); globalThis.clearTimeout(timeout); resolve(active); }
  });
}

export function useVoiceInput({ enabled, chatId, transcript, draft, onDraftChange }: {
  enabled: boolean;
  chatId?: string;
  transcript: ChatTranscript | null;
  draft: string;
  onDraftChange: (value: string) => void;
}) {
  const [phase, setPhase] = useState<Phase>('idle');
  const [error, setError] = useState('');
  const [needsSettings, setNeedsSettings] = useState(false);
  const session = useRef<Session | null>(null);
  const mounted = useRef(true);
  const timeout = useRef<ReturnType<typeof setTimeout> | null>(null);

  function clearTimeout() {
    if (timeout.current) globalThis.clearTimeout(timeout.current);
    timeout.current = null;
  }
  function finish() {
    session.current = null;
    clearTimeout();
    if (mounted.current) setPhase('idle');
  }
  function cancel() {
    const owned = session.current?.started;
    finish();
    if (owned) {
      try { speech.abort(); } catch { nativeEnd?.release(); }
    }
  }

  useSpeechRecognitionEvent('start', () => {
    if (session.current?.started) {
      clearTimeout();
      setPhase('listening');
    }
  });
  useSpeechRecognitionEvent('result', event => {
    const current = session.current;
    const text = event.results[0]?.transcript;
    if (!current?.started || !text) return;
    // Each interim result replaces the previous hypothesis for this utterance.
    const separator = current.base && !/\s$/.test(current.base) ? ' ' : '';
    current.draft = `${current.base}${separator}${text}`.slice(0, 8000);
    onDraftChange(current.draft);
  });
  useSpeechRecognitionEvent('end', () => { if (session.current?.started) finish(); });
  useSpeechRecognitionEvent('error', event => {
    if (!session.current?.started) return;
    if (event.error !== 'aborted') {
      setError(event.error === 'no-speech' || event.error === 'speech-timeout'
        ? 'No speech detected. Tap the microphone to try again.'
        : event.error === 'not-allowed'
          ? 'Allow microphone and speech recognition access in Settings to use voice input.'
          : event.error === 'network'
            ? 'Speech recognition could not connect. Check your connection and try again.'
            : event.error === 'language-not-supported'
              ? 'Speech recognition does not support your device language.'
              : 'Speech recognition is unavailable. Tap the microphone to try again.');
      setNeedsSettings(event.error === 'not-allowed');
    }
    finish();
  });

  useEffect(() => {
    mounted.current = true;
    const subscription = AppState.addEventListener('change', state => {
      if (state === 'background' && !session.current?.awaitingPermission) cancel();
    });
    return () => { mounted.current = false; subscription.remove(); cancel(); };
  }, []);
  useEffect(() => {
    setError(''); setNeedsSettings(false);
    if (!enabled) cancel();
    return cancel;
  }, [enabled, chatId, transcript?.videoId, transcript?.trackId]);
  useEffect(() => {
    // A draft replaced by navigation, typing, or a suggestion owns its new content.
    if (session.current && draft !== session.current.draft) cancel();
  }, [draft]);

  async function start() {
    if (!enabled || session.current) return;
    const current: Session = { base: draft, draft, started: false, awaitingPermission: true };
    session.current = current;
    setPhase('preparing'); setError(''); setNeedsSettings(false);
    try {
      if (draft.length >= 8000) throw Error('The message is full. Send it or shorten it before adding voice input.');
      if (!speech.isRecognitionAvailable()) throw Error('Speech recognition is unavailable. Enable a speech recognition service on your device.');
      const permission = await speech.requestPermissionsAsync();
      if (session.current !== current) return;
      current.awaitingPermission = false;
      if (!permission.granted) {
        setNeedsSettings(!permission.canAskAgain);
        throw Error(permission.canAskAgain ? 'Microphone and speech recognition access are needed for voice input. Tap the microphone to try again.' : 'Allow microphone and speech recognition access in Settings to use voice input.');
      }
      if (!await foregroundAfterPermission()) { if (session.current === current) cancel(); return; }
      if (session.current !== current) return;
      const contextualStrings = await aiRequest<string[]>({ op: 'ai_chat_speech_hints', chatId, transcript, draft }).catch(() => []);
      if (session.current !== current) return;
      await drainNativeEnd();
      if (session.current !== current) return;
      if (await speech.getStateAsync() !== 'inactive') throw Error('Speech recognition is finishing. Tap the microphone to try again.');
      if (session.current !== current) return;
      trackNativeEnd();
      current.started = true;
      timeout.current = globalThis.setTimeout(() => {
        if (session.current === current) { setError('Speech recognition did not start. Tap the microphone to try again.'); cancel(); }
      }, 10000);
      speech.start({
        lang: Intl.DateTimeFormat().resolvedOptions().locale.split('-u-')[0],
        contextualStrings, interimResults: true, continuous: false, maxAlternatives: 1,
        addsPunctuation: true, iosTaskHint: 'dictation', recordingOptions: { persist: false },
      });
    } catch (e) {
      if (session.current !== current) return;
      setError(e instanceof Error ? e.message : 'Could not start voice input.');
      cancel();
    }
  }

  function stop() {
    if (phase === 'preparing' || !session.current?.started) { cancel(); return; }
    setPhase('stopping'); clearTimeout();
    try {
      // Some services omit their end event after stop; keep the visible draft.
      timeout.current = globalThis.setTimeout(cancel, 5000);
      speech.stop();
    } catch { cancel(); }
  }

  return { active: phase !== 'idle', phase, error, needsSettings, cancel, toggle: () => { if (session.current) stop(); else void start(); } };
}
