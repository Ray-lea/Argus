//! The figures above the process columns' labels.
use super::*;

impl Desktop {
    pub(super) fn process_column_totals(
        &self,
        preferences: &columns::Preferences,
    ) -> HashMap<String, String> {
        if self.page != "processes" {
            return HashMap::new();
        }
        preferences
            .selected
            .iter()
            .map(|key| (key.clone(), self.column_total(key)))
            .collect()
    }
    /// The figure above a process column label.
    pub(in crate::frontend) fn column_total(&self, key: &str) -> String {
        let sys = &self.snapshot["system"];
        if sys.is_null() {
            return String::new();
        }
        let processes = values(&self.snapshot["processes"]);
        match key {
            "name" => {
                let total = tr!(
                    "processes.total",
                    count = number(sys, "processCount") as usize
                );
                // With system processes hidden, how many of them the list leaves out.
                let hidden = self.visibility_rules("processes").map_or(0, |rules| {
                    processes
                        .iter()
                        .filter(|process| rules.hides_process(process))
                        .count()
                });
                if hidden == 0 {
                    total
                } else {
                    tr!("processes.totalHidden", total = total, count = hidden)
                }
            }
            "cpu" => format!("{}%", number(sys, "cpu").round()),
            "mem" => format!(
                "{}%",
                (number(sys, "memUsed") / number(sys, "memTotal").max(1.) * 100.).round()
            ),
            "gpu" => {
                let gpus = values(&self.snapshot["hardware"]["gpus"]);
                if gpus.is_empty() {
                    String::new()
                } else {
                    format!(
                        "{}%",
                        gpus.iter()
                            .map(|g| number(g, "util"))
                            .fold(0., f64::max)
                            .round()
                    )
                }
            }
            "io" => theme::bytes(
                number(sys, "ioRead") + number(sys, "ioWrite") + number(sys, "ioOther"),
                true,
            ),
            "diskRead" | "diskWrite" => processes
                .iter()
                .filter_map(|p| p[key].as_f64())
                .reduce(|a, b| a + b)
                .map(charts::byte_rate)
                .unwrap_or_else(|| label("format.missing")),
            "net" => {
                let bits = self.prefs.get::<String>("netUnit", "bytes".into()) == "bits";
                let total = processes
                    .iter()
                    .map(|p| number(p, "netSent") + number(p, "netReceived"))
                    .sum::<f64>();
                if total < 1. {
                    "0".into()
                } else {
                    theme::network(total, bits)
                }
            }
            "threads" => theme::count(number(sys, "threadCount")),
            "handles" => theme::count(number(sys, "handleCount")),
            _ => String::new(),
        }
    }
}
