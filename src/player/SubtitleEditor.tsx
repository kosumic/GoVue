import { useThemeColors, useThemeStyles, type ThemeColors } from '../theme';
import { useEffect, useRef, useState } from 'react';
import {
  Image,
  PanResponder,
  Pressable,
  StyleSheet,
  Text,
  TextInput,
  useWindowDimensions,
  View,
} from 'react-native';
import { LinearGradient } from 'expo-linear-gradient';
import { coreRequest } from '../../modules/govue-core';
import { defaultMediaSettings, type SubtitleStyle } from '../core/media';
import { Range } from './Range';
import { YouTubeSwitch as Switch } from '../components/YouTubeSwitch';

type Color = {
  color: string;
  hue: number;
  saturation: number;
  value: number;
  hueColor: string;
};
export function SubtitleEditor({
  value,
  onChange,
  onClose,
  videoId,
}: {
  value: SubtitleStyle;
  onChange: (value: SubtitleStyle) => void;
  onClose: () => void;
  videoId?: string;
}) {
  const colors = useThemeColors();
  const s = useThemeStyles(makeStyles);
  const screen = useWindowDimensions();
  const [draft, setDraft] = useState(value);
  const [size, setSize] = useState({ width: 1, height: 1 });
  const [textSize, setTextSize] = useState({ width: 0, height: 0 });
  const current = useRef({ draft, onChange, size });
  current.current = { draft, onChange, size };
  const origin = useRef(value);
  const pushed = useRef(0);
  const pending = useRef(false);
  const flush = () => {
    if (!pending.current) return;
    pending.current = false;
    pushed.current = Date.now();
    current.current.onChange(current.current.draft);
  };
  const edit = (changes: Partial<SubtitleStyle>) => {
    const next = { ...current.current.draft, ...changes };
    current.current.draft = next;
    setDraft(next);
    pending.current = true;
    if (Date.now() - pushed.current >= 500) flush();
  };
  useEffect(() => () => flush(), []);
  const position = useRef(
    PanResponder.create({
      onStartShouldSetPanResponder: () => true,
      onMoveShouldSetPanResponder: () => true,
      onPanResponderGrant: () => {
        origin.current = current.current.draft;
      },
      onPanResponderMove: (_e, gesture) =>
        edit({
          horizontal: Math.max(
            -1,
            Math.min(
              1,
              origin.current.horizontal +
                (gesture.dx / current.current.size.width) * 2,
            ),
          ),
          vertical: Math.max(
            0,
            Math.min(
              1,
              origin.current.vertical +
                gesture.dy / current.current.size.height,
            ),
          ),
        }),
      onPanResponderRelease: flush,
      onPanResponderTerminate: flush,
    }),
  ).current;
  return (
    <View style={s.editor}>
      <View
        style={[s.preview, { height: screen.height < 500 ? 100 : 168 }]}
        onLayout={(event) => setSize(event.nativeEvent.layout)}
        {...position.panHandlers}
      >
        {videoId && (
          <Image
            source={{ uri: `https://i.ytimg.com/vi/${videoId}/hqdefault.jpg` }}
            style={StyleSheet.absoluteFill}
            resizeMode="cover"
          />
        )}
        <Text
          pointerEvents="none"
          onLayout={(event) => setTextSize(event.nativeEvent.layout)}
          style={[
            s.caption,
            {
              left:
                10 +
                (Math.max(0, size.width - textSize.width - 20) *
                  (draft.horizontal + 1)) /
                  2,
              top:
                10 +
                Math.max(0, size.height - textSize.height - 20) *
                  draft.vertical,
              maxWidth: Math.max(1, size.width - 20),
              fontSize: 14 * draft.scale,
              color: draft.color,
              fontWeight: draft.bold ? '700' : '400',
              backgroundColor:
                draft.background === 'none'
                  ? 'transparent'
                  : draft.background === 'opaque'
                    ? '#000'
                    : '#0009',
            },
          ]}
        >
          Subtitle preview
        </Text>
      </View>
      <Text style={s.hint}>Drag the preview to position subtitles</Text>
      <View style={s.labels}>
        <Text style={s.label}>Text size</Text>
        <Text style={s.value}>{Math.floor(draft.scale * 100)}%</Text>
      </View>
      <Range
        label="Subtitle text size"
        value={draft.scale}
        min={0.5}
        max={3}
        color={colors.action}
        trackColor={colors.line}
        onChange={(scale) => {
          edit({ scale });
          flush();
        }}
      />
      <Text style={s.label}>Background</Text>
      <View style={s.chips}>
        {(['none', 'semi', 'opaque'] as const).map((background) => (
          <Pressable
            key={background}
            accessibilityRole="button"
            accessibilityState={{ selected: draft.background === background }}
            onPress={() => {
              edit({ background });
              flush();
            }}
            style={[s.chip, draft.background === background && s.selected]}
          >
            <Text
              style={[
                s.chipText,
                draft.background === background && s.selectedText,
              ]}
            >
              {background === 'none'
                ? 'None'
                : background === 'semi'
                  ? 'Translucent'
                  : 'Opaque'}
            </Text>
          </Pressable>
        ))}
      </View>
      <View style={s.labels}>
        <Text style={s.label}>Bold</Text>
        <Switch
          accessibilityLabel="Bold subtitles"
          value={draft.bold}
          onValueChange={(bold) => {
            edit({ bold });
            flush();
          }}
        />
      </View>
      <ColorPicker
        color={draft.color}
        onChange={(color) => edit({ color })}
        onFinish={flush}
      />
      <View style={s.actions}>
        <Pressable
          accessibilityRole="button"
          onPress={() => {
            edit(defaultMediaSettings.subtitleStyle);
            flush();
          }}
          style={s.action}
        >
          <Text style={s.actionText}>Reset</Text>
        </Pressable>
        <Pressable
          accessibilityRole="button"
          onPress={() => {
            flush();
            onClose();
          }}
          style={s.action}
        >
          <Text style={s.actionText}>Close</Text>
        </Pressable>
      </View>
    </View>
  );
}

