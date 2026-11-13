// Counterfactual history availability; never reads real history or selects a real PID.
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
const fixture=JSON.parse(readFileSync(new URL('desktop.json',import.meta.url),'utf8'));
const coverage=(samples,valid,partial=0)=>({samples,valid,partial,legacyFrames:0});
const known=coverage(1,1),missing=coverage(1,0),mixed=coverage(2,1),partial=coverage(1,1,1);
fixture.historySeries=fixture.historySeries.map((point,index)=>({...point,disk:index%3===0?0:1000,diskCoverage:index%3===0?known:index%3===1?missing:mixed}));
fixture.historySeries.at(-1).disk=0;fixture.historySeries.at(-1).diskCoverage=known;
const system={...fixture.historyMoment.system,disk:0,diskCoverage:known};
const rows=[['idle.exe',known,0],['unavailable.exe',missing,0],['mixed.exe',mixed,1000]].map(([name,netCoverage,net],index)=>({pid:44101+index,name,cpu:3-index,mem:1048576,io:0,gpu:0,net,netCoverage}));
fixture.historyMoment={system,processes:rows};
mkdirSync('target',{recursive:true});writeFileSync('target/history-coverage.json',JSON.stringify(fixture));
const moment=(disk,diskCoverage,processes=rows)=>({system:{...system,disk,diskCoverage},processes});
let revision=0;
const data=(disk,diskCoverage,processes=rows)=>({historyMoment:moment(disk,diskCoverage,processes),historySeries:fixture.historySeries.map((point,index)=>index===fixture.historySeries.length-1?{...point,disk,diskCoverage}:point),historyWindow:{...fixture.historyWindow,to:fixture.historyWindow.to+(++revision)}});
const steps=[
 {control:{page:'history',pointerOutside:true},wait:700,inspect:'layout',label:'idle-and-unavailable',expect:{'historyDiskCoverage.valid':1,'historyNetworkCoverage.0.valid':1,'historyNetworkCoverage.1.valid':0,'historyNetworkCoverage.2.samples':2},capture:'known-zero-and-unknown'},
 {control:{data:data(0,missing,rows.map(row=>({...row,net:0,netCoverage:missing}))),pointerOutside:true},wait:500,inspect:'layout',label:'missing-source',expect:{'historyDiskCoverage.valid':0},capture:'unavailable'},
 {control:{data:data(1000,mixed),pointerOutside:true},wait:500,inspect:'layout',label:'mixed-interval',expect:{'historyDiskCoverage.samples':2,'historyDiskCoverage.valid':1},capture:'mixed-interval'},
 {control:{data:data(1000,partial),pointerOutside:true},wait:500,inspect:'layout',label:'partial-devices',expect:{'historyDiskCoverage.partial':1},capture:'partial-devices'},
 {control:{data:data(0,null,rows.map(row=>({...row,net:0,netCoverage:null}))),pointerOutside:true},wait:500,inspect:'layout',label:'legacy-unknown',expect:{historyDiskCoverage:null},capture:'legacy-unknown'}
];
writeFileSync('target/history-coverage-steps.json',JSON.stringify({window:[1200,1000],steps},null,2));
