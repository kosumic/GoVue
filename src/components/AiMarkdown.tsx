import { Alert, Linking, Platform } from 'react-native';
import { EnrichedMarkdownText, type MarkdownStyle } from 'react-native-enriched-markdown';
import { useThemeColors, useThemeStyles, type ThemeColors } from '../theme';

const mono = Platform.OS === 'ios' ? 'Menlo' : 'monospace';
const makeMarkdownStyle = (colors: ThemeColors): MarkdownStyle => ({
  paragraph: { color: colors.ink, fontSize: 15, lineHeight: 24, marginBottom: 12 },
  h1: { color: colors.ink, fontSize: 24, lineHeight: 30, fontWeight: '700', marginTop: 8, marginBottom: 12 },
  h2: { color: colors.ink, fontSize: 20, lineHeight: 27, fontWeight: '600', marginTop: 8, marginBottom: 10 },
  h3: { color: colors.ink, fontSize: 17, lineHeight: 24, fontWeight: '600', marginTop: 6, marginBottom: 8 },
  h4: { color: colors.ink, fontSize: 16, lineHeight: 24, fontWeight: '600', marginBottom: 8 },
  h5: { color: colors.ink, fontSize: 15, lineHeight: 23, fontWeight: '600', marginBottom: 8 },
  h6: { color: colors.muted, fontSize: 14, lineHeight: 22, fontWeight: '600', marginBottom: 8 },
  strong: { color: colors.ink, fontWeight: 'bold' },
  em: { color: colors.ink, fontStyle: 'italic' },
  list: { color: colors.ink, fontSize: 15, lineHeight: 24, bulletColor: colors.accent, markerColor: colors.accent, gapWidth: 8, marginBottom: 12 },
  link: { color: colors.accent, underline: true },
  code: { color: colors.ink, fontFamily: mono, backgroundColor: colors.accentSoft },
  codeBlock: {
    color: colors.ink, fontFamily: mono, fontSize: 13, lineHeight: 20, backgroundColor: colors.surface,
    borderColor: colors.line, borderWidth: 1, borderRadius: 10, padding: 12, marginBottom: 12,
    syntaxColors: {
      attribute: colors.accent, comment: colors.muted, constant: '#835A20', embedded: colors.ink,
      function: colors.accent, keyword: colors.accent, number: '#835A20', operator: colors.muted,
      property: colors.ink, punctuation: colors.muted, string: colors.accent, tag: colors.accent,
      type: '#67469A', variable: colors.ink,
    },
  },
  blockquote: { color: colors.muted, borderColor: colors.accent, borderWidth: 3, gapWidth: 12, backgroundColor: colors.surface, marginBottom: 12 },
  table: { color: colors.ink, fontSize: 13, lineHeight: 20, borderColor: colors.line, borderWidth: 1, borderRadius: 10, cellPaddingHorizontal: 10, cellPaddingVertical: 8, headerBackgroundColor: colors.accentSoft, headerTextColor: colors.ink, rowEvenBackgroundColor: colors.surface, rowOddBackgroundColor: colors.background, marginBottom: 12 },
  thematicBreak: { color: colors.line, height: 1, marginTop: 4, marginBottom: 12 },
  math: { color: colors.ink, fontSize: 17, backgroundColor: colors.surface, padding: 8, marginBottom: 12 },
  inlineMath: { color: colors.ink },
});
const streamingConfig = { codeBlockMode: 'progressive', tableMode: 'progressive' } as const;
const md4cFlags = { highlight: true, latexMath: true, subscript: true, superscript: true } as const;

function openLink({ url }: { url: string }) {
  if (!/^https?:\/\//i.test(url)) return;
  void Linking.openURL(url).catch(() => Alert.alert('Could not open link', 'Please try again.'));
}

export function AiMarkdown({ content, streaming = false }: { content: string; streaming?: boolean }) {
  const colors = useThemeColors();
  const markdownStyle = useThemeStyles(makeMarkdownStyle);
  return <EnrichedMarkdownText markdown={content} markdownStyle={markdownStyle} md4cFlags={md4cFlags} flavor="github"
    selectable allowFontScaling allowTrailingMargin={false} enableLinkPreview={false} enableTaskListItemToggle={false}
    onLinkPress={openLink} selectionColor="#2F624740" selectionHandleColor={colors.accent}
    streamingAnimation={streaming} streamingConfig={streaming ? streamingConfig : undefined} />;
}
