import { AppRegistry } from 'react-native';
import { getDownloadActivityCount } from '../../modules/govue-core';

// A foreground service alone does not keep RN timers running after onHostPause.
// This lifecycle anchor lets existing platform mux/export callbacks finish;
// Rust remains authoritative for transfer work, cancellation and stored state.
AppRegistry.registerHeadlessTask('GoVueDownloads', () => async () => {
  while ((await getDownloadActivityCount()) > 0) {
    await new Promise<void>((resolve) => setTimeout(resolve, 1000));
  }
});