function ColorPicker({
  color,
  onChange,
  onFinish,
}: {
  color: string;
  onChange: (color: string) => void;
  onFinish: () => void;
}) {
  const s = useThemeStyles(makeStyles);
  const [selection, setSelection] = useState<Color>({
    color,
    hue: 0,
    saturation: 0,
    value: 1,
    hueColor: '#FF0000',
  });
  const [hex, setHex] = useState(color);
  const [pad, setPad] = useState({ width: 1, height: 1 });
  const [bar, setBar] = useState(1);
  const sequence = useRef(0);
  const current = useRef({ selection, color, onChange });
  current.current = { selection, color, onChange };
  const owned = useRef('');
  useEffect(() => {
    if (owned.current === color) return;
    const request = ++sequence.current;
    void coreRequest<Color>({ op: 'player_caption_color', hex: color })
      .then((result) => {
        if (request !== sequence.current) return;
        setSelection(result);
        setHex(result.color);
      })
      .catch(() => {});
  }, [color]);
  useEffect(
    () => () => {
      ++sequence.current;
    },
    [],
  );
  const convert = (values: Record<string, unknown>, updateHex = true) => {
    const request = ++sequence.current;
    void coreRequest<Color>({ op: 'player_caption_color', ...values })
      .then((result) => {
        if (request !== sequence.current) return;
        current.current.selection = result;
        owned.current = result.color;
        setSelection(result);
        if (updateHex) setHex(result.color);
        current.current.onChange(result.color);
      })
      .catch(() => {});
  };
  const pick = (changes: Partial<Color>) => {
    const next = { ...current.current.selection, ...changes };
    current.current.selection = next;
    setSelection(next);
    convert({ hue: next.hue, saturation: next.saturation, value: next.value });
  };
  return (
    <View style={s.picker}>
      <Text style={s.label}>Color</Text>
      <View
        accessibilityLabel="Subtitle saturation and brightness"
        style={[s.pad, { backgroundColor: selection.hueColor }]}
        onLayout={(event) => setPad(event.nativeEvent.layout)}
        onStartShouldSetResponder={() => true}
        onResponderGrant={(event) =>
          pick({
            saturation: Math.max(
              0,
              Math.min(1, event.nativeEvent.locationX / pad.width),
            ),
            value:
              1 -
              Math.max(
                0,
                Math.min(1, event.nativeEvent.locationY / pad.height),
              ),
          })
        }
        onResponderMove={(event) =>
          pick({
            saturation: Math.max(
              0,
              Math.min(1, event.nativeEvent.locationX / pad.width),
            ),
            value:
              1 -
              Math.max(
                0,
                Math.min(1, event.nativeEvent.locationY / pad.height),
              ),
          })
        }
        onResponderRelease={onFinish}
        onResponderTerminationRequest={() => false}
      >
        <LinearGradient
          pointerEvents="none"
          colors={['#FFFFFF', '#FFFFFF00']}
          start={{ x: 0, y: 0 }}
          end={{ x: 1, y: 0 }}
          style={StyleSheet.absoluteFill}
        />
        <LinearGradient
          pointerEvents="none"
          colors={['#00000000', '#000000']}
          style={StyleSheet.absoluteFill}
        />
        <View
          pointerEvents="none"
          style={[
            s.thumb,
            {
              left: selection.saturation * (pad.width - 1) - 8,
              top: (1 - selection.value) * (pad.height - 1) - 8,
            },
          ]}
        />
      </View>
      <View
        accessibilityRole="adjustable"
        accessibilityLabel="Subtitle color hue"
        accessibilityValue={{ min: 0, max: 360, now: selection.hue }}
        accessibilityActions={[{ name: 'increment' }, { name: 'decrement' }]}
        onAccessibilityAction={(event) =>
          pick({
            hue: Math.max(
              0,
              Math.min(
                360,
                selection.hue +
                  (event.nativeEvent.actionName === 'increment' ? 10 : -10),
              ),
            ),
          })
        }
        style={s.hue}
        onLayout={(event) => setBar(event.nativeEvent.layout.width)}
        onStartShouldSetResponder={() => true}
        onResponderGrant={(event) =>
          pick({
            hue:
              Math.max(0, Math.min(1, event.nativeEvent.locationX / bar)) * 360,
          })
        }
        onResponderMove={(event) =>
          pick({
            hue:
              Math.max(0, Math.min(1, event.nativeEvent.locationX / bar)) * 360,
          })
        }
        onResponderRelease={onFinish}
        onResponderTerminationRequest={() => false}
      >
        <LinearGradient
          pointerEvents="none"
          colors={['#f00', '#ff0', '#0f0', '#0ff', '#00f', '#f0f', '#f00']}
          start={{ x: 0, y: 0 }}
          end={{ x: 1, y: 0 }}
          style={StyleSheet.absoluteFill}
        />
        <View
          pointerEvents="none"
          style={[s.hueThumb, { left: (selection.hue / 360) * (bar - 4) }]}
        />
      </View>
      <View style={s.hex}>
        <Text style={s.legend}>Hex color</Text>
        <TextInput
          accessibilityLabel="Subtitle color hex"
          value={hex}
          autoCapitalize="characters"
          autoCorrect={false}
          maxLength={9}
          style={s.input}
          onChangeText={(text) => {
            setHex(text);
            convert({ hex: text }, false);
          }}
          onBlur={() => {
            setHex(current.current.color);
            onFinish();
          }}
          onSubmitEditing={onFinish}
        />
        <View
          pointerEvents="none"
          style={[s.swatch, { backgroundColor: color }]}
        />
      </View>
    </View>
  );
}

