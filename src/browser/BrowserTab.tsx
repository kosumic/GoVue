import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
} from 'react';
import { Platform, Pressable, StyleSheet, Text, View } from 'react-native';
import WebView, { type WebViewMessageEvent } from 'react-native-webview';
import { useWebViewRenderer } from './useWebViewRenderer';
import { isWebUrl } from './address';
import {
  BROWSER_BRIDGE,
  BROWSER_BOOTSTRAP,
  browserInsetsScript,
  browserPreferencesScript,
} from '../youtube/browserBridge';
import { YOUTUBE_BRIDGE } from '../youtube/pageScripts';
import { mediaSettingsScript } from '../youtube/mediaBridge';
import type { MediaSettings } from '../core/media';
import { useThemeColors, useThemeStyles, type ThemeColors } from '../theme';
export type BrowserTabRecord = {
  id: number;
  url: string;
  kind: string;
  generation?: number;
};
export type TabHandle = {
  view: React.RefObject<WebView | null>;
  back: () => void;
  reload: () => void;
  navigate: (url: string) => void;
  inject: (script: string) => void;
};
export const BrowserTab = forwardRef<
  TabHandle,
  {
    tab: BrowserTabRecord;
    active: boolean;
    userAgent?: string;
    width: number;
    height: number;
    bottomInset?: number;
    settings: MediaSettings;
    onMessage: (id: number, event: WebViewMessageEvent) => void;
    onUrl: (id: number, url: string, back: boolean) => void;
    onDead: (id: number) => void;
    onReady?: (id: number, url: string) => void;
    onDocumentStart?: (id: number, url: string, generation: number) => void;
  }
