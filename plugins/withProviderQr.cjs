const { withAndroidManifest, withDangerousMod } = require('@expo/config-plugins');
const fs = require('node:fs');
const path = require('node:path');

module.exports = config => {
  config = withAndroidManifest(config, config => {
    const manifest = config.modResults.manifest;
    // QR images and chat attachments also work without camera hardware.
    const features = manifest['uses-feature'] ||= [];
    for (const name of ['android.hardware.camera', 'android.hardware.camera.autofocus']) {
      const feature = features.find(feature => feature.$?.['android:name'] === name);
      if (feature) feature.$['android:required'] = 'false';
      else features.push({ $: { 'android:name': name, 'android:required': 'false' } });
    }
    const queries = manifest.queries ||= [];
    // Expo Camera 58 checks this package before decoding images. Android package
    // visibility otherwise makes an installed Play Store appear to be absent.
    if (!queries.some(query => query.package?.some(pkg => pkg.$?.['android:name'] === 'com.android.vending'))) {
      const query = queries[0] ||= {};
      (query.package ||= []).push({ $: { 'android:name': 'com.android.vending' } });
    }
    return config;
  });
  return withDangerousMod(config, ['android', async config => {
    const file = path.join(config.modRequest.platformProjectRoot, 'app/proguard-rules.pro');
    const rules = fs.existsSync(file) ? fs.readFileSync(file, 'utf8') : '';
    const marker = '# GoVue provider QR scanner';
    // Expo probes the facade by name; ML Kit discovers registrars through manifest
    // metadata. Older Firebase consumer rules preserve names but omit constructors.
    const additions = [
      '-keep class com.google.mlkit.vision.barcode.BarcodeScanning { *; }',
      '-keep class com.google.mlkit.** implements com.google.firebase.components.ComponentRegistrar { public <init>(); }',
    ].filter(rule => !rules.includes(rule));
    if (additions.length) {
      fs.writeFileSync(file, `${rules}\n${rules.includes(marker) ? '' : `${marker}\n`}${additions.join('\n')}\n`);
    }
    return config;
  }]);
};
