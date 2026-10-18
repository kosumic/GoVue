import { useMemo } from 'react';
import { Platform } from 'react-native';
import { EnrichedMarkdownText, type MarkdownStyle } from 'react-native-enriched-markdown';
import { useThemeColors } from '../theme';
import { useChatInputSelectionMenu } from './ChatInputSelection';

const plainTextFlags = { latexMath: false, hardSoftBreaks: true, preserveBlankLines: true };
const selectionMenuConfig = { copyAsMarkdown: { enabled: false } };

export function SelectableChatText({ text }: { text: string }) {
  const colors = useThemeColors();
  const contextMenuItems = useChatInputSelectionMenu();
  // A plain-styled fence preserves literal text, including indentation and Markdown.
  // Make the delimiter longer than any backtick run in the user's text.
  const markdown = useMemo(() => {
    const fence = '`'.repeat(Math.max(3, ...Array.from(text.matchAll(/`+/g), match => match[0].length + 1)));
    return `${fence}\n${text}\n${fence}`;
  }, [text]);
  const markdownStyle = useMemo<MarkdownStyle>(() => ({
    paragraph: { color: colors.ink, fontSize: 16, lineHeight: 25, marginTop: 0, marginBottom: 0 },
    codeBlock: { color: colors.ink, fontSize: 16, lineHeight: 25, fontFamily: Platform.OS === 'ios' ? 'System' : 'sans-serif',
      marginTop: 0, marginBottom: 0, padding: 0, borderWidth: 0, borderRadius: 0, backgroundColor: 'transparent' },
  }), [colors.ink]);
  return <EnrichedMarkdownText markdown={markdown} flavor="commonmark" markdownStyle={markdownStyle} md4cFlags={plainTextFlags}
    selectable allowFontScaling allowTrailingMargin={false} selectionColor="#2F624740" selectionHandleColor={colors.accent}
    selectionMenuConfig={selectionMenuConfig} contextMenuItems={contextMenuItems} />;
}
