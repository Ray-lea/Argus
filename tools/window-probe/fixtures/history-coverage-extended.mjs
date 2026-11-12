// Independent per-metric availability, with explicit counterfactual reasons.
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
const fixture=JSON.parse(readFileSync(new URL('desktop.json',import.meta.url),'utf8'));
const coverage=(samples,valid,missing=0,partial=0)=>({samples,valid,partial,missing,legacyFrames:0});
const known=coverage(1,1), denied=coverage(1,0,2), stale=coverage(1,0,4), mixed=coverage(3,1,14,1);
const rows=(c,value)=>['controlled-a.exe','controlled-b.exe','controlled-c.exe'].map((name,index)=>({pid:44101+index,name,cpu:3-index,mem:1048576,io:value*100,ioCoverage:c,gpu:value,gpuCoverage:c,net:value*200,netCoverage:c}));
let revision=0;
const data=(c,value)=>({
 historyMoment:{system:{...fixture.historyMoment.system,disk:value*100,diskCoverage:c,gpu:value,gpuCoverage:c,net:value*200,netCoverage:c},processes:rows(c,value)},
 historySeries:fixture.historySeries.map(point=>({...point,disk:value*100,diskCoverage:c,gpu:value,gpuCoverage:c,net:value*200,netCoverage:c})),
 historyWindow:{...fixture.historyWindow,to:fixture.historyWindow.to+(++revision)}
});
Object.assign(fixture,data(known,0));
mkdirSync('target',{recursive:true});
writeFileSync('target/history-coverage-extended.json',JSON.stringify(fixture));
const steps=[
 {control:{page:'history',pointerOutside:true},wait:800,inspect:'layout',label:'measured-zero',expect:{'historyGpuCoverage.valid':1,'historySystemNetworkCoverage.valid':1,'historyIoCoverage.0.valid':1},capture:'measured-zero'},
 {control:{data:data(denied,99),pointerOutside:true},wait:600,inspect:'layout',label:'denied',expect:{'historyGpuCoverage.valid':0,'historyGpuCoverage.missing':2,'historyIoCoverage.0.valid':0},capture:'denied'},
 {control:{data:data(stale,99),pointerOutside:true},wait:600,inspect:'layout',label:'stale',expect:{'historyGpuCoverage.valid':0,'historySystemNetworkCoverage.missing':4,'historyIoCoverage.0.missing':4},capture:'stale'},
 {control:{data:data(mixed,12),pointerOutside:true},wait:600,inspect:'layout',label:'mixed',expect:{'historyGpuCoverage.valid':1,'historyGpuCoverage.samples':3,'historySystemNetworkCoverage.partial':1,'historyIoCoverage.0.samples':3},capture:'mixed'},
 {control:{data:data(null,0),pointerOutside:true},wait:600,inspect:'layout',label:'legacy',expect:{historyGpuCoverage:null,historySystemNetworkCoverage:null,'historyIoCoverage.0':null},capture:'legacy'},
 {control:{data:data(denied,99),contentZoom:1.5,theme:'light',pointerOutside:true},probe:'size 900 700',wait:700,inspect:'layout',label:'small-denied',expect:{'historyGpuCoverage.valid':0},capture:'small-denied'},
 {control:{wheel:{x:300,y:250,lines:-80}},wait:600,capture:'small-bottom'}
];
writeFileSync('target/history-coverage-extended-steps.json',JSON.stringify({window:[1200,1000],steps},null,2));
