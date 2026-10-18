import {
  cloneElement,
  isValidElement,
  useEffect,
  useRef,
  useState,
  type ReactElement,
  type ReactNode,
} from 'react';
import {
  Animated,
  Modal,
  Keyboard,
  KeyboardAvoidingView,
  Platform,
  PanResponder,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  useWindowDimensions,
  View,
} from 'react-native';
import { ArrowLeft as ChevronLeft, Check, X } from './icons';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import { useHaptic } from './Haptics';
import { useThemeColors, useThemeStyles, type ThemeColors } from '../theme';

export function SheetRow({
  title,
  detail,
  selected,
  disabled,
  expanded,
  onPress,
  icon,
  trailing,
  section = false,
  radio = false,
  showCheck = true,
  iconColor,
  labelColor,
}: {
  title: string;
  detail?: string;
  selected?: boolean;
  disabled?: boolean;
  expanded?: boolean;
  onPress: () => void;
  icon?: ReactNode;
  trailing?: ReactNode;
  section?: boolean;
  radio?: boolean;
  showCheck?: boolean;
  iconColor?: string;
  labelColor?: string;
}) {
  const vibrate = useHaptic();
  const playerColors = useThemeColors();
  const s = useThemeStyles(makeStyles);
  return (
    <Pressable
      accessibilityRole={radio ? 'radio' : 'button'}
      accessibilityLabel={title}
      accessibilityState={{
        selected: radio ? undefined : selected,
        checked: radio ? !!selected : undefined,
        disabled,
        expanded,
      }}
      disabled={disabled}
      onPress={() => {
        vibrate();
        onPress();
      }}
      style={({ pressed }) => [
        s.row,
        section && s.sectionRow,
        radio && s.radioRow,
        pressed && s.pressed,
        disabled && s.disabled,
      ]}
    >
      {!icon && section && <View style={{ width: 8 }} />}
      {radio && (
        <View style={s.radioSlot}>
          <View
            style={[
              s.radioRing,
              {
                borderColor: selected
                  ? playerColors.action
                  : playerColors.muted,
              },
            ]}
          >
            {selected && (
              <View
                style={[s.radioDot, { backgroundColor: playerColors.action }]}
              />
            )}
          </View>
        </View>
      )}
      {icon && (
        <View style={[s.icon, section && s.sectionIcon]}>
          {isValidElement(icon)
            ? cloneElement(icon as ReactElement<{ color?: string }>, {
                color: iconColor || playerColors.ink,
              })
            : icon}
        </View>
      )}
      <View style={s.grow}>
        <Text
          style={[
            s.label,
            selected && s.selected,
            labelColor && { color: labelColor },
          ]}
        >
          {title}
        </Text>
        {!!detail && <Text style={s.detail}>{detail}</Text>}
      </View>
      {trailing &&
        (section ? (
          <View style={s.sectionTrailing}>{trailing}</View>
        ) : (
          trailing
        ))}
      {selected && !radio && showCheck && (
        <Check size={20} color={playerColors.ink} />
      )}
    </Pressable>
  );
}
export type MenuAnchor = {
  x: number;
  y: number;
  width: number;
  height: number;
};
export function PlayerSheet({
  visible,
  title,
  onClose,
  onBack,
  children,
  footer,
  anchor,
  scrollable = true,
  presentation = 'sheet',
  showCloseButton = true,
  limitPlayerHeight = false,
  showHeading = true,
  maxContentHeight,
  popupWidth,
  contentKey,
}: {
  visible: boolean;
  title: string;
  onClose: () => void;
  onBack?: () => void;
  children: ReactNode;
  footer?: ReactNode;
  anchor?: MenuAnchor;
  scrollable?: boolean;
  presentation?: 'sheet' | 'dialog';
  showCloseButton?: boolean;
  limitPlayerHeight?: boolean;
  showHeading?: boolean;
  maxContentHeight?: number;
  popupWidth?: number;
  contentKey?: string;
}) {
  const inset = useSafeAreaInsets();
  const playerColors = useThemeColors();
  const s = useThemeStyles(makeStyles);
  const screen = useWindowDimensions();
  const dialog = presentation === 'dialog';
  const drag = useRef(new Animated.Value(0)).current;
  const latest = useRef({ onClose, height: screen.height });
  latest.current = { onClose, height: screen.height };
  const handle = useRef(
    PanResponder.create({
      onMoveShouldSetPanResponder: (_event, gesture) =>
        gesture.dy > 6 && gesture.dy > Math.abs(gesture.dx),
      onPanResponderMove: (_event, gesture) =>
        drag.setValue(Math.max(0, gesture.dy)),
      onPanResponderRelease: (_event, gesture) => {
        if (gesture.dy > 60 || gesture.vy > 0.8) {
          Animated.timing(drag, {
            toValue: latest.current.height,
            duration: 180,
            useNativeDriver: true,
          }).start(({ finished }) => {
            if (finished) latest.current.onClose();
          });
        } else {
          Animated.spring(drag, {
            toValue: 0,
            useNativeDriver: true,
          }).start();
        }
      },
      onPanResponderTerminate: () => {
        Animated.spring(drag, { toValue: 0, useNativeDriver: true }).start();
      },
    }),
  ).current;
  useEffect(() => {
    drag.stopAnimation();
    drag.setValue(0);
    return () => drag.stopAnimation();
  }, [visible, title, drag]);
  const [contentHeight, setContentHeight] = useState(200);
  const scroll = useRef<ScrollView | null>(null);
  useEffect(() => {
    if (!visible || anchor) return;
    const listener = Keyboard.addListener('keyboardDidShow', () =>
      scroll.current?.scrollToEnd({ animated: true }),
    );
    return () => listener.remove();
  }, [visible, anchor]);
  const width =
    popupWidth ??
    (title === 'Subtitles'
      ? 250
      : title === 'Segments'
        ? Math.min(380, screen.width - 32)
        : 180);
  const height = Math.min(contentHeight, screen.height * 0.7);
  const popup = anchor
    ? {
        position: 'absolute' as const,
        left: Math.max(
          8,
          Math.min(screen.width - width - 8, anchor.x + anchor.width - width),
        ),
        top:
          anchor.y + anchor.height + height > screen.height - inset.bottom
            ? Math.max(inset.top + 8, anchor.y - height)
            : anchor.y + anchor.height,
        width,
        maxHeight: screen.height * 0.7,
        borderRadius: 12,
        minHeight: 0,
        paddingBottom: 0,
      }
    : undefined;
  return (
    <Modal
      visible={visible}
      transparent
      animationType={dialog ? 'fade' : anchor ? 'none' : 'slide'}
      statusBarTranslucent
      navigationBarTranslucent
      onRequestClose={dialog || anchor ? onClose : onBack || onClose}
    >
      <KeyboardAvoidingView
        enabled={!anchor}
        behavior={Platform.OS === 'ios' ? 'padding' : 'height'}
        style={[
          s.modal,
          dialog && s.dialogBackdrop,
          !!anchor && s.popupBackdrop,
        ]}
      >
        <Pressable
          style={StyleSheet.absoluteFill}
          accessibilityRole="button"
          accessibilityLabel={`Close ${title}`}
          onPress={onClose}
        />
        <Animated.View
          accessibilityViewIsModal
          style={[
            s.sheet,
            { paddingBottom: Math.max(12, inset.bottom) },
            !dialog &&
              !anchor && {
                transform: [{ translateY: drag }],
                ...(limitPlayerHeight && {
                  maxHeight: Math.min(
                    screen.height - inset.top,
                    Math.max(
                      200,
                      Math.min(520, Math.floor(screen.height * 0.75)),
                    ) +
                      Math.max(12, inset.bottom) +
                      28,
                  ),
                }),
              },
            dialog && [
              s.dialog,
              {
                width: Math.min(360, screen.width - 48),
                maxHeight:
                  screen.height - Math.max(96, inset.top + inset.bottom + 32),
              },
            ],
            popup,
          ]}
        >
          {dialog
            ? showHeading && (
                <Text
                  accessibilityRole="header"
                  style={[s.title, s.dialogTitle]}
                >
                  {title}
                </Text>
              )
            : !anchor && (
                <>
                  <View
                    {...handle.panHandlers}
                    style={s.handleTarget}
                    accessible
                    accessibilityRole="button"
                    accessibilityLabel={`Close ${title}`}
                    accessibilityActions={[
                      { name: 'activate' },
                      { name: 'dismiss' },
                    ]}
                    onAccessibilityAction={onClose}
                  >
                    <View style={s.handle} />
                  </View>
                  {showHeading && (
                    <View style={s.heading}>
                      {onBack && (
                        <Pressable
                          onPress={onBack}
                          accessibilityRole="button"
                          accessibilityLabel="Back to player options"
                          style={s.button}
                        >
                          <ChevronLeft size={24} color={playerColors.ink} />
                        </Pressable>
                      )}
                      <Text accessibilityRole="header" style={s.title}>
                        {title}
                      </Text>
                      {showCloseButton && (
                        <Pressable
                          onPress={onClose}
                          accessibilityRole="button"
                          accessibilityLabel="Close player options"
                          style={s.button}
                        >
                          <X size={22} color={playerColors.ink} />
                        </Pressable>
                      )}
                    </View>
                  )}
                </>
              )}
          {scrollable ? (
            <ScrollView
              ref={scroll}
              style={
                maxContentHeight === undefined
                  ? undefined
                  : { maxHeight: maxContentHeight }
              }
              key={contentKey}
              keyboardShouldPersistTaps="handled"
              onContentSizeChange={(_width, height) => setContentHeight(height)}
              contentContainerStyle={[s.content, dialog && s.dialogContent]}
            >
              {children}
            </ScrollView>
          ) : (
            <View
              style={{ flexShrink: 1 }}
              onLayout={(event) =>
                setContentHeight(event.nativeEvent.layout.height)
              }
            >
              {children}
            </View>
          )}
          {footer ||
            (dialog && (
              <View style={s.dialogActions}>
                <Pressable
                  accessibilityRole="button"
                  onPress={onClose}
                  style={s.dialogButton}
                >
                  <Text style={s.dialogAction}>Cancel</Text>
                </Pressable>
              </View>
            ))}
        </Animated.View>
      </KeyboardAvoidingView>
    </Modal>
  );
}
const makeStyles = (playerColors: ThemeColors) =>
  StyleSheet.create({
    modal: { flex: 1, justifyContent: 'flex-end', backgroundColor: '#0006' },
    dialogBackdrop: {
      justifyContent: 'center',
      alignItems: 'center',
      paddingVertical: 48,
    },
    dialog: { borderRadius: 12, paddingBottom: 0, maxHeight: '100%' },
    dialogTitle: {
      flex: 0,
      fontSize: 24,
      paddingLeft: 24,
      paddingRight: 24,
      paddingTop: 24,
      paddingBottom: 16,
    },
    dialogContent: { paddingHorizontal: 8, paddingBottom: 8 },
    dialogActions: {
      flexDirection: 'row',
      justifyContent: 'flex-end',
      paddingHorizontal: 16,
      paddingBottom: 12,
    },
    dialogButton: {
      minWidth: 64,
      minHeight: 48,
      alignItems: 'center',
      justifyContent: 'center',
      paddingHorizontal: 12,
    },
    dialogAction: {
      color: playerColors.action,
      fontSize: 14,
      fontWeight: '500',
    },
    sheet: {
      backgroundColor: playerColors.dialog,
      borderTopLeftRadius: 16,
      borderTopRightRadius: 16,
      maxHeight: '80%',
      minHeight: 100,
    },
    popupBackdrop: { backgroundColor: 'transparent' },
    handle: {
      height: 4,
      width: 32,
      backgroundColor: '#777',
      borderRadius: 2,
      alignSelf: 'center',
    },
    handleTarget: { height: 24, justifyContent: 'center' },
    heading: {
      flexDirection: 'row',
      alignItems: 'center',
      paddingHorizontal: 12,
      minHeight: 48,
    },
    title: {
      flex: 1,
      color: playerColors.ink,
      fontSize: 20,
      fontWeight: '500',
      paddingLeft: 4,
    },
    button: {
      width: 44,
      height: 44,
      alignItems: 'center',
      justifyContent: 'center',
    },
    content: { paddingBottom: 24 },
    row: {
      minHeight: 48,
      paddingHorizontal: 16,
      paddingVertical: 8,
      flexDirection: 'row',
      alignItems: 'center',
      gap: 8,
    },
    icon: { width: 32, alignItems: 'flex-start' },
    sectionRow: { paddingHorizontal: 0, paddingVertical: 0 },
    sectionIcon: { width: 40, alignItems: 'center' },
    sectionTrailing: { width: 40, alignItems: 'center' },
    radioRow: { minHeight: 52, paddingVertical: 0, gap: 12 },
    radioSlot: {
      width: 48,
      height: 48,
      alignItems: 'center',
      justifyContent: 'center',
    },
    radioRing: {
      width: 20,
      height: 20,
      borderRadius: 10,
      borderWidth: 2,
      alignItems: 'center',
      justifyContent: 'center',
    },
    radioDot: { width: 10, height: 10, borderRadius: 5 },
    grow: { flex: 1 },
    label: { fontSize: 14, color: playerColors.ink, lineHeight: 20 },
    detail: {
      fontSize: 12,
      lineHeight: 18,
      color: playerColors.muted,
      marginTop: 2,
    },
    selected: { color: playerColors.ink },
    pressed: { backgroundColor: playerColors.accentSoft },
    disabled: { opacity: 0.4 },
  });
