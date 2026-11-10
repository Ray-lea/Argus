//! Volume-manager counters for storage stacks that reject physical-disk counters.
use std::collections::HashMap;
use windows::Win32::Storage::FileSystem::GetDriveTypeW;
use windows::Win32::System::Ioctl::DISK_PERFORMANCE;
use windows::core::PCWSTR;

use super::{DiskInfo, Rates, access, performance, rates, volume_disks};
use crate::util::{from_wide, wide};

#[derive(Clone, Copy)]
pub(super) enum Reading {
    Physical(DISK_PERFORMANCE),
    Volume(DISK_PERFORMANCE),
}

pub(super) enum Batch {
    Physical(DISK_PERFORMANCE),
    Volumes(Vec<(String, DISK_PERFORMANCE)>),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Identity {
    manager: [u16; 8],
    number: u32,
}
impl From<&DISK_PERFORMANCE> for Identity {
    fn from(value: &DISK_PERFORMANCE) -> Self {
        Self {
            manager: value.StorageManagerName,
            number: value.StorageDeviceNumber,
        }
    }
}

#[derive(Default)]
pub(super) struct Counters {
    previous: HashMap<Identity, DISK_PERFORMANCE>,
}

pub(super) struct Sample {
    pub rates: Option<Rates>,
    pub volumes: Option<Vec<String>>,
}

impl Counters {
    pub fn clear(&mut self) {
        self.previous.clear();
    }

    pub fn update(&mut self, batch: Batch) -> Sample {
        let mut current = HashMap::new();
        let mut names = None;
        match batch {
            Batch::Physical(value) => {
                current.insert(Identity::from(&value), value);
            }
            Batch::Volumes(values) => {
                let mut volumes = Vec::new();
                for (name, value) in values {
                    // A volume exposed through two drive-letter aliases is one
                    // counter stream, not twice the I/O.
                    if let std::collections::hash_map::Entry::Vacant(entry) =
                        current.entry(Identity::from(&value))
                    {
                        entry.insert(value);
                        volumes.push(name);
                    }
                }
                volumes.sort();
                names = Some(volumes);
            }
        }
        let same = !current.is_empty()
            && current.len() == self.previous.len()
            && current.keys().all(|key| self.previous.contains_key(key));
        let measured = same
            .then(|| {
                current
                    .iter()
                    .map(|(key, value)| rates(&self.previous[key], value))
                    .collect::<Option<Vec<_>>>()
            })
            .flatten();
        let combined = measured.and_then(|values| {
            let mut total = Rates {
                active: None,
                read: 0,
                write: 0,
                io_time: 0.,
                transfers: 0,
            };
            if values.len() == 1 {
                total.active = values[0].active;
            }
            for value in values {
                total.read = total.read.checked_add(value.read)?;
                total.write = total.write.checked_add(value.write)?;
                total.transfers = total.transfers.checked_add(value.transfers)?;
                total.io_time += value.io_time;
            }
            Some(total)
        });
        self.previous = current;
        Sample {
            rates: combined,
            volumes: names,
        }
    }
}

/// A logical volume spanning several physical disks cannot be attributed to
/// any one of them. All represented volumes must have an unambiguous owner.
pub(super) fn exclusive_paths<'a>(
    disks: impl Iterator<Item = &'a DiskInfo>,
) -> HashMap<u32, Vec<String>> {
    let disks: Vec<_> = disks.collect();
    let mut owners = HashMap::<&str, usize>::new();
    for disk in &disks {
        for volume in &disk.volumes {
            *owners.entry(volume).or_default() += 1;
        }
    }
    disks
        .into_iter()
        .filter(|disk| {
            !disk.volumes.is_empty()
                && disk
                    .volumes
                    .iter()
                    .all(|volume| drive_letter(volume) && owners[volume.as_str()] == 1)
        })
        .map(|disk| (disk.index, disk.volumes.clone()))
        .collect()
}

fn drive_letter(value: &str) -> bool {
    value.len() == 2 && value.as_bytes()[0].is_ascii_uppercase() && value.as_bytes()[1] == b':'
}

pub(super) fn read_set(
    index: u32,
    paths: &[String],
    read: &mut impl FnMut(u32, &str) -> Result<Reading, u32>,
) -> Result<Batch, u32> {
    let mut values = Vec::new();
    let mut failure = None;
    for path in paths {
        match read(index, path) {
            Ok(Reading::Physical(value)) => return Ok(Batch::Physical(value)),
            Ok(Reading::Volume(value)) => values.push((path.clone(), value)),
            Err(code) => {
                failure.get_or_insert(code);
            }
        }
    }
    match failure {
        Some(code) => Err(code),
        None if !values.is_empty() => Ok(Batch::Volumes(values)),
        None => Err(13),
    }
}

pub(super) fn read(index: u32, volume: &str) -> Result<Reading, u32> {
    if !drive_letter(volume) {
        return Err(87);
    }
    let root = wide(&format!(r"{volume}\"));
    if !matches!(
        unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) },
        super::DRIVE_FIXED | super::DRIVE_REMOVABLE
    ) {
        return Err(13);
    }
    access::read_scoped(&format!(r"\\.\{volume}"), |handle| {
        // Validate the current mapping on the same open volume handle. Drive
        // letters can change between the inventory scan and this sample.
        if volume_disks(handle)? != [index] {
            return Err(13);
        }
        let value = performance(handle)?;
        classify(index, value)
    })
}

