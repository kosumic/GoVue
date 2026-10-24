// Postinstall patches native packages without changing their versions. Include
// their source files so fingerprints continue to identify the patched binary.
module.exports = {
  nativeModuleSourceType: 'files',
};
