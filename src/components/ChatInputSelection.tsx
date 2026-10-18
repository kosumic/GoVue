import { createContext, useContext, useMemo } from 'react';
import type { EnrichedMarkdownTextProps } from 'react-native-enriched-markdown';

export const ChatInputSelection = createContext<((text: string) => void) | undefined>(undefined);

export function useChatInputSelectionMenu(): EnrichedMarkdownTextProps['contextMenuItems'] {
  const addToInput = useContext(ChatInputSelection);
  return useMemo(() => addToInput ? [{
    text: 'Add to chat input',
    icon: 'text.badge.plus',
    onPress: ({ text }: { text: string }) => addToInput(text),
  }] : undefined, [addToInput]);
}
