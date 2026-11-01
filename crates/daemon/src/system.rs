//! CPU, GPU and memory load for the side strip.
//!
//! The strip is one display, 80 by 380 pixels on the unit under test. It is
//! addressed through key 16. The daemon samples once a second and writes a new
//! image only when a value moved.
//!
//! Sources, all user mode and none of them a device driver:
//!
//! 1. CPU from `GetSystemTimes`, the delta between two samples.
//! 2. Memory from `GlobalMemoryStatusEx`.
//! 3. GPU from the PDH counter `\GPU Engine(*)\Utilization Percentage`. Every
//!    instance is one process on one engine, so the sum per engine and the
//!    busiest engine give the overall load.

use std::collections::HashMap;

use streamdock_core::image;
use streamdock_core::model;
use streamdock_core::proto;
use streamdock_render::meter::{self, Meter};

use windows_sys::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhOpenQueryW,
    PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
};
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::System::Threading::GetSystemTimes;

/// Key that addresses the side strip.
pub const STRIP_KEY: u8 = 16;

/// Time between two samples.
pub const SAMPLE_MS: u64 = 1000;

/// Smallest change of one value that is worth a new image.
pub const REDRAW_PERCENT: f32 = 1.0;

/// Colour of the CPU meter.
pub const CPU_COLOUR: [u8; 3] = [63, 140, 255];
/// Colour of the GPU meter.
pub const GPU_COLOUR: [u8; 3] = [80, 214, 140];
/// Colour of the memory meter.
pub const MEMORY_COLOUR: [u8; 3] = [255, 190, 80];

/// One set of readings, in percent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reading {
    pub cpu: f32,
    pub gpu: Option<f32>,
    pub memory: f32,
    pub used_bytes: u64,
    pub total_bytes: u64,
}

/// The sampler. It keeps the counters between calls.
pub struct Load {
    previous: Option<(u64, u64, u64)>,
    gpu: Option<Gpu>,
    last: Option<Reading>,
}

impl Default for Load {
    fn default() -> Self {
        Self::new()
    }
}

impl Load {
    /// A sampler with one open GPU query. A machine without GPU counters still
    /// works; the GPU meter then shows `--`.
    pub fn new() -> Self {
        Self {
            previous: None,
            gpu: Gpu::open(),
            last: None,
        }
    }

    /// Take one sample.
    pub fn sample(&mut self) -> Reading {
        let times = system_times();
        let cpu = match self.previous {
            Some(previous) => cpu_percent(previous, times),
            None => 0.0,
        };
        self.previous = Some(times);

        let (total_bytes, available) = memory();
        let used_bytes = total_bytes.saturating_sub(available);
        let memory = if total_bytes == 0 {
            0.0
        } else {
            used_bytes as f32 * 100.0 / total_bytes as f32
        };

        let gpu = self.gpu.as_ref().and_then(|gpu| gpu.percent());

        let reading = Reading {
            cpu,
            gpu,
            memory,
            used_bytes,
            total_bytes,
        };
        self.last = Some(reading);
        reading
    }

    /// The reading of the last sample, when there is one.
    pub fn last(&self) -> Option<Reading> {
        self.last
    }

    /// True when the machine has no GPU engine counters.
    pub fn gpu_missing(&self) -> bool {
        self.gpu.is_none()
    }
}

/// True when a value moved enough to draw a new image.
pub fn changed(before: &Reading, after: &Reading) -> bool {
    let moved = |a: f32, b: f32| (a - b).abs() >= REDRAW_PERCENT;
    moved(before.cpu, after.cpu)
        || moved(before.memory, after.memory)
        || match (before.gpu, after.gpu) {
            (Some(a), Some(b)) => moved(a, b),
            (None, None) => false,
            _ => true,
        }
}

