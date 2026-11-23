// Native-window fixture: commands are mocked; actual identity/collector is tested in Rust.
import {mkdirSync,readFileSync,writeFileSync} from 'node:fs';
const fixture=JSON.parse(readFileSync(new URL('desktop.json',import.meta.url),'utf8'));
const base=Date.now()-60000;
const pid=44101,created=base-20000;
const point=(ts,memory,state='recording',extra={})=>memory===null
 ? {ts,state}
 : {
  ts,memory,commit:memory+1048576,workingSet:memory+2097152,
  cpu:extra.cpu??14.5,gpu:extra.gpu??6.2,gpuMemory:extra.gpuMemory??(32<<20),
  diskRead:extra.diskRead??262144,diskWrite:extra.diskWrite??65536,
  ioRead:extra.ioRead??524288,ioWrite:extra.ioWrite??131072,
  netReceived:extra.netReceived??40960,netSent:extra.netSent??12288,
  threads:extra.threads??18,handles:extra.handles??312,
  state
 };
const runs=[
 {pid,createTime:created,firstTs:base,lastTs:base+15000,points:1008,state:'exited'},
 {pid:44102,createTime:base+16000,firstTs:base+20000,lastTs:base+50000,points:7,state:'recording'}
];
const view={version:1,pid,createTime:created,name:'same.exe',state:'recording',liveRuns:1,
 program:{nativePath:'\\Device\\HarddiskVolume3\\Argus test\\one\\same.exe',displayPath:'C:\\Argus test\\one\\same.exe'},
 runs,totalRuns:2,totalPoints:1008,runOffset:0,damaged:false,unconfirmed:0,
 firstSequence:101,hasEarlier:true,hasLater:false,
 points:[
  point(base,4<<20,'recording',{cpu:8.2,handles:290}),
  point(base+5000,12<<20,'recording',{cpu:34.6,handles:340}),
  point(base+10000,null,'gap'),
  point(base+15000,null,'exited')
 ]
};
fixture.snapshot.processes=[{...fixture.snapshot.processes[0],pid,ppid:0,createTime:created,instanceKey:undefined,name:'same.exe',description:'same.exe',appWindow:true}];
fixture.snapshot.system.processCount=1;
fixture.processWatch=null;
const second={...view,pid:44102,createTime:runs[1].createTime,totalPoints:7,firstSequence:1200,hasEarlier:false,
 points:[
  point(base+20000,2<<20,'recording',{cpu:4.0,handles:120}),
  point(base+25000,3<<20,'recording',{cpu:9.5,handles:145}),
  point(base+30000,4<<20,'recording',{cpu:12.1,handles:168})
 ]};
