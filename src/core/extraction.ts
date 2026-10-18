import { coreRequest, getBrowserUserAgent } from '../../modules/govue-core';

export type TokenTask = { taskId: number; stage: 'snapshot' | 'mint' } & Record<string, unknown>;
export type TokenRuntime = {
  evaluate: (requestId: string, task: TokenTask) => Promise<string>;
  release: (requestId: string) => void;
};
let runtime: TokenRuntime | null = null;
let sequence = 0;
export function registerTokenRuntime(value: TokenRuntime) {
  runtime = value;
  return () => { if (runtime === value) runtime = null; };
}
const cancelled = () => Object.assign(Error('Extraction cancelled.'), { name: 'AbortError' });

// Native calls are adapters. Rust owns the job, protocol, cache and time limit;
// this loop delivers isolated-runtime requests and presents the final result.
export async function extract<T>(kind: 'media' | 'transcript', videoId: string, options: {
  signal?: AbortSignal; trackId?: string; audioKey?: string; fresh?: boolean;
} = {}): Promise<T> {
  const requestId = `extract:${Date.now()}:${++sequence}`;
  const engine = runtime;
  const { signal } = options;
  if (signal?.aborted) throw cancelled();
  const release = () => {
    engine?.release(requestId);
    void coreRequest({ op: 'extract_release', requestId }).catch(() => {});
  };
  signal?.addEventListener('abort', release, { once: true });
  try {
    const userAgent = await getBrowserUserAgent().catch(() => null);
    if (signal?.aborted) throw cancelled();
    await coreRequest({ op: 'extract_start', requestId, kind, videoId,
      trackId: options.trackId ?? 'default', audioKey: options.audioKey ?? '', fresh: options.fresh ?? false, userAgent: userAgent ?? '' });
    while (!signal?.aborted) {
      const result = await coreRequest<{ done: boolean; error?: string; value?: T; task?: TokenTask }>({ op: 'extract_poll', requestId });
      if (signal?.aborted) throw cancelled();
      if (result.done) {
        if (result.error) throw Error(result.error);
        return result.value as T;
      }
      if (result.task) {
        if (!engine) throw Error('YouTube verification runtime is unavailable.');
        const value = await engine.evaluate(requestId, result.task);
        if (signal?.aborted) throw cancelled();
        await coreRequest({ op: 'extract_token', requestId, taskId: result.task.taskId, value });
      } else await new Promise<void>(resolve => setTimeout(resolve, 100));
    }
    throw cancelled();
  } catch (error) {
    if (signal?.aborted) throw cancelled();
    throw error;
  } finally {
    signal?.removeEventListener('abort', release);
    release();
  }
}