>(
  (
    {
      tab,
      active,
      userAgent,
      width,
      height,
      bottomInset = 0,
      settings,
      onMessage,
      onUrl,
      onDead,
      onReady,
      onDocumentStart,
    },
    ref,
  ) => {
    const renderer = useWebViewRenderer();
    const colors = useThemeColors();
    const s = useThemeStyles(makeStyles);
    const initialSource = useRef({ uri: tab.url });
    const currentUrl = useRef(tab.url);
    const documentGeneration = useRef(0);
    const [stopped, setStopped] = useState(false);
    const stoppedRef = useRef(false);
    const [error, setError] = useState('');
    const [loading, setLoading] = useState(true);
    const [source, setSource] = useState(initialSource.current);
    const valid = () => renderer.isCurrent() && !stoppedRef.current;
    const scripts =
      YOUTUBE_BRIDGE +
      '\n' +
      mediaSettingsScript(settings) +
      '\n' +
      BROWSER_BRIDGE +
      '\n' +
      browserPreferencesScript(settings) +
      '\n' +
      browserInsetsScript(bottomInset);
    const documentScripts = () =>
      scripts +
      `\nwindow.__lingoplayBrowser?.setDocumentGeneration(${JSON.stringify(documentGeneration.current)});true;`;
    const reload = () => {
      setError('');
      stoppedRef.current = false;
      setStopped(false);
      if (!renderer.mounted) {
        setSource({ uri: currentUrl.current });
        renderer.recreate();
      } else renderer.view.current?.reload();
    };
    const inject = (script: string) => {
      if (valid()) renderer.view.current?.injectJavaScript(script);
    };
    const navigate = (url: string) => {
      if (!isWebUrl(url)) return;
      currentUrl.current = url;
      if (!valid()) {
        setError('');
        stoppedRef.current = false;
        setStopped(false);
        setSource({ uri: url });
        if (!renderer.mounted) renderer.recreate();
      } else inject(`window.location.assign(${JSON.stringify(url)});true;`);
    };
    useImperativeHandle(
      ref,
      () => ({
        view: renderer.view,
        back: () => {
          if (valid()) renderer.view.current?.goBack();
          else reload();
        },
        reload,
        navigate,
        inject,
      }),
      [renderer, stopped],
    );
    useEffect(() => {
      inject(
        mediaSettingsScript(settings) +
          '\n' +
          browserPreferencesScript(settings) +
          '\n' +
          browserInsetsScript(bottomInset),
      );
    }, [settings, bottomInset]);
    useEffect(() => {
      inject(
        `window.dispatchEvent(new CustomEvent('tabVisibilityChanged',{detail:{active:${active}}}));${active ? '' : "document.querySelectorAll('video,audio').forEach(v=>v.pause());"}true;`,
      );
    }, [active]);
    const died = () => {
      if (!valid()) return;
      stoppedRef.current = true;
      setStopped(true);
      setLoading(false);
      setError('The browser stopped. Reload this page to continue.');
      if (Platform.OS === 'android') renderer.retire();
      onDead(tab.id);
    };
    return (
      <View
        pointerEvents={active ? 'auto' : 'none'}
        accessibilityElementsHidden={!active}
        importantForAccessibility={active ? 'auto' : 'no-hide-descendants'}
        style={[s.tab, { width, height: height + bottomInset }, !active && s.parked]}
      >
        {renderer.mounted && (
          <WebView
            ref={renderer.attach}
            key={renderer.generation}
            source={source}
            userAgent={userAgent}
            // Match browser text sizing: Android font scaling clips YouTube's line-clamped titles.
            textZoom={100}
            style={s.fill}
            containerStyle={s.fill}
            javaScriptEnabled
            domStorageEnabled
            sharedCookiesEnabled
            allowsInlineMediaPlayback
            allowsBackForwardNavigationGestures
            allowsPictureInPictureMediaPlayback
            mediaPlaybackRequiresUserAction={false}
            setSupportMultipleWindows={false}
            geolocationEnabled={false}
            allowFileAccess={false}
            allowFileAccessFromFileURLs={false}
            allowUniversalAccessFromFileURLs={false}
            mixedContentMode="never"
            webviewDebuggingEnabled={
              process.env.EXPO_PUBLIC_LINGOPLAY_DEBUG === '1'
            }
            injectedJavaScript={scripts}
            injectedJavaScriptBeforeContentLoaded={
              BROWSER_BOOTSTRAP + '\n' + mediaSettingsScript(settings)
            }
            onShouldStartLoadWithRequest={(request) =>
              valid() &&
              (request.url === 'about:blank' || isWebUrl(request.url))
            }
            onMessage={(event) => {
              if (valid()) onMessage(tab.id, event);
            }}
            onNavigationStateChange={(event) => {
              if (valid()) {
                currentUrl.current = event.url;
                onUrl(tab.id, event.url, event.canGoBack);
              }
            }}
            onLoadStart={(event) => {
              if (valid()) {
                documentGeneration.current = Math.max(
                  documentGeneration.current + 1,
                  (tab.generation || 0) + 1,
                  Date.now(),
                );
                onDocumentStart?.(
                  tab.id,
                  event.nativeEvent.url,
                  documentGeneration.current,
                );
                setLoading(true);
                setError('');
              }
            }}
            onLoadProgress={(event) => {
              if (valid() && event.nativeEvent.progress >= 0.5) {
                renderer.view.current?.injectJavaScript(documentScripts());
                onReady?.(tab.id, event.nativeEvent.url);
              }
            }}
            onLoadEnd={(event) => {
              if (valid()) {
                setLoading(false);
                renderer.view.current?.injectJavaScript(
                  documentScripts() +
                    `\nwindow.dispatchEvent(new CustomEvent('tabVisibilityChanged',{detail:{active:${active}}}));${active ? '' : "document.querySelectorAll('video,audio').forEach(v=>v.pause());"}true;`,
                );
                onReady?.(tab.id, event.nativeEvent.url);
              }
            }}
            onError={(event) => {
              setLoading(false);
              setError(
                event.nativeEvent.description || 'This page could not load.',
              );
            }}
            onRenderProcessGone={died}
            onContentProcessDidTerminate={died}
          />
        )}
        {active && loading && <View pointerEvents="none" style={s.progress} />}
        {active && !!error && (
          <View style={[s.browserError, { bottom: bottomInset }]}>
            <Text style={s.errorText}>{error}</Text>
            <Pressable
              accessibilityRole="button"
              accessibilityLabel="Reload page"
              style={s.retry}
              onPress={reload}
            >
              <Text style={s.errorText}>Reload</Text>
            </Pressable>
          </View>
        )}
      </View>
    );
  },
);

const makeStyles = (colors: ThemeColors) =>
  StyleSheet.create({
    fill: { flex: 1, backgroundColor: colors.background },
    tab: { position: 'absolute', top: 0, left: 0 },
    parked: { opacity: 0, zIndex: -1 },
    progress: {
      position: 'absolute',
      top: 0,
      left: 0,
      right: 0,
      height: 2,
      backgroundColor: '#f00',
    },
    browserError: {
      position: 'absolute',
      bottom: 0,
      left: 0,
      right: 0,
      backgroundColor: colors.surface,
      padding: 16,
      flexDirection: 'row',
      alignItems: 'center',
      gap: 12,
    },
    errorText: { color: colors.ink, fontSize: 13, flexShrink: 1 },
    retry: { padding: 12 },
  });
