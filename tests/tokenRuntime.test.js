import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import React, { createElement, forwardRef, useImperativeHandle, useRef } from 'react';
import { act, create } from 'react-test-renderer';
import { build } from 'esbuild';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const require = createRequire(import.meta.url);

async function harness(t, os = 'android') {
  let engine;
  const WebView = forwardRef((props,ref) => {
    const handle=useRef({injections:[],injectJavaScript(code){this.injections.push(code);}});
    useImperativeHandle(ref,()=>handle.current);
    return createElement('WebView',{...props,handle:handle.current});
  });
  const edges={
    'react-native':{Platform:{OS:os},AppState:{addEventListener:()=>({remove(){}})},View:'View',StyleSheet:{create:value=>value}},
    'core-edge':{coreRequest:async()=>({requests:[],tasks:[]}),observeCoreUpdates:()=>({remove(){}})},
    'react-native-webview':WebView,
    'extraction-edge':{registerTokenRuntime:value=>{engine=value;return()=>{engine=null;};}},
  };
  const result=await build({entryPoints:['src/media/TokenRuntime.tsx'],bundle:true,write:false,platform:'node',format:'cjs',jsx:'automatic',packages:'external',
    plugins:[{name:'native-token-edge',setup(builder){builder.onResolve({filter:/modules\/govue-core$/},()=>({path:'core-edge',external:true}));builder.onResolve({filter:/core\/extraction$/},()=>({path:'extraction-edge',external:true}));}}]});
  const module={exports:{}};
  Function('require','module','exports',result.outputFiles[0].text)(name=>name in edges?edges[name]:require(name),module,module.exports);
  let root;
  await act(()=>{root=create(createElement(module.exports.TokenRuntime));});
  t.after(async()=>{await act(()=>root.unmount());});
  return {root, engine};
}

test('token renderer isolates challenges and rejects interrupted, stale and cancelled work', async t => {
  const {root,engine}=await harness(t);
  const old=root.root.findByType('WebView').props;
  assert.match(old.source.html,/connect-src 'none'/);
  assert.match(old.source.html,/default-src 'none'/);
  assert.match(old.source.html,/Document.prototype,'cookie'/);
  assert.equal(old.sharedCookiesEnabled,false);
  assert.equal(old.onShouldStartLoadWithRequest({url:'https://www.youtube.com/watch?v=abcdefghijk'}),false);
  let interrupted;
  await act(()=>{interrupted=engine.evaluate('old',{taskId:1,stage:'snapshot'});});
  assert.equal(old.handle.injections.length,0,'wait for the local runtime handshake');
  const rejected=assert.rejects(interrupted,/runtime stopped/);
  await act(()=>old.onRenderProcessGone());
  await rejected;
  const fresh=root.root.findByType('WebView').props;
  assert.notEqual(fresh.handle,old.handle);
  let pending;
  await act(()=>{pending=engine.evaluate('current',{taskId:2,stage:'mint'});});
  const send=(props,value)=>props.onMessage({nativeEvent:{data:JSON.stringify(value)}});
  await act(()=>send(old,{ready:true}));
  assert.equal(fresh.handle.injections.length,0,'old renderer cannot release new work');
  await act(()=>send(fresh,{ready:true}));
  assert.equal(fresh.handle.injections.length,1);
  await act(()=>send(old,{requestId:'current',taskId:2,value:'stale'}));
  await act(()=>send(fresh,{requestId:'current',taskId:2,value:'proof'}));
  assert.equal(await pending,'proof');
  let cancelled;
  await act(()=>{cancelled=engine.evaluate('cancelled',{taskId:3,stage:'snapshot'});});
  const cancellation=assert.rejects(cancelled,{name:'AbortError'});
  await act(()=>engine.release('cancelled'));
  await cancellation;
  await act(()=>send(fresh,{requestId:'cancelled',taskId:3,value:'late'}));
});

for (const os of ['android', 'ios']) test(`${os} token renderer permits only its local document and blocks later HTTPS navigation`, async t => {
  const {root}=await harness(t,os);
  const initial={url:'https://www.youtube.com/',navigationType:'other',isTopFrame:true};
  const view=()=>root.root.findByType('WebView').props;
  for(const request of [
    {...initial,isTopFrame:false},
    {...initial,navigationType:'click'},
    {...initial,url:'https://www.youtube.com/watch?v=abcdefghijk'},
    {...initial,url:'https://www.youtube.com/?proof=private'},
    {...initial,url:'https://www.youtube.com.evil.test/'},
    {...initial,url:'http://www.youtube.com/'},
  ]) assert.equal(view().onShouldStartLoadWithRequest(request),false);
  assert.equal(view().onShouldStartLoadWithRequest({url:'about:blank'}),true);
  assert.equal(view().onShouldStartLoadWithRequest(initial),os==='ios','WebKit reports the base URL for its local HTML load');
  assert.equal(view().onShouldStartLoadWithRequest(initial),false,'the base origin is not a network-navigation exception');

  await act(()=>view().onContentProcessDidTerminate());
  assert.equal(view().onShouldStartLoadWithRequest(initial),os==='ios','a replacement document gets its own initial load');
  await act(()=>view().onMessage({nativeEvent:{data:JSON.stringify({ready:true})}}));
  assert.equal(view().onShouldStartLoadWithRequest(initial),false);

  await act(()=>view().onContentProcessDidTerminate());
  await act(()=>view().onMessage({nativeEvent:{data:JSON.stringify({ready:true})}}));
  assert.equal(view().onShouldStartLoadWithRequest(initial),false,'a ready document cannot claim another initial load');
});
