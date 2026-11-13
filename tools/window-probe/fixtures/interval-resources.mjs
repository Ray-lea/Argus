// Renders reports produced by the real history writer/reader on controlled synthetic inputs.
// Native commands are mocked. Process controls never execute in this profile.
import {readFileSync,writeFileSync} from 'node:fs';
const rankPath=process.argv[2]??'target/local-1-0/interval-resources-truth.json';
const gpuPath=process.argv[3]??'target/local-1-0/interval-gpu-truth.json';
const phasePath=process.argv[4]??'target/local-1-0/interval-truth.json';
const rawRanks=JSON.parse(readFileSync(rankPath,'utf8'));
const rawGpu=JSON.parse(readFileSync(gpuPath,'utf8'));
const rawPhases=JSON.parse(readFileSync(phasePath,'utf8'));
const base=Math.floor(Date.now()/5000)*5000-60000;
const timeKeys=new Set(['from','to','requestedFrom','requestedTo','peakTs','firstTs','lastTs','ts','frameTs','cpuPeakTs']);
function shifted(value,delta){
 if(Array.isArray(value))return value.map(item=>shifted(item,delta));
 if(value&&typeof value==='object')return Object.fromEntries(Object.entries(value).map(([key,item])=>[key,
  item===null?null:timeKeys.has(key)?item+delta:key==='createTime'?base-100000+item:shifted(item,delta)]));
 return value;
}
const ranks=shifted(rawRanks,base-rawRanks.cpu.from),gpu=shifted(rawGpu,base-rawGpu.zero.from);
const phases=shifted(rawPhases,base-rawPhases.first.from);
const fixture=JSON.parse(readFileSync(new URL('desktop.json',import.meta.url),'utf8'));
fixture.processWatch=null;
fixture.historySeries=ranks._series;
fixture.historyWindow={from:base-60000,to:base+60000};
fixture.historyMoment=ranks._moments.at(-1);
const answer=(report,generation)=>({resolve:{command:'history_interval',cache:'historyInterval',args:{from:report.from,to:report.to,metric:report.metric,generation},ok:true,value:report}});
const state=(report)=>({intervalStart:report.from,intervalEnd:report.to-5000});
const moment=(index)=>({resolve:{command:'history_moment',cache:'historyMoment',args:{cursor:gpu._moments[index].system.ts,ts:gpu._moments[index].system.ts},ok:true,value:gpu._moments[index]}});
const top={wheel:{x:300,y:250,lines:80}},down={wheel:{x:300,y:250,lines:-8}};
const steps=[
 {control:{page:'history',pointerOutside:true},wait:400},
 {control:{data:{...state(ranks.cpu),intervalGeneration:1,intervalMetric:'cpu'}},wait:250},
 {control:answer(ranks.cpu,1),wait:400,inspect:'layout',label:'cpu-leader',expect:{'interval.metric':'cpu','interval.value':20,'interval.leaderPids':[17,18]}},
 {control:down,wait:350,capture:'cpu-ranked'}
];
let generation=1;
for(const metric of ['memory','io','gpu','network']){
 generation++;
 const report=ranks[metric];
 steps.push(
  {control:{clickSelect:'interval-metric'},wait:200},
  {control:{keys:['down','enter']},wait:250,inspect:'layout',label:`choose-${metric}`,expect:{'interval.query.metric':metric}},
  {control:answer(report,generation),wait:400,inspect:'layout',label:`rank-${metric}`,expect:{'interval.metric':metric,'interval.leaderPids':[18,17],...(metric==='io'?{'interval.leaderReadout':'50 B/s'}:{})}},
  {capture:`${metric}-ranked`}
 );
}
// Switch to GPU and choose each saved one-frame interval. Every response matches actual query bounds.
for(const [index,name] of ['zero','unavailable','partial','legacy'].entries()){
 generation+=2;
 const report=gpu[name];
 const expected={
  zero:{'interval.value':0,'interval.readout':'0%','interval.coverage.validFrames':1},
  unavailable:{'interval.value':null,'interval.readout':'—','interval.coverage.unavailableFrames':1},
  partial:{'interval.value':12,'interval.readout':'≥12%','interval.coverage.partialFrames':1},
  legacy:{'interval.value':null,'interval.readout':'—','interval.coverage.legacyFrames':1,'interval.leaders':['old.exe']}
 }[name];
 steps.push(
  {control:{data:{...(index===0?{historySeries:gpu._series,historyMoment:gpu._moments.at(-1),historyMomentAt:gpu._moments.at(-1).system.ts}:index===3?{historySeries:gpu._series}:{})}},wait:150},
  {control:{keys:[index===0?'home':index===3?'end':'right']},wait:200},
  {control:moment(index),wait:200},
  {control:{wheel:{x:300,y:250,lines:-80}},wait:300},
  {control:{clickAnchor:'interval-start'},wait:200},
  {control:{clickAnchor:'interval-end'},wait:250},
  {control:top,wait:200},{control:down,wait:300}
 );
 if(index===0){
  generation++;
  steps.push({control:{clickSelect:'interval-metric'},wait:200},{control:{keys:['up','enter']},wait:250});
 }
 steps.push(
  {inspect:'layout',label:`query-${name}`,expect:{'interval.query.metric':'gpu','interval.query.from':report.from,'interval.query.to':report.to}},
  {control:answer(report,generation),wait:400,inspect:'layout',label:`gpu-${name}`,expect:expected},
  {capture:`gpu-${name}`}
 );
 if(name==='partial'){
  // Layout-only label variation; numeric and identity fields remain the exported controlled data.
  const long=structuredClone(report);
  long.leaders[0].name='controlled_application_'.repeat(8)+'.exe';
  steps.push({control:answer(long,generation),wait:300,capture:'long-process-name'},{control:answer(report,generation),wait:200});
  steps.push(
  {control:{data:{historySeries:gpu._coarseSeries}},wait:150},
  {control:{clickAnchor:'interval-peak'},wait:250,inspect:'layout',label:'partial-peak-frame',expect:{'history.requestedFrame':report.gpu.peakTs}}
  );
 }
}
steps.push(
 {control:answer(ranks.network,5),wait:250,inspect:'layout',label:'stale-network-rejected',expect:{'interval.metric':'gpu','interval.leaders':['old.exe'],'interval.coverage.legacyFrames':1}},
 {control:{contentZoom:1.5,theme:'light'},probe:'size 900 700',wait:600},
 {control:top,wait:200},{control:{wheel:{x:300,y:250,lines:-18}},wait:500,capture:'legacy-small-150'},
 {control:{wheel:{x:300,y:250,lines:6}},wait:350,capture:'legacy-small-150-upper'},
 {control:{contentZoom:1,theme:'dark'},probe:'size 1200 1000',wait:400},
 {control:{data:{historySeries:phases._series,historyMoment:phases._moments.at(-1),historyMomentAt:phases._moments.at(-1).system.ts}},wait:200}
);
const phaseMoment=index=>({resolve:{command:'history_moment',cache:'historyMoment',args:{cursor:phases._moments[index].system.ts,ts:phases._moments[index].system.ts},ok:true,value:phases._moments[index]}});
for(const [index,report] of [phases.ioFirst,phases.ioSecond].entries()){
 generation+=2;
 steps.push(
  {control:{keys:[index===0?'home':'right']},wait:200},{control:phaseMoment(index*2),wait:200},
  {control:{wheel:{x:300,y:250,lines:-80}},wait:250},{control:{clickAnchor:'interval-start'},wait:200},
  {control:{keys:['right']},wait:200},{control:phaseMoment(index*2+1),wait:200},
  {control:{wheel:{x:300,y:250,lines:-80}},wait:250},{control:{clickAnchor:'interval-end'},wait:200},
  {control:top,wait:200},{control:down,wait:300}
 );
 if(index===0){generation++;steps.push({control:{clickSelect:'interval-metric'},wait:200},{control:{keys:['up','enter']},wait:250});}
 steps.push(
  {control:answer(report,generation),wait:350,inspect:'layout',label:index===0?'compile-io-only':'copy-io-only',expect:{'interval.metric':'io','interval.value':index===0?0:700,'interval.leaders':[index===0?'compile.exe':'copy.exe']}},
  {capture:index===0?'compile-io':'copy-io'}
 );
}
steps.push(
 {control:{data:{historySeries:phases._coarseSeries}},wait:150},
 {control:{clickAnchor:'interval-peak'},wait:250,inspect:'layout',label:'disk-peak-not-cpu-peak',expect:{'history.requestedFrame':phases.ioSecond.disk.peakTs}},
 {control:{clickAnchor:'interval-clear'},wait:250,inspect:'layout',label:'cleared',expect:{'interval.start':null,'interval.mean':null}}
);
writeFileSync('target/interval-resources.json',JSON.stringify(fixture));
writeFileSync('target/interval-resources-steps.json',JSON.stringify({window:[1200,1000],source:[rankPath,gpuPath,phasePath],steps},null,2));
