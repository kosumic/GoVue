import { useEffect, useRef, useState } from 'react';
import { AppState } from 'react-native';
import * as WebBrowser from 'expo-web-browser';
import { aiRequest } from '../../modules/govue-core';

export type ProviderKind = 'chatgpt' | 'codex' | 'openai' | 'anthropic' | 'gemini' | 'openrouter' | 'xai' | 'kimi' | 'copilot' | 'custom';
export type Model = { id: string; name: string; hidden?: boolean; contextWindow?: number | null; maxOutputTokens?: number | null; reasoning?: string; reasoningEfforts?: string[]; temperature?: number | null; topP?: number | null; inputModalities?: string[]; outputModalities?: string[] };
export type ConnectionPreview = { canEditEndpoint: boolean; canAppendV1: boolean; effectiveBaseUrl: string | null; error?: string | null };
export type ProviderDraft = { id: string; label: string; kind: ProviderKind; baseUrl: string; model: string; enabled?: boolean; auth?: string; protocol?: string; azure?: boolean; appendV1Suffix?: boolean; customUserAgent?: string; models?: Model[] };
export type Provider = ProviderDraft & { enabled: boolean; models: Model[]; connected: boolean; account?: string; planEnabled: boolean; needsReauth?: boolean; credentialError?: string; connection?: ConnectionPreview };
export type Group = { id: string; name: string; members: { providerId: string; model: string }[]; strategy: 'fallback' | 'loadBalance'; fallbackOnAnyError: boolean; reasoning: string; contextLimit: number | null; connected?: boolean };
export type ProviderChoice = { kind: ProviderKind; name: string; auth: string[]; description: string; advanced?: boolean };
type Config = { providers: Provider[]; groups: Group[]; choices: ProviderChoice[]; activeId: string | null; agentMd: string };
type Login = { url: string; returnUrl?: string; mode: 'browser' | 'device'; userCode?: string };
type Task = { status: 'running' | 'completed' | 'failed' | 'cancelled'; text: string; message?: string; error?: string; login?: Login; result?: { models?: Model[]; planEnabled?: boolean; revoked?: boolean; connected?: boolean } };
type Started = { taskId: string; url?: string; returnUrl?: string };

