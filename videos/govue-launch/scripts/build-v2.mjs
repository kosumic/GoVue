import fs from 'node:fs';
import path from 'node:path';
import * as fontkit from 'fontkit';

const root = path.resolve(import.meta.dirname, '..');
const put = (name, value) => { fs.mkdirSync(path.dirname(path.join(root, name)), { recursive:true }); fs.writeFileSync(path.join(root, name), value); };
const scenes = [
  { id:'phrase', start:0, duration:6, title:'You heard it.', style:'small', sub:'A real phrase. From a real video.', noteAt:3.4 },
  { id:'meaning', start:6, duration:6, title:'Now you get it.', style:'small', sub:'Same line. A whole new meaning.', noteAt:3.3 },
  { id:'reader', start:12, duration:8, title:'Read along.\nIn both languages.', style:'reader', sub:'Tap a line. Hear it again.', noteAt:4.8 },
  { id:'context', start:20, duration:7, title:'A phrase worth\nkeeping.', style:'context', sub:'Examples. Grammar. The original context.', noteAt:4.2 },
  { id:'prompt', start:27, duration:6, title:'Make this\ninto a game.', style:'prompt', sub:'Ask your AI.', noteAt:.1, exitAt:2.4 },
  { id:'practice', start:33, duration:8, title:'Your turn.', style:'practice', sub:'Practice with the lesson’s own audio.', noteAt:4.2 },
  { id:'close', start:41, duration:4, title:'GoVue', style:'close', sub:'Learn from\nevery line.', noteAt:1.2 },
];
put('scenes.json', JSON.stringify(scenes, null, 2) + '\n');
for (const scene of scenes) {
  const cid = `v2-${scene.id}`;
  const isPrompt = scene.style === 'prompt', isClose = scene.style === 'close';
  const lines = scene.title.split('\n');
  put(`compositions/v2/${scene.id}.html`, `<!doctype html><html lang="en"><body><template>
<style>
#${cid}-root{position:absolute;inset:0;width:100%;height:100%;pointer-events:none;color:#151B24}
#${cid}-root .copy{position:absolute;left:96px;right:145px;top:${isClose ? '585' : isPrompt ? '485' : scene.style === 'small' ? '345' : '270'}px}
#${cid}-root h1{margin:0;font:${isClose ? '600 194' : isPrompt ? '400 154' : scene.style === 'small' ? '400 80' : scene.style === 'practice' ? '500 137' : '500 104'}px/${isPrompt ? '0.96' : '1.02'} ${isPrompt ? "'Instrument Serif'" : 'Barlow'},sans-serif;letter-spacing:-.045em;${isPrompt ? 'font-style:italic;' : ''}}
#${cid}-root .line{display:block;width:100%}
#${cid}-root .sub{position:absolute;left:96px;right:145px;bottom:385px;margin:0;font:400 37px/1.2 Barlow,sans-serif;color:#354353;max-width:780px}
${scene.style === 'reader' || scene.style === 'practice' ? `#${cid}-root .sub{top:${scene.style === 'reader' ? 500 : 450}px;bottom:auto}` : ''}
${isPrompt ? `#${cid}-root .sub{top:350px;bottom:auto;font-size:42px;color:#151B24}` : ''}
${isClose ? `#${cid}-root .sub{top:835px;bottom:auto;font:italic 400 100px/1.02 'Instrument Serif',serif;letter-spacing:-.025em;color:#151B24}` : ''}
#${cid}-root .logo{position:absolute;left:96px;top:430px;width:118px;height:118px;object-fit:contain}
#${cid}-root .cta{position:absolute;left:96px;right:145px;top:1315px}
#${cid}-root .platform{font:400 30px/1.3 Barlow,sans-serif;margin:0 0 21px}
#${cid}-root .address{font:500 25px/1.25 'IBM Plex Mono',monospace;letter-spacing:-.025em;margin:0}
</style>
<div id="${cid}-root" data-composition-id="${cid}" data-width="1080" data-height="1920" data-duration="${scene.duration}">
 <div id="${cid}-content" class="clip" data-start="0" data-duration="${scene.duration}" data-track-index="1">
  ${isClose ? '<img class="logo" id="v2-close-logo" src="assets/icon.png" alt="GoVue">' : ''}
  <div class="copy" id="${cid}-copy"><h1>${lines.map((line,i)=>`<span id="${cid}-line-${i}" class="line">${line}</span>`).join('')}</h1></div>
  <p id="${cid}-sub" class="sub">${scene.sub.split('\n').map(line=>`<span class="line">${line}</span>`).join('')}</p>
  ${isClose ? '<div id="v2-close-cta" class="cta"><p class="platform">Android + iOS · open source</p><p class="address">github.com/kosumic/GoVue</p></div>' : ''}
 </div>
</div>
<script>(()=>{
 const out=p=>p===1?1:1-Math.pow(2,-10*p);
 const into=p=>p===0?0:Math.pow(2,10*(p-1));
 const tl=gsap.timeline({paused:true});
 ${lines.map((_,i)=>scene.id==='phrase' && i===0 ? `tl.fromTo('#${cid}-line-${i}',{x:-12,opacity:1},{x:0,opacity:1,duration:.5,ease:out},0);` : `tl.fromTo('#${cid}-line-${i}',{y:35,opacity:0},{y:0,opacity:1,duration:.65,ease:out},${(isClose ? .8 : scene.id === 'reader' ? 1.15 : .18)+i*.12});`).join('\n')}
 tl.fromTo('#${cid}-sub',{y:18,opacity:0},{y:0,opacity:1,duration:.5,ease:out},${scene.noteAt});
 ${isClose ? `tl.fromTo('#v2-close-logo',{scale:.9,opacity:0},{scale:1,opacity:1,duration:.6,ease:out},.7);tl.fromTo('#v2-close-cta',{y:15,opacity:0},{y:0,opacity:1,duration:.6,ease:out},1.6);` : `tl.to(['#${cid}-copy','#${cid}-sub'],{y:-25,opacity:0,duration:.3,ease:into},${scene.exitAt ?? scene.duration-.3});`}
 window.__timelines['${cid}']=tl;
})();</script>
</template></body></html>\n`);
}

put('compositions/v2/identity.html', `<!doctype html><html lang="en"><body><template>
<style>#v2-identity-root{position:absolute;inset:0;width:100%;height:100%;pointer-events:none}#v2-brand{position:absolute;left:96px;top:145px;display:flex;align-items:center;gap:14px;font:600 29px Barlow,sans-serif;letter-spacing:-.03em;color:#151B24}#v2-brand img{width:43px;height:43px;object-fit:contain}</style>
<div id="v2-identity-root" data-composition-id="v2-identity" data-width="1080" data-height="1920" data-duration="45"><div id="v2-identity-content" class="clip" data-start="0" data-duration="45" data-track-index="1"><div id="v2-brand"><img src="assets/icon.png" alt=""><span>GoVue</span></div></div></div>
<script>(()=>{const tl=gsap.timeline({paused:true});tl.to('#v2-brand',{opacity:0,duration:.2},41);window.__timelines['v2-identity']=tl;})();</script>
</template></body></html>`);

put('index.html', `<!doctype html><html lang="en"><head>
<meta charset="UTF-8"><meta name="viewport" content="width=1080,height=1920"><title>GoVue — A phrase becomes a lesson</title>
<script src="assets/vendor/gsap.min.js"></script>
<script type="importmap">{"imports":{"three":"./assets/vendor/three.module.js","three/addons/":"./assets/vendor/three-addons/"}}</script>
<style>
@font-face{font-family:Barlow;font-style:normal;font-weight:400;src:url('assets/fonts/Barlow-400.ttf') format('truetype')}
@font-face{font-family:Barlow;font-style:normal;font-weight:500 600;src:url('assets/fonts/Barlow-600.ttf') format('truetype')}
@font-face{font-family:Barlow;font-style:normal;font-weight:700;src:url('assets/fonts/Barlow-700.ttf') format('truetype')}
@font-face{font-family:'IBM Plex Mono';font-style:normal;font-weight:500;src:url('assets/fonts/IBM-Plex-Mono-500.ttf') format('truetype')}
@font-face{font-family:'Instrument Serif';font-style:normal;font-weight:400;src:url('assets/fonts/InstrumentSerif-Regular.ttf') format('truetype')}
@font-face{font-family:'Instrument Serif';font-style:italic;font-weight:400;src:url('assets/fonts/InstrumentSerif-Italic.ttf') format('truetype')}
*{box-sizing:border-box}html,body{margin:0;width:100%;height:100%;overflow:hidden;background:#F4F1EA;color:#151B24;font-family:Barlow,sans-serif}
#root{position:relative;width:100%;height:100%;overflow:hidden;background:#F4F1EA}
.clip{position:absolute;inset:0;width:100%;height:100%}
#v2-stage{display:block;width:100%;height:100%}
</style></head><body>
<div id="root" data-composition-id="govue-v2" data-start="0" data-width="1080" data-height="1920" data-duration="45">
 <canvas id="v2-stage" class="clip" data-start="0" data-duration="45" data-track-index="0" data-layout-allow-overflow width="1080" height="1920"></canvas>
 ${scenes.map(s=>`<div id="v2-mount-${s.id}" data-composition-id="v2-${s.id}" data-composition-src="compositions/v2/${s.id}.html" data-start="${s.start}" data-duration="${s.duration}" data-track-index="1" data-track-kind="graphics" data-width="1080" data-height="1920"></div>`).join('\n')}
 <div id="v2-mount-identity" data-composition-id="v2-identity" data-composition-src="compositions/v2/identity.html" data-start="0" data-duration="45" data-track-index="2" data-track-kind="graphics" data-width="1080" data-height="1920"></div>
 <audio id="v2-score" src="assets/audio/govue-score.wav" data-start="0" data-duration="45" data-track-index="3" data-volume="0.75" data-automation="{&quot;version&quot;:1,&quot;lanes&quot;:[{&quot;target&quot;:&quot;volume&quot;,&quot;points&quot;:[{&quot;t&quot;:0,&quot;v&quot;:0},{&quot;t&quot;:0.16,&quot;v&quot;:1},{&quot;t&quot;:43.8,&quot;v&quot;:1},{&quot;t&quot;:45,&quot;v&quot;:0}]}]}"></audio>
</div>
<script>const main=gsap.timeline({paused:true});window.__timelines['govue-v2']=main;</script>
<script type="module">
import * as THREE from 'three';window.THREE=THREE;
window.__hf=window.__hf||{};window.__hf.buildReady=window.__hf.buildReady||{};
window.__hf.buildReady.govueV2=import('./stage-v2.js').then(m=>m.buildStage(THREE));
</script></body></html>\n`);

const glyphs = {};
const font = fontkit.create(fs.readFileSync(path.join(root, 'assets/fonts/InstrumentSerif-Regular.ttf')));
for (const char of new Set("Commenttu t’appelles ?What’syour name?")) {
  const glyph = font.glyphForCodePoint(char.codePointAt(0));
  glyphs[char] = { advance:glyph.advanceWidth, commands:glyph.path.commands };
}
put('assets/phrase-glyphs.json', JSON.stringify({ units:font.unitsPerEm, glyphs }));
console.log('Built phrase-led v2: 45 seconds, seven scenes, one continuous 3D phrase carrier.');
