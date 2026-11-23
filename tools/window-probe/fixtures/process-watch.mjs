// Native-window fixture: commands are mocked; actual identity/collector is tested in Rust.
// The observation's records are relative to the fixture sample's time, which
// is the clock the history page's lanes follow under a fixture.
import {mkdirSync,readFileSync,writeFileSync} from 'node:fs';
const fixture=JSON.parse(readFileSync(new URL('desktop.json',import.meta.url),'utf8'));
const now=fixture.snapshot.timestamp;
const SAMPLE=5000;
const pid=44101,created=now-12*60000;
const older=44090,olderCreated=now-50*60000;
const point=(ts,memory,state='recording',extra={})=>memory===null
 ? {ts,state}
 : {
  ts,memory,commit:extra.commit??memory+(18<<20),workingSet:extra.workingSet??memory+(41<<20),
  cpu:extra.cpu??14.5,gpu:extra.gpu??6.2,gpuMemory:extra.gpuMemory??(32<<20),
  diskRead:extra.diskRead??262144,diskWrite:extra.diskWrite??65536,
  ioRead:extra.ioRead??524288,ioWrite:extra.ioWrite??131072,
  netReceived:extra.netReceived??40960,netSent:extra.netSent??12288,
  threads:extra.threads??18,handles:extra.handles??312,
  state
 };
// Deterministic smooth noise, as the history fixture's.
const hash=n=>{n=Math.imul(n^(n>>>16),0x45d9f3b);n=Math.imul(n^(n>>>16),0x45d9f3b);return((n^(n>>>16))>>>0)/4294967296;};
const smooth=(t,seed,period)=>{const x=t/period,i=Math.floor(x),f=x-i,s=f*f*(3-2*f);return hash(i*7919+seed)*(1-s)+hash((i+1)*7919+seed)*s;};
// A run recorded every 5 s from `first` to `last`: memory creeping up with
// bursts of CPU, and a stretch of missing samples at `gapAt`.
const records=(first,last,base,gapAt=null)=>{
 const out=[];
 for(let ts=first,k=0;ts<=last;ts+=SAMPLE,k++){
  if(gapAt!==null&&ts>=gapAt&&ts<gapAt+30000){
   if(ts===gapAt)out.push(point(ts,null,'gap'));
   continue;
  }
  const s=ts/1000;
  const memory=Math.round(base+k*96*1024+(24<<20)*smooth(s,1,90));
  const cpu=Math.round((3+9*smooth(s,2,40)+(smooth(s,3,25)>0.7?28*smooth(s,4,6):0))*10)/10;
  out.push(point(ts,memory,'recording',{cpu,gpu:Math.round(4*smooth(s,5,60)*10)/10,
   diskRead:Math.round(600000*smooth(s,6,30)**3),diskWrite:Math.round(120000*smooth(s,7,20)**2),
   netReceived:Math.round(90000*smooth(s,8,45)**2),netSent:Math.round(30000*smooth(s,9,35)**2),
   threads:22+Math.round(4*smooth(s,10,120)),handles:340+Math.round(30*smooth(s,11,150))}));
 }
 return out;
};
const first=now-10*60000;
const live=records(first,now-SAMPLE,180<<20,now-6*60000);
const done=records(now-48*60000,now-30*60000,150<<20);
done.push(point(now-30*60000+SAMPLE,null,'exited'));
const runs=[
 {pid:older,createTime:olderCreated,firstTs:done[0].ts,lastTs:done.at(-1).ts,points:done.length,state:'exited'},
 {pid,createTime:created,firstTs:first,lastTs:live.at(-1).ts,points:live.length,state:'recording'}
];
const view={version:1,pid,createTime:created,name:'same.exe',state:'recording',liveRuns:1,
 program:{nativePath:'\\Device\\HarddiskVolume3\\Argus test\\one\\same.exe',displayPath:'C:\\Argus test\\one\\same.exe'},
 runs,totalRuns:2,totalPoints:live.length,runOffset:0,damaged:false,unconfirmed:0,
 firstSequence:101,hasEarlier:false,hasLater:false,points:live};
