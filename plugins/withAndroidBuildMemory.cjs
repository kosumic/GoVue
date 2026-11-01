const { withGradleProperties } = require('@expo/config-plugins');

module.exports = config => withGradleProperties(config, config => {
  // Expo's 512 MiB metaspace default is too small for release R8 and lint.
  const key = 'org.gradle.jvmargs';
  const value = '-Xmx2048m -XX:MaxMetaspaceSize=1024m';
  const property = config.modResults.find(item => item.type === 'property' && item.key === key);
  if (property) property.value = value;
  else config.modResults.push({ type: 'property', key, value });
  return config;
});
