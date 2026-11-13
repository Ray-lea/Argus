// Numeric reports come from the real bounded watch/history writers and readers.
// Native commands are mocked; no process actions run in this profile.
import {readFileSync,writeFileSync} from 'node:fs';
const source=process.argv[2]??'target/local-1-0/observation-interval-truth.json';
const raw=JSON.parse(readFileSync(source,'utf8'));
const base=Math.floor(Date.now()/5000)*5000-60000;
const delta=base-raw.report.from;
const times=new Set(['from','to','requestedFrom','requestedTo','ts','frameTs','cpuPeakTs','peakTs','firstTs','lastTs','firstSavedTs','lastSavedTs']);
function shift(value){
 if(Array.isArray(value))return value.map(shift);
 if(value&&typeof value==='object')return Object.fromEntries(Object.entries(value).map(([key,item])=>[key,
  item===null?null:times.has(key)?item+delta:key==='createTime'?base-100000+item*10:shift(item)]));
 return value;
}
const truth=shift(raw),report=truth.report;
const fixture=JSON.parse(readFileSync(new URL('desktop.json',import.meta.url),'utf8'));
fixture.historySeries=truth._series;
fixture.historyMoment=truth._moments.at(-1);
fixture.historyWindow={from:base-60000,to:base+60000};
fixture.processWatch=null;
const answer=(value,generation)=>({resolve:{command:'history_interval',cache:'historyInterval',args:{from:value.from,to:value.to,metric:'memory',generation},ok:true,value}});
const steps=[
 {control:{page:'history',pointerOutside:true},wait:400},
 {control:{data:{intervalStart:report.from,intervalEnd:report.to-5000,intervalGeneration:1,intervalMetric:'memory'}},wait:200},
 {control:answer(report,1),wait:350,inspect:'layout',label:'watch-outside-top',expect:{
  'interval.leaderPids':[88], 'interval.observations.runsSeen':2,
  'interval.observations.runs.0.pid':17,'interval.observations.runs.0.memory.mean':10,
  'interval.observations.runs.0.commit.mean':20,'interval.observations.runs.0.gapEvents':1,
  'interval.observations.runs.0.terminalEvents':1,'interval.observations.runs.1.memory.mean':100}},
 {control:{wheel:{x:300,y:250,lines:-80}},wait:400,capture:'saved-watch-runs'},
 {control:{contentZoom:1.5,theme:'light'},probe:'size 900 700',wait:500},
 {control:{wheel:{x:300,y:250,lines:-80}},wait:350,capture:'watch-small-150-bottom'},
 {control:{wheel:{x:300,y:250,lines:18}},wait:350,capture:'watch-small-150-upper'},
 {control:{contentZoom:1,theme:'dark'},probe:'size 1200 1000',wait:350},
 // A successful mutation invalidates the interval generation and rejects the old snapshot.
 {control:{resolve:{command:'watch_process',args:{action:{op:'delete'},generation:0},ok:true,value:null}},wait:250,
  inspect:'layout',label:'delete-invalidates-snapshot',expect:{'interval.observations':null,'interval.query.generation':2}},
 {control:answer(report,1),wait:250,inspect:'layout',label:'stale-observation-rejected',expect:{'interval.observations':null}},
 {control:answer(truth.cleared,2),wait:300,inspect:'layout',label:'deleted-watch-not-in-report',expect:{
  'interval.leaderPids':[88],'interval.observations.sourceFiles':0,'interval.observations.runs':[]}},
 {control:{wheel:{x:300,y:250,lines:-80}},wait:300,capture:'after-watch-delete'}
];
writeFileSync('target/observation-interval.json',JSON.stringify(fixture));
writeFileSync('target/observation-interval-steps.json',JSON.stringify({window:[1200,1000],source,steps},null,2));
