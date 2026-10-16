import { requireNativeModule } from 'expo';

const native = requireNativeModule<{
  execute(request: string): Promise<string>;
  ai(request: string): Promise<string>;
  mux(video: string, audio: string, output: string): Promise<string>;
  transcriptTrace?(requestId: string, stage: string, cookie: number, begin: boolean): boolean;
}>('LingoPlayCore');

// Android async atrace slices are collected by Perfetto. Other platforms and
// older development clients can keep using the same transcript loading code.
export const transcriptTrace = (requestId: string, stage: string, cookie: number, begin: boolean) =>
  native.transcriptTrace?.(requestId, stage, cookie, begin) ?? false;

export async function aiRequest<T>(command: Record<string, unknown>): Promise<T> {
  const reply = JSON.parse(await native.ai(JSON.stringify(command)));
  if (reply?.ok !== true) throw new Error(reply?.error || 'Could not complete the AI request.');
  return reply.value as T;
}

export const muxMedia = (video: string, audio: string, output: string) => native.mux(video, audio, output);

export async function coreRequest<T>(command: Record<string, unknown>): Promise<T> {
  const reply = JSON.parse(await native.execute(JSON.stringify(command)));
  if (reply?.ok !== true) throw new Error(reply?.error || 'Transcript core could not complete this request.');
  return reply.value as T;
}
