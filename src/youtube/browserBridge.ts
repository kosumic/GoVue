export { BROWSER_BRIDGE } from './generated/browserBridge';
export { BROWSER_BOOTSTRAP } from './generated/browserBootstrap';
export const browserPreferencesScript=(settings:unknown)=>`window.__govueBrowser?.configure(${JSON.stringify(settings)}); true;`;
export const browserInsetsScript=(bottom:number)=>`window.__govueBrowser?.setBottomInset(${JSON.stringify(bottom)}); true;`;
