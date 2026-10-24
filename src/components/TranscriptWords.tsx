import { Fragment, useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore } from 'react';
import { Alert, Text, type StyleProp, type TextInstance, type TextStyle } from 'react-native';
import { captionLineStarts, captionWords, type WordToken } from '../core/vocabulary';
import type { Transcript } from '../youtube/transcript';
import { idleTranscriptClock, type TranscriptClock } from '../media/transcriptClock';
import type { TranscriptFocus } from './TranscriptList';

type VisualLine = { text: string; y: number; height: number };
type Layout = { source: Transcript; segmentIndex: number; lines: VisualLine[] };

export function TranscriptWords({ source, segmentIndex, position, clock, style, activeWordStyle, separatorStyle, selected, selectedTokenIndex, selectedEndTokenIndex, selectedWordStyle, onHeight, onFollow, onSeek, onSeekWord, onSave, onTranslate }: {
  source: Transcript;
  segmentIndex: number;
  position: number | null;
  clock?: TranscriptClock;
  style: StyleProp<TextStyle>;
  activeWordStyle: StyleProp<TextStyle>;
  separatorStyle?: StyleProp<TextStyle>;
  selected: boolean;
  selectedTokenIndex?: number;
  selectedEndTokenIndex?: number;
  selectedWordStyle?: StyleProp<TextStyle>;
  onHeight?: (height: number) => void;
  onFollow?: (focus: TranscriptFocus) => void;
  onSeek?: () => void;
  onSeekWord?: (seconds: number) => void;
  onSave: (tokenIndex: number, word: string) => void;
  onTranslate: (tokenIndex: number, word: string, anchor?: { x: number; y: number }) => void;
}) {
  const text = source.segments[segmentIndex].text;
  const [parsed, setParsed] = useState<{ source: Transcript; segmentIndex: number; tokens: WordToken[] } | null>(null);
  const [error, setError] = useState('');
  const [retry, setRetry] = useState(0);
  const node = useRef<TextInstance>(null);
  const [layout, setLayout] = useState<Layout | null>(null);
  const [plan, setPlan] = useState<{ layout: Layout; starts: number[] } | null>(null);
  const following = !!onFollow;
  useEffect(() => {
    if (!following || !layout || layout.source !== source || layout.segmentIndex !== segmentIndex) return;
    let active = true;
    void captionLineStarts(source, segmentIndex, layout.lines.map(line => line.text)).then(starts => {
      if (active && starts.length === layout.lines.length) setPlan({ layout, starts });
    }).catch(() => {});
    return () => { active = false; };
  }, [following, layout, source, segmentIndex]);
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
  const inSelection = (index: number) => selectedTokenIndex !== undefined && index >= selectedTokenIndex && index <= (selectedEndTokenIndex ?? selectedTokenIndex);
  return <><Text ref={node} style={style} onLayout={onHeight ? event => onHeight(event.nativeEvent.layout.height) : undefined}
    onTextLayout={event => {
      const lines = event.nativeEvent.lines;
      if (!lines.length || lines.some(line => !Number.isFinite(line.y) || !Number.isFinite(line.height) || line.height <= 0)) return;
      setLayout(current => current?.source === source && current.segmentIndex === segmentIndex && current.lines.length === lines.length &&
        current.lines.every((line, index) => line.text === lines[index].text && line.y === lines[index].y && line.height === lines[index].height)
        ? current : { source, segmentIndex, lines });
    }} accessible accessibilityLabel={spokenText} accessibilityRole={onSeek ? 'button' : undefined} accessibilityState={{ selected }}
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
      <TranscriptWord token={token} index={index} position={position} clock={clock} selected={selected}
        activeWordStyle={activeWordStyle} selectedWordStyle={inSelection(index) ? selectedWordStyle : undefined}
        onSeek={onSeek} onSeekWord={onSeekWord} onTranslate={onTranslate} />
    </Fragment> : inSelection(index) ? <Text key={index} accessible={false} style={selectedWordStyle}>{token.text}</Text> : token.text) : text}
  </Text>{selected && onFollow && plan?.layout === layout && layout?.source === source && layout.segmentIndex === segmentIndex &&
    <TranscriptLineFollow node={node} lines={layout.lines} starts={plan.starts} position={position} clock={clock} onFollow={onFollow} />}</>;
}

function TranscriptLineFollow({ node, lines, starts, position, clock, onFollow }: {
  node: React.RefObject<TextInstance | null>; lines: VisualLine[]; starts: number[];
  position: number | null; clock?: TranscriptClock; onFollow: (focus: TranscriptFocus) => void;
}) {
  const line = useSyncExternalStore(clock?.subscribe ?? idleTranscriptClock, () => {
    const seconds = clock ? clock.getSnapshot() : position;
    if (seconds === null) return -1;
    let low = 0, high = starts.length;
    while (low < high) {
      const mid = (low + high) >>> 1;
      if (starts[mid] <= seconds) low = mid + 1; else high = mid;
    }
    return low - 1;
  });
  const follow = useRef(onFollow);
  follow.current = onFollow;
  useLayoutEffect(() => {
    if (node.current && lines[line]) follow.current({ node: node.current, y: lines[line].y, height: lines[line].height });
  }, [line, lines, node]);
  return null;
}

// Only words entering or leaving the active interval render on a clock tick.
function TranscriptWord({ token, index, position, clock, selected, activeWordStyle, selectedWordStyle, onSeek, onSeekWord, onTranslate }: {
  token: WordToken;
  index: number;
  position: number | null;
  clock?: TranscriptClock;
  selected: boolean;
  activeWordStyle: StyleProp<TextStyle>;
  selectedWordStyle?: StyleProp<TextStyle>;
  onSeek?: () => void;
  onSeekWord?: (seconds: number) => void;
  onTranslate: (index: number, word: string, anchor?: { x: number; y: number }) => void;
}) {
  const active = useSyncExternalStore(selected && token.start !== undefined && clock ? clock.subscribe : idleTranscriptClock, () => {
    const seconds = clock ? clock.getSnapshot() : position;
    return selected && seconds !== null && token.start !== undefined && seconds >= token.start && (token.end === undefined || seconds < token.end);
  });
  return <Text accessible={false} style={[
    token.bold !== undefined && { fontWeight: token.bold ? '700' : '400' },
    token.italic !== undefined && { fontStyle: token.italic ? 'italic' : 'normal' },
    token.underline !== undefined && { textDecorationLine: token.underline ? 'underline' : 'none' },
    active && activeWordStyle,
    selectedWordStyle,
  ]} onPress={onSeek ? event => {
    event.stopPropagation();
    if (onSeekWord && token.start !== undefined) onSeekWord(token.start);
    else onSeek();
  } : undefined}
    onLongPress={event => { event.stopPropagation(); onTranslate(index, token.text, { x: event.nativeEvent.pageX, y: event.nativeEvent.pageY }); }}>{token.text}{token.reading && <Text style={{ fontSize: 11, fontWeight: '400' }}>（{token.reading}）</Text>}</Text>;
}