/// Render and encode the strip image for one reading.
pub fn strip_jpeg(
    reading: &Reading,
    size: (u16, u16),
    background: [u8; 3],
    foreground: [u8; 3],
) -> Result<Vec<u8>, String> {
    let cpu = format!("{:.0}%", reading.cpu);
    let gpu = match reading.gpu {
        Some(value) => format!("{value:.0}%"),
        None => "--".to_string(),
    };
    let memory = format!("{:.0}%", reading.memory);

    let meters = [
        Meter {
            label: "CPU",
            value: &cpu,
            percent: Some(reading.cpu),
            colour: CPU_COLOUR,
        },
        Meter {
            label: "GPU",
            value: &gpu,
            percent: reading.gpu,
            colour: GPU_COLOUR,
        },
        Meter {
            label: "MEM",
            value: &memory,
            percent: Some(reading.memory),
            colour: MEMORY_COLOUR,
        },
    ];

    let upright = meter::strip(size, &meters, background, foreground);
    let rotated = image::rotate(&upright, model::ROTATION);
    image::encode_max(&rotated, proto::MAX_IMAGE_BYTES)
}

/// Idle, kernel and user time of the whole machine, in 100 ns units.
fn system_times() -> (u64, u64, u64) {
    let mut idle: i64 = 0;
    let mut kernel: i64 = 0;
    let mut user: i64 = 0;
    unsafe {
        GetSystemTimes(
            &mut idle as *mut i64 as *mut _,
            &mut kernel as *mut i64 as *mut _,
            &mut user as *mut i64 as *mut _,
        );
    }
    (idle as u64, kernel as u64, user as u64)
}

/// Busy share between two samples. Kernel time includes idle time.
fn cpu_percent(before: (u64, u64, u64), after: (u64, u64, u64)) -> f32 {
    let idle = after.0.saturating_sub(before.0);
    let kernel = after.1.saturating_sub(before.1);
    let user = after.2.saturating_sub(before.2);

    let total = kernel + user;
    if total == 0 {
        return 0.0;
    }
    let busy = total.saturating_sub(idle);
    (busy as f64 * 100.0 / total as f64) as f32
}

/// Total and available physical memory in bytes.
fn memory() -> (u64, u64) {
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    unsafe {
        GlobalMemoryStatusEx(&mut status);
    }
    (status.ullTotalPhys, status.ullAvailPhys)
}

/// The open PDH query for the GPU engines.
struct Gpu {
    query: PDH_HQUERY,
    counter: PDH_HCOUNTER,
}

impl Gpu {
    fn open() -> Option<Self> {
        let path = wide("\\GPU Engine(*)\\Utilization Percentage");
        unsafe {
            let mut query: PDH_HQUERY = std::ptr::null_mut();
            if PdhOpenQueryW(std::ptr::null(), 0, &mut query) != 0 {
                return None;
            }
            let mut counter: PDH_HCOUNTER = std::ptr::null_mut();
            if PdhAddEnglishCounterW(query, path.as_ptr(), 0, &mut counter) != 0 {
                return None;
            }
            if PdhCollectQueryData(query) != 0 {
                return None;
            }
            Some(Self { query, counter })
        }
    }

    /// The busiest engine, in percent.
    fn percent(&self) -> Option<f32> {
        let values = self.read();
        if values.is_empty() {
            return None;
        }
        let engines = engine_load(&values);
        engines
            .values()
            .cloned()
            .fold(None, |best: Option<f32>, value| Some(best.map_or(value, |b| b.max(value))))
    }

    /// One value per counter instance.
    fn read(&self) -> HashMap<String, f32> {
        let mut values = HashMap::new();
        unsafe {
            if PdhCollectQueryData(self.query) != 0 {
                return values;
            }
            let mut size = 0u32;
            let mut count = 0u32;
            let status = PdhGetFormattedCounterArrayW(
                self.counter,
                PDH_FMT_DOUBLE,
                &mut size,
                &mut count,
                std::ptr::null_mut(),
            );
            if status != PDH_MORE_DATA || size == 0 {
                return values;
            }
            let mut buffer = vec![0u8; size as usize];
            let items = buffer.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
            if PdhGetFormattedCounterArrayW(
                self.counter,
                PDH_FMT_DOUBLE,
                &mut size,
                &mut count,
                items,
            ) != 0
            {
                return values;
            }
            for index in 0..count as usize {
                let item = &*items.add(index);
                if item.FmtValue.CStatus != 0 {
                    continue;
                }
                let mut length = 0usize;
                while *item.szName.add(length) != 0 {
                    length += 1;
                }
                let name = String::from_utf16_lossy(std::slice::from_raw_parts(item.szName, length));
                values.insert(name, item.FmtValue.Anonymous.doubleValue as f32);
            }
        }
        values
    }
}

