import { AppState, Platform } from 'react-native';
import { aiRequest, dictationRequest } from '../../modules/govue-core';
import type { ChatTranscript } from './useChats';

type Recognizer = Pick<typeof import('expo-speech-recognition').ExpoSpeechRecognitionModule,
  'addListener' | 'start' | 'stop' | 'abort' | 'isRecognitionAvailable' | 'requestPermissionsAsync' | 'getStateAsync'>;
export type VoicePhase = 'idle' | 'preparing' | 'listening' | 'finishing';
export type VoiceSnapshot = { owner: string | null; phase: VoicePhase; interimText: string };
type Reply = Omit<VoiceSnapshot, 'owner'> & { draft?: string; noSpeech?: boolean };
type Context = { draft: string; chatId?: string; transcript: ChatTranscript | null };
type Request = Context & { owner: string; readDraft: () => string; onDraft: (text: string) => void; onError: (message: string, settings: boolean) => void };
type Dependencies = {
  load: () => Promise<Recognizer>;
  core: typeof dictationRequest;
  hints: (context: Context) => Promise<string[]>;
  locale: () => string;
};
type Session = {
  id: string; request: Request; coreOwned: boolean; started: boolean; cancelled: boolean; awaitingPermission: boolean;
  recognizer?: Recognizer; subscriptions: { remove: () => void }[]; timer?: ReturnType<typeof setTimeout>;
};

const IDLE: VoiceSnapshot = { owner: null, phase: 'idle', interimText: '' };
const CONTEXT_TIMEOUT = 1500;
const RESULT_TIMEOUT = 10000;
const DRAIN_TIMEOUT = 5000;
const NO_SPEECH = 'No speech detected. Tap the microphone to try again.';
const PERMISSION = 'Allow microphone and speech recognition access in Settings to use voice input.';
const ERRORS: Record<string, string> = {
  'no-speech': NO_SPEECH, 'speech-timeout': NO_SPEECH,
  'not-allowed': PERMISSION,
  network: 'Speech recognition could not connect. Check your connection and try again.',
  'language-not-supported': 'Speech recognition does not support your device language.',
};
let sequence = 0;

function boundedHints(read: () => Promise<string[]>, fallback: string[]) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  return Promise.race([
    Promise.resolve().then(read).catch(() => fallback),
    new Promise<string[]>(resolve => { timer = setTimeout(() => resolve(fallback), CONTEXT_TIMEOUT); }),
  ]).finally(() => clearTimeout(timer));
}

async function foregroundAfterPermission() {
  if (AppState.currentState === 'active') return true;
  return new Promise<boolean>(resolve => {
    const subscription = AppState.addEventListener('change', state => { if (state === 'active') done(true); });
    const timer = setTimeout(() => done(false), CONTEXT_TIMEOUT);
    function done(active: boolean) { subscription.remove(); clearTimeout(timer); resolve(active); }
  });
}

const dependencies: Dependencies = {
  load: async () => (await import('expo-speech-recognition')).ExpoSpeechRecognitionModule,
  core: dictationRequest,
  hints: ({ draft, chatId, transcript }) => aiRequest({ op: 'ai_chat_speech_hints', draft, chatId, transcript }),
  locale: () => Intl.DateTimeFormat().resolvedOptions().locale.split('-u-')[0],
};

/** The native recognizer has no event request IDs. Keep a single microphone
 * owner, including while cancelled native events drain after a view unmounts. */
export class VoiceInput {
  private current: Session | null = null;
  private snapshot = IDLE;
  private readonly listeners = new Set<() => void>();
  constructor(private readonly deps: Dependencies = dependencies) {}

  getSnapshot = () => this.snapshot;
  subscribe = (listener: () => void) => { this.listeners.add(listener); return () => { this.listeners.delete(listener); }; };

  private publish(snapshot: VoiceSnapshot) {
    this.snapshot = snapshot;
    for (const listener of this.listeners) listener();
  }
  private update(session: Session, op: string, fields: Record<string, unknown> = {}) {
    const reply = this.deps.core<Reply>({ op, sessionId: session.id, ...fields });
    this.publish({ owner: session.request.owner, phase: reply.phase, interimText: reply.interimText });
    return reply;
  }
  private live(session: Session) { return this.current === session && !session.cancelled; }
  private clearTimer(session: Session) { clearTimeout(session.timer); session.timer = undefined; }

