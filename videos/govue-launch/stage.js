import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';
import { RoomEnvironment } from 'three/addons/environments/RoomEnvironment.js';
import { RoundedBoxGeometry } from 'three/addons/geometries/RoundedBoxGeometry.js';

// Every visible state is a pure function of HyperFrames time. No animation loop.
export async function buildStage(THREE) {
  const W = 1080, H = 1920;
  const canvas = document.getElementById('three-stage');
  const renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: false, preserveDrawingBuffer: true });
  renderer.setSize(W, H, false);
  renderer.setPixelRatio(1);
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1.14;
  const scene = new THREE.Scene();
  scene.background = new THREE.Color('#0F0F0F');
  const camera = new THREE.PerspectiveCamera(35, W / H, .1, 100);
  const environment = new RoomEnvironment();
  const pmrem = new THREE.PMREMGenerator(renderer);
  scene.environment = pmrem.fromScene(environment, .025).texture;
  environment.dispose();
  scene.add(new THREE.HemisphereLight(0xeaf5ff, 0x133050, 2.2));
  const key = new THREE.DirectionalLight(0xeef6ff, 4.2);
  key.position.set(-4, 6, 9); scene.add(key);
  const rim = new THREE.DirectionalLight(0x3ea6ff, 5.5);
  rim.position.set(5, 2, -3); scene.add(rim);
  const fill = new THREE.DirectionalLight(0xffffff, 2);
  fill.position.set(-5, -3, 6); scene.add(fill);

  const blue = new THREE.MeshPhysicalMaterial({ color:0x3ea6ff, metalness:.4, roughness:.19, clearcoat:1, clearcoatRoughness:.16 });
  const chrome = new THREE.MeshStandardMaterial({ color:0xc9d9e7, metalness:1, roughness:.15 });
  const pale = new THREE.MeshPhysicalMaterial({ color:0xffffff, metalness:.16, roughness:.27, clearcoat:.8 });
  const dark = new THREE.MeshPhysicalMaterial({ color:0x142435, metalness:.8, roughness:.22, clearcoat:1 });
  const textures = {};
  const textureLoader = new THREE.TextureLoader();
  await document.fonts.ready;
  await Promise.all(['playback', 'transcript', 'word-card', 'chat', 'game', 'browse', 'search'].map(async name => {
    const texture = await textureLoader.loadAsync(`assets/screens/${name}.png`);
    texture.colorSpace = THREE.SRGBColorSpace;
    texture.anisotropy = Math.min(8, renderer.capabilities.getMaxAnisotropy());
    textures[name] = texture;
  }));
  const iconSource = await textureLoader.loadAsync('assets/icon.png');
  const iconCanvas = document.createElement('canvas'); iconCanvas.width = 1024; iconCanvas.height = 1024;
  const iconContext = iconCanvas.getContext('2d');
  iconContext.fillStyle = '#FFFFFF'; iconContext.fillRect(0, 0, 1024, 1024);
  iconContext.drawImage(iconSource.image, 82, 82, 860, 860);
  const icon = new THREE.CanvasTexture(iconCanvas); icon.colorSpace = THREE.SRGBColorSpace;
  const glyphData = await fetch('assets/glyphs.json').then(response => response.json());
  const gltf = await new GLTFLoader().loadAsync('models/iphone.glb');

  function makePhone(texture) {
    const group = new THREE.Group();
    const model = gltf.scene.clone(true);
    let screen;
    const bounds = new THREE.Box3().setFromObject(model);
    model.scale.setScalar(6.2 / bounds.getSize(new THREE.Vector3()).y);
    bounds.setFromObject(model);
    model.position.sub(bounds.getCenter(new THREE.Vector3()));
    model.traverse(mesh => {
      if (!mesh.isMesh) return;
      mesh.material = mesh.material.clone();
      if (mesh.name.startsWith('front-glass')) {
        mesh.geometry = mesh.geometry.clone();
        mesh.geometry.computeBoundingBox();
        const b = mesh.geometry.boundingBox;
        const p = mesh.geometry.attributes.position;
        const uv = new Float32Array(p.count * 2);
        for (let i = 0; i < p.count; i++) {
          uv[i * 2] = (p.getX(i) - b.min.x) / (b.max.x - b.min.x);
          uv[i * 2 + 1] = (p.getY(i) - b.min.y) / (b.max.y - b.min.y);
        }
        mesh.geometry.setAttribute('uv', new THREE.BufferAttribute(uv, 2));
        mesh.material = new THREE.MeshBasicMaterial({ map:texture, toneMapped:false });
        screen = mesh;
      } else if (mesh.name === 'body') {
        mesh.material.color.set(0x6b879f);
        mesh.material.metalness = 1;
        mesh.material.roughness = .22;
      }
    });
    model.updateWorldMatrix(true, true);
    const front = model.getObjectByName('front-glass');
    const back = model.getObjectByName('rear-window');
    if (front && back) {
      const f = front.getWorldPosition(new THREE.Vector3());
      const b = back.getWorldPosition(new THREE.Vector3());
      if (f.z < b.z) model.rotation.y += Math.PI;
    }
    group.add(model);
    group.userData.screen = screen;
    return group;
  }
  const phone = makePhone(textures.playback);
  const phoneB = makePhone(textures.chat);
  const phoneC = makePhone(textures.game);
  scene.add(phone, phoneB, phoneC);
  const stageProps = new THREE.Group(); scene.add(stageProps);

  function roundedPlate(width, height, depth, material) {
    return new THREE.Mesh(new RoundedBoxGeometry(width, height, depth, 3, Math.min(.14, depth / 2)), material);
  }
  function labelTexture(lines, options = {}) {
    const c = document.createElement('canvas'); c.width = 1536; c.height = options.height || 512;
    const ctx = c.getContext('2d');
    ctx.fillStyle = options.bg || '#FFFFFF'; ctx.fillRect(0, 0, c.width, c.height);
    ctx.textBaseline = 'middle';
    lines.forEach((line, i) => {
      ctx.fillStyle = line.color || '#0F0F0F';
      ctx.font = `${line.weight || 700} ${line.size || 104}px Barlow`;
      ctx.fillText(line.text, 90, line.y || (c.height / (lines.length + 1)) * (i + 1));
    });
    const t = new THREE.CanvasTexture(c); t.colorSpace = THREE.SRGBColorSpace; t.anisotropy = 8;
    return t;
  }
  function card(width, height, lines, options = {}) {
    const group = new THREE.Group();
    const base = roundedPlate(width, height, .14, options.blue ? blue : pale);
    const tex = labelTexture(lines, { bg:options.blue ? '#3EA6FF' : '#FFFFFF', height:options.textureHeight || 512 });
    const face = new THREE.Mesh(new THREE.PlaneGeometry(width - .1, height - .1), new THREE.MeshBasicMaterial({ map:tex, toneMapped:false }));
    face.position.z = .075;
    group.add(base, face); stageProps.add(group); return group;
  }
  const french = card(4.1, 1.15, [{ text:'FR / FRENCH', size:54, weight:600, y:103 }, { text:'Comment tu t’appelles ?', size:118, y:300 }], { blue:true });
  const english = card(4.1, 1.15, [{ text:'EN / ENGLISH', size:54, weight:600, y:103 }, { text:'What’s your name?', size:128, y:300 }]);
  const saved = card(2.9, 1.05, [{ text:'saved in context', size:112, y:260 }], { blue:true });
  const meaning = card(3.6, 1.3, [{ text:'comment', size:182, y:210 }, { text:'how · in this phrase', size:74, weight:400, y:392 }]);
  const ask = card(3.2, 1, [{ text:'Explain this phrase.', size:117, y:260 }]);
  const answer = card(3.2, 1.3, [{ text:'your AI', size:152, y:190 }, { text:'the lesson in context', size:78, weight:400, y:375 }]);
  const quizA = card(2.6, .85, [{ text:'bonjour', size:170, y:270 }], { blue:true });
  const quizB = card(2.6, .85, [{ text:'je m’appelle…', size:136, y:270 }]);
  const quizC = card(2.6, .85, [{ text:'à toi de jouer', size:142, y:270 }], { blue:true });

  // Extruded Barlow wordmark: real geometry, rather than flat text in a 3D wrapper.
  function word(text, size, material, depth = .14) {
    const group = new THREE.Group(); let offset = 0;
    const scale = size / glyphData.units;
    for (const char of text) {
      const glyph = glyphData.glyphs[char];
      if (!glyph) continue;
      const pen = new THREE.ShapePath();
      for (const command of glyph.commands) {
        const a = command.args;
        if (command.command === 'moveTo') pen.moveTo(a[0] * scale, a[1] * scale);
        if (command.command === 'lineTo') pen.lineTo(a[0] * scale, a[1] * scale);
        if (command.command === 'quadraticCurveTo') pen.quadraticCurveTo(...a.map(v => v * scale));
        if (command.command === 'bezierCurveTo') pen.bezierCurveTo(...a.map(v => v * scale));
        if (command.command === 'closePath') pen.currentPath.closePath();
      }
      const geometry = new THREE.ExtrudeGeometry(pen.toShapes(), { depth, bevelEnabled:true, bevelSegments:3, steps:1, bevelSize:.018, bevelThickness:.018, curveSegments:8 });
      const mesh = new THREE.Mesh(geometry, material); mesh.position.x = offset; group.add(mesh);
      offset += glyph.advance * scale;
    }
    const b = new THREE.Box3().setFromObject(group);
    const center = b.getCenter(new THREE.Vector3());
    group.children.forEach(child => child.position.sub(center));
    scene.add(group); return group;
  }
  const wordmark = word('GoVue', 1.34, blue, .23);
  const cc = word('CC', 1.9, chrome, .22);
  const playShape = new THREE.Shape();
  playShape.moveTo(-.72, -.94); playShape.lineTo(-.72, .94); playShape.quadraticCurveTo(-.72, 1.05, -.6, .99);
  playShape.lineTo(.94, .08); playShape.quadraticCurveTo(1.08, 0, .94, -.08); playShape.lineTo(-.6, -.99); playShape.quadraticCurveTo(-.72, -1.05, -.72, -.94);
  const play = new THREE.Mesh(new THREE.ExtrudeGeometry(playShape, { depth:.38, bevelEnabled:true, bevelSize:.075, bevelThickness:.075, bevelSegments:5, curveSegments:16 }), blue);
  scene.add(play);
  const ring = new THREE.Mesh(new THREE.TorusGeometry(3.1, .24, 24, 120), chrome);
  const ring2 = new THREE.Mesh(new THREE.TorusGeometry(3.8, .075, 16, 128), blue);
  scene.add(ring, ring2);
  const iconTile = new THREE.Group();
  iconTile.add(roundedPlate(1.6, 1.6, .35, dark));
  const iconFace = new THREE.Mesh(new THREE.PlaneGeometry(1.49, 1.49), new THREE.MeshBasicMaterial({ map:icon, toneMapped:false }));
  iconFace.position.z = .18; iconTile.add(iconFace); scene.add(iconTile);

  // Long physical ribbons give the camera a near, middle, and far plane.
  const ribbon = new THREE.Group();
  for (let i = 0; i < 4; i++) {
    const arc = new THREE.Mesh(new THREE.TorusGeometry(4.2 + i * .24, .032 + i * .014, 8, 96, Math.PI * 1.3), i % 2 ? blue : chrome);
    arc.rotation.x = .5 + i * .08; arc.rotation.z = i * .45; arc.position.z = -3 - i * .28; ribbon.add(arc);
  }
  scene.add(ribbon);
  const cuts = [0, 5.625, 13.125, 21.5625, 29.0625, 34.6875, 40.3125, 45];
  const clamp = x => Math.max(0, Math.min(1, x));
  const smooth = x => { x = clamp(x); return x * x * (3 - 2 * x); };
  const ease = x => 1 - (1 - clamp(x)) ** 4;
  const lerp = (a, b, x) => a + (b - a) * x;
  const pose = (object, p, r = [0, 0, 0], scale = 1) => {
    object.visible = true;
    object.position.set(...p); object.rotation.set(...r); object.scale.setScalar(scale);
  };
  let currentTexture = '';
  function screen(name) {
    if (currentTexture === name) return;
    phone.userData.screen.material.map = textures[name];
    currentTexture = name;
  }
  const all = [phone, phoneB, phoneC, french, english, saved, meaning, ask, answer, quizA, quizB, quizC, wordmark, cc, play, ring, ring2, iconTile, ribbon];

  function renderAt(rawTime) {
    const t = Math.max(0, Math.min(45, rawTime));
    let shot = 6;
    for (let i = 0; i < 7; i++) if (t < cuts[i + 1]) { shot = i; break; }
    const u = t - cuts[shot];
    const duration = cuts[shot + 1] - cuts[shot];
    const enter = ease(u / 1.5);
    const travel = smooth(u / duration);
    const brand = document.getElementById('brand');
    if (brand) brand.style.color = shot === 4 ? '#0F0F0F' : '#FFFFFF';
    all.forEach(object => { object.visible = false; });
    camera.position.set(0, .15, 19.2);
    camera.lookAt(0, 0, 0);
    scene.background.set('#0F0F0F');

    if (shot === 0) {
      screen('playback');
      pose(phone, [lerp(1.4, .6, enter), -1.3, lerp(-2, 0, enter)], [.08, lerp(-1.48, -.18, enter), lerp(-.22, -.07, enter)], .98);
      pose(play, [lerp(-2.3, -1.7, enter), -1.1, 1.1], [.16, lerp(-.7, .55, travel), -.15], 1.12);
      pose(ring, [.7, -1.8, -4.8], [1.0, .3, -.3 + travel * .65], 1.03);
      pose(ribbon, [0, -2, -2], [0, 0, -.3 + .18 * travel]);
      camera.position.x = lerp(-1.2, .3, travel);
      camera.lookAt(0, 0, 0);
      if (u > 4.7) phone.position.z += ease((u - 4.7) / .925) * 2;
    } else if (shot === 1) {
      screen('playback');
      pose(phone, [.35, -1.45, .5], [.04, lerp(.36, -.16, travel), -.08], 1.02);
      pose(play, [-1.65 + Math.sin(travel * Math.PI) * .65, -1.35, -1.2], [.1, travel * 1.7, -.2], 1.35);
      pose(ring, [.3, -1.7, -5.1], [1.05, -.4, .1 + .9 * travel], 1.15);
      pose(ring2, [.3, -1.7, -5.1], [.9, -.6, -.3 + .6 * travel], 1.02);
      camera.position.set(lerp(2.5, -.7, travel), .2, lerp(20.5, 18.4, travel)); camera.lookAt(.1, -.1, 0);
    } else if (shot === 2) {
      screen('transcript');
      pose(phone, [.52, -1.38, -.2], [.07, lerp(-.35, -.08, travel), -.04], .98);
      pose(cc, [-1.7, -.25, -2.2], [.1, .25, -.15], .9);
      const lift = ease((u - 2.1) / 1.4);
      const settle = smooth((u - 5.2) / 1.4);
      if (u >= 2.1) {
        pose(french, [lerp(.15, -.26, lift), lerp(-1.45, -.65, lift), lerp(.2, 2.3, lift)], [lerp(.28, -.03, lift), lerp(-.45, .1, lift), -.04], lerp(.65, 1, lift));
        pose(english, [lerp(.15, .1, lift), lerp(-2, -1.95, lift), lerp(.3, 2.8, lift)], [lerp(.35, .02, lift), lerp(.4, -.08, lift), .035], lerp(.65, 1, lift));
        phone.position.x = lerp(.52, .75, lift);
        phone.rotation.y = lerp(-.2, -.36, lift);
        french.position.z -= settle * .15;
      }
      pose(ribbon, [0, -1.5, -2], [.2, -.1, -.5 + .4 * travel]);
      camera.position.set(lerp(-.8, .4, travel), .15, 19); camera.lookAt(0, -.1, 0);
    } else if (shot === 3) {
      screen('word-card');
      pose(phone, [1.0, -1.45, -.2], [.05, -.29 + .16 * travel, -.065], .99);
      const lift = ease((u - 1.8) / 1.15);
      if (u >= 1.8) {
        pose(meaning, [lerp(-1.4, -.9, lift), lerp(-2.2, -.75, lift), lerp(-1, 2.2, lift)], [0, lerp(-.9, .1, lift), -.06], .9);
        pose(saved, [lerp(-1.2, -.7, lift), lerp(-3.3, -2.28, lift), lerp(-.5, 2.5, lift)], [0, -.1, .07], .9);
      }
      pose(ribbon, [.35, -1.9, -3], [.35, -.35, -.3 + .5 * travel], .95);
      camera.position.set(lerp(-.6, .4, travel), .2, 19.4); camera.lookAt(0, 0, 0);
    } else if (shot === 4) {
      scene.background.set('#3EA6FF');
      screen('chat');
      pose(phone, [.7, -1.45, .05], [.07, lerp(.32, -.16, enter), -.07], 1.0);
      const lift = ease((u - 1.75) / 1.2);
      if (u >= 1.75) {
        pose(ask, [lerp(-2.2, -.85, lift), -.65, lerp(-1, 2.1, lift)], [0, lerp(-.6, .1, lift), -.065], .88);
        pose(answer, [lerp(2.3, -.42, lift), -2.05, lerp(-2, 2.5, lift)], [0, lerp(.8, -.1, lift), .045], .85);
      }
      pose(ring2, [0, -1.8, -4.8], [.6, .4, -.2 + .5 * travel], 1);
      camera.position.set(lerp(.7, -.4, travel), .2, 19.3); camera.lookAt(0, -.05, 0);
    } else if (shot === 5) {
      screen('game');
      pose(phone, [.38, -1.35, .2], [.03, lerp(-.65, .06, enter), -.04], 1.02);
      const lift = ease((u - 1.4) / 1.0);
      if (u >= 1.4) {
        pose(quizA, [-1.8, -.75 + .1 * travel, lerp(-2, 1.5, lift)], [.08, .35, -.15], .75);
        pose(quizB, [1.8, -1.35, lerp(-3, -.6, lift)], [.1, -.3, .13], .78);
        pose(quizC, [-1.2, -3.35, lerp(-1.5, 1.8, lift)], [-.06, .1, -.06], .86);
      }
      pose(play, [-1.4, -1.6, -3.1], [.18, -.5 + .6 * travel, -.1], 1.8);
      camera.position.set(lerp(-.8, .5, travel), .2, lerp(20, 18.7, travel)); camera.lookAt(0, -.05, 0);
      if (u > 4.85) phone.rotation.y += ease((u - 4.85) / .775) * .5;
    } else {
      screen('transcript');
      const fan = ease(u / 1.45);
      pose(phoneB, [lerp(0, -1.5, fan), -.7, -.85], [.02, -.3, lerp(0, .17, fan)], .64);
      pose(phoneC, [lerp(0, 1.55, fan), -.68, -.8], [.02, .3, lerp(0, -.17, fan)], .64);
      pose(phone, [0, -.6, .5], [.015, lerp(-.15, .03, fan), 0], .68);
      pose(wordmark, [0, 4.1, .5], [.03, lerp(.65, -.06, fan), -.02], 1.1);
      pose(iconTile, [0, -1.15, lerp(4, 2.9, fan)], [.04, lerp(1.3, -.08, fan), -.08], .95);
      // Move the physical icon off the center after its reveal to expose the reader.
      const leave = ease((u - 1.1) / .75);
      iconTile.position.x = lerp(0, -2.0, leave);
      iconTile.position.y = lerp(-1.15, -2.1, leave);
      iconTile.scale.setScalar(lerp(.95, .58, leave));
      pose(ring2, [0, -1.1, -4.8], [.8, .3, .5], 1.02);
      camera.position.set(.05, .05, 20); camera.lookAt(0, -.1, 0);
    }
    renderer.render(scene, camera);
  }
  window.addEventListener('hf-seek', event => renderAt(event.detail.time));
  window.__govueStage = { renderAt, renderer, scene, camera };
  renderAt(window.__hfThreeTime || 0);
}
