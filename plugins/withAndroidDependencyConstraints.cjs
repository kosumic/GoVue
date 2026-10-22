const { withAppBuildGradle } = require('@expo/config-plugins');
const { mergeContents } = require('@expo/config-plugins/build/utils/generateCode');

module.exports = config => withAppBuildGradle(config, config => {
  if (config.modResults.language !== 'groovy') {
    throw Error('Review Android security constraints for the new Gradle language.');
  }
  config.modResults.contents = mergeContents({
    tag: 'govue-security-constraints',
    src: config.modResults.contents,
    anchor: /^dependencies \{$/m,
    offset: 1,
    comment: '//',
    newSrc: `    constraints {
        // Expo file-system/document-picker still request Commons IO 2.6.
        implementation('commons-io:commons-io:2.14.0') {
            because 'CVE-2021-29425 and CVE-2024-47554'
        }
        implementation('com.google.code.gson:gson:2.13.2') {
            because 'CVE-2022-25647'
        }
    }`,
  }).contents;
  return config;
});
