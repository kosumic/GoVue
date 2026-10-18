import { useEffect, useRef, useState } from 'react';
import {
  ActivityIndicator,
  Alert,
  FlatList,
  Image,
  Modal,
  PanResponder,
  Pressable,
  StyleSheet,
  Text,
  useWindowDimensions,
  View,
} from 'react-native';
import {
  SafeAreaView,
  useSafeAreaInsets,
} from 'react-native-safe-area-context';
import { ArrowLeft, Download, RotateCcw, Share2 } from 'lucide-react-native';
import { IconButton } from './IconButton';
import { colors, ThemeContext } from '../theme';
import {
  exportGalleryImage,
  loadGalleryImage,
  type GalleryImage,
  type GalleryTarget,
} from '../core/gallery';

export function Gallery({
  target,
  onClose,
}: {
  target: GalleryTarget;
  onClose: () => void;
}) {
  const screen = useWindowDimensions();
  const insets = useSafeAreaInsets();
  const width = Math.max(1, screen.width - insets.left - insets.right);
  const [index, setIndex] = useState(target.index);
  const [currentImage, setCurrentImage] = useState<GalleryImage | null>(null);
  const [busy, setBusy] = useState(false);
  const [zoomed, setZoomed] = useState(false);
  const current = useRef({ target, index });
  current.current = { target, index };
  const images = useRef(new Map<string, Promise<GalleryImage>>());
  const load = (url: string, retry = false) => {
    if (retry) images.current.delete(url);
    let pending = images.current.get(url);
    if (!pending) {
      pending = loadGalleryImage(url);
      images.current.set(url, pending);
      void pending.catch(() => {
        images.current.delete(url);
      });
    }
    return pending;
  };
  const act = async (save: boolean) => {
    if (!currentImage || busy) return;
    setBusy(true);
    try {
      await exportGalleryImage(currentImage, save);
    } catch (error) {
      Alert.alert(
        'GoVue',
        error instanceof Error ? error.message : 'Image could not be saved.',
      );
    } finally {
      setBusy(false);
    }
  };
  return (
    <ThemeContext.Provider value={colors}>
      <Modal visible={!!target} animationType="fade" onRequestClose={onClose}>
        <SafeAreaView style={s.safe}>
          <View style={s.heading}>
            <IconButton
              icon={ArrowLeft}
              label="Back from gallery"
              onPress={onClose}
            />
            <Text accessibilityRole="header" style={s.count}>
              {target ? index + 1 : 0} / {target?.urls.length || 0}
            </Text>
            {busy ? (
              <ActivityIndicator color="#eee" style={s.busy} />
            ) : (
              <>
                <IconButton
                  icon={Download}
                  label="Save image"
                  disabled={!currentImage}
                  onPress={() => {
                    void act(true);
                  }}
                />
                <IconButton
                  icon={Share2}
                  label="Share image"
                  disabled={!currentImage}
                  onPress={() => {
                    void act(false);
                  }}
                />
              </>
            )}
          </View>
          {target && (
            <FlatList
              key={width}
              data={target.urls}
              horizontal
              pagingEnabled
              scrollEnabled={!zoomed}
              showsHorizontalScrollIndicator={false}
              initialScrollIndex={index}
              getItemLayout={(_data, i) => ({
                length: width,
                offset: width * i,
                index: i,
              })}
              keyExtractor={(_url, i) => String(i)}
              windowSize={3}
              initialNumToRender={3}
              maxToRenderPerBatch={3}
              onMomentumScrollEnd={(event) => {
                const next = Math.max(
                  0,
                  Math.min(
                    target.urls.length - 1,
                    Math.round(event.nativeEvent.contentOffset.x / width),
                  ),
                );
                if (next !== index) {
                  setIndex(next);
                  setCurrentImage(null);
                  setZoomed(false);
                }
              }}
              renderItem={({ item: url, index: page }) => (
                <ImagePage
                  url={url}
                  page={page}
                  active={page === index}
                  load={load}
                  shouldLoad={Math.abs(page - index) <= 1}
                  width={width}
                  onImage={(image) => {
                    if (
                      current.current.target?.urls[current.current.index] ===
                      url
                    )
                      setCurrentImage(image);
                  }}
                  onZoom={(value) => {
                    if (current.current.index === page) setZoomed(value);
                  }}
                />
              )}
            />
          )}
        </SafeAreaView>
      </Modal>
    </ThemeContext.Provider>
  );
}
function ImagePage({
  url,
  page,
  active,
  shouldLoad,
  width,
  load,
  onImage,
  onZoom,
}: {
  url: string;
  page: number;
  active: boolean;
  shouldLoad: boolean;
  width: number;
  load: (url: string, retry?: boolean) => Promise<GalleryImage>;
  onImage: (image: GalleryImage) => void;
  onZoom: (zoomed: boolean) => void;
}) {
  const [image, setImage] = useState<GalleryImage | null>(null);
  const [error, setError] = useState(false);
  const [retry, setRetry] = useState(0);
  const [height, setHeight] = useState(1);
  const [natural, setNatural] = useState({ width: 1, height: 1 });
  const [transform, setTransform] = useState({ scale: 1, x: 0, y: 0 });
  const callbacks = useRef({ load, onImage, onZoom });
  callbacks.current = { load, onImage, onZoom };
  const state = useRef({ transform, width, height, natural });
  state.current = { transform, width, height, natural };
  const origin = useRef({ scale: 1, x: 0, y: 0, pinch: 0 });
  const lastTap = useRef(0);
  useEffect(() => {
    if (!shouldLoad) return;
    let live = true;
    setError(false);
    void callbacks.current
      .load(url, retry > 0)
      .then((value) => {
        if (!live) return;
        setImage(value);
        void Image.getSize(value.uri)
          .then((size) => {
            if (live) setNatural(size);
          })
          .catch(() => {
            if (live) setError(true);
          });
      })
      .catch(() => {
        if (live) setError(true);
      });
    return () => {
      live = false;
    };
  }, [url, shouldLoad, retry]);
  useEffect(() => {
    if (active && image) callbacks.current.onImage(image);
    if (!active) {
      setTransform({ scale: 1, x: 0, y: 0 });
    } else callbacks.current.onZoom(false);
  }, [active, image]);
  const change = (next: { scale: number; x: number; y: number }) => {
    const c = state.current;
    const fit = Math.min(
      c.width / c.natural.width,
      c.height / c.natural.height,
    );
    const xBound = Math.max(
      0,
      (c.natural.width * fit * next.scale - c.width) / 2,
    );
    const yBound = Math.max(
      0,
      (c.natural.height * fit * next.scale - c.height) / 2,
    );
    const clamped = {
      ...next,
      x: Math.max(-xBound, Math.min(xBound, next.x)),
      y: Math.max(-yBound, Math.min(yBound, next.y)),
    };
    state.current.transform = clamped;
    setTransform(clamped);
    callbacks.current.onZoom(next.scale > 1.01);
  };
  const pan = useRef(
    PanResponder.create({
      onStartShouldSetPanResponder: (e) =>
        e.nativeEvent.touches.length > 1 ||
        state.current.transform.scale > 1.01,
      onMoveShouldSetPanResponder: (e) =>
        e.nativeEvent.touches.length > 1 ||
        state.current.transform.scale > 1.01,
      onPanResponderGrant: (e) => {
        const points = e.nativeEvent.touches;
        origin.current = {
          ...state.current.transform,
          pinch:
            points.length > 1
              ? Math.hypot(
                  points[1].pageX - points[0].pageX,
                  points[1].pageY - points[0].pageY,
                )
              : 0,
        };
      },
      onPanResponderMove: (e, g) => {
        const points = e.nativeEvent.touches;
        if (points.length > 1) {
          const distance = Math.hypot(
            points[1].pageX - points[0].pageX,
            points[1].pageY - points[0].pageY,
          );
          if (!origin.current.pinch) {
            origin.current = { ...state.current.transform, pinch: distance };
            return;
          }
          change({
            ...origin.current,
            scale: Math.max(
              1,
              Math.min(
                5,
                (origin.current.scale * distance) / origin.current.pinch,
              ),
            ),
          });
        } else if (!origin.current.pinch)
          change({
            ...origin.current,
            x: origin.current.x + g.dx,
            y: origin.current.y + g.dy,
          });
      },
    }),
  ).current;
  const zoom = () => change({ scale: transform.scale > 1 ? 1 : 2, x: 0, y: 0 });
  return (
    <View
      style={[s.page, { width }]}
      onLayout={(event) => setHeight(event.nativeEvent.layout.height)}
      {...pan.panHandlers}
    >
      {error ? (
        <View style={s.center}>
          <Text style={s.muted}>Image could not load</Text>
          <Pressable
            accessibilityRole="button"
            style={s.retry}
            onPress={() => setRetry((v) => v + 1)}
          >
            <Text style={s.text}>Retry</Text>
          </Pressable>
        </View>
      ) : image ? (
        <Pressable
          style={StyleSheet.absoluteFill}
          accessibilityRole="image"
          accessibilityLabel={`Image ${page + 1}`}
          accessibilityActions={[
            { name: 'activate', label: 'Zoom image' },
            { name: 'reset', label: 'Reset zoom' },
          ]}
          onAccessibilityAction={(event) =>
            event.nativeEvent.actionName === 'reset'
              ? change({ scale: 1, x: 0, y: 0 })
              : zoom()
          }
          onPress={() => {
            const now = Date.now();
            if (now - lastTap.current < 300) {
              lastTap.current = 0;
              zoom();
            } else lastTap.current = now;
          }}
        >
          <Image
            source={{ uri: image.uri }}
            resizeMode="contain"
            resizeMethod="resize"
            onError={() => setError(true)}
            style={[
              StyleSheet.absoluteFill,
              {
                transform: [
                  { translateX: transform.x },
                  { translateY: transform.y },
                  { scale: transform.scale },
                ],
              },
            ]}
          />
        </Pressable>
      ) : (
        <ActivityIndicator color="#eee" style={s.center} />
      )}
      {transform.scale > 1 && (
        <Pressable
          accessibilityRole="button"
          accessibilityLabel="Reset image zoom"
          style={s.reset}
          onPress={() => change({ scale: 1, x: 0, y: 0 })}
        >
          <RotateCcw color="#eee" size={18} />
          <Text style={s.text}>Reset zoom · {transform.scale.toFixed(1)}×</Text>
        </Pressable>
      )}
    </View>
  );
}
const s = StyleSheet.create({
  safe: { flex: 1, backgroundColor: '#0f0f0f' },
  heading: {
    height: 56,
    flexDirection: 'row',
    alignItems: 'center',
    paddingHorizontal: 4,
  },
  count: { flex: 1, color: '#eee', fontSize: 20, paddingLeft: 8 },
  busy: { width: 88 },
  page: {
    flex: 1,
    overflow: 'hidden',
    backgroundColor: '#000',
    justifyContent: 'center',
  },
  center: { alignItems: 'center', justifyContent: 'center', gap: 12 },
  text: { color: '#eee', fontSize: 14 },
  muted: { color: '#aaa', fontSize: 14 },
  retry: { padding: 16 },
  reset: {
    position: 'absolute',
    bottom: 24,
    alignSelf: 'center',
    flexDirection: 'row',
    gap: 8,
    backgroundColor: '#282828dd',
    borderRadius: 24,
    padding: 12,
  },
});
