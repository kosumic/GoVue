import { useEffect, useRef, useState } from 'react';
import { Animated, Easing, StyleSheet, Text, View } from 'react-native';
import Svg, { Ellipse, Path } from 'react-native-svg';
import { Brightness, Speaker } from './icons';
import { useReducedMotion } from './useReducedMotion';

export type GestureFeedback =
  | { kind: 'scrub'; target: number; delta: number }
  | { kind: 'edge'; volume: boolean; percent: number }
  | { kind: 'double'; forward: boolean; delta: number; sequence: number }
  | { kind: 'speed' };

const time = (seconds: number) => {
  const value = Math.max(0, Math.floor(seconds));
  return `${value >= 3600 ? `${Math.floor(value / 3600)}:` : ''}${value >= 3600 ? String(Math.floor(value / 60) % 60).padStart(2, '0') : Math.floor(value / 60)}:${String(value % 60).padStart(2, '0')}`;
};
const offset = (seconds: number) =>
  `${seconds >= 0 ? '+' : ''}${Math.round(seconds)}s`;

function Arrows({ forward, reduced }: { forward: boolean; reduced: boolean }) {
  const phase = useRef(new Animated.Value(0)).current;
  useEffect(() => {
    phase.setValue(0);
    if (reduced) return;
    const loop = Animated.loop(
      Animated.timing(phase, {
        toValue: 1,
        duration: 480,
        isInteraction: false,
        easing: Easing.linear,
        useNativeDriver: true,
      }),
    );
    loop.start();
    return () => loop.stop();
  }, [forward, reduced, phase]);
  return (
    <View style={s.arrows}>
      {[0, 1].map((index) => {
        const order = forward ? index : 1 - index;
        return (
          <Animated.View
            key={index}
            style={{
              opacity: reduced
                ? 1
                : phase.interpolate({
                    inputRange: [0, 0.25, 0.5, 0.75, 1],
                    outputRange:
                      order === 0
                        ? [0.625, 1, 0.625, 0.25, 0.625]
                        : [0.625, 0.25, 0.625, 1, 0.625],
                  }),
            }}
          >
            <Svg width={14} height={24} viewBox="0 0 14 24">
              <Path
                d={forward ? 'M4 5L13 12L4 19Z' : 'M13 5L4 12L13 19Z'}
                fill="white"
              />
            </Svg>
          </Animated.View>
        );
      })}
    </View>
  );
}

function DoubleSeek({
  feedback,
  width,
  height,
  reduced,
}: {
  feedback: Extract<GestureFeedback, { kind: 'double' }>;
  width: number;
  height: number;
  reduced: boolean;
}) {
  const ripple = useRef(new Animated.Value(1)).current;
  useEffect(() => {
    ripple.stopAnimation();
    if (reduced) {
      ripple.setValue(1);
      return;
    }
    ripple.setValue(0);
    Animated.timing(ripple, {
      toValue: 1,
      duration: 400,
      isInteraction: false,
      easing: Easing.linear,
      useNativeDriver: true,
    }).start();
    return () => ripple.stopAnimation();
  }, [feedback.sequence, reduced, ripple]);
  const sideWidth = width * 0.42,
    radius = Math.min(sideWidth, height);
  return (
    <View
      style={[
        s.double,
        { width: sideWidth, height },
        feedback.forward ? { right: 0 } : { left: 0 },
      ]}
    >
      <Svg width={sideWidth} height={height} style={StyleSheet.absoluteFill}>
        <Ellipse
          cx={sideWidth * (feedback.forward ? 0.88 : 0.12)}
          cy={height * 0.5}
          rx={sideWidth * 0.8}
          ry={height * 0.7}
          fill="#30303066"
        />
      </Svg>
      {!reduced && (
        <Animated.View
          style={{
            position: 'absolute',
            width: radius * 2,
            height: radius * 2,
            borderRadius: radius,
            left: sideWidth * (feedback.forward ? 0.65 : 0.35) - radius,
            top: height / 2 - radius,
            backgroundColor: 'white',
            opacity: ripple.interpolate({
              inputRange: [0, 1],
              outputRange: [0.14, 0],
            }),
            transform: [
              {
                scale: ripple.interpolate({
                  inputRange: [0, 1],
                  outputRange: [0.2, 1],
                }),
              },
            ],
          }}
        />
      )}
      <View style={s.doubleLabel}>
        <Arrows forward={feedback.forward} reduced={reduced} />
        <Text style={s.doubleText}>{offset(feedback.delta)}</Text>
      </View>
    </View>
  );
}

