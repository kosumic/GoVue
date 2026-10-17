import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import React, { createElement, forwardRef, useImperativeHandle, useRef } from 'react';
import { act, create } from 'react-test-renderer';
import { build } from 'esbuild';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const require = createRequire(import.meta.url);

test('token renderer isolates challenges and rejects interrupted, stale and cancelled work', async t => {
  let engine;
  const WebView = forwardRef((props,ref) => {
    const handle=useRef({injections:[],injectJavaScript(code){this.injections.push(code);}});
    useImperativeHandle(ref,()=>handle.current);
    return createElement('WebView',{...props,handle:handle.current});
  });
  const edges={
    'react-native':{Platform:{OS:'android'},View:'View',StyleSheet:{create:value=>value}},
    'react-native-webview':WebView,
    'extraction-edge':{registerTokenRuntime:value=>{engine=value;return()=>{engine=null;};}},
  };
  const result=await build({entryPoints:['src/media/TokenRuntime.tsx'],bundle:true,write:false,platform:'node',format:'cjs',jsx:'automatic',packages:'external',
    plugins:[{name:'native-token-edge',setup(builder){builder.onResolve({filter:/core\/extraction$/},()=>({path:'extraction-edge',external:true}));}}]});
  const module={exports:{}};
  Function('require','module','exports',result.outputFiles[0].text)(name=>name in edges?edges[name]:require(name),module,module.exports);
  let root;
  await act(()=>{root=create(createElement(module.exports.TokenRuntime));});
  t.after(async()=>{await act(()=>root.unmount());});
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