fn classify(index: u32, value: DISK_PERFORMANCE) -> Result<Reading, u32> {
    match from_wide(&value.StorageManagerName)
        .trim()
        .to_ascii_uppercase()
        .as_str()
    {
        "PARTMGR" | "PHYSDISK" if value.StorageDeviceNumber == index => {
            Ok(Reading::Physical(value))
        }
        "FTDISK" | "DMIO" | "VOLMGR" | "LOGIDISK" => Ok(Reading::Volume(value)),
        // Never label an unknown provider or another disk's counters as ours.
        _ => Err(13),
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::counters;
    use super::*;

    fn volume(number: u32, second: i64, bytes: i64) -> DISK_PERFORMANCE {
        let mut value = counters(second, bytes);
        value.StorageDeviceNumber = number;
        value
            .StorageManagerName
            .copy_from_slice(&wide("VOLMGR  ")[..8]);
        value.ReadTime = second * 20_000;
        value.WriteTime = second * 10_000;
        value
    }

    #[test]
    fn volume_aliases_are_counted_once_and_source_changes_reprime() {
        let mut source = Counters::default();
        source.update(Batch::Volumes(vec![
            ("E:".into(), volume(6, 1, 100)),
            ("F:".into(), volume(6, 1, 100)),
        ]));
        let sample = source.update(Batch::Volumes(vec![
            ("E:".into(), volume(6, 2, 300)),
            ("F:".into(), volume(6, 2, 300)),
        ]));
        assert_eq!(sample.volumes.unwrap().len(), 1);
        let rates = sample.rates.unwrap();
        assert_eq!(rates.read, 200);
        assert_eq!(rates.active, Some(50.));
        assert_eq!(rates.io_time / rates.transfers as f64 / 10_000., 1.5);
        assert!(
            source
                .update(Batch::Volumes(vec![("E:".into(), volume(7, 3, 20000))]))
                .rates
                .is_none()
        );
    }

    #[test]
    fn multiple_volumes_sum_io_without_inventing_physical_busy_time() {
        let mut source = Counters::default();
        let slower = |second, bytes| {
            let mut value = volume(7, second, bytes);
            value.ReadCount = (second * 3) as u32;
            value.ReadTime = second * 120_000;
            value.WriteTime = second * 80_000;
            value
        };
        source.update(Batch::Volumes(vec![
            ("E:".into(), volume(6, 1, 100)),
            ("F:".into(), slower(1, 200)),
        ]));
        let sample = source.update(Batch::Volumes(vec![
            ("E:".into(), volume(6, 2, 300)),
            ("F:".into(), slower(2, 700)),
        ]));
        let rates = sample.rates.unwrap();
        assert_eq!(rates.read, 700);
        assert_eq!(rates.write, 1400);
        assert!(rates.active.is_none());
        assert!((rates.io_time / rates.transfers as f64 / 10_000. - 23. / 6.).abs() < 1e-6);
        assert!(
            source
                .update(Batch::Volumes(vec![("E:".into(), volume(6, 3, 400))]))
                .rates
                .is_none()
        );
    }

    #[test]
    fn spanning_volumes_and_unrecognized_providers_are_not_misattributed() {
        let disk = |index, letter: &str| DiskInfo {
            index,
            model: String::new(),
            size: 100,
            bus: "nvme",
            ssd: Some(true),
            removable: false,
            volumes: vec![letter.into()],
        };
        let disks = [disk(0, "E:"), disk(1, "E:"), disk(2, "F:")];
        let paths = exclusive_paths(disks.iter());
        assert!(!paths.contains_key(&0) && !paths.contains_key(&1));
        assert_eq!(paths[&2], ["F:"]);
        assert!(classify(0, volume(6, 1, 10)).is_ok());
        let mut physical = volume(1, 1, 10);
        physical
            .StorageManagerName
            .copy_from_slice(&wide("Partmgr ")[..8]);
        assert!(classify(0, physical).is_err());
        assert!(matches!(classify(1, physical), Ok(Reading::Physical(_))));
        physical.StorageManagerName = [0; 8];
        assert!(classify(1, physical).is_err());
    }

    #[test]
    fn a_physical_provider_is_used_once_and_incomplete_volume_sets_stay_missing() {
        let paths = vec!["E:".into(), "F:".into()];
        let mut calls = 0;
        let batch = read_set(0, &paths, &mut |_, _| {
            calls += 1;
            Ok(Reading::Physical(counters(1, 100)))
        })
        .unwrap();
        assert!(matches!(batch, Batch::Physical(_)));
        assert_eq!(calls, 1);
        assert!(
            read_set(0, &paths, &mut |_, path| if path == "E:" {
                Ok(Reading::Volume(volume(6, 1, 10)))
            } else {
                Err(1)
            })
            .is_err()
        );
    }
}
