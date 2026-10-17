import { useEffect, useRef, useState } from 'react';
import { Animated, Easing, PanResponder, StyleSheet, View } from 'react-native';
import { LinearGradient } from 'expo-linear-gradient';
import { coreRequest } from '../../modules/lingoplay-core';
import { Delete } from './icons';
import { haptic } from './Haptics';

type Frame = { x: number; y: number; width: number };
type Layout = Frame & {
  height: number;
  right: number;
  bottom: number;
  closeArmed: boolean;
  dismiss: boolean;
  close: { left: number; top: number; width: number; height: number };
};
type Mode = 'drag' | 'release' | 'settle';
type Request = {
  frame?: Frame;
  mode: Mode;
  pinch?: boolean;
  generation: number;
};

/** Pointer input and animation stay in RN; Rust clamps, docks and stores placement. */
export function useMiniPlayer({
  size,
  bottomInset,
  active,
  strength,
  onDismiss,
  onError,
}: {
  size: { width: number; height: number };
  bottomInset: number;
  active: boolean;
  strength: number;
  onDismiss: () => void;
  onError: (error: unknown) => void;
}) {
  const [layout, setLayout] = useState<Layout>({
    x: 0,
    y: 0,
    width: 190,
    height: 106,
    right: 12,
    bottom: 68,
    closeArmed: false,
    dismiss: false,
    close: { left: 0, top: 0, width: 96, height: 72 },
  });
  const frame = useRef(layout);
  const motion = useRef(new Animated.ValueXY()).current;
  const scale = useRef(new Animated.Value(1)).current;
  const opacity = useRef(new Animated.Value(1)).current;
  const [dragging, setDragging] = useState(false);
  const [armed, setArmed] = useState(false);
  const draggingRef = useRef(false),
    armedRef = useRef(false),
    closing = useRef(false);
  const generation = useRef(0),
    mounted = useRef(true);
  const pending = useRef<Request | null>(null),
    running = useRef(false);
  const origin = useRef<(Frame & { pinch: number }) | null>(null);
  const desired = useRef<Frame | null>(null);
  const holding = useRef(false);
  const latest = useRef({
    size,
    bottomInset,
    active,
    strength,
    onDismiss,
    onError,
  });
  latest.current = { size, bottomInset, active, strength, onDismiss, onError };
  const apply = (next: Layout, request: Request) => {
    frame.current = next;
    // Coordinate ticks travel through animated values without rendering the browser.
    setLayout((previous) =>
      (['width', 'height', 'right', 'bottom'] as const).every(
        (key) => previous[key] === next[key],
      ) &&
      previous.close.left === next.close.left &&
      previous.close.top === next.close.top
        ? previous
        : next,
    );
    if (next.closeArmed && !armedRef.current && draggingRef.current)
      haptic(latest.current.strength);
    armedRef.current = next.closeArmed;
    setArmed(next.closeArmed);
    if (next.dismiss) {
      closing.current = true;
      motion.setValue({ x: next.x, y: next.y });
      Animated.parallel([
        Animated.timing(scale, {
          toValue: 0.1,
          duration: 240,
          useNativeDriver: true,
        }),
        Animated.timing(opacity, {
          toValue: 0,
          duration: 240,
          useNativeDriver: true,
        }),
      ]).start(({ finished }) => {
        if (
          finished &&
          mounted.current &&
          request.generation === generation.current
        )
          latest.current.onDismiss();
      });
    } else if (request.mode === 'drag') {
      motion.setValue({ x: next.x, y: next.y });
    } else {
      Animated.timing(motion, {
        toValue: { x: next.x, y: next.y },
        duration: 260,
        easing: Easing.bezier(0.2, 0, 0, 1),
        useNativeDriver: true,
      }).start();
    }
  };
  const submit = (request: Request) => {
    pending.current = request;
    if (running.current) return;
    running.current = true;
    void (async () => {
      try {
        while (pending.current && mounted.current) {
          const request = pending.current;
          pending.current = null;
          const current = latest.current;
          try {
            const next = await coreRequest<Layout>({
              op: 'player_mini',
              width: current.size.width,
              height: current.size.height,
              bottomInset: current.bottomInset,
              frame: request.frame,
              mode: request.mode,
              pinch: request.pinch,
            });
            if (mounted.current && request.generation === generation.current)
              apply(next, request);
          } catch (error) {
            if (
              mounted.current &&
              request.generation === generation.current &&
              request.mode !== 'drag'
            )
              latest.current.onError(error);
          }
        }
      } finally {
        running.current = false;
      }
    })();
  };
  const finish = (cancelled: boolean) => {
    holding.current = false;
    draggingRef.current = false;
    setDragging(false);
    if (!origin.current || !latest.current.active || closing.current) return;
    const pinch = origin.current.pinch > 0;
    origin.current = null;
    submit({
      frame: desired.current || frame.current,
      mode: cancelled ? 'settle' : 'release',
      pinch,
      generation: generation.current,
    });
  };
  const handlers = useRef({ submit, finish });
  handlers.current = { submit, finish };
  const responder = useRef(
    PanResponder.create({
      onMoveShouldSetPanResponder: (_event, gesture) =>
        latest.current.active &&
        !closing.current &&
        (Math.abs(gesture.dx) > 8 ||
          Math.abs(gesture.dy) > 8 ||
          gesture.numberActiveTouches > 1),
      onPanResponderGrant: (event) => {
        generation.current++;
        const owner = generation.current;
        holding.current = true;
        pending.current = null;
        const points = event.nativeEvent.touches;
        const pinch =
          points.length > 1
            ? Math.hypot(
                points[1].pageX - points[0].pageX,
                points[1].pageY - points[0].pageY,
              )
            : 0;
        origin.current = null;
        desired.current = null;
        draggingRef.current = !pinch;
        setDragging(!pinch);
        motion.stopAnimation((position) => {
          if (
            !latest.current.active ||
            !holding.current ||
            generation.current !== owner
          )
            return;
          origin.current = { ...frame.current, ...position, pinch };
          desired.current = origin.current;
        });
      },
      onPanResponderMove: (event, gesture) => {
        const start = origin.current;
        if (!start || closing.current) return;
        const points = event.nativeEvent.touches;
        if (points.length > 1) {
          draggingRef.current = false;
          setDragging(false);
          const distance = Math.hypot(
            points[1].pageX - points[0].pageX,
            points[1].pageY - points[0].pageY,
          );
          if (!start.pinch) {
            origin.current = { ...frame.current, pinch: distance };
            return;
          }
          desired.current = {
            x: start.x,
            y: start.y,
            width: (start.width * distance) / start.pinch,
          };
        } else {
          if (start.pinch) return;
          desired.current = {
            x: start.x + gesture.dx,
            y: start.y + gesture.dy,
            width: start.width,
          };
        }
        handlers.current.submit({
          frame: desired.current,
          mode: 'drag',
          pinch: start.pinch > 0,
          generation: generation.current,
        });
      },
      onPanResponderRelease: () => handlers.current.finish(false),
      onPanResponderTerminate: () => handlers.current.finish(true),
      onPanResponderTerminationRequest: () => false,
    }),
  ).current;
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      generation.current++;
      pending.current = null;
      motion.stopAnimation();
      scale.stopAnimation();
      opacity.stopAnimation();
    };
  }, [motion, scale, opacity]);
  useEffect(() => {
    generation.current++;
    pending.current = null;
    origin.current = null;
    holding.current = false;
    draggingRef.current = false;
    setDragging(false);
    closing.current = false;
    scale.stopAnimation();
    opacity.stopAnimation();
    scale.setValue(1);
    opacity.setValue(1);
    if (size.width >= 32 && size.height >= 32)
      submit({ mode: 'settle', generation: generation.current });
  }, [size.width, size.height, bottomInset, active]);
  return {
    dismiss: () => {
      if (!latest.current.active || closing.current) return;
      generation.current++;
      pending.current = null;
      holding.current = false;
      origin.current = null;
      draggingRef.current = false;
      setDragging(false);
      apply(
        { ...frame.current, dismiss: true },
        { mode: 'release', generation: generation.current },
      );
    },
    panHandlers: responder.panHandlers,
    style: {
      width: layout.width,
      height: layout.height,
      right: layout.right,
      bottom: layout.bottom,
      borderRadius: 16,
      borderWidth: 1,
      borderColor: '#ffffff12',
      opacity,
      transform: [
        { translateX: motion.x },
        { translateY: motion.y },
        { scale },
      ],
    },
    dropTarget: active && dragging && (
      <View
        pointerEvents="none"
        style={s.drop}
        accessibilityElementsHidden
        importantForAccessibility="no-hide-descendants"
      >
        <LinearGradient
          colors={
            armed ? ['#ff00008f', '#ff0000b8'] : ['#6060608f', '#606060b8']
          }
          style={s.shade}
        />
        <View style={[s.target, layout.close]}>
          <Delete size={36} color={armed ? 'white' : '#fffffff2'} />
        </View>
      </View>
    ),
  };
}

const s = StyleSheet.create({
  drop: { ...StyleSheet.absoluteFill, zIndex: 1 },
  shade: {
    position: 'absolute',
    left: 0,
    right: 0,
    bottom: 0,
    height: 156,
    borderTopLeftRadius: 24,
    borderTopRightRadius: 24,
  },
  target: {
    position: 'absolute',
    alignItems: 'center',
    justifyContent: 'center',
  },
});
