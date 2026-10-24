import { useEffect, useRef, useState } from 'react';
import { aiRequest, importChatAttachment, type ChatAttachmentSource } from '../../modules/govue-core';
import { discardChatAttachment, pickChatAttachment, stageChatAttachment, type AttachmentSource } from './chatAttachmentSources';

export type ChatAttachment = { id: string; kind: 'image' | 'pdf' | 'text'; name: string; mimeType: string; uri: string; width: number; height: number; bytes: number };
type AttachmentLimits = { maxAttachments: number; maxSourceBytes: number };
async function release(ids: string[]) {
  for (let index = 0; index < ids.length; index += 4) await aiRequest({ op: 'ai_chat_attachment_release', attachments: ids.slice(index, index + 4) }).catch(() => {});
}
/** React owns the draft; native files cross into Rust by URI, never by base64. */
export function useChatAttachments(contextId: string) {
  const [attachments, setAttachments] = useState<ChatAttachment[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const current = useRef(attachments), imported = useRef(new Set<string>()), generation = useRef(0);
  const pending = useRef(0), queue = useRef(Promise.resolve());
  function change(next: ChatAttachment[]) { current.current = next; setAttachments(next); }
  useEffect(() => {
    change([]); setError(''); setBusy(false); pending.current = 0;
    return () => {
      ++generation.current;
      const files = [...imported.current]; imported.current.clear();
      // Finish old imports and releases before a new chat can reuse a file ID.
      queue.current = queue.current.then(() => release(files), () => release(files));
    };
  }, [contextId]);
  function enqueue(acquire: () => Promise<ChatAttachmentSource[]>, claimed?: ChatAttachmentSource) {
    const version = generation.current;
    ++pending.current; setBusy(true); setError('');
    const operation = async () => {
      let files: ChatAttachmentSource[] = claimed ? [claimed] : [];
      try {
        if (version !== generation.current) return;
        const limits = await aiRequest<AttachmentLimits>({ op: 'ai_chat_attachment_limits' });
        if (version !== generation.current) return;
        files = await acquire();
        if (version !== generation.current) return;
        if (current.current.length + files.length > limits.maxAttachments) throw Error(`Attach up to ${limits.maxAttachments} files per message.`);
        for (const source of files) {
          if (version !== generation.current) return;
          const staged = await stageChatAttachment(source, limits.maxSourceBytes);
          try {
            if (version !== generation.current) return;
            const file = await importChatAttachment<ChatAttachment>(staged.source);
            if (version !== generation.current) { await release([file.id]); return; }
            imported.current.add(file.id);
            if (!current.current.some(previous => previous.id === file.id)) change([...current.current, file]);
          } finally { staged.dispose(); }
        }
      } catch (e) {
        if (version === generation.current) setError(e instanceof Error ? e.message : 'Could not attach this file.');
      } finally {
        for (const file of files) discardChatAttachment(file);
        if (version === generation.current) { --pending.current; setBusy(pending.current > 0); }
      }
    };
    queue.current = queue.current.then(operation, operation);
    return queue.current;
  }
  function pick(source: AttachmentSource = 'file') { if (!pending.current) return enqueue(() => pickChatAttachment(source)); }
  function paste(source: ChatAttachmentSource) { return enqueue(async () => [source], source); }
  return { attachments, busy, error, pick, paste, change, reportError: setError };
}
export type ChatAttachmentsController = ReturnType<typeof useChatAttachments>;
