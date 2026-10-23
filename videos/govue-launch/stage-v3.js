import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';
import { RoomEnvironment } from 'three/addons/environments/RoomEnvironment.js';
import { RoundedBoxGeometry } from 'three/addons/geometries/RoundedBoxGeometry.js';

export async function buildStage(THREE) {
  const canvas = document.getElementById('live-stage');
  const variables = window.__hyperframes.getVariables();
  const rasterScale = Math.max(.25, Math.min(1, Number(variables.rasterScale) || .5));
  const renderer = new THREE.WebGLRenderer({ canvas, antialias: rasterScale > .5, alpha: false, preserveDrawingBuffer: true });
  renderer.setPixelRatio(1); renderer.setSize(Math.round(1080 * rasterScale), Math.round(1920 * rasterScale), false);
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  renderer.toneMapping = THREE.ACESFilmicToneMapping; renderer.toneMappingExposure = .94;
  renderer.shadowMap.enabled = false;
  const scene = new THREE.Scene();
  const LIGHT = new THREE.Color('#e9edf2'), DARK = new THREE.Color('#0d1420');
  scene.background = LIGHT.clone();
  const camera = new THREE.PerspectiveCamera(35, 1080 / 1920, .05, 100);
  const environment = new RoomEnvironment();
  environment.traverse(object => { if (object.isLight) object.intensity *= .25; });
  const studio = new THREE.Scene(); studio.add(environment);
  for (const [x, z, width, intensity] of [[-5, 4, 2.8, 5], [5, -2, 1.3, 7]]) {
    const strip = new THREE.Mesh(new THREE.PlaneGeometry(width, 10), new THREE.MeshBasicMaterial({ color: 0xffffff, side: THREE.DoubleSide }));
    strip.material.color.multiplyScalar(intensity); strip.position.set(x, 2, z); strip.lookAt(0, 0, 0); studio.add(strip);
  }
  const pmrem = new THREE.PMREMGenerator(renderer); scene.environment = pmrem.fromScene(studio, .04).texture;
  scene.environmentIntensity = .65;
  let previewEnvironment;
  if (rasterScale < 1) {
    const target = new THREE.WebGLCubeRenderTarget(128, { type: THREE.HalfFloatType });
    new THREE.CubeCamera(.1, 100, target).update(renderer, studio);
    previewEnvironment = target.texture;
  }
  const key = new THREE.DirectionalLight(0xfffaf4, 3.4); key.position.set(-5, 7, 9);
  const rim = new THREE.DirectionalLight(0x3ea6ff, 3); rim.position.set(5, 1, -4);
  scene.add(key, rim, new THREE.HemisphereLight(0xffffff, 0x8e9aad, 1.9));
  const floor = new THREE.Mesh(new THREE.PlaneGeometry(100, 100), new THREE.ShadowMaterial({ opacity: .065 }));
  floor.rotation.x = -Math.PI / 2; floor.position.y = -3.65; floor.visible = false; scene.add(floor);

  const gltf = await new GLTFLoader().loadAsync('assets/models/iphone-17-pro/iphone-17-pro.glb');
  const glyphs = await fetch('assets/glyphs-v3.json').then(r => r.json());
  const plan = await fetch('scenes.json').then(r => r.json());
  const sceneStart = id => plan.find(s => s.id === id).start;
  const cuts = { reader: sceneStart('reader'), word: sceneStart('longpress'), chat: sceneStart('chat'), quiz: sceneStart('practice'), close: sceneStart('close'), end: plan.at(-1).start + plan.at(-1).duration };
  gltf.scene.updateMatrixWorld(true);
  const bounds = new THREE.Box3().setFromObject(gltf.scene), center = bounds.getCenter(new THREE.Vector3());
  const size = bounds.getSize(new THREE.Vector3()), unit = 5.8 / Math.max(size.x, size.y, size.z);
  const screenMesh = gltf.scene.getObjectByName('Object_33');
  if (!screenMesh) throw new Error('Whip iPhone model: Object_33 display is missing');
  const screenBox = new THREE.Box3().setFromObject(screenMesh);
  const sc = screenBox.getCenter(new THREE.Vector3()), ss = screenBox.getSize(new THREE.Vector3());
  gltf.scene.traverse(mesh => {
    if (!mesh.isMesh) return;
    mesh.material = mesh.material.clone();
    // The live display replaces these two full-screen surfaces. Leaving them
    // beneath it makes software WebGL shade an entire hidden phone screen.
    if (['Screen_BG', 'Screen_Glass'].includes(mesh.material.name)) mesh.visible = false;
    if (mesh.material.isMeshStandardMaterial) {
      mesh.material.roughness = Math.max(.18, Math.min(.65, mesh.material.roughness)); mesh.material.envMapIntensity = .8;
    }
    if (['Camera_Pixel_Glass_002', 'Camera_Pixel__002'].includes(mesh.material.name)) {
      mesh.material.map = null; mesh.material.color.set('#111b29'); mesh.material.metalness = .2;
      mesh.material.roughnessMap = null; mesh.material.metalnessMap = null;
      mesh.material.specularIntensityMap = null; mesh.material.specularColorMap = null;
      mesh.material.roughness = .18; mesh.material.envMapIntensity = .45;
    }
    if (rasterScale < 1 && mesh.material.isMeshStandardMaterial) {
      const source = mesh.material;
      mesh.material = new THREE.MeshPhongMaterial({
        color: source.color, map: source.map, normalMap: source.normalMap,
        normalScale: source.normalScale, envMap: previewEnvironment,
        reflectivity: Math.min(.35, .08 + source.metalness * .25),
        specular: new THREE.Color('#4f5d6a'), shininess: Math.max(12, 100 * (1 - source.roughness)),
        transparent: source.transparent, opacity: source.opacity,
        alphaTest: source.alphaTest, side: source.side, depthWrite: source.depthWrite,
      });
    }
  });
  const maskCanvas = document.createElement('canvas'); maskCanvas.width = 600; maskCanvas.height = 1304;
  const ctx = maskCanvas.getContext('2d'); ctx.fillStyle = 'white'; ctx.beginPath(); ctx.roundRect(0, 0, 600, 1304, 76); ctx.fill();
  const mask = new THREE.CanvasTexture(maskCanvas);
  const media = new Map(['browse', 'reader', 'longpress', 'chat', 'quiz'].map(name => {
    const video = document.getElementById(`live-${name}`), live = new THREE.VideoTexture(video), extracted = new THREE.Texture();
    for (const texture of [live, extracted]) {
      texture.colorSpace = THREE.SRGBColorSpace; texture.minFilter = THREE.LinearFilter; texture.magFilter = THREE.LinearFilter;
    }
    return [name, { video, live, extracted }];
  }));
  function textureFor(name) {
    const source = media.get(name), id = window.__hfMediaId?.(source.video) || source.video.id;
    const frame = document.getElementById(`__render_frame_${id}__`);
    if (frame?.complete && frame.naturalWidth > 10) {
      source.extracted.image = frame; source.extracted.needsUpdate = true; return source.extracted;
    }
    if (source.video.readyState >= HTMLMediaElement.HAVE_CURRENT_DATA) source.live.needsUpdate = true;
    return source.live;
  }
  function phone() {
    const rig = new THREE.Group(), model = gltf.scene.clone(true), normalizer = new THREE.Group();
    model.position.sub(center); normalizer.add(model); normalizer.scale.setScalar(unit); rig.add(normalizer);
    const display = new THREE.Mesh(new THREE.PlaneGeometry(ss.x * unit * .982, ss.y * unit * .983),
      new THREE.MeshBasicMaterial({ map: media.get('browse').live, alphaMap: mask, alphaTest: .5, toneMapped: false }));
    display.position.set((sc.x - center.x) * unit, (sc.y - center.y) * unit, (sc.z - center.z) * unit + .022);
    display.renderOrder = -10; rig.add(display); scene.add(rig); return { rig, display };
  }
  const main = phone(), back = phone();
  // Adapt the registry touch-indicator contact/lift envelope to the moving glass.
  // The long hold uses the recorded touch duration; it never hovers over the UI.
  const contact = new THREE.Group();
  const contactCore = new THREE.Mesh(new THREE.CircleGeometry(.09 * main.display.geometry.parameters.width, 48),
    new THREE.MeshBasicMaterial({ color: 0xffffff, transparent: true, opacity: .35, depthWrite: false, toneMapped: false }));
  const contactEdge = new THREE.Mesh(new THREE.RingGeometry(.088 * main.display.geometry.parameters.width, .09 * main.display.geometry.parameters.width, 48),
    new THREE.MeshBasicMaterial({ color: 0xffffff, transparent: true, opacity: .6, depthWrite: false, toneMapped: false }));
  contact.add(contactCore, contactEdge); main.rig.add(contact);
  const frontMaterial = new THREE.MeshStandardMaterial({ color: 0xf2f6fa, roughness: .32, metalness: .15 });
  const sideMaterial = new THREE.MeshPhysicalMaterial({ color: 0x3ea6ff, roughness: .22, metalness: .55, clearcoat: 1 });
  const wordmark = new THREE.Group(); let advance = 0;
  for (const char of 'GoVue') {
    const glyph = glyphs.glyphs[char], pen = new THREE.ShapePath();
    for (const c of glyph.commands) {
      const a = c.args.map(v => v / glyphs.units);
      if (c.command === 'moveTo') pen.moveTo(...a); if (c.command === 'lineTo') pen.lineTo(...a);
      if (c.command === 'quadraticCurveTo') pen.quadraticCurveTo(...a); if (c.command === 'bezierCurveTo') pen.bezierCurveTo(...a);
      if (c.command === 'closePath') pen.currentPath.closePath();
    }
    const mesh = new THREE.Mesh(new THREE.ExtrudeGeometry(pen.toShapes(), { depth: .12, bevelEnabled: true, bevelThickness: .008, bevelSize: .006, bevelSegments: 3, curveSegments: 12 }), [frontMaterial, sideMaterial]);
    mesh.position.x = advance; wordmark.add(mesh); advance += glyph.advance / glyphs.units;
  }
  const wb = new THREE.Box3().setFromObject(wordmark), wc = wb.getCenter(new THREE.Vector3());
  wordmark.children.forEach(o => o.position.sub(wc));
  wordmark.scale.setScalar(2.8 / wb.getSize(new THREE.Vector3()).x);
  wordmark.position.x = .65;
  const logoTexture = await new THREE.TextureLoader().loadAsync('assets/icon.png');
  logoTexture.colorSpace = THREE.SRGBColorSpace;
  const logoBadge = new THREE.Mesh(new RoundedBoxGeometry(1.1, 1.1, .16, 5, .075),
    new THREE.MeshPhysicalMaterial({ color: 0xffffff, roughness: .28, metalness: .08, clearcoat: .65 }));
  logoBadge.position.set(-1.47, 0, .02);
  const logoFace = new THREE.Mesh(new THREE.PlaneGeometry(.98, .98),
    new THREE.MeshBasicMaterial({ map: logoTexture, transparent: true, alphaTest: .01, toneMapped: false }));
  logoFace.position.set(-1.47, 0, .105);
  const brand = new THREE.Group(); brand.add(logoBadge, logoFace, wordmark); scene.add(brand);
  const clamp = x => Math.max(0, Math.min(1, x));
  const smooth = x => { x = clamp(x); return x * x * (3 - 2 * x); };
  const out = x => 1 - (1 - clamp(x)) ** 3;
  const mix = THREE.MathUtils.lerp, v = (a, b, p) => a.map((x, i) => mix(x, b[i], p));
  const pose = (rig, p, r, scale = 1) => { rig.position.set(...p); rig.rotation.set(...r); rig.scale.setScalar(scale); };
  const view = (p, target, roll = 0) => { camera.position.set(...p); camera.lookAt(...target); camera.rotateZ(roll); };
  let time = 0, sourceName = 'browse', mediaRevision = 0, lastRevision = -1, lastTime = -1;
  function renderAt(raw) {
    const t = Math.max(0, Math.min(cuts.end, raw)); time = t;
    if (window[Symbol.for('hyperframes.webglDrawGate')]?.skip) return;
    if (Math.abs(t - lastTime) < 1e-6 && mediaRevision === lastRevision) return;
    lastTime = t; lastRevision = mediaRevision;
    const darken = smooth((t - cuts.close) / .65); scene.background.copy(LIGHT).lerp(DARK, darken);
    floor.material.opacity = .065 * (1 - darken); key.intensity = mix(3.4, 4.5, darken); rim.intensity = mix(3, 5, darken);
    main.rig.visible = true; back.rig.visible = t >= cuts.close; brand.visible = t < 3 || t >= cuts.close;
    frontMaterial.color.set(t < cuts.close ? '#111b29' : '#f2f6fa');
    sourceName = t < cuts.reader ? 'browse' : t < cuts.word ? 'reader' : t < cuts.chat ? 'longpress' : t < cuts.quiz ? 'chat' : 'quiz';
    const texture = textureFor(sourceName);
    if (main.display.material.map !== texture) { main.display.material.map = texture; main.display.material.needsUpdate = true; }
    if (t < 2.45) {
      const reveal = out(t / 1.5), settle = smooth((t - 1.5) / .95);
      pose(main.rig, v([.4, -.25, 0], [.05, -.6, 0], reveal), [mix(.16, -.035, reveal), mix(3.38, -.28, reveal) + settle * .08, mix(-.28, .035, reveal)], mix(1.1, .88, reveal));
      view(v([1.1, 1.85, 6.5], [.2, .15, 15.2], reveal), v([0, 1.5, 0], [0, 0, 0], reveal), mix(-.14, 0, reveal));
    } else if (t < cuts.reader) {
      const p = smooth((t - 2.45) / (cuts.reader - 2.45));
      pose(main.rig, v([.05, -.6, 0], [0, -.35, 0], p), [mix(-.035, .01, p), mix(-.2, -.045, p), mix(.035, -.012, p)], mix(.88, 1.07, p));
      view(v([.2, .15, 15.2], [.25, 1.05, 9.6], p), v([0, 0, 0], [0, 1.05, 0], p));
    } else if (t < cuts.word) {
      const move = smooth((t - cuts.reader - 1.1) / 1.35), exit = smooth((t - cuts.word + .65) / .65);
      pose(main.rig, [0, -.35, 0], [.01, mix(-.045, -.12, exit), -.012], 1.07);
      view(v([.25, 1.05, 9.6], [.08, -.65, 9.8], move), v([0, 1.05, 0], [0, -.65, 0], move));
    } else if (t < cuts.chat) {
      const p = smooth((t - cuts.word) / 1.1), focus = smooth((t - cuts.word - 1.1) / 1.2);
      pose(main.rig, [0, -.35, 0], [.01, mix(-.12, .055, p), mix(-.012, .012, p)], 1.07);
      view(v([.08, -.65, 9.8], [.15, -.85, 9.3], focus), [0, -.7, 0]);
    } else if (t < cuts.quiz) {
      const p = out((t - cuts.chat) / 1.15), reframe = smooth((t - cuts.chat - 2.4) / 1.25);
      pose(main.rig, [0, -.25, 0], [mix(.04, .01, p), mix(-.45, -.035, p), mix(-.06, 0, p)], 1.04);
      view(v([1.35, -.4, 11.5], [.1, mix(.25, -.75, reframe), 9.5], p), [0, mix(.25, -.75, reframe), 0]);
    } else if (t < cuts.close) {
      const p = out((t - cuts.quiz) / 1.1), pull = smooth((t - cuts.close + .9) / .9);
      pose(main.rig, [0, mix(-.25, -.5, pull), 0], [.01, mix(.25, -.035, p), 0], mix(1.04, .7, pull));
      view(v([.1, -.75, 9.5], [0, -.1, 14.8], pull), [0, mix(-.75, -.1, pull), 0]);
    } else {
      const p = out((t - cuts.close) / 1.1);
      pose(main.rig, v([0, -.5, 0], [-.64, -.42, .35], p), [.025, mix(-.035, -.18, p), mix(0, -.07, p)], mix(.7, .63, p));
      pose(back.rig, v([0, -.5, -.4], [.75, -.3, -.5], p), [.055, mix(1.4, 2.8, p), .09], mix(.45, .64, p));
      brand.position.set(0, 2.72, 0); brand.rotation.set(mix(-.5, 0, p), mix(-.35, 0, p), 0); brand.scale.setScalar(.94 * mix(.8, 1, p));
      view([0, 0, 15.2], [0, 0, 0]);
    }
    if (t < 3) {
      camera.updateMatrixWorld(true);
      brand.position.copy(camera.localToWorld(new THREE.Vector3(0, .84, -4)));
      brand.quaternion.copy(camera.quaternion);
      brand.rotateX(mix(.12, 0, out(t / .6))); brand.rotateY(mix(-.3, 0, out(t / .6)));
      brand.scale.setScalar(.24 * (1 - smooth((t - 2.55) / .45)));
    }
    const gesture = t - cuts.word;
    contact.visible = gesture >= .65 && gesture < 1.75;
    if (contact.visible) {
      const appear = out((gesture - .65) / .067), lift = 1 - clamp((gesture - 1.55) / .2);
      contact.position.copy(main.display.position).add(new THREE.Vector3(
        (291 / 402 - .5) * main.display.geometry.parameters.width,
        (.5 - 550 / 874) * main.display.geometry.parameters.height, .018));
      contact.scale.setScalar(mix(.9, 1, appear)); contactCore.material.opacity = .35 * appear * lift;
      contactEdge.material.opacity = .6 * appear * lift;
    }
    renderer.render(scene, camera);
  }
  for (const [name, m] of media) {
    const refresh = () => { if (name === sourceName) { mediaRevision += 1; renderAt(time); } };
    m.video.addEventListener('seeked', refresh); m.video.addEventListener('loadeddata', refresh);
    if (m.video.requestVideoFrameCallback) {
      const decoded = () => { refresh(); m.video.requestVideoFrameCallback(decoded); }; m.video.requestVideoFrameCallback(decoded);
    }
  }
  document.addEventListener('load', event => {
    if (event.target instanceof HTMLImageElement && event.target.classList.contains('__render_frame__')) { mediaRevision += 1; renderAt(time); }
  }, true);
  const reseek = window.__hfReseekGpu;
  window.__hfReseekGpu = t => { reseek?.(t); mediaRevision += 1; renderAt(t); };
  window.addEventListener('hf-seek', event => renderAt(Number(event.detail?.time ?? event.detail) || 0));
  window.__govueV3 = { renderAt, scene, renderer, camera, main, back, media, brand };
  renderer.compile(scene, camera); renderAt(window.__hfThreeTime || 0);
}
