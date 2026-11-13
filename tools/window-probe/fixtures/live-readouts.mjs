// Resource readouts through failure and recovery; all processes and sources are modelled.
import {readFileSync,writeFileSync} from 'node:fs';
import './live-coverage.mjs';
const fixture=JSON.parse(readFileSync('target/live-coverage.json','utf8'));
const state=(valid,missing=0)=>({valid,partial:false,missing});
const snapshot=(valid,value)=>{
 const s=structuredClone(fixture.snapshot);s.seq+=value+1;s.timestamp+=1000*(value+1);
 s.hardware.networkState=state(valid,valid?0:2);
 for(const nic of s.hardware.nics){nic.rx=value*1000;nic.tx=value*100;nic.rateState=state(valid,valid?0:2);}
 for(const gpu of s.hardware.gpus){gpu.util=value;gpu.engines=gpu.engines.map(()=>value);gpu.utilState=state(valid,valid?0:2);}
 return s;
};
const failed=snapshot(false,99), recovered=snapshot(true,0);
const steps=[
 {control:{page:'processes',view:'apps',pointerOutside:true},wait:700,inspect:'layout',label:'corrected-totals',expect:{processNetworkTotal:2000},capture:'corrected-totals'},
 {control:{sample:{set:{'/hardware':failed.hardware}},pointerOutside:true},wait:700,inspect:'layout',label:'sources-unavailable',expect:{liveGpuValue:null,liveNetworkValue:null},capture:'sources-unavailable'},
 {control:{page:'performance',deviceKind:'network',pointerOutside:true},wait:700,inspect:'layout',label:'network-unavailable',expect:{page:'performance'},capture:'network-unavailable'},
 {control:{deviceKind:'gpu',pointerOutside:true},wait:700,capture:'gpu-unavailable'},
 {control:{sample:{set:{'/hardware':recovered.hardware}},pointerOutside:true},wait:700,inspect:'layout',label:'measured-zero',expect:{liveGpuValue:0,liveNetworkValue:0},capture:'gpu-measured-zero'},
 {control:{deviceKind:'network',pointerOutside:true},wait:700,capture:'network-measured-zero'},
 {control:{page:'processes',contentZoom:1.5,theme:'light',pointerOutside:true},probe:'size 1100 800',wait:700,capture:'large-text'}
];
writeFileSync('target/live-readouts.json',JSON.stringify(fixture));
writeFileSync('target/live-readouts-steps.json',JSON.stringify({window:[1500,900],steps},null,2));
