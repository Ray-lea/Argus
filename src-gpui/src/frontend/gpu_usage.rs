//! GPU usage of several processes. Independent engines run
//! concurrently: contributions to each physical engine add up first, then the
//! busiest engine counts; already-reduced percentages are never added.
use super::model::{text, values};
use serde_json::Value;
use std::borrow::Borrow;

/// One physical engine's share: `id` names the adapter and node, so engines
/// with the same display name on different nodes or GPUs stay apart.
#[derive(Clone, Debug, PartialEq)]
pub struct EngineUsage {
    pub id: String,
    pub name: String,
    pub util: f64,
}

/// A process's or a group's GPU figure, its busiest engine, and the sparse
/// engine contributions behind it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GpuUsage {
    pub gpu: f64,
    pub engine: Option<String>,
    pub engines: Vec<EngineUsage>,
}

impl GpuUsage {
    /// A process's reading as the snapshot reports it; a missing figure is not finite.
    pub fn of_process(process: &Value) -> Self {
        Self {
            gpu: process["gpu"].as_f64().unwrap_or(f64::NAN),
            engine: process["gpuEngine"].as_str().map(str::to_owned),
            engines: values(&process["gpuEngines"])
                .iter()
                .map(|engine| EngineUsage {
                    id: text(engine, "id").to_owned(),
                    name: text(engine, "name").to_owned(),
                    util: engine["util"].as_f64().unwrap_or(f64::NAN),
                })
                .collect(),
        }
    }
}

/// The busiest physical engine across `rows`, which may themselves be aggregates.
/// A row without engine contributions counts on its own; invalid readings count as none.
pub fn aggregate<U: Borrow<GpuUsage>>(rows: impl IntoIterator<Item = U>) -> GpuUsage {
    let mut engines: Vec<EngineUsage> = Vec::new();
    let mut unknown = 0f64;
    for row in rows {
        let row = row.borrow();
        let labeled;
        let list: &[EngineUsage] = if !row.engines.is_empty() {
            &row.engines
        } else if let Some(name) = row.engine.as_deref().filter(|name| !name.is_empty()) {
            // Older readings name only the busiest engine.
            labeled = [EngineUsage {
                id: format!("label:{name}"),
                name: name.to_owned(),
                util: row.gpu,
            }];
            &labeled
        } else {
            &[]
        };
        if list.is_empty() && row.gpu.is_finite() {
            unknown = unknown.max(row.gpu);
        }
        for engine in list {
            if !engine.util.is_finite() || engine.util <= 0. {
                continue;
            }
            // Few physical engines exist; a linear search beats hashing here.
            match engines.iter_mut().find(|known| known.id == engine.id) {
                Some(known) => {
                    known.name.clone_from(&engine.name);
                    known.util = (known.util + engine.util).min(100.);
                }
                None => engines.push(EngineUsage {
                    util: engine.util.min(100.),
                    ..engine.clone()
                }),
            }
        }
    }
    let mut gpu = unknown.clamp(0., 100.);
    let mut engine = None;
    for candidate in &engines {
        if candidate.util > gpu {
            gpu = candidate.util;
            engine = Some(&candidate.name);
        }
    }
    GpuUsage {
        gpu,
        engine: engine.cloned(),
        engines,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn engine(id: &str, util: f64) -> EngineUsage {
        named(id, util, id)
    }
    fn named(id: &str, util: f64, name: &str) -> EngineUsage {
        EngineUsage {
            id: id.into(),
            name: name.into(),
            util,
        }
    }
    fn row(engines: Vec<EngineUsage>) -> GpuUsage {
        GpuUsage {
            gpu: engines.iter().map(|e| e.util).fold(0., f64::max),
            engine: None,
            engines,
        }
    }
    fn unknown(gpu: f64) -> GpuUsage {
        GpuUsage {
            gpu,
            ..GpuUsage::default()
        }
    }

    #[test]
    fn independent_engines_and_adapters_are_not_added_into_a_false_75_percent_group() {
        let result = aggregate([
            row(vec![engine("gpu0:0:0", 40.)]),
            row(vec![engine("gpu1:0:6", 35.)]),
        ]);
        assert_eq!(result.gpu, 40.);
        assert_eq!(result.engine.as_deref(), Some("gpu0:0:0"));
    }

    #[test]
    fn contributions_on_the_same_physical_node_add_and_saturate_at_100_percent() {
        let gpu = |a: f64, b: f64| {
            aggregate([
                row(vec![engine("gpu0:0:0", a)]),
                row(vec![engine("gpu0:0:0", b)]),
            ])
            .gpu
        };
        assert_eq!(gpu(40., 35.), 75.);
        assert_eq!(gpu(80., 35.), 100.);
    }

    #[test]
    fn a_process_secondary_engine_can_become_the_busiest_engine_of_its_group() {
        let result = aggregate([
            row(vec![engine("3d", 80.), engine("copy", 70.)]),
            row(vec![engine("copy", 40.)]),
        ]);
        assert_eq!(result.gpu, 100.);
        assert_eq!(result.engine.as_deref(), Some("copy"));
    }

    #[test]
    fn identical_engine_names_do_not_merge_different_nodes_or_physical_gpus() {
        let result = aggregate([
            row(vec![named("a:0:1", 40., "Copy")]),
            row(vec![named("a:0:2", 35., "Copy")]),
            row(vec![named("a:1:1", 20., "Copy")]),
        ]);
        assert_eq!(result.gpu, 40.);
        assert_eq!(result.engines.len(), 3);
    }

    #[test]
    fn section_aggregation_preserves_the_same_result_with_folded_application_groups() {
        let first = row(vec![engine("3d", 20.), engine("copy", 30.)]);
        let second = row(vec![engine("3d", 30.)]);
        let third = row(vec![engine("copy", 40.)]);
        let folded = aggregate([aggregate([&first, &second]), third.clone()]);
        assert_eq!(folded.gpu, aggregate([&first, &second, &third]).gpu);
        assert_eq!(folded.gpu, 70.);
    }

    #[test]
    fn unknown_and_invalid_engine_readings_do_not_create_invented_totals() {
        assert_eq!(aggregate([unknown(40.), unknown(35.)]).gpu, 40.);
        assert_eq!(
            aggregate([
                row(vec![engine("a", f64::NAN)]),
                row(vec![engine("a", f64::INFINITY)]),
                unknown(-5.),
            ])
            .gpu,
            0.,
        );
    }

    #[test]
    fn snapshot_processes_read_their_engines_or_the_engine_label() {
        let process = serde_json::json!({
            "gpu": 30.,
            "gpuEngine": "GPU 0 - 3D",
            "gpuEngines": [{"id": "1:0:0", "name": "GPU 0 - 3D", "util": 30.}],
        });
        let reading = GpuUsage::of_process(&process);
        assert_eq!(reading.engines, [named("1:0:0", 30., "GPU 0 - 3D")]);
        // Without contributions the engine label stands for the reading.
        let labeled = serde_json::json!({"gpu": 40., "gpuEngine": "GPU 0 - 3D"});
        let result = aggregate([
            GpuUsage::of_process(&labeled),
            GpuUsage::of_process(&labeled),
        ]);
        assert_eq!(
            (result.gpu, result.engine.as_deref()),
            (80., Some("GPU 0 - 3D"))
        );
        assert_eq!(
            aggregate([GpuUsage::of_process(&serde_json::json!({}))]).gpu,
            0.
        );
    }
}
