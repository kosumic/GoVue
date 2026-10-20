import { browserUrlPolicy } from '../../modules/govue-core';
import { openBrowserAsync } from 'expo-web-browser';

// Keep synchronous WebView decisions on the same Rust policy as tab state.
export { browserUrlPolicy };
export const isWebUrl = (url: string) => browserUrlPolicy(url) === 'internal';
// Use the browser explicitly so YouTube app links cannot loop back into GoVue.
export const openExternalUrl = (url: string) => openBrowserAsync(url);