fixture.snapshot.processes=[{...fixture.snapshot.processes[0],pid,ppid:0,createTime:created,instanceKey:undefined,name:'same.exe',description:'same.exe',appWindow:true}];
fixture.snapshot.system.processCount=1;
fixture.processWatch=null;
const second={...view,pid:older,createTime:olderCreated,totalPoints:done.length,points:done};
const waiting={...view,liveRuns:0,runs:runs.map(run=>({...run,state:'exited'})),points:live.concat([point(now,null,'exited')])};
const stopped={...waiting,state:'stopped'};
const answer=(op,value,generation)=>({resolve:{command:'watch_process',args:{action:{op},generation},ok:true,value}});
const steps=[
 {control:{page:'processes',view:'list',selectRow:0,pointerOutside:true},wait:700,capture:'details-watch-button'},
 {control:{hoverAnchor:'details-watch'},wait:400,capture:'details-watch-hover'},
 {control:{rowMenu:0},wait:400,inspect:'menu',label:'row-menu-watch',expect:{'state.open':true,'state.items.9':'观察进程','state.disabled.9':false},capture:'row-menu'},
 {control:{keys:['escape']},wait:300},
 {control:{clickAnchor:'details-watch'},wait:250,inspect:'layout',label:'start-action',expect:{'watchAction.op':'start','watchAction.pid':pid}},
 {control:answer('start',view,1),wait:900,inspect:'layout',label:'watch-history',expect:{page:'history','history.page':'watch','history.range':'Last(300000)','watch.runs':2,'watch.pid':pid,'watch.lanes.0.reading':'memory','watch.lanes.0.overlay':'commit'},capture:'first-launch'},
 {control:{mouse:{type:'move',x:760,y:300}},wait:500,capture:'hover'},
 {control:{pointerOutside:true,clickAnchor:'history-lane-name-1'},wait:500,inspect:'layout',label:'lane-chooser',expect:{'history.chooser.open':true},capture:'lane-chooser'},
 {control:{formValues:{historyReadingSearch:'CPU'}},wait:400},
 {control:{keys:['enter']},wait:500,inspect:'layout',label:'switch-to-cpu',expect:{'watch.lanes.0.reading':'cpu'},capture:'cpu'},
 {control:{clickAnchor:'history-add-lane'},wait:500,inspect:'layout',label:'add-lane-chooser',expect:{'history.chooser.open':true}},
 {control:{formValues:{historyReadingSearch:'句柄'}},wait:400},
 {control:{keys:['enter']},wait:700,inspect:'layout',label:'second-lane-added',expect:{'watch.lanes.length':2,'watch.lanes.1.reading':'handles'},capture:'two-lanes'},
 {control:{clickAnchor:'seg:history-page:0'},wait:700,inspect:'layout',label:'switch-to-system',expect:{page:'history','history.page':'system','history.lanes.0.reading':'cpu','history.lanes.0.overlay':'cpuTemp'},capture:'system-subpage'},
 {control:{clickAnchor:'seg:history-page:1'},wait:700,inspect:'layout',label:'switch-to-watch',expect:{page:'history','history.page':'watch','watch.lanes.length':2},capture:'back-to-watch'},
 {control:{clickAnchor:'history-range'},wait:400,capture:'ranges'},
 {control:{keys:['escape']},wait:300},
 {control:{clickAnchor:'watch-run-1'},wait:300,inspect:'layout',label:'choose-older',expect:{'watchAction.op':'selectRun','watchAction.pid':older}},
 {control:answer('selectRun',second,2),wait:900,inspect:'layout',label:'older-run',expect:{'watch.pid':older},capture:'older-run'},
 {control:answer('status',waiting,2),wait:600,inspect:'layout',label:'waiting',expect:{'watch.state':'recording','watch.liveRuns':0},capture:'waiting'},
 {control:{contentZoom:1.5,theme:'light'},probe:'size 900 700',wait:900,capture:'small-150'},
 {control:{contentZoom:1,theme:'dark'},probe:'size 1200 900',wait:700},
 {control:{clickAnchor:'watch-stop'},wait:200,inspect:'layout',label:'stop-action',expect:{'watchAction.op':'stop'}},
 {control:answer('stop',stopped,3),wait:500,inspect:'layout',label:'stopped',expect:{'watch.state':'stopped'},capture:'stopped'},
 {control:{clickAnchor:'watch-delete'},wait:200,inspect:'layout',label:'delete-action',expect:{'watchAction.op':'delete'}},
 {control:answer('delete',null,4),wait:400,inspect:'layout',label:'deleted',expect:{'watch.state':null},capture:'deleted'},
 {control:answer('status',view,3),wait:250,inspect:'layout',label:'stale-rejected',expect:{'watch.state':null}}
];
mkdirSync('target',{recursive:true});
writeFileSync('target/process-watch.json',JSON.stringify(fixture));
writeFileSync('target/process-watch-steps.json',JSON.stringify({window:[1200,900],steps},null,2));
if(process.argv[2]){
 const real=JSON.parse(readFileSync(process.argv[2],'utf8'));
 const realSteps=[
  {control:{page:'history',historyPage:'watch',pointerOutside:true},wait:400},
  {control:answer('status',real,0),wait:900,inspect:'layout',label:'real-runs',expect:{'watch.runs':real.runs.length},capture:'real-recording'},
  {control:{contentZoom:1.5,theme:'light'},probe:'size 900 700',wait:700,capture:'real-small-150'},
  {control:{contentZoom:1,theme:'dark'},probe:'size 1200 900',wait:500},
 ];
 writeFileSync('target/process-watch-real-steps.json',JSON.stringify({window:[1200,900],source:process.argv[2],steps:realSteps},null,2));
}
