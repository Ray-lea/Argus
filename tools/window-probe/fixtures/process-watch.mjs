// Window test data; accepts a WatchView exported by the real process_watch example.
// Without an input path the trajectory is synthetic. No process commands run.
import {mkdirSync,readFileSync,writeFileSync} from 'node:fs';
const base=Date.now()-40000;
const point=(offset,memory,state='recording')=>({ts:base+offset,memory,commit:memory===null?null:memory+1048576,state});
const recorded=process.argv[2]?JSON.parse(readFileSync(process.argv[2],'utf8')):{version:1,pid:44101,createTime:base-1000,name:'controlled-memory-helper',state:'exited',points:[point(0,4<<20),point(5000,4<<20),point(10000,12<<20),point(15000,null,'gap'),point(30000,2<<20),point(35000,null,'exited')]};
const fixture=JSON.parse(readFileSync(new URL('desktop.json',import.meta.url),'utf8'));
fixture.snapshot.processes=[{...fixture.snapshot.processes[0],pid:recorded.pid,ppid:0,createTime:recorded.createTime,instanceKey:undefined,name:recorded.name,description:recorded.name,appWindow:true}];
fixture.snapshot.system.processCount=1;
fixture.processWatch=null;
mkdirSync('target',{recursive:true});
writeFileSync('target/process-watch.json',JSON.stringify(fixture));
const active={...recorded,state:'recording',points:recorded.points.filter(point=>point.state==='recording').slice(0,3)};
const stopped={...active,state:'stopped',points:[...active.points,{ts:recorded.points.at(-1).ts,memory:null,commit:null,state:'stopped'}]};
const resolve=(action,value,generation)=>({resolve:{command:'watch_process',args:{action:{op:action},generation},ok:true,value}});
const steps=[
 {control:{page:'processes',view:'list',selectRow:0,pointerOutside:true},wait:700,inspect:'layout',label:'selected',expect:{'processTree.visibleRows':1},capture:'start-button'},
 {control:{clickAnchor:'details-watch'},wait:300,inspect:'layout',label:'start-action',expect:{'watchAction.op':'start','watchAction.pid':recorded.pid}},
 {control:resolve('start',active,1),wait:600,inspect:'layout',label:'recording',expect:{page:'history','watch.state':'recording','watch.points':active.points.length},capture:'recording'},
 {control:{clickAnchor:'watch-stop'},wait:300,inspect:'layout',label:'stop-action',expect:{'watchAction.op':'stop'}},
 {control:resolve('stop',stopped,2),wait:400,inspect:'layout',label:'stopped',expect:{'watch.state':'stopped'},capture:'stopped'},
 {control:resolve('status',recorded,2),wait:500,inspect:'layout',label:'real-exit-and-gap',expect:{'watch.state':'exited','watch.missing':recorded.points.filter(point=>point.memory===null).length},capture:'exit-and-gap'},
 {control:{contentZoom:1.5,theme:'light'},probe:'size 900 700',wait:900,capture:'small-150'},
 {control:{contentZoom:1,theme:'dark'},probe:'size 1200 800',wait:900},
 {control:{clickAnchor:'watch-delete'},wait:300,inspect:'layout',label:'delete-action',expect:{'watchAction.op':'delete'}},
 {control:resolve('delete',null,3),wait:400,inspect:'layout',label:'deleted',expect:{'watch.state':null,'watch.points':0},capture:'deleted'},
 {control:resolve('status',active,2),wait:300,inspect:'layout',label:'stale-status-rejected',expect:{'watch.state':null,'watch.points':0}},
];
writeFileSync('target/process-watch-steps.json',JSON.stringify({window:[1200,800],source:process.argv[2]??'synthetic',steps},null,2));
