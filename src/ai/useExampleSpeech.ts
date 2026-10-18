import { useEffect, useRef, useState } from 'react';
import { AppState } from 'react-native';

export function useExampleSpeech(context: unknown, beforePlay?: () => Promise<void> | void) {
  const [active, setActive] = useState<string | null>(null);
  const [error, setError] = useState('');
  const epoch = useRef(0);
  const owned = useRef(false);
  const mounted = useRef(true);
  function stop() {
    ++epoch.current;
    if (owned.current) {
      owned.current = false;
      void import('expo-speech').then(speech => speech.stop()).catch(() => {});
    }
    if (mounted.current) setActive(null);
  }
  useEffect(() => {
    mounted.current = true;
    setError(''); setActive(null);
    const subscription = AppState.addEventListener('change', state => { if (state !== 'active') stop(); });
    return () => { mounted.current = false; stop(); subscription.remove(); };
  }, [context]);

  async function speak(id: string, text: string, language: string) {
    stop();
    const version = epoch.current;
    const current = () => mounted.current && version === epoch.current;
    setError(''); setActive(id);
    try {
      const speech = await import('expo-speech');
      if (!current()) return;
      await beforePlay?.();
      if (!current()) return;
      await speech.stop();
      const voices = await speech.getAvailableVoicesAsync();
      if (!current()) return;
      const voice = voices.find(voice => voice.language.toLowerCase().split(/[-_]/)[0] === language);
      if (!voice) throw Error('No voice is available for this language. Add it in your device’s text-to-speech settings.');
      owned.current = true;
      const done = () => { if (current()) { owned.current = false; setActive(null); } };
      speech.speak(text, { language, voice: voice.identifier, useApplicationAudioSession: false,
        onDone: done, onStopped: done,
        onError: () => { if (current()) setError('Could not speak this example. Please try again.'); done(); },
      });
    } catch (failure) {
      if (current()) { setActive(null); setError(failure instanceof Error ? failure.message : 'Could not speak this example.'); }
    }
  }
  return { active, error, speak, stop };
}
