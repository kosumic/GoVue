export { BROWSER_BRIDGE } from './generated/browserBridge';
export { BROWSER_BOOTSTRAP } from './generated/browserBootstrap';
export const browserPreferencesScript=(settings:unknown)=>`window.__lingoplayBrowser?.configure(${JSON.stringify(settings)}); true;`;
export const browserInsetsScript=(bottom:number)=>`window.__lingoplayBrowser?.setBottomInset(${JSON.stringify(bottom)}); true;`;