  async start(request: Request) {
    if (this.current) return;
    const session: Session = { id: `voice-${Date.now().toString(36)}-${++sequence}`, request, coreOwned: false,
      started: false, cancelled: false, awaitingPermission: false, subscriptions: [] };
    this.current = session;
    try {
      if (Platform.OS !== 'android' && Platform.OS !== 'ios') throw Error('Voice input is unavailable on this platform.');
      const reply = this.deps.core<Reply>({ op: 'begin', sessionId: session.id, draft: request.draft });
      session.coreOwned = true;
      this.publish({ owner: request.owner, phase: reply.phase, interimText: reply.interimText });
      session.subscriptions.push(AppState.addEventListener('change', state => {
        if (state === 'background' && !session.awaitingPermission) this.cancel(request.owner);
      }));
      const fallback = this.deps.core<string[]>({ op: 'hints', draft: request.draft });
      const hints = boundedHints(() => this.deps.hints(request), fallback);
      const recognizer = await this.deps.load();
      if (!this.live(session)) return;
      session.recognizer = recognizer;
      if (!recognizer.isRecognitionAvailable()) throw Error('Speech recognition is unavailable. Enable a speech recognition service on your device.');
      session.awaitingPermission = true;
      const permission = await recognizer.requestPermissionsAsync();
      if (!this.live(session)) return;
      session.awaitingPermission = false;
      if (!permission.granted) {
        request.onError(permission.canAskAgain ? 'Microphone and speech recognition access are needed for voice input. Tap the microphone to try again.' : PERMISSION, !permission.canAskAgain);
        this.cancel(request.owner); return;
      }
      const foreground = await foregroundAfterPermission();
      if (!this.live(session)) return;
      if (!foreground) { this.cancel(request.owner); return; }
      const contextualStrings = await hints;
      if (!this.live(session)) return;
      if (await recognizer.getStateAsync() !== 'inactive') throw Error('Speech recognition is finishing. Tap the microphone to try again.');
      if (!this.live(session)) return;
      session.subscriptions.push(
        recognizer.addListener('start', () => this.event(session, () => {
          this.clearTimer(session); this.update(session, 'listening');
        })),
        recognizer.addListener('result', event => this.event(session, () => {
          const reply = this.update(session, 'result', { text: event.results[0]?.transcript ?? '', isFinal: event.isFinal,
            ...(event.isFinal ? { draft: request.readDraft() } : {}) });
          if (reply.draft !== undefined) request.onDraft(reply.draft);
        })),
        recognizer.addListener('error', event => {
          if (!this.live(session)) return;
          if (event.error !== 'aborted') request.onError(ERRORS[event.error] ?? 'Speech recognition is unavailable. Tap the microphone to try again.', event.error === 'not-allowed');
          session.cancelled = true;
          this.clearTimer(session);
          try { this.update(session, 'cancel'); } catch { /* Native end still releases ownership. */ }
          this.drain(session);
        }),
        recognizer.addListener('end', () => this.release(session, true)),
      );
      session.started = true;
      session.timer = setTimeout(() => {
        if (!this.live(session)) return;
        request.onError('Speech recognition did not start. Tap the microphone to try again.', false);
        this.cancel(request.owner);
      }, RESULT_TIMEOUT);
      recognizer.start({ lang: this.deps.locale(), contextualStrings, interimResults: true, maxAlternatives: 1,
        continuous: Platform.OS === 'ios', addsPunctuation: true, iosTaskHint: 'dictation', recordingOptions: { persist: false } });
    } catch (error) {
      if (!this.live(session)) return;
      request.onError(error instanceof Error ? error.message : 'Could not start voice input.', false);
      this.cancel(request.owner);
    }
  }

  private event(session: Session, run: () => void) {
    if (!this.live(session)) return;
    try { run(); } catch {
      session.request.onError('Could not update voice input. Tap the microphone to try again.', false);
      this.cancel(session.request.owner);
    }
  }

  stop(owner: string) {
    const session = this.current;
    if (!session || session.request.owner !== owner || session.cancelled) return;
    if (this.snapshot.phase === 'preparing') { this.cancel(owner); return; }
    if (this.snapshot.phase !== 'listening') return;
    this.event(session, () => {
      this.update(session, 'finish'); this.clearTimer(session);
      session.timer = setTimeout(() => {
        if (!this.live(session)) return;
        session.request.onError('Speech recognition did not finish. Tap the microphone to try again.', false);
        this.cancel(owner);
      }, RESULT_TIMEOUT);
      session.recognizer?.stop();
    });
  }

  cancel(owner: string) {
    const session = this.current;
    if (!session || session.request.owner !== owner || session.cancelled) return;
    session.cancelled = true; this.clearTimer(session);
    if (session.coreOwned) { try { this.update(session, 'cancel'); } catch { /* Release below still ends the core session. */ } }
    if (!session.started) { this.release(session); return; }
    this.drain(session);
    try { session.recognizer?.abort(); } catch { /* The drain checks native state before releasing. */ }
  }

  private drain(session: Session) {
    this.clearTimer(session);
    session.timer = setTimeout(() => {
      if (this.current !== session) return;
      void session.recognizer?.getStateAsync().then(state => {
        if (this.current !== session) return;
        if (state === 'inactive') { this.release(session); return; }
        this.drain(session);
        try { session.recognizer?.abort(); } catch { /* Keep ownership until native recording has stopped. */ }
      }).catch(() => { if (this.current === session) this.drain(session); });
    }, DRAIN_TIMEOUT);
  }

  private release(session: Session, nativeEnd = false) {
    if (this.current !== session) return;
    this.clearTimer(session);
    if (session.coreOwned) {
      try {
        const reply = this.deps.core<Reply>({ op: 'end', sessionId: session.id });
        if (nativeEnd && reply.noSpeech && !session.cancelled) session.request.onError(NO_SPEECH, false);
      } catch { /* A stale session must not change another core owner. */ }
      session.coreOwned = false;
    }
    for (const subscription of session.subscriptions) subscription.remove();
    this.current = null;
    this.publish(IDLE);
  }
}

export const voiceInput = new VoiceInput();
