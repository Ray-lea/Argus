// Uses CPU reports emitted by the core's adjacent-phase test; shifts only the clock origin.
import {readFileSync,writeFileSync} from 'node:fs';
const truth=JSON.parse(readFileSync(process.argv[2]??'target/local-1-0/interval-truth.json','utf8'));
const fixture=JSON.parse(readFileSync(new URL('desktop.json',import.meta.url),'utf8'));
const base=Math.floor(Date.now()/5000)*5000-60000;
const delta=base-truth.first.from;
const timeKeys=new Set(['from','to','requestedFrom','requestedTo','peakTs','firstTs','lastTs','ts','frameTs','cpuPeakTs']);
function shift(value){
 if(Array.isArray(value))return value.map(shift);
 if(value&&typeof value==='object')return Object.fromEntries(Object.entries(value).map(([key,item])=>[key,
  item===null?null:timeKeys.has(key)?item+delta:key==='createTime'?base-100000+item:shift(item)]));
 return value;
}
const first=shift(truth.first),second=shift(truth.second);
fixture.historySeries=shift(truth._series);
fixture.historyWindow={from:base-60000,to:base+30000};
const moments=shift(truth._moments);
const moment=index=>moments[index];
fixture.historyMoment=moment(0);
fixture.processWatch=null;
writeFileSync('target/history-interval.json',JSON.stringify(fixture));
const result=(command,value,args,cache)=>({resolve:{command,value,args,cache,ok:true}});
const select=(index)=>result('history_moment',moment(index),{ts:base+index*5000,cursor:base+index*5000},'historyMoment');
const answer=(value,generation)=>result('history_interval',value,{from:value.from,to:value.to,metric:'cpu',generation},'historyInterval');
const down={wheel:{x:300,y:250,lines:-80}},up={wheel:{x:300,y:250,lines:10}};
const steps=[
 {control:{page:'history',pointerOutside:true},wait:500},
 {control:{keys:['home']},wait:300},
 {control:select(0),wait:300},
 {control:down,wait:400,capture:'markers'},
 {control:{clickAnchor:'interval-start'},wait:250,inspect:'layout',label:'first-start',expect:{'interval.start':base}},
 {control:{keys:['right']},wait:250},
 {control:{clickAnchor:'interval-end'},wait:200,inspect:'layout',label:'pending-frame-blocked',expect:{'interval.end':null}},
 {control:select(1),wait:300},
 {control:down,wait:250},
 {control:{clickAnchor:'interval-end'},wait:250,inspect:'layout',label:'first-end',expect:{'interval.query.from':first.from,'interval.query.to':first.to}},
 {control:answer(first,2),wait:400,inspect:'layout',label:'compile-only',expect:{'interval.mean':50,'interval.leaders':['compile.exe'],'interval.missing':0}},
 {control:up,wait:500,capture:'compile-interval'},
 {control:{keys:['right']},wait:250},
 {control:select(2),wait:250},
 {control:down,wait:300},
 {control:{clickAnchor:'interval-start'},wait:250},
 {control:{keys:['right']},wait:250},
 {control:select(3),wait:250},
 {control:down,wait:250},
 {control:{clickAnchor:'interval-end'},wait:250,inspect:'layout',label:'second-end',expect:{'interval.query.from':second.from,'interval.query.to':second.to}},
 {control:answer(first,2),wait:250,inspect:'layout',label:'stale-compile-rejected',expect:{'interval.mean':null}},
 {control:answer(second,4),wait:350,inspect:'layout',label:'copy-only',expect:{'interval.mean':5,'interval.leaders':['copy.exe']}},
 {control:up,wait:500,capture:'copy-interval'},
 {control:{data:{historySeries:shift(truth._coarseSeries)}},wait:250},
 {control:{clickAnchor:'interval-peak'},wait:300,inspect:'layout',label:'interval-peak-exact-despite-coarse-bucket',expect:{'history.requestedFrame':second.cpu.peakTs}},
 {control:{contentZoom:1.5,theme:'light'},probe:'size 900 700',wait:700,capture:'small-150'},
 {control:down,wait:400,capture:'small-150-bottom'},
 {control:up,wait:400,capture:'small-150-interval'},
 {control:{contentZoom:1,theme:'dark'},probe:'size 1200 900',wait:500},
 {control:up,wait:300},
 {control:{clickAnchor:'interval-clear'},wait:250,inspect:'layout',label:'clear-interval',expect:{'interval.start':null,'interval.mean':null}},
 {control:answer(second,4),wait:250,inspect:'layout',label:'stale-after-clear-rejected',expect:{'interval.start':null,'interval.mean':null}}
];
writeFileSync('target/history-interval-steps.json',JSON.stringify({window:[1200,900],source:process.argv[2]??'target/local-1-0/interval-truth.json',steps},null,2));
