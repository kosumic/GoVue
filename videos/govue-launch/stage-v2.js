import { RoomEnvironment } from 'three/addons/environments/RoomEnvironment.js';
import { RoundedBoxGeometry } from 'three/addons/geometries/RoundedBoxGeometry.js';

export async function buildStage(THREE) {
  const renderer = new THREE.WebGLRenderer({canvas:document.getElementById('v2-stage'),antialias:true,alpha:false,preserveDrawingBuffer:true});
  renderer.setSize(1080,1920,false);renderer.setPixelRatio(1);
  renderer.outputColorSpace=THREE.SRGBColorSpace;
  renderer.toneMapping=THREE.ACESFilmicToneMapping;renderer.toneMappingExposure=.9;
  renderer.shadowMap.enabled=true;renderer.shadowMap.type=THREE.VSMShadowMap;
  const scene=new THREE.Scene();scene.background=new THREE.Color('#F4F1EA');
  const camera=new THREE.PerspectiveCamera(32,1080/1920,.1,100);
  const pmrem=new THREE.PMREMGenerator(renderer);
  const room=new RoomEnvironment();scene.environment=pmrem.fromScene(room,.02).texture;room.dispose();
  scene.add(new THREE.HemisphereLight(0xffffff,0x8c9aaa,1.25));
  const key=new THREE.DirectionalLight(0xffffff,2);key.position.set(-3,6,18);key.castShadow=true;
  key.shadow.mapSize.set(2048,2048);Object.assign(key.shadow.camera,{left:-9,right:9,top:10,bottom:-10,near:.5,far:40});
  key.shadow.bias=-.0002;key.shadow.normalBias=.025;key.shadow.radius=10;key.shadow.blurSamples=12;scene.add(key);
  const fill=new THREE.DirectionalLight(0xdcecff,.65);fill.position.set(6,0,5);scene.add(fill);
  const paper=new THREE.MeshStandardMaterial({color:0xfaf9f5,roughness:.56,metalness:0});
  const edge=new THREE.MeshPhysicalMaterial({color:0x187ff2,roughness:.3,metalness:.08,clearcoat:.6});
  const ink=new THREE.MeshStandardMaterial({color:0x151b24,roughness:.42,metalness:.05});
  const dark=new THREE.MeshStandardMaterial({color:0x16191c,roughness:.43,metalness:.04});
  const shadow=new THREE.Mesh(new THREE.PlaneGeometry(40,40),new THREE.ShadowMaterial({color:0x263847,opacity:.08}));
  shadow.position.z=-1.65;shadow.receiveShadow=true;scene.add(shadow);

  await document.fonts.ready;
  const loader=new THREE.TextureLoader();
  const screenshots={};
  await Promise.all(['transcript','word-card','game','playback','chat'].map(async name=>{
    const tex=await loader.loadAsync(`assets/screens/${name}.png`);tex.colorSpace=THREE.SRGBColorSpace;
    tex.anisotropy=Math.min(8,renderer.capabilities.getMaxAnisotropy());screenshots[name]=tex;
  }));
  const glyphs=await fetch('assets/phrase-glyphs.json').then(r=>r.json());

  function roundShape(w,h,r){
    const s=new THREE.Shape();const x=-w/2,y=-h/2;
    s.moveTo(x+r,y);s.lineTo(x+w-r,y);s.quadraticCurveTo(x+w,y,x+w,y+r);
    s.lineTo(x+w,y+h-r);s.quadraticCurveTo(x+w,y+h,x+w-r,y+h);
    s.lineTo(x+r,y+h);s.quadraticCurveTo(x,y+h,x,y+h-r);s.lineTo(x,y+r);s.quadraticCurveTo(x,y,x+r,y);return s;
  }
  function faceGeometry(w,h,r=.09){
    const geometry=new THREE.ShapeGeometry(roundShape(w,h,r),24);
    const p=geometry.attributes.position;const uv=new Float32Array(p.count*2);
    for(let i=0;i<p.count;i++){uv[i*2]=(p.getX(i)+w/2)/w;uv[i*2+1]=(p.getY(i)+h/2)/h;}
    geometry.setAttribute('uv',new THREE.BufferAttribute(uv,2));return geometry;
  }
  function slab(w,h,d,material){
    const m=new THREE.Mesh(new RoundedBoxGeometry(w,h,d,4,Math.min(.1,d/2)),material);m.castShadow=true;m.receiveShadow=true;return m;
  }
  function screen(name,box,width){
    const tex=screenshots[name].clone();tex.needsUpdate=true;
    const im=tex.image;const [x,y,w,h]=box;
    tex.repeat.set(w/im.width,h/im.height);tex.offset.set(x/im.width,1-(y+h)/im.height);
    const height=width*h/w;const g=new THREE.Group();
    g.add(slab(width+.06,height+.06,.12,dark));
    const m=new THREE.Mesh(faceGeometry(width,height),new THREE.MeshBasicMaterial({map:tex,toneMapped:false}));
    m.position.z=.068;g.add(m);g.userData.height=height;g.userData.width=width;scene.add(g);return g;
  }
  function word(text,width){
    const group=new THREE.Group();let advance=0;
    for(const char of text){
      const glyph=glyphs.glyphs[char];if(!glyph)continue;
      if(glyph.commands.length){
        const pen=new THREE.ShapePath();
        for(const c of glyph.commands){const a=c.args.map(v=>v/glyphs.units);
          if(c.command==='moveTo')pen.moveTo(...a);if(c.command==='lineTo')pen.lineTo(...a);
          if(c.command==='quadraticCurveTo')pen.quadraticCurveTo(...a);
          if(c.command==='bezierCurveTo')pen.bezierCurveTo(...a);
          if(c.command==='closePath')pen.currentPath.closePath();
        }
        const geo=new THREE.ExtrudeGeometry(pen.toShapes(),{depth:.06,bevelEnabled:true,bevelThickness:.008,bevelSize:.004,bevelSegments:2,curveSegments:12,steps:1});
        const mesh=new THREE.Mesh(geo,[ink,edge]);mesh.position.x=advance;mesh.castShadow=true;group.add(mesh);
      }
      advance+=glyph.advance/glyphs.units;
    }
    const bb=new THREE.Box3().setFromObject(group);const c=bb.getCenter(new THREE.Vector3());
    group.children.forEach(child=>child.position.sub(c));
    const size=bb.getSize(new THREE.Vector3());group.scale.setScalar(width/size.x);return group;
  }

  // The same physical caption carries both language faces through the first handoff.
  const phrase=new THREE.Group();scene.add(phrase);
  const carrierMaterial=paper.clone();carrierMaterial.transparent=true;
  const carrier=slab(4.65,2.7,.115,carrierMaterial);phrase.add(carrier);
  const front=new THREE.Group();front.position.z=.09;phrase.add(front);
  const fr1=word('Comment',4.05);fr1.position.y=.59;front.add(fr1);
  const fr2=word('tu t’appelles ?',4.13);fr2.position.y=-.59;front.add(fr2);
  const reverse=new THREE.Group();reverse.rotation.y=Math.PI;reverse.position.z=-.09;phrase.add(reverse);
  const en1=word('What’s',2.7);en1.position.y=.59;reverse.add(en1);
  const en2=word('your name?',4.05);en2.position.y=-.59;reverse.add(en2);
  const selection=new THREE.Group();selection.position.z=-.012;front.add(selection);
  const selectionBars=[.59,-.59].map(y=>{const m=slab(4.35,.95,.018,edge);m.position.y=y;selection.add(m);return m;});
  const handles=new THREE.Group();front.add(handles);
  for(const [x,y,dy]of[[-2.22,.92,.24],[2.22,-.92,-.24]]){
    const stem=new THREE.Mesh(new THREE.BoxGeometry(.026,.58,.026),edge);stem.position.set(x,y,0);handles.add(stem);
    const dot=new THREE.Mesh(new THREE.SphereGeometry(.095,20,16),edge);dot.position.set(x,y+dy,.005);handles.add(dot);
  }
  const reader=screen('transcript',[0,1005,1206,1550],4.45);
  const phraseHelp=screen('word-card',[45,1000,1117,1480],4.45);
  const example=screen('word-card',[92,1700,1022,560],4);
  const quiz=screen('game',[65,425,1076,1300],4.05);
  const underline=slab(2.65,.05,.08,edge);underline.castShadow=false;scene.add(underline);
  const objects=[phrase,reader,phraseHelp,example,quiz,underline];
  const clamp=x=>Math.max(0,Math.min(1,x));
  const out=x=>{x=clamp(x);return x===1?1:1-2**(-10*x);};
  const into=x=>{x=clamp(x);return x===0?0:2**(10*(x-1));};
  const smooth=x=>{x=clamp(x);return x*x*(3-2*x);};
  const mix=(a,b,t)=>a+(b-a)*t;
  const pose=(o,p,r=[0,0,0],s=1)=>{o.visible=true;o.position.set(...p);o.rotation.set(...r);o.scale.setScalar(s);};
  const paperColor=new THREE.Color('#F4F1EA'),blueColor=new THREE.Color('#3EA6FF');

  function renderAt(raw){
    const t=Math.max(0,Math.min(45,raw));
    for(const o of objects)o.visible=false;
    camera.position.set(0,0,17.5);camera.lookAt(0,0,0);
    scene.background.copy(paperColor);carrierMaterial.opacity=1;carrier.castShadow=true;carrier.receiveShadow=true;carrier.visible=true;handles.visible=false;selection.visible=false;
    front.visible=true;reverse.visible=true;
    if(t<6){
      const select=out((t-1.1)/1.2);const lift=smooth((t-4.2)/1.8);
      pose(phrase,[-.15,0,mix(0,.25,lift)],[mix(.055,-.1,lift),mix(-.13,.34,lift),mix(.015,0,lift)],1);
      carrierMaterial.opacity=smooth((t-2.5)/1.1);
      carrier.castShadow=carrierMaterial.opacity>.5;
      carrier.receiveShadow=carrier.castShadow;carrier.visible=carrierMaterial.opacity>.001;
      selection.visible=t>=1.1;
      for(let i=0;i<2;i++){const p=out((t-1.1-i*.2)/.7);selectionBars[i].scale.x=Math.max(.001,p);selectionBars[i].position.x=-4.35*(1-p)/2;}
      handles.visible=select>.5;handles.scale.setScalar(out((t-1.55)/.45));
      reverse.visible=false;
    }else if(t<12){
      const flip=smooth((t-6.25)/1.75);const leave=into((t-11.15)/.85);
      pose(phrase,[-.15,mix(0,.35,leave),.25],[mix(-.1,0,flip),mix(.34,Math.PI,flip),0],mix(1,.94,leave));
      selection.visible=true;handles.visible=flip<.4;handles.scale.setScalar(1-flip);
      for(const bar of selectionBars){bar.scale.x=1;bar.position.x=0;}
    }else if(t<20){
      const turn=smooth((t-12)/1.2);const leave=into((t-19.15)/.85);
      const position=[-.15,mix(.35,-.7,turn),mix(.25,0,turn)];
      if(t<12.6){pose(phrase,position,[0,Math.PI*(1-turn),0],.94);}
      if(t>=12.6){pose(reader,position,[mix(0,.015,turn),mix(Math.PI,-.025,turn),mix(0,-.015,turn)],.95);}
      reader.position.x+=leave*.22;reader.rotation.y-=leave*.25;
    }else if(t<27){
      const arrive=out((t-20)/1.1);const focus=smooth((t-22.25)/1.35);const leave=into((t-26.2)/.8);
      pose(phraseHelp,[mix(.07,-.15,arrive),-.7,0],[.01,mix(-.275,-.025,arrive),-.015],.94);
      phraseHelp.scale.setScalar(mix(.94,.66,focus));phraseHelp.position.y=mix(-.7,.05,focus);phraseHelp.position.z=-focus*.8;
      if(t>=22.25){pose(example,[-.15,mix(-.7,-1.1,focus),mix(.08,1.15,focus)],[mix(.16,.015,focus),mix(-.25,.025,focus),-.018],mix(.62,1,focus));}
      phraseHelp.rotation.x-=leave*.5;example.rotation.x-=leave*.5;
      phraseHelp.position.y-=leave*.8;example.position.y-=leave*.8;
    }else if(t<33){
      scene.background.copy(blueColor);
      const arrive=out((t-29.7)/1.15);const turn=smooth((t-31)/2);
      if(t>=29.7){pose(quiz,[mix(1.1,-.15,arrive),mix(-1.1,-.7,arrive),0],[mix(-.16,0,arrive),mix(-1.3,-.03,turn),-.015],mix(.8,1,arrive));}
    }else if(t<41){
      const settle=out((t-33)/1);const closer=smooth((t-35)/2.5);const exit=into((t-40.2)/.8);
      pose(quiz,[-.15,-.7,0],[0,mix(-.03,0,settle),mix(-.015,0,settle)],mix(1,1.035,closer));
      quiz.rotation.x=-exit*.5;quiz.scale.y*=mix(1,.02,exit);quiz.position.y=mix(-.7,.86,exit);
    }else{
      const settle=out((t-41)/.7);
      pose(underline,[mix(-.15,-.96,settle),.86,.15],[0,0,0],1);underline.scale.x=mix(1.7,1,settle);
    }
    renderer.render(scene,camera);
  }
  window.addEventListener('hf-seek',event=>renderAt(event.detail.time));
  window.__govueV2={renderAt,scene,renderer,camera};
  renderAt(window.__hfThreeTime||0);
}
