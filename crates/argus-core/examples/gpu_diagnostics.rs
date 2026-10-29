//! Read-only GPU accounting comparison; does not capture the desktop or change settings.
//! cargo run -p argus-core --example gpu_diagnostics --release -- <pid> <samples>

#[allow(dead_code)]
#[path = "../src/hardware/nvml.rs"]
mod nvml;
#[allow(dead_code)]
#[path = "../src/pdh.rs"]
mod pdh;
#[allow(dead_code)]
#[path = "../src/util.rs"]
mod util;

use argus_core::{Collector, MetaStore};
use serde_json::json;
use std::{
    collections::HashMap,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use windows::{Wdk::Graphics::Direct3D::*, Win32::System::Performance::*, core::w};

fn raw(counter: PDH_HCOUNTER) -> Vec<(String, i64, i64, u32)> {
    let (mut size, mut count) = (0, 0);
    if unsafe { PdhGetRawCounterArrayW(counter, &mut size, &mut count, None) } != PDH_MORE_DATA {
        return vec![];
    }
    let mut buf = util::AlignedBuf::with_capacity(size as usize);
    let items = buf.as_mut_ptr().cast::<PDH_RAW_COUNTER_ITEM_W>();
    if unsafe { PdhGetRawCounterArrayW(counter, &mut size, &mut count, Some(items)) } != 0 {
        return vec![];
    }
    (0..count as usize)
        .map(|i| {
            let item = unsafe { &*items.add(i) };
            (
                unsafe { item.szName.to_string() }.unwrap_or_default(),
                item.RawValue.FirstValue,
                item.RawValue.SecondValue,
                item.RawValue.CStatus,
            )
        })
        .collect()
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let pid = args
        .get(1)
        .and_then(|s| s.parse::<u32>().ok())
        .expect("target PID");
    let samples = args
        .get(2)
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(10)
        .min(120);
    let mut collector = Collector::new(MetaStore::new());
    let nvml = nvml::Nvml::load();
    let query = pdh::Query::open();
    let counter = query
        .as_ref()
        .and_then(|q| q.add(w!(r"\GPU Engine(*)\Utilization Percentage")));
    if let Some(q) = &query {
        q.collect();
    }
    let mut enumeration = D3DKMT_ENUMADAPTERS2::default();
    assert!(unsafe { D3DKMTEnumAdapters2(&mut enumeration) }.is_ok());
    let mut adapters = vec![D3DKMT_ADAPTERINFO::default(); enumeration.NumAdapters as usize];
    enumeration.pAdapters = adapters.as_mut_ptr();
    assert!(unsafe { D3DKMTEnumAdapters2(&mut enumeration) }.is_ok());
    adapters.truncate(enumeration.NumAdapters as usize);
    let mut previous = HashMap::new();
    let mut previous_raw = HashMap::new();
    let mut last = Instant::now();
    let mut buf = util::AlignedBuf::with_capacity(256 * 1024);
    for round in 0..=samples {
        if round > 0 {
            std::thread::sleep(Duration::from_secs(1));
        }
        let start = Instant::now();
        let snapshot = collector.sample();
        let collection_ms = start.elapsed().as_secs_f64() * 1000.0;
        let pdh_ok = query.as_ref().is_some_and(|q| q.collect());
        let now = Instant::now();
        let ticks = now.duration_since(last).as_secs_f64() * 1e7;
        last = now;
        let mut nodes = Vec::new();
        let mut devices = Vec::new();
        for a in &adapters {
            let luid =
                ((a.AdapterLuid.HighPart as u32 as u64) << 32) | a.AdapterLuid.LowPart as u64;
            let mut address = D3DKMT_ADAPTERADDRESS::default();
            let mut caps = D3DKMT_WDDM_2_7_CAPS::default();
            let pci_ok = unsafe {
                D3DKMTQueryAdapterInfo(&mut D3DKMT_QUERYADAPTERINFO {
                    hAdapter: a.hAdapter,
                    Type: KMTQAITYPE_ADAPTERADDRESS,
                    pPrivateDriverData: (&raw mut address).cast(),
                    PrivateDriverDataSize: size_of_val(&address) as u32,
                })
            }
            .is_ok();
            let caps_ok = unsafe {
                D3DKMTQueryAdapterInfo(&mut D3DKMT_QUERYADAPTERINFO {
                    hAdapter: a.hAdapter,
                    Type: KMTQAITYPE_WDDM_2_7_CAPS,
                    pPrivateDriverData: (&raw mut caps).cast(),
                    PrivateDriverDataSize: size_of_val(&caps) as u32,
                })
            }
            .is_ok();
            let driver = if pci_ok {
                nvml.as_ref().and_then(|n| {
                    n.utilization(
                        address.BusNumber,
                        address.DeviceNumber,
                        address.FunctionNumber,
                    )
                })
            } else {
                None
            };
            devices.push(json!({"luid":luid.to_string(),"pci":pci_ok.then_some([address.BusNumber,address.DeviceNumber,address.FunctionNumber]),"wddm27Caps":caps_ok.then_some(unsafe{caps.Anonymous.Value}),"nvmlCore":driver}));
            let mut q: D3DKMT_QUERYSTATISTICS = unsafe { std::mem::zeroed() };
            q.AdapterLuid = a.AdapterLuid;
            q.Type = D3DKMT_QUERYSTATISTICS_ADAPTER;
            if unsafe { D3DKMTQueryStatistics(&raw mut q) }.is_err() {
                previous.retain(|&(id, _), _| id != luid);
                continue;
            }
            let count = unsafe { q.QueryResult.AdapterInformation.NodeCount }.min(256);
            for node in 0..count {
                q.Type = D3DKMT_QUERYSTATISTICS_NODE;
                q.Anonymous.QueryNode.NodeId = node;
                let status = unsafe { D3DKMTQueryStatistics(&raw mut q) };
                if status.is_err() {
                    previous.remove(&(luid, node));
                    nodes.push(json!({"luid":luid.to_string(),"node":node,"error":status.0}));
                    continue;
                }
                let running =
                    unsafe { q.QueryResult.NodeInformation.GlobalInformation.RunningTime };
                let at = Instant::now();
                let pair = previous.insert((luid, node), (running, at));
                let delta = pair.and_then(|(old, _)| running.checked_sub(old));
                let node_ticks = pair.map(|(_, time)| at.duration_since(time).as_secs_f64() * 1e7);
                let pct = delta.zip(node_ticks).and_then(|(delta, ticks)| {
                    (delta >= 0 && ticks > 0.0).then_some(delta as f64 / ticks * 100.0)
                });
                nodes.push(json!({"luid":luid.to_string(),"node":node,"running":running,"delta":delta,"ticks":node_ticks,"pct":pct}));
            }
        }
        let prefix = format!("pid_{pid}_");
        let mut formatted = HashMap::new();
        let mut rows = Vec::new();
        if pdh_ok && let Some(counter) = counter {
            pdh::each(counter, &mut buf, |name, value| {
                if name.starts_with(&prefix) {
                    formatted.insert(name.to_owned(), value);
                }
            });
            for (name, first, second, status) in raw(counter)
                .into_iter()
                .filter(|(name, ..)| name.starts_with(&prefix))
            {
                let old = previous_raw.insert(name.clone(), (first, second));
                let delta = old.and_then(|(a, b)| first.checked_sub(a).zip(second.checked_sub(b)));
                rows.push(json!({"instance":name,"first":first,"second":second,"status":status,"delta":delta,"formatted":formatted.get(&name)}));
            }
        } else {
            previous_raw.clear();
        }
        let process = snapshot.processes.iter().find(|p|p.pid == pid).map(|p|json!({"pid":p.pid,"created":p.create_time,"gpu":p.gpu,"engine":p.gpu_engine,"engines":p.gpu_engines}));
        let gpus: Vec<_> = snapshot.hardware.gpus.iter().map(|g|json!({"id":g.info.id,"name":g.info.name,"gpu":g.util,"source":g.util_source,"engines":g.info.engines,"usage":g.engines})).collect();
        println!(
            "{}",
            json!({"round":round,"timestamp":SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis(),"collectionMs":collection_ms,"ticks":ticks,"gpus":gpus,"process":process,"nodes":nodes,"devices":devices,"pdhOk":pdh_ok,"pdh":rows})
        );
    }
    for a in adapters {
        let _ = unsafe {
            D3DKMTCloseAdapter(&D3DKMT_CLOSEADAPTER {
                hAdapter: a.hAdapter,
            })
        };
    }
}
