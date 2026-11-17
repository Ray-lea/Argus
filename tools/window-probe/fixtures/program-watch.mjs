// Native-window fixture: commands are mocked; actual identity/collector is tested in Rust.
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
const fixture=JSON.parse(readFileSync(new URL('desktop.json',import.meta.url),'utf8'));
const base=Date.now()-60000;
const pid=44101,created=base-20000;
const point=(ts,memory,state='recording')=>({ts,memory,commit:memory===null?null:memory+1048576,state});
const runs=[
 {pid,createTime:created,firstTs:base,lastTs:base+15000,points:1008,state:'exited'},
 {pid:44102,createTime:base+16000,firstTs:base+20000,lastTs:base+50000,points:7,state:'recording'}
];
const view={version:2,pid,createTime:created,name:'same.exe',mode:'program',state:'recording',liveRuns:1,
 program:{nativePath:'\\Device\\HarddiskVolume3\\Argus test\\one\\same.exe',displayPath:'C:\\Argus test\\one\\same.exe'},
 runs,totalRuns:2,totalPoints:1008,runOffset:0,damaged:false,unconfirmed:0,
 firstSequence:101,hasEarlier:true,hasLater:false,
 points:[point(base,4<<20),point(base+5000,12<<20),point(base+10000,null,'gap'),point(base+15000,null,'exited')]
};
fixture.snapshot.processes=[{...fixture.snapshot.processes[0],pid,ppid:0,createTime:created,instanceKey:undefined,name:'same.exe',description:'same.exe',appWindow:true}];
fixture.snapshot.system.processCount=1;
fixture.processWatch=null;
const second={...view,pid:44102,createTime:runs[1].createTime,totalPoints:7,firstSequence:1200,hasEarlier:false,
 points:[point(base+20000,2<<20),point(base+25000,3<<20),point(base+30000,4<<20)]};
const earlier={...view,firstSequence:1,hasEarlier:false,hasLater:true,points:[point(base-40000,1<<20),point(base-35000,2<<20)]};
const waiting={...view,liveRuns:0,runs:runs.map(run=>({...run,state:'exited'}))};
const stopped={...waiting,state:'stopped'};
const answer=(op,value,generation)=>({resolve:{command:'watch_process',args:{action:{op},generation},ok:true,value}});
const steps=[
 {control:{page:'processes',view:'list',selectRow:0,pointerOutside:true},wait:700,capture:'two-choices'},
 {control:{clickAnchor:'details-watch-program'},wait:250,inspect:'layout',label:'start-program-action',expect:{'watchAction.op':'startProgram','watchAction.pid':pid}},
 {control:answer('startProgram',view,1),wait:600,inspect:'layout',label:'program-history',expect:{page:'history','watch.mode':'program','watch.runs':2,'watch.pid':pid},capture:'first-launch'},
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
writeFileSync('target/program-watch.json',JSON.stringify(fixture));
writeFileSync('target/program-watch-steps.json',JSON.stringify({window:[1200,900],steps},null,2));
if(process.argv[2]){
 const real=JSON.parse(readFileSync(process.argv[2],'utf8'));
 const last=real.runs.at(-1);
 const realSteps=[
  {control:{page:'history',pointerOutside:true},wait:400},
  {control:answer('status',real,0),wait:600,inspect:'layout',label:'real-runs',expect:{'watch.mode':'program','watch.runs':real.runs.length},capture:'real-recording'},
  {control:{contentZoom:1.5,theme:'light'},probe:'size 900 700',wait:700,capture:'real-small-150'},
  {control:{contentZoom:1,theme:'dark'},probe:'size 1200 900',wait:500},
  {control:{clickSelect:'watch-run'},wait:300,capture:'real-launch-list'},
  {control:{keys:[...Array(real.runs.length-1).fill('down'),'enter']},wait:300,inspect:'layout',label:'last-real-launch',expect:{'watchAction.op':'selectRun','watchAction.pid':last.pid}},
 ];
 writeFileSync('target/program-watch-real-steps.json',JSON.stringify({window:[1200,900],source:process.argv[2],steps:realSteps},null,2));
}
