// Real writer/reader and renderer output. Native Save is mocked here; no file picker opens.
import {readFileSync,writeFileSync} from 'node:fs';
const source=process.argv[2]??'target/local-1-0/report-real-truth.json';
const truth=JSON.parse(readFileSync(source,'utf8')),report=truth.report;
const fixture=JSON.parse(readFileSync(new URL('desktop.json',import.meta.url),'utf8'));
fixture.historySeries=truth._series;fixture.historyMoment=truth._moments.at(-1);
fixture.historyWindow={from:report.from-60000,to:report.to+60000};fixture.processWatch=null;
const args=format=>({from:report.from,to:report.to,metric:'cpu',format,generation:1});
const preview=(format,value)=>({resolve:{command:'history_report_preview',args:args(format),ok:true,value}});
const save=(value,ok=true)=>({resolve:{command:'history_report_save',args:{id:value.id,sha256:value.sha256},ok,value:ok?true:'controlled write failure'}});
const top={wheel:{x:300,y:250,lines:80}},down={wheel:{x:300,y:250,lines:-10}};
const steps=[
 {control:{page:'history',pointerOutside:true},wait:350},
 {control:{data:{intervalStart:report.from,intervalEnd:report.to-5000,intervalGeneration:1,intervalMetric:'cpu'}},wait:150},
 {control:{resolve:{command:'history_interval',cache:'historyInterval',args:{from:report.from,to:report.to,metric:'cpu',generation:1},ok:true,value:report}},wait:300},
 {control:down,wait:350},
 {control:{clickAnchor:'interval-report-markdown'},wait:250,inspect:'layout',label:'markdown-request',expect:{'report.request.format':'markdown','report.request.from':report.from,'report.request.to':report.to}},
 {control:preview('markdown',truth.md),wait:450,inspect:'layout',label:'markdown-preview',expect:{'report.preview.id':truth.md.id,'report.preview.sha256':truth.md.sha256,'dialog.kind':'historyReport'}},
 {capture:'markdown-preview'},
 {control:{wheel:{x:500,y:400,lines:-80}},wait:350,capture:'markdown-data'},
 {control:{clickAnchor:'report-save'},wait:200,inspect:'layout',label:'save-reviewed-id',expect:{'report.save.id':truth.md.id,'report.save.sha256':truth.md.sha256}},
 {control:save(truth.md,false),wait:300,inspect:'layout',label:'save-error-keeps-preview',expect:{'dialog.hasError':true,'report.preview.id':truth.md.id}},
 {control:{clickAnchor:'report-save'},wait:200},{control:save(truth.md),wait:5500},
 {control:{keys:['escape']},wait:350,inspect:'layout',label:'close-preview',expect:{'report.preview':null}},
 {control:{wheel:{x:300,y:250,lines:80}},wait:200},{control:down,wait:300},
 {control:{clickAnchor:'interval-report-csv'},wait:200,inspect:'layout',label:'csv-request',expect:{'report.request.format':'csv'}},
 {control:preview('csv',truth.csv),wait:450,inspect:'layout',label:'csv-preview',expect:{'report.preview.format':'csv','report.preview.sha256':truth.csv.sha256}},
 {capture:'csv-preview'},
 {control:{contentZoom:1.5,theme:'light'},probe:'size 900 700',wait:500,capture:'csv-small-150'},
 {control:{wheel:{x:300,y:250,lines:-80}},wait:350,inspect:'layout',label:'small-preview-scrolls',expect:{'report.preview.scrolled':true},capture:'csv-small-150-data'},
 {control:{keys:['escape']},wait:300},
 // A preview for an interval that has since been cleared cannot reopen a dialog.
 {control:{data:{intervalGeneration:2,intervalStart:null,intervalEnd:null,historyInterval:null}},wait:150},
 {control:preview('markdown',truth.md),wait:200,inspect:'layout',label:'stale-preview-rejected',expect:{'report.preview':null}}
];
writeFileSync('target/report-preview.json',JSON.stringify(fixture));
writeFileSync('target/report-preview-steps.json',JSON.stringify({window:[1200,1000],source,steps},null,2));
