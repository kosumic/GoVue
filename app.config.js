import appVersion from './scripts/app-version.cjs';

export default ({ config }) => ({
  ...config,
  version: appVersion.version,
});
