import { useEffect, useState } from 'react';
import {
  AppState,
  Linking,
  Modal,
  Pressable,
  Share,
  ScrollView,
  StyleSheet,
  Text,
  useWindowDimensions,
  View,
} from 'react-native';
import { Cast, CastConnected } from './icons';
import {
  castDiscoverySnapshot,
  coreRequest,
} from '../../modules/govue-core';
import { useThemeColors, useThemeStyles, type ThemeColors } from '../theme';
import { SheetRow } from './PlayerSheet';
import {
  castNetworkAccess,
  type CastDevice,
  type useCastSession,
} from './useCastSession';

type Discovery = {
  available: boolean;
  error: string | null;
  transportAvailable: boolean;
  devices: CastDevice[];
};
const unavailable: Discovery = {
  available: false,
  error: null,
  transportAvailable: false,
  devices: [],
};

export function CastPicker({
  visible,
  onClose,
  controller,
  onError,
}: {
  visible: boolean;
  onClose: () => void;
  controller: ReturnType<typeof useCastSession>;
  onError: (message: string) => void;
}) {
  const colors = useThemeColors();
  const s = useThemeStyles(makeStyles);
  const screen = useWindowDimensions();
  const [state, setState] = useState(unavailable);
  const [pending, setPending] = useState(false);
  const [shareRequested, setShareRequested] = useState(false);
  const session = controller.state;
  const casting = session.active && !session.link;
  const busy =
    pending ||
    (session.active &&
      ['preparing', 'connecting', 'loading'].includes(session.phase));
  const work = async (action: () => Promise<unknown>) => {
    if (pending) return;
    setPending(true);
    try {
      await action();
    } catch (error) {
      setShareRequested(false);
      onError(error instanceof Error ? error.message : 'Could not start Cast.');
    } finally {
      setPending(false);
    }
  };
  useEffect(() => {
    if (!visible || !shareRequested || !session.url) return;
    setShareRequested(false);
    void Share.share({ message: session.url, title: 'GoVue' }).catch(() =>
      onError('Could not share this link.'),
    );
  }, [visible, shareRequested, session.url]);
  useEffect(() => {
    if (!visible || session.error) setShareRequested(false);
  }, [visible, session.error]);
  useEffect(() => {
    if (!visible) return;
    let active = true,
      busy = false,
      foreground = AppState.currentState === 'active';
    let networkAccess: boolean | null = null;
    setState(unavailable);
    const refresh = async () => {
      if (busy || !active || !foreground) return;
      busy = true;
      try {
        if (networkAccess === null) networkAccess = await castNetworkAccess();
        if (!networkAccess) throw Error('Local-network access was denied.');
        if (!active || !foreground) return;
        const raw = await castDiscoverySnapshot(true);
        const next = raw
          ? await coreRequest<Discovery>({ op: 'player_cast_devices', raw })
          : unavailable;
        if (active && foreground) setState(next);
      } catch {
        if (active)
          setState({
            ...unavailable,
            error:
              'Device discovery could not start. Check local-network access.',
          });
      } finally {
        busy = false;
      }
    };
    void refresh();
    const interval = setInterval(() => void refresh(), 1500);
    const app = AppState.addEventListener('change', (value) => {
      foreground = value === 'active';
      if (foreground) void refresh();
      else void castDiscoverySnapshot(false).catch(() => {});
    });
    return () => {
      active = false;
      clearInterval(interval);
      app.remove();
      void castDiscoverySnapshot(false).catch(() => {});
    };
  }, [visible]);
  return (
    <Modal
      visible={visible}
      transparent
      animationType="fade"
      statusBarTranslucent
      navigationBarTranslucent
      onRequestClose={onClose}
    >
      <View style={s.overlay}>
        <Pressable
          accessibilityLabel="Dismiss Cast"
          onPress={onClose}
          style={StyleSheet.absoluteFill}
        />
        <View
          style={[
            s.dialog,
            {
              width: Math.min(360, screen.width - 48),
              maxHeight: screen.height - 100,
            },
          ]}
        >
          <Text accessibilityRole="header" style={s.title}>
            Cast
          </Text>
          <ScrollView>
            {casting && (
              <>
                <Text style={s.heading}>
                  {session.phase === 'preparing' ||
                  session.phase === 'connecting'
                    ? 'Connecting to'
                    : 'Casting to'}{' '}
                  {session.device}
                </Text>
                <Pressable
                  accessibilityRole="button"
                  onPress={() => void work(controller.stop)}
                  style={s.button}
                >
                  <Text style={[s.link, { color: colors.error }]}>
                    Stop casting
                  </Text>
                </Pressable>
              </>
            )}
            <Text style={s.heading}>Available devices</Text>
            {state.devices.map((device) => (
              <SheetRow
                key={device.id}
                title={device.name}
                selected={casting && session.deviceId === device.id}
                showCheck={false}
                labelColor={
                  casting && session.deviceId === device.id
                    ? colors.error
                    : colors.ink
                }
                iconColor={
                  casting && session.deviceId === device.id
                    ? colors.error
                    : colors.muted
                }
                icon={
                  casting && session.deviceId === device.id ? (
                    <CastConnected size={22} />
                  ) : (
                    <Cast size={22} />
                  )
                }
                disabled={!state.transportAvailable || busy}
                onPress={() => void work(() => controller.start(device))}
              />
            ))}
            {!state.devices.length && (
              <>
                <Text style={s.help}>
                  {state.available
                    ? 'No Cast devices found'
                    : 'Cast unavailable'}
                </Text>
                <Text style={s.hint}>
                  {state.error ||
                    'Make sure casting is set up on the same network.'}
                </Text>
                <Pressable
                  accessibilityRole="link"
                  onPress={() =>
                    void Linking.openURL(
                      'https://support.google.com/googlecast/answer/6102923',
                    ).catch(() => {})
                  }
                  style={s.button}
                >
                  <Text style={s.link}>Learn about casting</Text>
                </Pressable>
              </>
            )}
            {!!state.devices.length && !state.transportAvailable && (
              <Text style={s.help}>Cast unavailable</Text>
            )}
            {!casting && (
              <>
                <View style={s.divider} />
                <Text style={s.heading}>Open on any device</Text>
                <Text style={s.hint}>On the same Wi-Fi network.</Text>
                {!!session.url && (
                  <Text selectable numberOfLines={3} style={s.hint}>
                    {session.url}
                  </Text>
                )}
                {session.linkConnected && (
                  <Text style={[s.hint, { color: colors.error }]}>
                    Device connected
                  </Text>
                )}
                <View style={{ flexDirection: 'row', gap: 8 }}>
                  <Pressable
                    accessibilityRole="button"
                    accessibilityLabel="Share casting link"
                    accessibilityState={{
                      disabled: !session.available || busy,
                    }}
                    disabled={!session.available || busy}
                    onPress={() =>
                      void work(async () => {
                        if (session.active && session.link && session.url)
                          await Share.share({
                            message: session.url,
                            title: 'GoVue',
                          });
                        else {
                          setShareRequested(true);
                          await controller.start();
                        }
                      })
                    }
                    style={[
                      s.button,
                      (!session.available || busy) && s.disabled,
                    ]}
                  >
                    <Text style={s.link}>Share</Text>
                  </Pressable>
                  {session.active && session.link && (
                    <Pressable
                      accessibilityRole="button"
                      onPress={() => void work(controller.stop)}
                      style={s.button}
                    >
                      <Text style={s.link}>Close link</Text>
                    </Pressable>
                  )}
                </View>
              </>
            )}
            {!!session.error && <Text style={s.help}>{session.error}</Text>}
          </ScrollView>
          <Pressable
            accessibilityRole="button"
            onPress={onClose}
            style={[s.button, s.close]}
          >
            <Text style={s.link}>Close</Text>
          </Pressable>
        </View>
      </View>
    </Modal>
  );
}
const makeStyles = (colors: ThemeColors) =>
  StyleSheet.create({
    overlay: {
      flex: 1,
      alignItems: 'center',
      justifyContent: 'center',
      backgroundColor: '#0009',
    },
    dialog: {
      backgroundColor: colors.dialog,
      borderRadius: 12,
      paddingHorizontal: 24,
      paddingTop: 24,
      paddingBottom: 12,
    },
    title: {
      fontSize: 24,
      fontWeight: '500',
      color: colors.ink,
      marginBottom: 16,
    },
    heading: { fontSize: 14, color: colors.ink, marginTop: 8, marginBottom: 4 },
    help: { fontSize: 13, color: colors.muted, marginTop: 4 },
    hint: { fontSize: 12, color: colors.muted, marginTop: 4, lineHeight: 17 },
    link: { fontSize: 14, color: colors.action, fontWeight: '500' },
    button: {
      minHeight: 44,
      paddingHorizontal: 8,
      justifyContent: 'center',
      alignSelf: 'flex-start',
    },
    close: { alignSelf: 'flex-end' },
    divider: {
      height: StyleSheet.hairlineWidth,
      backgroundColor: colors.line,
      marginVertical: 8,
    },
    disabled: { opacity: 0.45 },
  });
