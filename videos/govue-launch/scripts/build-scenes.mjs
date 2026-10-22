import fs from 'node:fs';
import path from 'node:path';
import * as fontkit from 'fontkit';

const root = path.resolve(import.meta.dirname, '..');
const write = (name, value) => fs.writeFileSync(path.join(root, name), value);
const scenes = [
  { id: '01-hook', start: 0, duration: 5.625, kicker: 'your watchlist has potential', lines: ['less scrolling.', 'more speaking.'], note: 'Turn the videos you love into a language you can use.', delays: [0.08, 1.4], noteAt: 3.1 },
  { id: '02-watch', start: 5.625, duration: 7.5, kicker: '01 / watch', lines: ['your videos.', 'upgraded.'], note: 'YouTube browsing. Native playback.', delays: [0.15, 1.5], noteAt: 3.6 },
  { id: '03-read', start: 13.125, duration: 8.4375, kicker: '02 / read', lines: ['one video.', 'two languages.'], note: 'Follow both. Tap a line. Hear it again.', delays: [0.1, 1.55], noteAt: 4.4 },
  { id: '04-words', start: 21.5625, duration: 7.5, kicker: '03 / understand', lines: ['hear it.', 'get it. keep it.'], note: 'Explain a phrase. Save it in context.', delays: [0.12, 1.7], noteAt: 4.3 },
  { id: '05-ask', start: 29.0625, duration: 5.625, kicker: '04 / ask', lines: ['wait. why', 'that word?'], note: 'Ask your AI, with the transcript attached.', delays: [0.08, 1.1], noteAt: 2.65 },
  { id: '06-play', start: 34.6875, duration: 5.625, kicker: '05 / play', lines: ['now make it', 'a game.'], note: 'Ask the agent for a quiz. Play with lesson audio.', delays: [0.12, 1.1], noteAt: 2.8 },
  { id: '07-close', start: 40.3125, duration: 4.6875, kicker: 'meet your next learning habit', lines: ['GoVue'], note: 'learn from every line.', delays: [0.25], noteAt: 1.15, close: true },
];
write('scenes.json', JSON.stringify(scenes, null, 2) + '\n');
write('compositions/chrome.html', `<!doctype html><html lang="en"><body><template>
<style>#chrome-root{position:absolute;inset:0;width:100%;height:100%;pointer-events:none}</style>
<div id="chrome-root" data-composition-id="chrome" data-width="1080" data-height="1920" data-duration="45">
 <div id="chrome-content" class="clip" data-start="0" data-duration="45" data-track-index="1">
  <div id="brand"><img src="assets/icon.png" alt="GoVue"><span>GoVue</span></div>
  <div id="progress-track" class="chapter-tick"><div id="progress"></div></div>
 </div>
</div>
<script>(()=>{const tl=gsap.timeline({paused:true});tl.fromTo('#progress',{scaleX:0},{scaleX:1,duration:45,ease:'none'},0);window.__timelines.chrome=tl;})();</script>
</template></body></html>`);
for (const s of scenes) {
  const cid = `f${s.id.slice(0, 2)}`;
  write(`compositions/frames/${s.id}.html`, `<!doctype html>
<html lang="en"><body><template>
<style>
#${cid}-root { position:absolute; inset:0; width:100%; height:100%; pointer-events:none; }
#${cid}-root .copy { position:absolute; inset:170px 132px auto 84px; }
#${cid}-root .kicker { font:500 24px 'IBM Plex Mono',monospace; letter-spacing:.08em; text-transform:uppercase; margin:0 0 34px; color:#AAAAAA; }
#${cid}-root h1 { margin:0; font:900 ${s.close ? '196' : s.id === '01-hook' ? '119' : '138'}px/.91 Barlow,sans-serif; letter-spacing:-.045em; color:#FFFFFF; }
#${cid}-root .line { display:block; width:100%; min-height:132px; }
#${cid}-root .line.accent { color:#3EA6FF; }
#${cid}-root .note { font:400 37px/1.2 Barlow,sans-serif; max-width:760px; margin:32px 0 0; color:#FFFFFF; }
#${cid}-root .close-note { font-size:58px; font-weight:600; letter-spacing:-.03em; }
#${cid}-root .cta { position:absolute; inset:auto 132px 380px 84px; color:#FFFFFF; }
#${cid}-root .platform { font:600 30px Barlow,sans-serif; margin:0 0 15px; }
#${cid}-root .url { font:500 25px 'IBM Plex Mono',monospace; margin:0; }
${s.id === '05-ask' ? `#${cid}-root h1,#${cid}-root .line.accent,#${cid}-root .note,#${cid}-root .kicker{color:#0F0F0F}` : ''}
${s.close ? `#${cid}-root h1{opacity:0}` : ''}
</style>
<div id="${cid}-root" data-composition-id="${cid}" data-width="1080" data-height="1920" data-duration="${s.duration}">
 <div id="${cid}-content" class="clip" data-start="0" data-duration="${s.duration}" data-track-index="1">
  <div class="copy">
   <p class="kicker" id="${cid}-kicker">${s.kicker}</p>
   <h1>${s.lines.map((line, i) => `<span class="line ${i ? 'accent' : ''}" id="${cid}-line${i}">${line}</span>`).join('')}</h1>
   <p class="note ${s.close ? 'close-note' : ''}" id="${cid}-note">${s.note}</p>
  </div>
  ${s.close ? `<div class="cta" id="${cid}-cta"><p class="platform">Android + iOS · open source</p><p class="url">github.com/kosumic/GoVue</p></div>` : ''}
 </div>
</div>
<script>
(() => {
 const tl = gsap.timeline({ paused:true });
 tl.fromTo('#${cid}-kicker', {y:16,opacity:0}, {y:0,opacity:1,duration:.42,ease:'power3.out'}, .06);
 ${s.close ? '' : s.lines.map((_, i) => s.id === '01-hook' && i === 0 ? `tl.fromTo('#${cid}-line0',{x:-12,opacity:1},{x:0,opacity:1,duration:.55,ease:'power3.out'},0);` : `tl.fromTo('#${cid}-line${i}', {y:80,rotationX:-48,opacity:0,transformPerspective:900,transformOrigin:'50% 100%'}, {y:0,rotationX:0,opacity:1,duration:.72,ease:'power4.out'}, ${s.delays[i]});`).join('\n')}
 tl.fromTo('#${cid}-note', {y:22,opacity:0}, {y:0,opacity:1,duration:.5,ease:'power3.out'}, ${s.noteAt});
 ${s.close ? `tl.fromTo('#${cid}-cta', {y:20,opacity:0}, {y:0,opacity:1,duration:.5,ease:'power3.out'}, 2);` : ''}
 ${!s.close ? `tl.to('#${cid}-root .copy', {y:-24,opacity:0,duration:.28,ease:'power2.in'}, ${s.duration - 0.28});` : ''}
 window.__timelines['${cid}'] = tl;
})();
</script>
</template></body></html>\n`);
}

write('index.html', `<!doctype html>
<html lang="en"><head>
<meta charset="UTF-8"><meta name="viewport" content="width=1080,height=1920">
<title>GoVue — Learn from every line</title>
<script src="assets/vendor/gsap.min.js"></script>
<script type="importmap">{"imports":{"three":"./assets/vendor/three.module.js","three/addons/":"./assets/vendor/three-addons/"}}</script>
<style>
@font-face{font-family:Barlow;font-style:normal;font-weight:400;src:url('assets/fonts/Barlow-400.ttf') format('truetype')}
@font-face{font-family:Barlow;font-style:normal;font-weight:600;src:url('assets/fonts/Barlow-600.ttf') format('truetype')}
@font-face{font-family:Barlow;font-style:normal;font-weight:700;src:url('assets/fonts/Barlow-700.ttf') format('truetype')}
@font-face{font-family:Barlow;font-style:normal;font-weight:800;src:url('assets/fonts/Barlow-800.ttf') format('truetype')}
@font-face{font-family:Barlow;font-style:normal;font-weight:900;src:url('assets/fonts/Barlow-900.ttf') format('truetype')}
@font-face{font-family:'IBM Plex Mono';font-style:normal;font-weight:500;src:url('assets/fonts/IBM-Plex-Mono-500.ttf') format('truetype')}
*{box-sizing:border-box}html,body{margin:0;width:100%;height:100%;overflow:hidden;background:#0F0F0F;color:#FFFFFF;font-family:Barlow,sans-serif}
#root{position:relative;width:100%;height:100%;overflow:hidden;background:#0F0F0F}
.clip{position:absolute;inset:0;width:100%;height:100%}
#three-stage{display:block;width:100%;height:100%}
#brand{position:absolute;left:84px;top:95px;display:flex;align-items:center;gap:14px;font-size:27px;font-weight:700;letter-spacing:-.02em}
#brand img{width:42px;height:42px;border-radius:10px;background:#FFFFFF;padding:4px}
.chapter-tick{position:absolute;left:84px;bottom:295px;width:814px;height:3px;background:#262728;overflow:hidden}
#progress{height:100%;width:100%;background:#3EA6FF;transform-origin:0 50%}
#recorded-source{position:absolute;left:0;top:0;width:1px;height:1px;opacity:0;pointer-events:none}
</style>
</head><body>
<div id="root" data-composition-id="govue-launch" data-start="0" data-duration="45" data-width="1080" data-height="1920">
 <canvas id="three-stage" class="clip" data-start="0" data-duration="45" data-track-index="0" data-layout-allow-overflow width="1080" height="1920"></canvas>
 ${scenes.map(s => `<div id="mount-${s.id}" data-composition-id="f${s.id.slice(0, 2)}" data-composition-src="compositions/frames/${s.id}.html" data-start="${s.start}" data-duration="${s.duration}" data-track-index="2" data-width="1080" data-height="1920"></div>`).join('\n')}
 <div id="mount-chrome" data-composition-id="chrome" data-composition-src="compositions/chrome.html" data-start="0" data-duration="45" data-track-index="3" data-width="1080" data-height="1920"></div>
 <audio id="score" src="assets/audio/govue-score.wav" data-start="0" data-duration="45" data-track-index="4" data-volume="0.85" data-automation="{&quot;version&quot;:1,&quot;lanes&quot;:[{&quot;target&quot;:&quot;volume&quot;,&quot;points&quot;:[{&quot;t&quot;:0,&quot;v&quot;:0},{&quot;t&quot;:0.18,&quot;v&quot;:1},{&quot;t&quot;:43.8,&quot;v&quot;:1},{&quot;t&quot;:45,&quot;v&quot;:0}]}]}"></audio>
</div>
<script>
const master = gsap.timeline({paused:true});
window.__timelines['govue-launch'] = master;
</script>
<script type="module">
import * as THREE from 'three';
window.THREE = THREE;
window.__hf = window.__hf || {};
window.__hf.buildReady = window.__hf.buildReady || {};
window.__hf.buildReady.govueStage = import('./stage.js').then(m=>m.buildStage(THREE));
</script>
</body></html>\n`);

// Keep the actual display font on the 3D wordmark, including its counters.
const font = fontkit.create(fs.readFileSync(path.join(root, 'assets/fonts/Barlow-900.ttf')));
const glyphs = {};
for (const char of new Set('GoVueplayCC')) {
  const glyph = font.glyphForCodePoint(char.codePointAt(0));
  glyphs[char] = { advance: glyph.advanceWidth, commands: glyph.path.commands };
}
write('assets/glyphs.json', JSON.stringify({ units: font.unitsPerEm, glyphs }));
console.log(`Built ${scenes.length} frame compositions, 45-second host, and 3D wordmark glyphs.`);
