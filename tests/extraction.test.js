import { test } from 'node:test';
import assert from 'node:assert/strict';
import { build } from 'esbuild';

async function harness(core) {
  const result = await build({entryPoints:['src/core/extraction.ts'],bundle:true,write:false,platform:'node',format:'cjs',packages:'external',
    plugins:[{name:'native-extraction-edge',setup(builder){builder.onResolve({filter:/modules\/lingoplay-core$/},()=>({path:'native-edge',external:true}));}}]});
  const module={exports:{}};
  Function('require','module','exports',result.outputFiles[0].text)(() => ({coreRequest:core,getBrowserUserAgent:async()=> 'fixture browser'}),module,module.exports);
  return module.exports;
}

test('standalone extraction correlates token tasks and releases native/runtime state after success', async () => {
  const calls=[],released=[],evaluated=[];
  let polls=0;
  const api=await harness(async command=>{
    calls.push(command);
    if(command.op==='extract_poll')return ++polls===1?{done:false,task:{taskId:7,stage:'snapshot',program:'untrusted program'}}:{done:true,value:{videoId:'abcdefghijk',segments:[]}};
    return null;
  });
  const unregister=api.registerTokenRuntime({evaluate:async(id,task)=>{evaluated.push({id,task});return 'snapshot';},release:id=>released.push(id)});
  const result=await api.extract('transcript','abcdefghijk',{trackId:'.en'});
  assert.equal(result.videoId,'abcdefghijk');
  const start=calls.find(c=>c.op==='extract_start');
  assert.equal(start.userAgent,'fixture browser');
  const reply=calls.find(c=>c.op==='extract_token');
  assert.equal(reply.requestId,start.requestId);
  assert.equal(reply.taskId,7);
  assert.equal(evaluated[0].id,start.requestId);
  assert.deepEqual(released,[start.requestId]);
  assert.ok(calls.some(c=>c.op==='extract_release'&&c.requestId===start.requestId));
  unregister();
});

test('navigation cancels the Rust job and discards a late isolated-runtime response', async () => {
  const calls=[];
  let resolveTask,entered;
  const started=new Promise(resolve=>{entered=resolve;});
  const api=await harness(async command=>{
    calls.push(command);
    if(command.op==='extract_poll')return {done:false,task:{taskId:1,stage:'mint'}};
    return null;
  });
  const unregister=api.registerTokenRuntime({evaluate:()=>{entered();return new Promise(resolve=>{resolveTask=resolve;});},release:()=>{}});
  const controller=new AbortController();
  const pending=api.extract('transcript','abcdefghijk',{signal:controller.signal});
  const rejected=assert.rejects(pending,{name:'AbortError'});
  await started;
  controller.abort();
  assert.ok(calls.some(c=>c.op==='extract_release'));
  resolveTask('late token');
  await rejected;
  assert.equal(calls.filter(c=>c.op==='extract_token').length,0);
  unregister();
});
