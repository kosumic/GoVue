import { spawn, execFile, execFileSync } from 'node:child_process';
import { writeFileSync, appendFileSync, mkdirSync, readFileSync, existsSync, renameSync } from 'node:fs';
import { resolve } from 'node:path';
import { promisify } from 'node:util';
const execAsync=promisify(execFile);
const [name,pkg,video,mode='cold',durationArg='20']=process.argv.slice(2);
if(!/^[a-z0-9-]+$/.test(name??'')||!['io.github.kaminarios.govue','com.google.android.youtube','com.brave.browser'].includes(pkg)||!/^[\w-]{11}$/.test(video??'')||!['cold','warm'].includes(mode)) throw Error('Invalid public benchmark case');
const offset=Number(process.env.GOVUE_START??0);if(!Number.isInteger(offset)||offset<0||offset>36000)throw Error('Invalid start offset');
if(process.env.GOVUE_VARIANT&&!/^[a-z0-9-]+$/.test(process.env.GOVUE_VARIANT))throw Error('Invalid variant.');
if(process.env.GOVUE_EXPECT_CACHE&&!['hit','miss','any'].includes(process.env.GOVUE_EXPECT_CACHE))throw Error('Invalid expected cache state.');
if(process.env.GOVUE_EXPECT_POSITION!==undefined&&(!Number.isFinite(Number(process.env.GOVUE_EXPECT_POSITION))||Number(process.env.GOVUE_EXPECT_POSITION)<0||Number(process.env.GOVUE_EXPECT_POSITION)>36000))throw Error('Invalid expected position.');
const duration=Number(durationArg);if(!Number.isInteger(duration)||duration<10||duration>60)throw Error('Invalid trace duration');
const dir=resolve(process.env.GOVUE_RESULTS_DIR ?? '.cache/startup-investigation/ablation');mkdirSync(dir,{recursive:true});
const adb=process.env.ADB ?? 'adb';
if(!process.env.GOVUE_DEVICE_SERIAL)throw Error('Set GOVUE_DEVICE_SERIAL.');
const serial=['-s',process.env.GOVUE_DEVICE_SERIAL];
if(process.env.GOVUE_REANALYZE!=='1'&&existsSync(resolve(dir,name+'.json')))throw Error('Case already exists.');
const run=(...a)=>execFileSync(adb,[...serial,...a],{encoding:'utf8',timeout:30000,maxBuffer:4e6});
const deviceCondition=()=>{const battery=run('shell','dumpsys','battery'),thermal=run('shell','dumpsys','thermalservice');return{temperatureTenthsC:Number(battery.match(/temperature:\s*([0-9]+)/)?.[1])||null,thermalStatus:Number(thermal.match(/Thermal Status:\s*([0-9]+)/i)?.[1]??-1)}};
const trialStartedAt=new Date().toISOString();
const beforeCondition=process.env.GOVUE_REANALYZE==='1'?null:deviceCondition();
const appInForeground=()=>new RegExp('(?:mResumedActivity|topResumedActivity)[^\\n]*'+pkg.replaceAll('.','\\.')).test(run('shell','dumpsys','activity','activities'));
let startMs;
if(process.env.GOVUE_REANALYZE==='1')startMs=Number(readFileSync(resolve(dir,name+'-launch.txt'),'utf8').split(' ')[0])*1000;
else {
if(mode==='cold')run('shell','am','force-stop',pkg);
const remote=`/data/misc/perfetto-traces/govue-ablation-${name}.pftrace`;
const recorder=spawn(adb,[...serial,'shell','perfetto','--txt','-c','-','-o',remote]);let log='';
const exited=new Promise((resolve,reject)=>{recorder.once('error',reject);recorder.once('exit',code=>code===0?resolve():reject(Error(`Recorder exited ${code}`)));});
const ready=new Promise((resolve,reject)=>{recorder.stderr.on('data',b=>{log+=b;if(log.includes('Connected to the Perfetto'))resolve();});recorder.once('error',reject);recorder.once('exit',()=>reject(Error('Recorder ended before ready')));});
recorder.stdin.end(`buffers { size_kb: 32768 fill_policy: RING_BUFFER }\nduration_ms: ${duration*1000}\ndata_sources { config { name: "linux.ftrace" ftrace_config { atrace_categories: "am" atrace_categories: "video" atrace_apps: "${pkg}" } } }\ndata_sources { config { name: "linux.process_stats" process_stats_config { scan_all_processes_on_start: true } } }`);
await ready;
const captureTimes=[];const captureAt=performance.now();
const captureTask=process.env.GOVUE_CAPTURE==='1'?(async()=>{for(let i=0;performance.now()-captureAt<9000;i++){const begin=performance.now()-captureAt;const {stdout}=await execAsync(adb,[...serial,'exec-out','screencap','-p'],{encoding:'buffer',maxBuffer:12e6});writeFileSync(`${dir}/${name}-screen-${i}.png`,stdout);captureTimes.push({index:i,beginMs:begin,endMs:performance.now()-captureAt});await new Promise(r=>setTimeout(r,160));}writeFileSync(`${dir}/${name}-screens.json`,JSON.stringify(captureTimes));})():Promise.resolve();
const tap=process.env.GOVUE_TAP?.split(',').map(Number);if(tap&&!(tap.length===2&&tap.every(Number.isInteger)&&tap[0]>=0&&tap[0]<960&&tap[1]>=0&&tap[1]<2142))throw Error('Invalid tap point');
const launchText=run('shell',tap?`cat /proc/uptime; input tap ${tap[0]} ${tap[1]}`:`cat /proc/uptime; am start -W -a android.intent.action.VIEW -d 'https://www.youtube.com/watch?v=${video}${offset?'&t='+offset+'s':''}' -p ${pkg}`);
startMs=Number(launchText.split(' ')[0])*1000;writeFileSync(`${dir}/${name}-launch.txt`,launchText);
console.log(JSON.stringify({event:'opened',name,pkg,video,mode,startMs}));
await exited;await captureTask;
run('pull',remote,`${dir}/${name}.pftrace`);run('shell','rm',remote);
}
let confirmedPids;
const pidFile=resolve(dir,name+'-pids.json');
if(process.env.GOVUE_REANALYZE==='1')confirmedPids=existsSync(pidFile)?JSON.parse(readFileSync(pidFile,'utf8')):[];
else{confirmedPids=run('shell','ps','-A','-o','PID,NAME').split('\n').map(l=>l.trim().match(/^(\d+)\s+(\S+)$/)).filter(m=>m&&m[2].startsWith(pkg)).map(m=>Number(m[1]));writeFileSync(pidFile,JSON.stringify(confirmedPids));}
const processFilter=`(p.name like '${pkg}%'${confirmedPids.length?' or p.pid in ('+confirmedPids.join(',')+')':''})`;
const base=`from slice s left join thread_track tt on tt.id=s.track_id left join thread t using(utid) left join process p using(upid)`;
const where=`s.ts>=${startMs}*1000000`;
const sql=`with frames as(select s.name,min(s.ts) as first ${base} where s.name glob 'CCodecBufferChannel::renderOutputBuffer*' and ${processFilter} and ${where} group by s.name), configuration as(select min(s.ts) as first ${base} where s.name='MediaCodec::configure#native' and ${processFilter} and ${where}) select 'new_decoder_first_frame',frames.name,round(frames.first/1e6-${startMs},3),0 from frames,configuration where frames.first>=configuration.first order by frames.first limit 4; select 'phase',s.name,round(s.ts/1e6-${startMs},3),round(s.dur/1e6,3) ${base} where (s.name glob 'GoVue extraction:*' or s.name glob 'GoVue playback:*') and ${where} order by s.ts; select 'network',s.name,round(s.ts/1e6-${startMs},3),round(s.dur/1e6,3) ${base} where s.name glob 'GoVue transport:*' and ${where} order by s.ts;`;
const output=execFileSync(resolve(process.env.GOVUE_TRACE_PROCESSOR ?? '.cache/trace_processor_shell'),['-Q',sql,`${dir}/${name}.pftrace`],{encoding:'utf8',maxBuffer:4e6,stdio:['ignore','pipe','ignore']});writeFileSync(`${dir}/${name}-timing.csv`,output);
const parsed=output.split('\n').map(line=>line.match(/^"(phase|network|new_decoder_first_frame)","([^"]+)",(-?[0-9.]+),(-?[0-9.]+)$/)).filter(Boolean).map(m=>({kind:m[1],name:m[2],atMs:Number(m[3]),durationMs:Number(m[4])}));
const first=parsed.find(r=>pkg==='io.github.kaminarios.govue'?r.name==='GoVue playback: first frame':r.kind==='new_decoder_first_frame');
const phases=parsed.filter(r=>r.kind==='phase');
const prepareLimit=first?.atMs??phases.find(r=>/^GoVue playback: format [0-9]+x/.test(r.name)||r.name==='GoVue playback: state 2')?.atMs;
const prepare=prepareLimit===undefined?undefined:phases.filter(r=>r.name==='GoVue playback: prepare'&&r.atMs<prepareLimit).at(-1);
const mediaSpans=phases.filter(r=>r.name==='GoVue extraction: media');
const extraction=mediaSpans.filter(r=>!prepare||r.atMs+r.durationMs<=prepare.atMs).at(-1)??mediaSpans[0];
const prefix=phases.find(r=>r.name==='GoVue extraction: configuration prefix'&&extraction&&r.atMs>=extraction.atMs&&r.atMs<extraction.atMs+extraction.durationMs);
const network=parsed.filter(r=>r.kind==='network'&&r.atMs>=(prepare?.atMs??0)&&r.atMs<(first?.atMs??Infinity));
const result={name,pkg,video,mode,startSeconds:offset,firstFrameMs:first?.atMs??null,traceWindowSeconds:duration,extractStartMs:extraction?.atMs??null,extractionMs:extraction?.durationMs??null,prefixMs:prefix?.durationMs??null,nativePrepareMs:prepare?.atMs??null,nativeToFirstFrameMs:first&&prepare?Number((first.atMs-prepare.atMs).toFixed(3)):null,upstreamRequests:network.filter(r=>r.name.endsWith('headers')).length,httpStatuses:network.filter(r=>/^GoVue transport: HTTP [0-9]+$/.test(r.name)).map(r=>r.name.split(' ').at(-1)),phases};
const previous = process.env.GOVUE_REANALYZE === '1' ? JSON.parse(readFileSync(resolve(dir, name + '.json'), 'utf8')) : null;
result.startUptimeMs = startMs;
result.trialStartedAt = previous?.trialStartedAt ?? trialStartedAt;
result.deviceBefore = previous?.deviceBefore ?? beforeCondition;
result.deviceAfter = previous?.deviceAfter ?? (previous ? null : deviceCondition());
result.foregroundAtEnd = previous?.foregroundAtEnd ?? (previous ? null : appInForeground());
result.protocolVersion = Number(process.env.GOVUE_PROTOCOL_VERSION ?? previous?.protocolVersion ?? 1);
result.variant = process.env.GOVUE_VARIANT ?? previous?.variant ?? pkg;
result.block = process.env.GOVUE_BLOCK ?? previous?.block ?? null;
result.role = process.env.GOVUE_TRIAL_ROLE ?? previous?.role ?? 'measurement';
result.ablationEligible = previous?.ablationEligible ?? true;
result.ablationExclusionReason = previous?.ablationExclusionReason ?? null;
result.requiredCachedTarget = process.env.GOVUE_REQUIRE_CACHED === undefined ? previous?.requiredCachedTarget ?? false : process.env.GOVUE_REQUIRE_CACHED === '1';
result.expectedCachedExtraction = process.env.GOVUE_EXPECT_CACHE ?? previous?.expectedCachedExtraction ?? (result.requiredCachedTarget ? 'hit' : 'any');
result.firstPlayingMs = phases.find(r => r.name === 'GoVue playback: playing true' && r.atMs >= (prepare?.atMs ?? 0))?.atMs ?? null;
const playingEvents = phases.filter(r => /^GoVue playback: playing (true|false)$/.test(r.name) && r.atMs >= (prepare?.atMs ?? 0));
result.firstFiveSecondsPlayingMs = playingEvents.find((r, i) => r.name.endsWith('true') &&
  (playingEvents.slice(i + 1).find(next => next.name.endsWith('false'))?.atMs ?? duration * 1000) - r.atMs >= 5000)?.atMs ?? null;