export function useAi(visible: boolean, contextId: string) {
  const [config, setConfig] = useState<Config>({ providers: [], groups: [], choices: [], activeId: null, agentMd: '' });
  const [ready, setReady] = useState(false);
  const [busy, setBusy] = useState('');
  const [text, setText] = useState('');
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const epoch = useRef(0);
  const task = useRef<{ id: string; kind: string } | null>(null);
  const previousContext = useRef(contextId);

  function stop() {
    ++epoch.current;
    const current = task.current; task.current = null;
    if (current?.kind === 'login') { try { WebBrowser.dismissAuthSession(); } catch { /* Android closes on its return link or browser close button. */ } }
    if (current) void aiRequest({ op: 'ai_cancel', taskId: current.id }).then(() => aiRequest({ op: 'ai_forget', taskId: current.id })).catch(() => {});
    setBusy('');
  }
  useEffect(() => {
    const foreground = (active: string) => { void aiRequest({ op: 'ai_foreground', active: active === 'active' }).catch(() => {}); };
    foreground(AppState.currentState);
    const subscription = AppState.addEventListener('change', foreground);
    return () => { subscription.remove(); stop(); };
  }, []);
  useEffect(() => {
    let active = true;
    setReady(false);
    if (visible) {
      void aiRequest<Config>({ op: 'ai_config' }).then(next => {
        if (active) { setConfig(next); setReady(true); }
      }).catch(e => { if (active) setError(e.message); });
    } else stop();
    return () => { active = false; };
  }, [visible]);
  useEffect(() => {
    if (previousContext.current === contextId) return;
    previousContext.current = contextId;
    if (task.current?.kind === 'chat') stop();
    setText(''); setError('');
  }, [contextId]);

  async function run(kind: string, id: string, extra: Record<string, unknown> = {}): Promise<Task['result']> {
    stop(); const version = epoch.current;
    setBusy(kind); setError(''); setNotice('');
    if (kind === 'chat') setText('');
    let started: Started | undefined;
    const browser: { result?: WebBrowser.WebBrowserAuthSessionResult; failed?: boolean } = {};
    let opened = false;
    let expectedReturn: string | undefined;
    function open(login: Login) {
      if (opened) return;
      opened = true; expectedReturn = login.returnUrl;
      if (login.mode === 'device') {
        setNotice(`Enter code ${login.userCode || ''} in the browser, then return here.`);
        void WebBrowser.openBrowserAsync(login.url, { createTask: false, showInRecents: false }).catch(() => { browser.failed = true; });
      } else {
        if (!login.returnUrl) throw new Error('The sign-in return address is missing. Please rebuild the app.');
        void WebBrowser.openAuthSessionAsync(login.url, login.returnUrl, {
          createTask: false, showInRecents: false, showTitle: true,
          toolbarColor: '#F5F4EE', preferEphemeralSession: false,
        }).then(result => { browser.result = result; }).catch(() => { browser.failed = true; });
      }
    }
    try {
      started = await aiRequest<Started>({ op: kind === 'login' ? 'ai_login' : 'ai_start', kind, id, ...extra });
      if (version !== epoch.current) { await aiRequest({ op: 'ai_cancel', taskId: started.taskId }); return; }
      task.current = { id: started.taskId, kind };
      if (started.url) open({ url: started.url, returnUrl: started.returnUrl, mode: 'browser' });
      while (version === epoch.current) {
        if (browser.failed) throw new Error('Could not open the in-app sign-in window. Please try again.');
        if (browser.result && (browser.result.type !== 'success' || browser.result.url !== expectedReturn)) throw new Error('Sign-in was cancelled or returned an unexpected address.');
        const state = await aiRequest<Task>({ op: 'ai_poll', taskId: started.taskId });
        if (version !== epoch.current) return;
        if (state.login) open(state.login);
        if (kind === 'chat') setText(state.text);
        if (state.message) setNotice(state.message);
        if (state.status === 'failed') throw new Error(state.error || 'The provider request failed.');
        if (state.status === 'cancelled') return;
        if (state.status === 'completed') {
          const fresh = await aiRequest<Config>({ op: 'ai_config' });
          if (version !== epoch.current) return;
          setConfig(fresh);
          if (kind === 'login') setNotice(state.result?.connected ? 'Account connected. Refresh models to see account access.' : state.result?.planEnabled ? 'Connected. Choose a model to use your ChatGPT plan.' : 'Account connected. ChatGPT plan usage was not granted; reconnect to enable it.');
          else if (kind === 'disconnect' || kind === 'remove') setNotice(state.result?.revoked === false ? 'Disconnected on this device. You can also revoke access in your provider’s account settings.' : 'Provider disconnected.');
          else setNotice('');
          return state.result;
        }
        await new Promise(resolve => setTimeout(resolve, 250));
      }
    } catch (e) {
      if (version === epoch.current) setError(e instanceof Error ? e.message : 'Could not complete the provider request.');
    } finally {
      if (started) {
        if (kind === 'login' && version === epoch.current) { try { WebBrowser.dismissAuthSession(); } catch { /* Android's return link closes its Custom Tab. */ } }
        // Browser dismissal must also close the scoped loopback listener.
        if (version === epoch.current) await aiRequest({ op: 'ai_cancel', taskId: started.taskId }).catch(() => {});
        await aiRequest({ op: 'ai_forget', taskId: started.taskId }).catch(() => {});
      }
      if (version === epoch.current) { task.current = null; setBusy(''); }
    }
  }
  async function save(provider: ProviderDraft, key?: string) {
    setError('');
    try { const next = await aiRequest<Config>({ op: 'ai_save', provider, ...(key?.trim() ? { key } : {}) }); setConfig(next); return next; }
    catch (e) { setError(e instanceof Error ? e.message : 'Could not save provider.'); return null; }
  }
  async function select(id: string, model?: string) {
    stop(); setText(''); setError(''); setNotice('');
    try { setConfig(await aiRequest<Config>({ op: 'ai_select', id, ...(model ? { model } : {}) })); return true; }
    catch (e) { setError(e instanceof Error ? e.message : 'Could not select provider.'); return false; }
  }
  async function mutate(command: Record<string, unknown>) {
    setBusy('settings'); setError(''); setNotice('');
    try { const next = await aiRequest<Config>(command); setConfig(next); return next; }
    catch (e) { setError(e instanceof Error ? e.message : 'Could not save settings.'); return null; }
    finally { setBusy(''); }
  }
  return { ...config, ready, busy, text, error, notice, run, save, select, mutate, stop, clear: () => { setText(''); setError(''); setNotice(''); } };
}
export type AiController = ReturnType<typeof useAi>;
