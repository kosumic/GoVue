import fs from 'node:fs';
import path from 'node:path';
import * as fontkit from 'fontkit';

const root = path.resolve(import.meta.dirname, '..');
const put = (name, text) => { fs.mkdirSync(path.dirname(path.join(root, name)), { recursive: true }); fs.writeFileSync(path.join(root, name), text); };
const archive = path.join(root, 'versions/v2/source.json');
if (!fs.existsSync(archive)) {
  const files = ['index.html', 'stage-v2.js', 'scripts/build-v2.mjs', 'scenes.json',
    ...fs.readdirSync(path.join(root, 'compositions/v2')).map(n => `compositions/v2/${n}`)];
  put('versions/v2/source.json', JSON.stringify(Object.fromEntries(files.map(n => [n, fs.readFileSync(path.join(root, n), 'utf8')])), null, 2));
}
const clips = JSON.parse(fs.readFileSync(path.join(root, 'capture/footage.json'), 'utf8'));
const scenes = [
  { id: 'intro', start: 0, duration: 4, title: 'Turn watching into learning.' },
  { id: 'reader', start: 4, duration: 10, title: 'Read along.' },
  { id: 'longpress', start: 14, duration: 5, title: 'Hold to translate.' },
  { id: 'chat', start: 19, duration: 6, title: 'Ask your AI.' },
  { id: 'practice', start: 25, duration: 6, title: 'Make it stick.' },
  { id: 'close', start: 31, duration: 4, title: 'Learn from every video lesson.' },
];
const duration = scenes.at(-1).start + scenes.at(-1).duration;
const volumeAutomation = JSON.stringify({ version: 1, lanes: [{ target: 'volume', points: [
  { t: 0, v: .9 }, { t: duration - 1.2, v: .9 }, { t: duration, v: 0 },
] }] }).replaceAll('"', '&quot;');
put('scenes.json', JSON.stringify(scenes, null, 2) + '\n');
for (const s of scenes) {
  const id = `v3-${s.id}`, intro = s.id === 'intro', close = s.id === 'close';
  put(`compositions/v3/${s.id}.html`, `<!doctype html><html lang="en"><body><template>
<style>
#${id}-root{position:absolute;inset:0;width:100%;height:100%;pointer-events:none;color:${close ? '#f2f6fa' : '#111b29'}}
#${id}-inner{position:absolute;left:96px;right:150px;top:${intro ? 470 : close ? 560 : 200}px}
${s.id === 'practice' ? `#${id}-inner{top:1450px;color:#1470c0}` : ''}
#${id}-root h1{margin:0;font:${intro ? '400 46' : close ? '400 52' : '600 83'}px/1.02 Barlow,sans-serif;letter-spacing:-.045em;max-width:830px}
${['reader', 'longpress', 'chat'].includes(s.id) ? `#${id}-root h1{display:inline-block;color:#f2f6fa;background:#111b29;font-size:72px;padding:16px 24px 20px;border-radius:12px}` : ''}
#${id}-root .desc{font:400 43px/1.15 Barlow,sans-serif;margin:24px 0 0;max-width:780px;letter-spacing:-.02em}
#${id}-root .closing{position:absolute;left:96px;right:150px;top:1450px;color:#e9edf2}
#${id}-root .platform{font:400 32px/1.3 Barlow,sans-serif;margin:0 0 18px}
${close ? `#${id}-root .platforms{display:flex;align-items:center;gap:32px;margin:0 0 18px}
#${id}-root .platform{display:inline-flex;align-items:center;gap:12px;margin:0}
#${id}-root .platform-icon{display:block;width:48px;height:48px;object-fit:contain}
#${id}-root .android-icon{width:64px;height:64px}
#${id}-root .platform-note{font:400 28px/1.3 Barlow,sans-serif;color:#b5c0cd}
` : ''}#${id}-root .address{font:600 30px/1.3 Barlow,sans-serif;margin:0}
#${id}-root .credit{font:400 22px/1.3 Barlow,sans-serif;color:#b5c0cd;margin:22px 0 0;max-width:800px}
</style>
<div id="${id}-root" data-composition-id="${id}" data-width="1080" data-height="1920" data-duration="${s.duration}">
 <div id="${id}-content" class="clip" data-start="0" data-duration="${s.duration}" data-track-index="0">
  <div id="${id}-inner"><h1>${s.title}</h1>${s.sub ? `<p class="desc">${s.sub}</p>` : ''}</div>
  ${close ? '<div id="v3-closing-info" class="closing"><div class="platforms" aria-label="Android and iOS, open source"><span class="platform"><img class="platform-icon android-icon" src="assets/platforms/android.svg" width="64" height="64" alt="">Android</span><span class="platform"><img class="platform-icon" src="assets/platforms/apple.svg" width="48" height="48" alt="">iOS</span><span class="platform-note">· open source</span></div><p class="address">github.com/kosumic/GoVue</p><p class="credit">iPhone 17 Pro: Ibrahim.Bhl · CC BY 4.0</p></div>' : ''}
 </div>
</div>
<script>(()=>{
const tl=gsap.timeline({paused:true});
tl.fromTo('#${id}-inner',{y:28,opacity:0},{y:0,opacity:1,duration:.55,ease:'power3.out'},${close ? .75 : .15});
${close ? "tl.fromTo('#v3-closing-info',{y:20,opacity:0},{y:0,opacity:1,duration:.6,ease:'power3.out'},1.15);" : `tl.to('#${id}-inner',{y:-18,opacity:0,duration:.25,ease:'power3.in'},${intro ? 2.55 : 1.05});`}
window.__timelines['${id}']=tl;
})();</script></template></body></html>`);
}
for (const obsolete of ['watch', 'search', 'brand']) fs.rmSync(path.join(root, `compositions/v3/${obsolete}.html`), { force: true });
put('index.html', `<!doctype html><html lang="en" data-composition-variables='[{"id":"rasterScale","type":"number","label":"3D render scale","default":0.5,"min":0.25,"max":1,"step":0.25}]'><head><meta charset="UTF-8"><meta name="viewport" content="width=1080,height=1920"><title>GoVue — Live on iPhone</title>
<script src="assets/vendor/gsap.min.js"></script><script type="importmap">{"imports":{"three":"./assets/vendor/three.module.js","three/addons/":"./assets/vendor/three-addons/"}}</script>
<style>
@font-face{font-family:Barlow;font-weight:400;src:url('assets/fonts/Barlow-400.ttf') format('truetype')}
@font-face{font-family:Barlow;font-weight:600;src:url('assets/fonts/Barlow-600.ttf') format('truetype')}
@font-face{font-family:Barlow;font-weight:700;src:url('assets/fonts/Barlow-700.ttf') format('truetype')}
*{box-sizing:border-box}html,body{margin:0;width:100%;height:100%;overflow:hidden;background:#e9edf2}
#root{position:relative;width:100%;height:100%;overflow:hidden;background:#e9edf2}.clip{position:absolute;inset:0;width:100%;height:100%}
#live-stage{display:block;width:100%;height:100%;z-index:2}.phone-source{width:1080px;height:1920px;z-index:0;pointer-events:none}.story-copy{position:absolute;inset:0;width:100%;height:100%;z-index:3;pointer-events:none}
</style></head><body>
<div id="root" data-composition-id="govue-live" data-width="1080" data-height="1920" data-duration="${duration}">
<canvas id="live-stage" class="clip" data-start="0" data-duration="${duration}" data-track-index="0" width="1080" height="1920" data-layout-allow-overflow></canvas>
${clips.map(c => `<video id="live-${c.name}" class="clip phone-source" src="${c.src}" data-start="${c.start}" data-duration="${c.duration}" data-track-index="1" data-layout-ignore data-hf-media-start-basis="local" preload="auto" muted playsinline></video>`).join('\n')}
${scenes.map(s => `<div id="v3-${s.id}" class="clip story-copy" data-composition-id="v3-${s.id}" data-composition-src="compositions/v3/${s.id}.html" data-start="${s.start}" data-duration="${s.duration}" data-track-index="2" data-width="1080" data-height="1920"></div>`).join('\n')}
<audio id="live-score" src="assets/audio/govue-v3-music.wav" data-start="0" data-duration="${duration}" data-track-index="3" data-volume="0.9" data-automation="${volumeAutomation}"></audio>
</div>
<script>window.__govueClock={time:0};window.__timelines['govue-live']=gsap.timeline({paused:true}).to(window.__govueClock,{time:${duration},duration:${duration},ease:'none',onUpdate:()=>window.__govueV3?.renderAt(window.__govueClock.time)},0);</script>
<script type="module">import * as THREE from 'three';window.THREE=THREE;window.__hf=window.__hf||{};window.__hf.buildReady=window.__hf.buildReady||{};window.__hf.buildReady.govueLive=import('./stage-v3.js').then(m=>m.buildStage(THREE));</script>
</body></html>`);
const font = fontkit.create(fs.readFileSync(path.join(root, 'assets/fonts/Barlow-700.ttf'))), glyphs = {};
for (const char of 'GoVue') { const g = font.glyphForCodePoint(char.codePointAt(0)); glyphs[char] = { advance: g.advanceWidth, commands: g.path.commands }; }
put('assets/glyphs-v3.json', JSON.stringify({ units: font.unitsPerEm, glyphs }));
console.log(`Built ${duration}-second live iPhone film. HyperFrames owns ${clips.length} recorded screen clips.`);
