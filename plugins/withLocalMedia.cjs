const { withAndroidManifest, withDangerousMod } = require('@expo/config-plugins');
const fs = require('node:fs');
const path = require('node:path');

module.exports = config => {
  config = withAndroidManifest(config, config => {
    const manifest = config.modResults.manifest;
    const application = manifest.application[0];
    application.$['android:networkSecurityConfig'] = '@xml/govue_network_security';
    application.profileable = [{
      $: { 'android:shell': String(process.env.EXPO_PUBLIC_GOVUE_TRACE_TRANSCRIPT === '1') },
    }];
    // Expo needs this unnamespaced marker to replace generated intent filters
    // on subsequent prebuilds. Suppress only its namespace lint finding.
    manifest.$['xmlns:tools'] = 'http://schemas.android.com/tools';
    for (const activity of application.activity || []) {
      for (const filter of activity['intent-filter'] || []) {
        if (filter.$?.['data-generated'] !== 'true') continue;
        const ignored = new Set((filter.$['tools:ignore'] || '').split(',').filter(Boolean));
        ignored.add('MissingPrefix');
        filter.$['tools:ignore'] = [...ignored].join(',');
      }
    }
    return config;
  });
  return withDangerousMod(config, ['android', async config => {
    const folder = path.join(config.modRequest.platformProjectRoot, 'app/src/main/res/xml');
    fs.mkdirSync(folder, { recursive: true });
    fs.writeFileSync(path.join(folder, 'govue_network_security.xml'), `<?xml version="1.0" encoding="utf-8"?>
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