const position = phases.find(r => /^GoVue playback: first position [0-9]+$/.test(r.name) && r.atMs >= (prepare?.atMs ?? 0));
result.firstPositionMs = position ? Number(position.name.split(' ').at(-1)) : null;
const height = phases.find(r => /^GoVue playback: first height [0-9]+$/.test(r.name) && r.atMs >= (prepare?.atMs ?? 0));
result.firstHeight = height ? Number(height.name.split(' ').at(-1)) : null;
result.formats = phases.filter(r => r.atMs >= (prepare?.atMs ?? 0)).flatMap(r => {
  const m = r.name.match(/^GoVue playback: format ([0-9]+)x([0-9]+) ([0-9]+)fps (avc|vp9|av1|hevc|other)$/);
  return m ? [{ atMs: r.atMs, width: Number(m[1]), height: Number(m[2]), fps: Number(m[3]), codec: m[4] }] : [];
});
result.firstFormat = result.formats.filter(r => r.atMs <= (first?.atMs ?? 0)).at(-1) ?? null;
result.qualityTenSecondsObserved = !!first && result.formats.length > 0 && first.atMs + 10000 < duration * 1000 - 1000;
result.formatAtTenSeconds = result.qualityTenSecondsObserved ? result.formats.filter(r => r.atMs <= first.atMs + 10000).at(-1) ?? null : null;
result.first1920InputMs = result.formats.find(r => r.width >= 1920)?.atMs ?? null;
const extractionEnd = extraction ? extraction.atMs + extraction.durationMs : 0;
result.cachedExtraction = !!prepare && !!first && !!extraction && !phases.some(r => r.atMs >= extraction.atMs && r.atMs < extractionEnd && /GoVue extraction: (GET|POST) headers/.test(r.name));
result.mediaProtocols = [...new Set(network.filter(r => /^GoVue transport: protocol /.test(r.name)).map(r => r.name.split(' ').at(-1)))];
result.extractionProtocols = [...new Set(phases.filter(r => /^GoVue extraction: protocol /.test(r.name)).map(r => r.name.split(' ').at(-1)))];
let bufferingAt = null;
result.completePostStartBufferMs = [];
for (const row of phases.filter(r => result.firstPlayingMs !== null && r.atMs > result.firstPlayingMs)) {
  if (row.name === 'GoVue playback: state 2') bufferingAt = row.atMs;
  else if (row.name === 'GoVue playback: state 3' && bufferingAt !== null) {
    result.completePostStartBufferMs.push(Number((row.atMs - bufferingAt).toFixed(3))); bufferingAt = null;
  }
}
result.unfinishedPostStartBuffer = bufferingAt !== null;
result.installedApkSha256 = previous?.installedApkSha256 ?? null;
if (!previous) {
  const address = run('shell', 'pm', 'path', pkg).split('\n').find(s => s.endsWith('/base.apk'))?.slice('package:'.length);
  if (address && /^\/data\/app\/[a-zA-Z0-9_./+=~-]+\/base\.apk$/.test(address))
    result.installedApkSha256 = run('shell', 'sha256sum', address).split(/\s+/)[0];
}
const buildFile = resolve('.cache/startup-investigation/ablation/build/apks', result.variant + '.json');
const expectedBuild = pkg === 'io.github.kaminarios.govue' && existsSync(buildFile) ? JSON.parse(readFileSync(buildFile, 'utf8')) : null;
result.expectedApkSha256 = expectedBuild?.apkSha256 ?? null;
result.expectedPositionMs = process.env.GOVUE_EXPECT_POSITION === undefined ? previous?.expectedPositionMs ?? null : Number(process.env.GOVUE_EXPECT_POSITION) * 1000;
result.valid = result.firstFrameMs !== null;
result.exclusionReason = result.valid ? null : 'No native first frame within the recording window.';
if (expectedBuild && expectedBuild.apkSha256 !== result.installedApkSha256) { result.valid = false; result.exclusionReason = 'Installed APK does not match the declared variant.'; }
if (result.firstFrameMs !== null && pkg === 'io.github.kaminarios.govue' && (result.firstPositionMs === null || result.firstPlayingMs === null)) {
  result.valid = false; result.exclusionReason = 'Missing source-correlated position or playing marker.';
}
if (result.firstFrameMs !== null && result.expectedPositionMs !== null && (result.firstPositionMs === null || Math.abs(result.firstPositionMs - result.expectedPositionMs) > 1500)) {
  result.valid = false; result.exclusionReason = 'First native position does not match the controlled resume offset.';
}
if (result.foregroundAtEnd === false) { result.valid = false; result.exclusionReason = 'The measured app was not in the foreground at the end of the trial.'; }

if(result.valid&&((result.expectedCachedExtraction==='hit'&&!result.cachedExtraction)||(result.expectedCachedExtraction==='miss'&&result.cachedExtraction))){result.valid=false;result.exclusionReason='Required extraction cache condition was not met.';}
result.measurementOutcome = result.valid ? 'ok' : result.firstFrameMs === null && result.foregroundAtEnd === true &&
  (!result.expectedApkSha256 || result.expectedApkSha256 === result.installedApkSha256) ? 'startup-failure' : 'excluded';

writeFileSync(`${dir}/${name}.json.tmp`,JSON.stringify(result,null,2));renameSync(`${dir}/${name}.json.tmp`,`${dir}/${name}.json`);appendFileSync(`${dir}/results.jsonl`,JSON.stringify(result)+'\n');console.log(JSON.stringify({event:'result',...result}));
