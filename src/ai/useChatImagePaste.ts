import { useEffect, useRef, type RefObject } from 'react';
import { findNodeHandle, type TextInputInstance } from 'react-native';
import { bindChatImagePaste, observeChatImagePaste, takeChatImagePaste, unbindChatImagePaste, type ChatAttachmentSource } from '../../modules/govue-core';
import { discardChatAttachment } from './chatAttachmentSources';

let sequence = 0;
export function useChatImagePaste(input: RefObject<TextInputInstance | null>, enabled: boolean, context: string, onPaste: (source: ChatAttachmentSource) => unknown, onError: (error: string) => void) {
  const handlers = useRef({ onPaste, onError }); handlers.current = { onPaste, onError };
  useEffect(() => {
    if (!enabled) return;
    const tag = findNodeHandle(input.current);
    if (tag == null) return;
    let active = true;
    const token = `chat:${++sequence}`;
    const subscription = observeChatImagePaste(event => {
      if (event.token !== token) return;
      if (typeof event.error === 'string') { if (active) handlers.current.onError(event.error); return; }
      if (typeof event.uri !== 'string' || typeof event.name !== 'string') return;
      const source = { uri: event.uri, name: event.name, mimeType: typeof event.mimeType === 'string' ? event.mimeType : undefined };
      let claimed = false;
      void takeChatImagePaste(token, source.uri).then(async taken => {
        claimed = taken;
        if (!taken) return;
        if (!active) discardChatAttachment(source);
        else await handlers.current.onPaste(source);
      }).catch(() => { if (claimed) discardChatAttachment(source); if (active) handlers.current.onError('Could not paste this image.'); });
    });
    void bindChatImagePaste(tag, token).catch(() => { if (active) handlers.current.onError('Image paste is unavailable. Use Attach files to pick an image.'); });
    return () => { active = false; subscription?.remove(); void unbindChatImagePaste(token).catch(() => {}); };
  }, [input, enabled, context]);
}
