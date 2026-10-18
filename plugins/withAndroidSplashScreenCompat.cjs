const { AndroidConfig, XML, withFinalizedMod } = require('@expo/config-plugins');
const fs = require('node:fs/promises');
const path = require('node:path');

module.exports = config => withFinalizedMod(config, ['android', async config => {
  // Run after Expo has written the splash theme so prebuilds stay repeatable.
  const res = path.join(config.modRequest.platformProjectRoot, 'app/src/main/res');
  const stylesPath = path.join(res, 'values/styles.xml');
  const styles = await AndroidConfig.Resources.readResourcesXMLAsync({ path: stylesPath });
  const splash = styles.resources.style?.find(style => style.$.name === 'Theme.App.SplashScreen');
  const behavior = 'android:windowSplashScreenBehavior';
  if (!splash?.item?.some(item => item.$.name === behavior)) return config;

  // Expo emits this API 33 attribute in values/. Keep the full theme for newer
  // Android versions and omit only the unsupported item on older versions.
  const api33Splash = structuredClone(splash);
  splash.item = splash.item.filter(item => item.$.name !== behavior);
  const folder = path.join(res, 'values-v33');
  await fs.mkdir(folder, { recursive: true });
  await XML.writeXMLAsync({
    path: path.join(folder, 'govue_splash_screen.xml'),
    xml: { resources: { $: styles.resources.$, style: [api33Splash] } },
  });
  await XML.writeXMLAsync({ path: stylesPath, xml: styles });
  return config;
}]);
