import { useCallback, useEffect, useRef, useState } from 'react';
import { AppState } from 'react-native';
import { coreRequest } from '../../modules/govue-core';

export function useExampleSpeech(context: unknown, beforePlay?: () => Promise<void> | void) {
  return useSpeech(context, false, beforePlay);
}

export function useResponseSpeech(context: unknown, beforePlay?: () => Promise<void> | void) {
  return useSpeech(context, true, beforePlay);
}

function useSpeech(context: unknown, response: boolean, beforePlay?: () => Promise<void> | void) {
  const [active, setActive] = useState<string | null>(null);
  const [error, setError] = useState('');
  const epoch = useRef(0);
  const owned = useRef<typeof import('expo-speech') | null>(null);
  const mounted = useRef(true);
  const stop = useCallback(() => {
    ++epoch.current;
    const speech = owned.current;
    owned.current = null;
    if (mounted.current) setActive(null);
    return speech ? speech.stop().catch(() => {}) : Promise.resolve();
  }, []);
  useEffect(() => {
    mounted.current = true;
    setError(''); setActive(null);
    const subscription = AppState.addEventListener('change', state => { if (state !== 'active') void stop(); });
    return () => { mounted.current = false; void stop(); subscription.remove(); };
  }, [context, stop]);

  async function speak(id: string, text: string, language?: string) {
    if (AppState.currentState !== 'active') return;
    const stopped = stop();
    const version = epoch.current;
    const current = () => mounted.current && version === epoch.current && AppState.currentState === 'active';
    setError(''); setActive(id);
    try {
      const speech = await import('expo-speech');
      await stopped;
      if (!current()) return;
      await beforePlay?.();
      if (!current()) return;
      await speech.stop();
      if (!current()) return;
      // Wait for the native engine to initialize before submitting a cancellable utterance.
      const voices = await speech.getAvailableVoicesAsync();
      if (!current()) return;
      if (!voices.length) throw Error('No text-to-speech voice is available. Add one in your device’s text-to-speech settings.');
      const voice = language ? voices.find(voice => voice.language.toLowerCase().split(/[-_]/)[0] === language) : undefined;
      if (language && !voice) throw Error('No voice is available for this language. Add it in your device’s text-to-speech settings.');
      const chunks = response ? (await coreRequest<{ chunks: string[] }>({ op: 'response_speech', text, maxLength: Math.min(4000, speech.maxSpeechInputLength) })).chunks : [text];
      if (!current()) return;
      owned.current = speech;
      const done = () => { if (current()) { ++epoch.current; owned.current = null; setActive(null); } };
      const fail = () => { if (current()) setError(response ? 'Could not read this response. Please try again.' : 'Could not speak this example. Please try again.'); done(); };
      function next(index: number) {
        if (!current()) return;
        if (index === chunks.length) { done(); return; }
        try {
          speech.speak(chunks[index], { language, voice: voice?.identifier, useApplicationAudioSession: false,
            onDone: () => next(index + 1), onStopped: done, onError: fail,
          });
        } catch { fail(); }
      }
      next(0);
    } catch (failure) {
      if (current()) { setActive(null); setError(failure instanceof Error ? failure.message : response ? 'Could not read this response.' : 'Could not speak this example.'); }
    }
  }
  return { active, error, speak, stop };
}
