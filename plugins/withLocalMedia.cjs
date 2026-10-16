const { withAndroidManifest, withDangerousMod } = require('@expo/config-plugins');
const fs = require('node:fs');
const path = require('node:path');

module.exports = config => {
  config = withAndroidManifest(config, config => {
    config.modResults.manifest.application[0].$['android:networkSecurityConfig'] = '@xml/lingoplay_network_security';
    config.modResults.manifest.application[0].profileable = [{
      $: { 'android:shell': String(process.env.EXPO_PUBLIC_LINGOPLAY_TRACE_TRANSCRIPT === '1') },
    }];
    return config;
  });
  return withDangerousMod(config, ['android', async config => {
    const folder = path.join(config.modRequest.platformProjectRoot, 'app/src/main/res/xml');
    fs.mkdirSync(folder, { recursive: true });
    fs.writeFileSync(path.join(folder, 'lingoplay_network_security.xml'), `<?xml version="1.0" encoding="utf-8"?>
<network-security-config>
  <base-config cleartextTrafficPermitted="false" />
  <domain-config cleartextTrafficPermitted="true">
    <domain includeSubdomains="false">127.0.0.1</domain>
    <domain includeSubdomains="false">localhost</domain>
  </domain-config>
</network-security-config>
`);
    return config;
  }]);
};