export function GestureOverlay({
  feedback,
  width,
  height,
  inset = 0,
}: {
  feedback: GestureFeedback | null;
  width: number;
  height: number;
  inset?: number;
}) {
  const reduced = useReducedMotion();
  const [bubbleHeight, setBubbleHeight] = useState(58);
  if (!feedback) return null;
  const side = Math.max(24, Math.floor(width * 0.12), inset);
  return (
    <View
      pointerEvents="none"
      accessibilityElementsHidden
      importantForAccessibility="no-hide-descendants"
      style={StyleSheet.absoluteFill}
    >
      {feedback.kind === 'double' && (
        <DoubleSeek
          feedback={feedback}
          width={width}
          height={height}
          reduced={reduced}
        />
      )}
      {feedback.kind === 'speed' && (
        <View style={[s.speed, { top: Math.max(16, inset) }]}>
          <Text style={s.speedText}>2×</Text>
          <Arrows forward reduced={reduced} />
        </View>
      )}
      {feedback.kind === 'scrub' && (
        <View
          onLayout={(event) => setBubbleHeight(event.nativeEvent.layout.height)}
          style={[s.scrub, { top: (height - bubbleHeight) * 0.25 }]}
        >
          <Text style={s.target}>{time(feedback.target)}</Text>
          <Text style={s.delta}>{offset(feedback.delta)}</Text>
        </View>
      )}
      {feedback.kind === 'edge' && (
        <View
          style={[s.edge, feedback.volume ? { left: side } : { right: side }]}
        >
          <View style={s.edgePill}>
            {feedback.volume ? (
              <Speaker
                size={18}
                color={feedback.percent > 0 ? 'white' : '#ffffff66'}
              />
            ) : (
              <Brightness
                size={18}
                color={feedback.percent > 0 ? 'white' : '#ffffff66'}
              />
            )}
            <View
              style={[
                s.track,
                { height: Math.max(48, Math.min(112, height * 0.26)) },
              ]}
            >
              <View
                style={[
                  s.fill,
                  {
                    height: `${Math.max(0, Math.min(100, feedback.percent))}%`,
                  },
                ]}
              />
            </View>
            <Text style={s.percent}>{feedback.percent}%</Text>
          </View>
        </View>
      )}
    </View>
  );
}

const s = StyleSheet.create({
  arrows: { width: 32, height: 24, flexDirection: 'row' },
  double: {
    position: 'absolute',
    top: 0,
    overflow: 'hidden',
    alignItems: 'center',
    justifyContent: 'center',
  },
  doubleLabel: { alignItems: 'center', gap: 4 },
  doubleText: { color: 'white', fontSize: 14, fontWeight: '500' },
  speed: {
    position: 'absolute',
    alignSelf: 'center',
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
    backgroundColor: '#000000b8',
    borderRadius: 100,
    paddingLeft: 16,
    paddingRight: 10,
    paddingVertical: 8,
  },
  speedText: { color: 'white', fontSize: 14, fontWeight: '600' },
  scrub: {
    position: 'absolute',
    alignSelf: 'center',
    alignItems: 'center',
    backgroundColor: '#0006',
    borderColor: '#ffffff24',
    borderWidth: 0.5,
    borderRadius: 16,
    paddingHorizontal: 14,
    paddingVertical: 5,
  },
  target: { color: 'white', fontSize: 18, fontWeight: '500' },
  delta: { color: '#ffffffbf', fontSize: 13 },
  edge: { position: 'absolute', top: 0, bottom: 0, justifyContent: 'center' },
  edgePill: {
    alignItems: 'center',
    gap: 8,
    backgroundColor: '#0006',
    borderColor: '#ffffff24',
    borderWidth: 0.5,
    borderRadius: 16,
    padding: 6,
  },
  track: {
    width: 4,
    backgroundColor: '#fff3',
    borderRadius: 2,
    overflow: 'hidden',
    justifyContent: 'flex-end',
  },
  fill: { width: 4, backgroundColor: 'white' },
  percent: {
    minWidth: 36,
    color: 'white',
    fontSize: 13,
    fontWeight: '500',
    textAlign: 'center',
  },
});
