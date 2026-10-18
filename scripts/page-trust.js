// Private to the injected bundle. Only YouTube's player code is evaluated here.
export const pageTrust = globalThis.trustedTypes?.createPolicy('govue-player', {
  createScript: source => source,
  createScriptURL: address => {
    const url = new URL(address, location.href);
    if (url.protocol !== 'https:' || !['www.youtube.com','m.youtube.com','www.google.com','www.gstatic.com'].includes(url.hostname)) throw Error('Unsupported player script origin.');
    return url.href;
  },
});