const earlier={...view,firstSequence:1,hasEarlier:false,hasLater:true,points:[point(base-40000,1<<20),point(base-35000,2<<20)]};
const waiting={...view,liveRuns:0,runs:runs.map(run=>({...run,state:'exited'}))};
const stopped={...waiting,state:'stopped'};
const answer=(op,value,generation)=>({resolve:{command:'watch_process',args:{action:{op},generation},ok:true,value}});
const steps=[
 {control:{page:'processes',view:'list',selectRow:0,pointerOutside:true},wait:700,capture:'details-watch-button'},
 {control:{hoverAnchor:'details-watch'},wait:400,capture:'details-watch-hover'},
 {control:{rowMenu:0},wait:400,inspect:'menu',label:'row-menu-watch',expect:{'state.open':true,'state.items.9':'观察进程','state.disabled.9':false},capture:'row-menu'},
 {control:{keys:['escape']},wait:300},
 {control:{clickAnchor:'details-watch'},wait:250,inspect:'layout',label:'start-action',expect:{'watchAction.op':'start','watchAction.pid':pid}},
 {control:answer('start',view,1),wait:600,inspect:'layout',label:'watch-history',expect:{page:'history','history.page':'watch','watch.runs':2,'watch.pid':pid,'watch.lanes.0.reading':'memory','watch.lanes.0.overlay':'commit'},capture:'first-launch'},
 {control:{clickAnchor:'watch-tab-cpu'},wait:300,inspect:'layout',label:'switch-to-cpu',expect:{'watch.lanes.0.reading':'cpu','watch.lanes.0.overlay':'commit'},capture:'cpu-with-commit-overlay'},
 {control:{clickAnchor:'watch-lane-overlay-1'},wait:300,inspect:'menu',label:'overlay-menu',expect:{'state.open':true}},
 {control:{keys:['down','enter']},wait:300,inspect:'layout',label:'overlay-changed',expect:{'watch.lanes.0.overlay':'memory'}},
 {control:{clickAnchor:'watch-add-lane'},wait:300,inspect:'menu',label:'add-lane-menu',expect:{'state.open':true}},
 {control:{keys:['down','down','down','down','down','down','enter']},wait:350,inspect:'layout',label:'second-lane-added',expect:{'watch.lanes.length':2},capture:'two-lanes'},
 {control:{clickAnchor:'watch-lane-remove-2'},wait:300,inspect:'layout',label:'second-lane-removed',expect:{'watch.lanes.length':1}},
 {control:{clickAnchor:'watch-tab-memory'},wait:250,inspect:'layout',label:'back-to-memory',expect:{'watch.lanes.0.reading':'memory'}},
 {control:{clickAnchor:'seg:history-page:0'},wait:400,inspect:'layout',label:'switch-to-system',expect:{page:'history','history.page':'system'},capture:'system-subpage'},
 {control:{clickAnchor:'seg:history-page:1'},wait:400,inspect:'layout',label:'switch-to-watch',expect:{page:'history','history.page':'watch'}},
 {control:{clickAnchor:'watch-earlier'},wait:250,inspect:'layout',label:'earlier-action',expect:{'watchAction.op':'page','watchAction.before':101}},
 {control:answer('page',earlier,2),wait:350,capture:'earlier-points'},
 {control:{clickAnchor:'watch-latest'},wait:200,inspect:'layout',label:'latest-action',expect:{'watchAction.op':'page','watchAction.before':null}},
 {control:answer('page',view,3),wait:300},
 {control:{clickSelect:'watch-run'},wait:250,capture:'launch-menu'},
 {control:{keys:['down','enter']},wait:250,inspect:'layout',label:'choose-second',expect:{'watchAction.op':'selectRun','watchAction.pid':44102}},
 {control:answer('selectRun',second,4),wait:400,inspect:'layout',label:'second-curve',expect:{'watch.pid':44102,'watch.points':3},capture:'second-launch'},
 {control:answer('status',waiting,4),wait:350,inspect:'layout',label:'waiting',expect:{'watch.state':'recording','watch.liveRuns':0},capture:'waiting'},
 {control:{contentZoom:1.5,theme:'light'},probe:'size 900 700',wait:800,capture:'small-150'},
 {control:{contentZoom:1,theme:'dark'},probe:'size 1200 900',wait:600},
 {control:{clickAnchor:'watch-stop'},wait:200,inspect:'layout',label:'stop-action',expect:{'watchAction.op':'stop'}},
 {control:answer('stop',stopped,5),wait:350,inspect:'layout',label:'stopped',expect:{'watch.state':'stopped','watch.points':4},capture:'stopped'},
 {control:{clickAnchor:'watch-delete'},wait:200,inspect:'layout',label:'delete-action',expect:{'watchAction.op':'delete'}},
 {control:answer('delete',null,6),wait:300,inspect:'layout',label:'deleted',expect:{'watch.state':null}},
 {control:answer('status',view,5),wait:250,inspect:'layout',label:'stale-rejected',expect:{'watch.state':null}}
];
mkdirSync('target',{recursive:true});
writeFileSync('target/process-watch.json',JSON.stringify(fixture));
writeFileSync('target/process-watch-steps.json',JSON.stringify({window:[1200,900],steps},null,2));
if(process.argv[2]){
 const real=JSON.parse(readFileSync(process.argv[2],'utf8'));
 const last=real.runs.at(-1);
 const realSteps=[
  {control:{page:'history',historyPage:'watch',pointerOutside:true},wait:400},
  {control:answer('status',real,0),wait:600,inspect:'layout',label:'real-runs',expect:{'watch.runs':real.runs.length},capture:'real-recording'},
  {control:{contentZoom:1.5,theme:'light'},probe:'size 900 700',wait:700,capture:'real-small-150'},
  {control:{contentZoom:1,theme:'dark'},probe:'size 1200 900',wait:500},
  {control:{clickSelect:'watch-run'},wait:300,capture:'real-launch-list'},
  {control:{keys:[...Array(real.runs.length-1).fill('down'),'enter']},wait:300,inspect:'layout',label:'last-real-launch',expect:{'watchAction.op':'selectRun','watchAction.pid':last.pid}},
 ];
 writeFileSync('target/process-watch-real-steps.json',JSON.stringify({window:[1200,900],source:process.argv[2],steps:realSteps},null,2));
}
