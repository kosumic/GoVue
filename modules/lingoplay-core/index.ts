import { requireNativeModule } from 'expo';

const native = requireNativeModule<{ execute(request: string): Promise<string> }>('LingoPlayCore');

export async function coreRequest<T>(command: Record<string, unknown>): Promise<T> {
  const reply = JSON.parse(await native.execute(JSON.stringify(command)));
  if (reply?.ok !== true) throw new Error(reply?.error || 'Transcript core could not complete this request.');
  return reply.value as T;
}
