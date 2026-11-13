import {readFileSync,writeFileSync} from 'node:fs';
const f=JSON.parse(readFileSync('tools/window-probe/fixtures/desktop.json','utf8'));
const base=Math.floor(Date.now()/5000)*5000-30000;
const point={...f.historySeries[0],ts:base+5000,frameTs:base,cpuPeakTs:base+5000,cpu:34,cpuPeak:98};
f.historySeries=[{...point,ts:base-60000,frameTs:base-60000,cpuPeakTs:base-60000,cpu:2,cpuPeak:2},point];
f.historyWindow={from:base-3600000,to:base+30000};
f.historyMoment={system:{...point,ts:base},processes:[{pid:44101,name:'quiet.exe',cpu:2,mem:1048576,io:0,gpu:0,net:0}]};
writeFileSync('target/history-peak.json',JSON.stringify(f));
const peakMoment={...f.historyMoment,system:{...point,ts:base+5000},processes:[{...f.historyMoment.processes[0],name:'burst.exe',cpu:98}]};
const steps=[
 {control:{page:'history',pointerOutside:true},wait:700,inspect:'layout',label:'representative',expect:{'history.requestedFrame':base},capture:'average'},
 {control:{mouse:{type:'down',button:'left',x:96,y:577}},wait:300},
 {control:{mouse:{type:'up',button:'left',x:96,y:577}},wait:300,inspect:'layout',label:'peak-selected',expect:{'history.cursor':base+5000,'history.requestedFrame':base+5000}},
 {control:{resolve:{command:'history_moment',cache:'historyMoment',args:{cursor:base+5000,ts:base+5000},ok:true,value:peakMoment}},wait:300,inspect:'layout',label:'peak-frame',expect:{'history.frame':base+5000},capture:'peak'},
 {control:{keys:['end']},wait:300,inspect:'layout',label:'ordinary-same-time',expect:{'history.cursor':base+5000,'history.requestedFrame':base}},
 {control:{resolve:{command:'history_moment',cache:'historyMoment',args:{cursor:base+5000,ts:base+5000},ok:true,value:peakMoment}},wait:300,inspect:'layout',label:'stale-peak-rejected',expect:{'history.requestedFrame':base}},
 {control:{resolve:{command:'history_moment',cache:'historyMoment',args:{cursor:base+5000,ts:base},ok:true,value:f.historyMoment}},wait:300,inspect:'layout',label:'ordinary-frame',expect:{'history.frame':base},capture:'ordinary'},
 {control:{contentZoom:1.5,theme:'light'},probe:'size 900 700',wait:600,capture:'peak-small'},
 {control:{wheel:{x:700,y:450,lines:-16}},wait:500,capture:'peak-small-card'},
];
writeFileSync('target/history-peak-steps.json',JSON.stringify({window:[1200,800],steps},null,2));