const makeStyles = (colors: ThemeColors) =>
  StyleSheet.create({
    editor: { paddingHorizontal: 20, paddingBottom: 12 },
    preview: {
      borderRadius: 12,
      overflow: 'hidden',
      backgroundColor: colors.line,
    },
    caption: {
      position: 'absolute',
      paddingHorizontal: 5,
      paddingVertical: 2,
      textAlign: 'center',
      textShadowColor: '#000',
      textShadowRadius: 2,
    },
    hint: { color: colors.muted, fontSize: 12, marginTop: 6, marginBottom: 4 },
    labels: {
      flexDirection: 'row',
      alignItems: 'center',
      justifyContent: 'space-between',
      marginTop: 8,
    },
    label: { color: colors.muted, fontSize: 14 },
    value: { color: colors.ink, fontSize: 12 },
    chips: { flexDirection: 'row', gap: 8, marginTop: 4 },
    chip: {
      flex: 1,
      paddingVertical: 8,
      alignItems: 'center',
      borderRadius: 8,
      backgroundColor: colors.raised,
    },
    chipText: { color: colors.ink, fontSize: 13 },
    selected: { backgroundColor: colors.ink },
    selectedText: { color: colors.surface },
    picker: { paddingTop: 10 },
    pad: { height: 132, overflow: 'hidden', borderRadius: 8, marginTop: 8 },
    thumb: {
      position: 'absolute',
      width: 16,
      height: 16,
      borderWidth: 2,
      borderColor: 'white',
      borderRadius: 8,
    },
    hue: { height: 20, borderRadius: 10, marginTop: 10, overflow: 'hidden' },
    hueThumb: {
      position: 'absolute',
      top: 0,
      bottom: 0,
      width: 4,
      borderRadius: 2,
      backgroundColor: 'white',
    },
    hex: {
      borderWidth: 1,
      borderColor: '#888',
      borderRadius: 4,
      marginTop: 12,
      flexDirection: 'row',
      alignItems: 'center',
    },
    legend: {
      position: 'absolute',
      left: 12,
      top: -9,
      backgroundColor: colors.surface,
      color: colors.muted,
      paddingHorizontal: 4,
      fontSize: 12,
    },
    input: {
      flex: 1,
      color: colors.ink,
      fontSize: 16,
      paddingHorizontal: 16,
      paddingVertical: 14,
    },
    swatch: {
      width: 24,
      height: 24,
      borderRadius: 12,
      borderWidth: 1,
      borderColor: '#888',
      marginRight: 12,
    },
    actions: {
      flexDirection: 'row',
      justifyContent: 'flex-end',
      paddingTop: 8,
    },
    action: { paddingHorizontal: 16, paddingVertical: 12 },
    actionText: { color: colors.action, fontSize: 14, fontWeight: '500' },
  });
