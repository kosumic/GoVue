import { useRef, useState } from 'react';
import { Image, PanResponder, Pressable, StyleSheet, Text, View, type NativeTouchEvent, type ViewInstance } from 'react-native';
import { useThemeStyles, type ThemeColors } from '../theme';

type Transform = { scale: number; x: number; y: number };
const initial: Transform = { scale: 1, x: 0, y: 0 };
function points(touches: readonly NativeTouchEvent[]) {
  const [a, b] = touches;
  return { distance: Math.hypot(b.pageX - a.pageX, b.pageY - a.pageY), x: (a.pageX + b.pageX) / 2, y: (a.pageY + b.pageY) / 2 };
}
/** Shared presentation gestures, bounded to the contained image on either platform. */
export function ZoomableChatImage({ uri, name, width, height }: { uri: string; name: string; width: number; height: number }) {
  const styles = useThemeStyles(makeStyles);
  const view = useRef<ViewInstance>(null);
  const frame = useRef({ x: 0, y: 0, width: 1, height: 1 });
  const [transform, setTransform] = useState(initial);
  const current = useRef(initial), natural = useRef({ width, height });
  natural.current = { width, height };
  const origin = useRef({ transform: initial, pinch: 0, x: 0, y: 0 });
  const lastTap = useRef(0);
  const change = (next: Transform) => {
    const bounds = frame.current, image = natural.current;
    const fit = Math.min(bounds.width / image.width, bounds.height / image.height);
    const scale = Math.max(1, Math.min(5, next.scale));
    const maxX = Math.max(0, (image.width * fit * scale - bounds.width) / 2);
    const maxY = Math.max(0, (image.height * fit * scale - bounds.height) / 2);
    current.current = { scale, x: Math.max(-maxX, Math.min(maxX, next.x)), y: Math.max(-maxY, Math.min(maxY, next.y)) };
    setTransform(current.current);
  };
  const pan = useRef(PanResponder.create({
    onStartShouldSetPanResponder: () => true,
    onMoveShouldSetPanResponder: () => true,
    onPanResponderGrant: event => {
      const touches = event.nativeEvent.touches;
      origin.current = { transform: current.current, pinch: 0, x: 0, y: 0 };
      if (touches.length > 1) { const p = points(touches); origin.current = { transform: current.current, pinch: p.distance, x: p.x, y: p.y }; }
    },
    onPanResponderMove: (event, gesture) => {
      const touches = event.nativeEvent.touches;
      if (touches.length > 1) {
        const p = points(touches);
        if (!origin.current.pinch) { origin.current = { transform: current.current, pinch: p.distance, x: p.x, y: p.y }; return; }
        const start = origin.current, bounds = frame.current;
        const scale = Math.max(1, Math.min(5, start.transform.scale * p.distance / start.pinch));
        const ratio = scale / start.transform.scale;
        change({ scale,
          x: p.x - bounds.x - bounds.width / 2 - (start.x - bounds.x - bounds.width / 2 - start.transform.x) * ratio,
          y: p.y - bounds.y - bounds.height / 2 - (start.y - bounds.y - bounds.height / 2 - start.transform.y) * ratio });
        lastTap.current = 0;
      } else if (!origin.current.pinch) {
        change({ ...origin.current.transform, x: origin.current.transform.x + gesture.dx, y: origin.current.transform.y + gesture.dy });
      }
    },
    onPanResponderRelease: (_event, gesture) => {
      if (!origin.current.pinch && Math.abs(gesture.dx) < 8 && Math.abs(gesture.dy) < 8) {
        const now = Date.now();
        if (lastTap.current && now - lastTap.current < 300) { change(current.current.scale > 1 ? initial : { ...initial, scale: 2 }); lastTap.current = 0; }
        else lastTap.current = now;
      } else lastTap.current = 0;
    },
    onPanResponderTerminationRequest: () => false,
  })).current;
  return <View style={styles.container}>
    <View ref={view} style={styles.image} {...pan.panHandlers} accessibilityRole="image" accessibilityLabel={name}
      accessibilityValue={{ text: `${transform.scale.toFixed(1)} times zoom` }}
      accessibilityActions={[{ name: 'activate', label: 'Zoom image' }, { name: 'reset', label: 'Reset zoom' }]}
      onAccessibilityAction={event => change(event.nativeEvent.actionName === 'reset' || current.current.scale > 1 ? initial : { ...initial, scale: 2 })}
      onLayout={event => {
        const { width: w, height: h } = event.nativeEvent.layout;
        frame.current = { ...frame.current, width: w, height: h };
        view.current?.measureInWindow((x, y) => { frame.current = { ...frame.current, x, y }; change(current.current); });
      }}>
      <Image pointerEvents="none" source={{ uri }} resizeMode="contain" style={[StyleSheet.absoluteFill, { transform: [{ translateX: transform.x }, { translateY: transform.y }, { scale: transform.scale }] }]} />
    </View>
    <Pressable accessibilityRole="button" accessibilityLabel="Reset image zoom" onPress={() => change(initial)} style={styles.reset}>
      <Text style={styles.help}>{transform.scale > 1 ? `Reset zoom · ${transform.scale.toFixed(1)}×` : 'Pinch or double-tap to zoom'}</Text>
    </Pressable>
  </View>;
}
const makeStyles = (colors: ThemeColors) => StyleSheet.create({ container: { flex: 1 }, image: { flex: 1, overflow: 'hidden' }, reset: { padding: 14, alignItems: 'center' }, help: { color: colors.muted, fontSize: 13 } });
