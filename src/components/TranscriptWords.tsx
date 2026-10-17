import { useEffect, useState } from 'react';
import { Alert, Text, type StyleProp, type TextStyle } from 'react-native';
import { tokenizeCaption, type WordToken } from '../core/vocabulary';

export function TranscriptWords({ text, style, selected, onSeek, onSave }: {
  text: string;
  style: StyleProp<TextStyle>;
  selected: boolean;
  onSeek?: () => void;
  onSave: (tokenIndex: number, word: string) => void;
}) {
  const [parsed, setParsed] = useState<{ text: string; tokens: WordToken[] } | null>(null);
  const [error, setError] = useState('');
  const [retry, setRetry] = useState(0);
  useEffect(() => {
    let active = true;
    setError('');
    tokenizeCaption(text).then(tokens => {
      if (active) setParsed({ text, tokens });
    }).catch(e => {
      if (active) setError(e instanceof Error ? e.message : 'Could not prepare words for saving.');
    });
    return () => { active = false; };
  }, [text, retry]);
  const tokens = parsed?.text === text ? parsed.tokens : null;
  return <Text style={style} accessible accessibilityLabel={text} accessibilityRole={onSeek ? 'button' : undefined} accessibilityState={{ selected }}
    accessibilityHint={onSeek ? 'Tap to play this line. Use the save word actions or hold a word to save it.' : 'Use the save word actions or hold a word to save it.'}
    accessibilityActions={tokens?.flatMap((token, index) => token.word ? [{ name: `save-word-${index}`, label: `Save ${token.text}` }] : [])}
    onAccessibilityAction={event => {
      const index = tokens?.findIndex((token, index) => token.word && event.nativeEvent.actionName === `save-word-${index}`) ?? -1;
      if (tokens && index >= 0) onSave(index, tokens[index].text);
    }}
    onPress={onSeek ? event => { event.stopPropagation(); onSeek(); } : undefined}
    onLongPress={!tokens ? event => {
      event.stopPropagation();
      Alert.alert('Word saving unavailable', error || 'Words are still loading. Please try again.', [
        { text: 'Cancel', style: 'cancel' }, { text: 'Retry', onPress: () => setRetry(value => value + 1) },
      ]);
    } : undefined}>
    {tokens ? tokens.map((token, index) => token.word ? <Text key={index} accessible={false}
      onPress={onSeek ? event => { event.stopPropagation(); onSeek(); } : undefined}
      onLongPress={event => { event.stopPropagation(); onSave(index, token.text); }}>{token.text}</Text> : token.text) : text}
  </Text>;
}
