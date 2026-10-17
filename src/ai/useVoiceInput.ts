import { useEffect, useRef, useState, useSyncExternalStore } from 'react';
import type { ChatTranscript } from './useChats';
import { voiceInput } from './voiceInput';

let identity = 0;

export function useVoiceInput({ enabled, chatId, transcript, draft, onDraftChange }: {
  enabled: boolean; chatId?: string; transcript: ChatTranscript | null; draft: string; onDraftChange: (value: string) => void;
}) {
  const [instance] = useState(() => ++identity);
  const owner = JSON.stringify([instance, chatId, transcript?.videoId, transcript?.trackId]);
  const latest = useRef({ owner, enabled, draft, onDraftChange });
  latest.current = { owner, enabled, draft, onDraftChange };
  const snapshot = useSyncExternalStore(voiceInput.subscribe, voiceInput.getSnapshot, voiceInput.getSnapshot);
  const [error, setError] = useState('');
  const [needsSettings, setNeedsSettings] = useState(false);

  useEffect(() => {
    setError(''); setNeedsSettings(false);
    if (!enabled) voiceInput.cancel(owner);
    return () => voiceInput.cancel(owner);
  }, [owner, enabled]);

  function changeDraft(value: string) {
    latest.current.draft = value;
    latest.current.onDraftChange(value);
  }
  const active = snapshot.owner === owner && snapshot.phase !== 'idle';
  return {
    active, phase: active ? snapshot.phase : 'idle', interimText: active ? snapshot.interimText : '', error, needsSettings,
    disabled: !enabled || (snapshot.owner !== null && snapshot.owner !== owner) || (active && snapshot.phase === 'finishing'),
    changeDraft, cancel: () => voiceInput.cancel(owner),
    toggle: () => {
      if (!enabled) return;
      if (active) { voiceInput.stop(owner); return; }
      setError(''); setNeedsSettings(false);
      void voiceInput.start({ owner, chatId, transcript, draft: latest.current.draft, readDraft: () => latest.current.draft,
        onDraft: value => { if (latest.current.owner === owner && latest.current.enabled) changeDraft(value); },
        onError: (message, settings) => { if (latest.current.owner === owner && latest.current.enabled) { setError(message); setNeedsSettings(settings); } },
      });
    },
  };
}
