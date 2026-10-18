import { Fragment, useEffect, useState } from 'react';
import { Alert, Text, type StyleProp, type TextStyle } from 'react-native';
import { captionWords, type WordToken } from '../core/vocabulary';
import type { Transcript } from '../youtube/transcript';

export function TranscriptWords({ source, segmentIndex, position, style, activeWordStyle, separatorStyle, selected, onSeek, onSeekWord, onSave, onTranslate }: {
  source: Transcript;
  segmentIndex: number;
  position: number | null;
  style: StyleProp<TextStyle>;
  activeWordStyle: StyleProp<TextStyle>;
  separatorStyle?: StyleProp<TextStyle>;
  selected: boolean;
  onSeek?: () => void;
  onSeekWord?: (seconds: number) => void;
  onSave: (tokenIndex: number, word: string) => void;
  onTranslate: (tokenIndex: number, word: string, anchor?: { x: number; y: number }) => void;
}) {
  const text = source.segments[segmentIndex].text;
  const [parsed, setParsed] = useState<{ source: Transcript; segmentIndex: number; tokens: WordToken[] } | null>(null);
  const [error, setError] = useState('');
  const [retry, setRetry] = useState(0);
  useEffect(() => {
    let active = true;
    setError('');
    captionWords(source, segmentIndex).then(({ text: currentText, tokens }) => {
      if (currentText !== text) throw Error('This caption changed. Refresh the transcript to prepare its words.');
      if (active) setParsed({ source, segmentIndex, tokens });
    }).catch(e => {
      if (active) setError(e instanceof Error ? e.message : 'Could not prepare caption words.');
    });
    return () => { active = false; };
  }, [source, segmentIndex, retry]);
  const tokens = parsed?.source === source && parsed.segmentIndex === segmentIndex ? parsed.tokens : null;
  const timed = !!onSeekWord && !!tokens?.some(token => token.start !== undefined);
  const spokenText = tokens?.map(token => (token.speakerChange ? ' Possible speaker change. ' : '') + token.text + (token.reading ? ` (${token.reading})` : '')).join('') ?? text;
  return <Text style={style} accessible accessibilityLabel={spokenText} accessibilityRole={onSeek ? 'button' : undefined} accessibilityState={{ selected }}
    accessibilityHint={onSeek ? `Tap ${timed ? 'a word' : 'the line'} to play. Hold a word to translate it, or use the translate and save word actions.` : 'Hold a word to translate it, or use the translate and save word actions.'}
    accessibilityActions={tokens?.flatMap((token, index) => token.word ? [
      ...(onSeekWord && token.start !== undefined ? [{ name: `play-word-${index}`, label: `Play from ${token.text}` }] : []),
      { name: `translate-word-${index}`, label: `Translate ${token.text}` },
      { name: `save-word-${index}`, label: `Save ${token.text}` },
    ] : [])}
    onAccessibilityAction={event => {
      tokens?.forEach((token, index) => {
        if (!token.word) return;
        if (event.nativeEvent.actionName === `translate-word-${index}`) onTranslate(index, token.text);
        if (event.nativeEvent.actionName === `save-word-${index}`) onSave(index, token.text);
        if (event.nativeEvent.actionName === `play-word-${index}` && token.start !== undefined) onSeekWord?.(token.start);
      });
    }}
    onPress={onSeek ? event => { event.stopPropagation(); onSeek(); } : undefined}
    onLongPress={!tokens ? event => {
      event.stopPropagation();
      Alert.alert('Word translation unavailable', error || 'Words are still loading. Please try again.', [
        { text: 'Cancel', style: 'cancel' }, { text: 'Retry', onPress: () => setRetry(value => value + 1) },
      ]);
    } : undefined}>
    {tokens ? tokens.map((token, index) => token.word ? <Fragment key={index}>
      {token.speakerChange && <Text accessible={false} style={[{ fontSize: 12, fontWeight: '400' }, separatorStyle]}>{`${index && !tokens[index - 1].text.endsWith('\n') ? '\n' : ''}───\n`}</Text>}
      <Text accessible={false}
      style={[
        token.bold !== undefined && { fontWeight: token.bold ? '700' : '400' },
        token.italic !== undefined && { fontStyle: token.italic ? 'italic' : 'normal' },
        token.underline !== undefined && { textDecorationLine: token.underline ? 'underline' : 'none' },
        selected && position !== null && token.start !== undefined && position >= token.start && (token.end === undefined || position < token.end) && activeWordStyle,
      ]}
      onPress={onSeek ? event => {
        event.stopPropagation();
        if (onSeekWord && token.start !== undefined) onSeekWord(token.start);
        else onSeek();
      } : undefined}
      onLongPress={event => { event.stopPropagation(); onTranslate(index, token.text, { x: event.nativeEvent.pageX, y: event.nativeEvent.pageY }); }}>{token.text}{token.reading && <Text style={{ fontSize: 11, fontWeight: '400' }}>（{token.reading}）</Text>}</Text>
    </Fragment> : token.text) : text}
  </Text>;
}