impl Drop for Gpu {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::System::Performance::PdhCloseQuery(self.query);
        }
    }
}

/// Sum the load of every process on the same engine.
///
/// An instance looks like
/// `pid_1234_luid_0x00000000_0x0000ABCD_phys_0_eng_0_engtype_3D`.
fn engine_load(values: &HashMap<String, f32>) -> HashMap<String, f32> {
    let mut engines: HashMap<String, f32> = HashMap::new();
    for (name, value) in values {
        let Some((_, tail)) = name.split_once("luid_") else {
            continue;
        };
        let parts: Vec<&str> = tail.split('_').collect();
        if parts.len() < 8 {
            continue;
        }
        let key = format!(
            "{}_{} phys {} eng {} {}",
            parts[0],
            parts[1],
            parts[3],
            parts[5],
            parts[7..].join(" ")
        );
        *engines.entry(key).or_insert(0.0) += value;
    }
    engines
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_time_uses_the_idle_part() {
        // 100 units of kernel and 100 of user, 100 of them idle: half busy.
        let before = (0, 0, 0);
        let after = (100, 150, 50);
        assert!((cpu_percent(before, after) - 50.0).abs() < 0.01);
    }

    #[test]
    fn a_longer_interval_gives_the_same_share() {
        let short = cpu_percent((0, 0, 0), (10, 15, 5));
        let long = cpu_percent((0, 0, 0), (100, 150, 50));
        assert!((short - long).abs() < 0.01);
    }

    #[test]
    fn an_idle_machine_is_zero() {
        assert_eq!(cpu_percent((0, 0, 0), (100, 100, 0)), 0.0);
    }

    #[test]
    fn two_equal_samples_give_zero() {
        assert_eq!(cpu_percent((5, 5, 5), (5, 5, 5)), 0.0);
    }

    #[test]
    fn engine_names_group_per_engine() {
        let mut values = HashMap::new();
        values.insert(
            "pid_100_luid_0x00000000_0x0000ABCD_phys_0_eng_0_engtype_3D".to_string(),
            10.0,
        );
        values.insert(
            "pid_200_luid_0x00000000_0x0000ABCD_phys_0_eng_0_engtype_3D".to_string(),
            15.0,
        );
        values.insert(
            "pid_100_luid_0x00000000_0x0000ABCD_phys_0_eng_7_engtype_VideoDecode".to_string(),
            5.0,
        );

        let engines = engine_load(&values);
        assert_eq!(engines.len(), 2);
        assert_eq!(
            engines["0x00000000_0x0000ABCD phys 0 eng 0 3D"],
            25.0,
            "two processes on the same engine add up"
        );
        assert_eq!(
            engines["0x00000000_0x0000ABCD phys 0 eng 7 VideoDecode"],
            5.0
        );
    }

    #[test]
    fn a_moved_value_asks_for_a_redraw() {
        let before = Reading {
            cpu: 10.0,
            gpu: Some(20.0),
            memory: 40.0,
            used_bytes: 0,
            total_bytes: 0,
        };
        let same = Reading {
            cpu: 10.4,
            gpu: Some(20.4),
            memory: 40.4,
            used_bytes: 0,
            total_bytes: 0,
        };
        let moved = Reading {
            cpu: 11.2,
            ..same
        };
        let lost = Reading { gpu: None, ..same };

        assert!(!changed(&before, &same));
        assert!(changed(&before, &moved));
        assert!(changed(&before, &lost));
        assert!(!changed(&before, &before));
    }

    #[test]
    fn the_strip_is_one_image_for_key_16() {
        let reading = Reading {
            cpu: 12.0,
            gpu: Some(34.0),
            memory: 56.0,
            used_bytes: 8_000_000_000,
            total_bytes: 16_000_000_000,
        };
        let jpeg = strip_jpeg(&reading, (96, 460), [20, 20, 24], [240, 240, 240]).unwrap();
        assert!(jpeg.len() > 500);
        assert!(jpeg.len() <= proto::MAX_IMAGE_BYTES);
        assert_eq!(&jpeg[..2], &[0xFF, 0xD8]);
        assert_eq!(STRIP_KEY, 16);
    }
}
