//! The side strip: what it shows and where the numbers come from.
//!
//! The strip is one display, 96 by 460 pixels on the unit under test. It is
//! addressed through key 16. The daemon samples once a second and writes a new
//! image only when a value moved.
//!
//! Sources, all user mode and none of them a device driver:
//!
//! 1. CPU from `GetSystemTimes`, the delta between two samples.
//! 2. Memory from `GlobalMemoryStatusEx`.
//! 3. Disk from `GetDiskFreeSpaceExW` on the Windows drive.
//! 4. GPU from the PDH counter `\GPU Engine(*)\Utilization Percentage`. Every
//!    instance is one process on one engine, so the sum per engine and the
//!    busiest engine give the overall load.
//! 5. The clock from `GetLocalTime`.

use std::collections::HashMap;
use std::sync::Arc;

use streamdock_core::image;
use streamdock_core::model;
use streamdock_core::proto;
use streamdock_ipc as ipc;
use streamdock_render as render;
use streamdock_render::strip::{self, Band};
use streamdock_store::profile::{ClockStyle, Metric, Profile, Slot};

use crate::panel::Panel;

use windows_sys::Win32::Foundation::SYSTEMTIME;
use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows_sys::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhOpenQueryW,
    PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
};
use windows_sys::Win32::System::SystemInformation::{GetLocalTime, GlobalMemoryStatusEx, MEMORYSTATUSEX};
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
/// Colour of the disk meter.
pub const DISK_COLOUR: [u8; 3] = [180, 140, 255];
/// Colour of the clock.
pub const CLOCK_COLOUR: [u8; 3] = [240, 240, 240];
/// Colour of the weather widget.
pub const WEATHER_COLOUR: [u8; 3] = [120, 200, 255];

/// Weekday names for the date line.
const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
/// Month names for the date line.
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// One set of load readings, in percent.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Reading {
    pub cpu: f32,
    pub gpu: Option<f32>,
    pub memory: f32,
    pub disk: Option<f32>,
    pub used_bytes: u64,
    pub total_bytes: u64,
}

/// What a weather widget shows. Filled by the weather cache.
#[derive(Debug, Clone, PartialEq)]
pub struct Weather {
    /// Temperature in the unit the profile asked for, already rounded.
    pub value: String,
    /// Short condition, such as `Cloudy`.
    pub text: String,
}

impl Weather {
    /// The band a weather widget draws before the first answer arrives.
    pub fn pending() -> Self {
        Self {
            value: "--".to_string(),
            text: "waiting".to_string(),
        }
    }
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
        let disk = disk_percent();

        let reading = Reading {
            cpu,
            gpu,
            memory,
            disk,
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
    let option_moved = |a: Option<f32>, b: Option<f32>| match (a, b) {
        (Some(a), Some(b)) => moved(a, b),
        (None, None) => false,
        _ => true,
    };

    moved(before.cpu, after.cpu)
        || moved(before.memory, after.memory)
        || option_moved(before.gpu, after.gpu)
        || option_moved(before.disk, after.disk)
}

/// One band, ready to draw.
struct Tip {
    label: String,
    value: String,
    subtitle: String,
    percent: Option<f32>,
    colour: [u8; 3],
}

/// Owns the strip: the sampler, the weather answers and what was drawn last.
pub struct Viewer {
    load: Load,
    weather: HashMap<String, Weather>,
    drawn: Option<Reading>,
    shape: Option<(Vec<Slot>, [u8; 3], [u8; 3])>,
    next: std::time::Instant,
    weather_dirty: bool,
}

impl Default for Viewer {
    fn default() -> Self {
        Self::new()
    }
}

impl Viewer {
    /// A viewer that samples at once, so the strip fills on start.
    pub fn new() -> Self {
        Self {
            load: Load::new(),
            weather: HashMap::new(),
            drawn: None,
            shape: None,
            next: std::time::Instant::now(),
            weather_dirty: false,
        }
    }

    /// True when the machine has no GPU engine counters.
    pub fn gpu_missing(&self) -> bool {
        self.load.gpu_missing()
    }

    /// Store a weather answer for one place and ask for a redraw.
    pub fn set_weather(&mut self, city: &str, weather: Weather) {
        self.weather.insert(city.to_string(), weather);
        self.weather_dirty = true;
    }

    /// Sample and redraw when due. True when the strip was written.
    pub fn tick(&mut self, panel: &Panel, profile: &Profile, shared: &Arc<ipc::Shared>) -> bool {
        if std::time::Instant::now() < self.next {
            return false;
        }
        self.next = std::time::Instant::now() + std::time::Duration::from_millis(SAMPLE_MS);

        let reading = self.load.sample();
        let palette = render::palette(&profile.theme).unwrap_or_default();
        let shape = (
            profile.screen.slots.clone(),
            palette.background,
            palette.foreground,
        );

        let moved = match self.drawn {
            None => true,
            Some(previous) => changed(&previous, &reading),
        };
        if !moved && !self.weather_dirty && self.shape.as_ref() == Some(&shape) {
            return false;
        }

        self.draw(panel, profile, &reading, &palette, shared);
        self.drawn = Some(reading);
        self.shape = Some(shape);
        self.weather_dirty = false;
        true
    }

    /// Draw at once, for a reconnect or a fresh profile.
    pub fn force(&mut self, panel: &Panel, profile: &Profile, shared: &Arc<ipc::Shared>) {
        let reading = match self.load.last() {
            Some(reading) => reading,
            None => self.load.sample(),
        };
        let palette = render::palette(&profile.theme).unwrap_or_default();
        self.draw(panel, profile, &reading, &palette, shared);
        self.drawn = Some(reading);
        self.shape = Some((
            profile.screen.slots.clone(),
            palette.background,
            palette.foreground,
        ));
        self.weather_dirty = false;
    }

    fn draw(
        &self,
        panel: &Panel,
        profile: &Profile,
        reading: &Reading,
        palette: &render::Palette,
        shared: &Arc<ipc::Shared>,
    ) {
        let size = (model::STRIP_WIDTH, model::STRIP_HEIGHT);
        match strip_images(
            profile,
            reading,
            &self.weather,
            size,
            palette.background,
            palette.foreground,
        ) {
            Ok((device, preview)) => {
                // The editor shows the same picture in its upright form.
                shared.set_strip(preview.into());
                panel.key(STRIP_KEY, device.into());
            }
            Err(e) => crate::log(format!("cannot draw the side strip: {e}")),
        }
    }
}

/// Render the strip twice: the bytes for the panel, and the upright copy for
/// the editor.
///
/// The panel shows every image rotated, so it gets the rotated copy. The
/// editor shows the upright copy.
pub fn strip_images(
    profile: &Profile,
    reading: &Reading,
    weather: &HashMap<String, Weather>,
    size: (u16, u16),
    background: [u8; 3],
    foreground: [u8; 3],
) -> Result<(Vec<u8>, Vec<u8>), String> {
    let tips = describe_slots(profile, reading, weather);

    let bands: Vec<Band> = tips
        .iter()
        .map(|tip| Band {
            label: &tip.label,
            value: &tip.value,
            subtitle: if tip.subtitle.is_empty() {
                None
            } else {
                Some(&tip.subtitle)
            },
            percent: tip.percent,
            colour: tip.colour,
        })
        .collect();

    let upright = strip::strip(
        size,
        &bands,
        profile.screen.text_scale,
        background,
        foreground,
    );
    let rotated = image::rotate(&upright, model::ROTATION);

    let device = image::encode_max(&rotated, proto::MAX_IMAGE_BYTES)?;
    let preview = image::encode_max(&upright, proto::MAX_IMAGE_BYTES)?;
    Ok((device, preview))
}

/// Build the three bands from the profile.
fn describe_slots(
    profile: &Profile,
    reading: &Reading,
    weather: &HashMap<String, Weather>,
) -> Vec<Tip> {
    let now = local_time();
    let mut tips = Vec::with_capacity(model::STRIP_SLOTS);

    for index in 0..model::STRIP_SLOTS {
        let slot = profile.screen.slots.get(index);
        tips.push(match slot {
            Some(Slot::Meter { metric }) => meter_tip(*metric, reading),
            Some(Slot::Clock { style }) => clock_tip(*style, &now),
            Some(Slot::Weather {
                city, units, ..
            }) => weather_tip(city, *units, weather),
            Some(Slot::Empty) | None => Tip {
                label: String::new(),
                value: String::new(),
                subtitle: String::new(),
                percent: None,
                colour: CLOCK_COLOUR,
            },
        });
    }

    tips
}

fn meter_tip(metric: Metric, reading: &Reading) -> Tip {
    let (label, value, colour) = match metric {
        Metric::Cpu => ("CPU", Some(reading.cpu), CPU_COLOUR),
        Metric::Gpu => ("GPU", reading.gpu, GPU_COLOUR),
        Metric::Memory => ("MEM", Some(reading.memory), MEMORY_COLOUR),
        Metric::Disk => ("DISK", reading.disk, DISK_COLOUR),
    };

    Tip {
        label: label.to_string(),
        value: match value {
            Some(percent) => format!("{percent:.0}%"),
            None => "--".to_string(),
        },
        subtitle: String::new(),
        percent: value,
        colour,
    }
}

fn clock_tip(style: ClockStyle, now: &(String, String)) -> Tip {
    let (time, date) = now;
    match style {
        ClockStyle::Time => Tip {
            label: String::new(),
            value: time.clone(),
            subtitle: String::new(),
            percent: None,
            colour: CLOCK_COLOUR,
        },
        ClockStyle::Date => Tip {
            label: String::new(),
            value: date.clone(),
            subtitle: String::new(),
            percent: None,
            colour: CLOCK_COLOUR,
        },
        ClockStyle::Both => Tip {
            label: String::new(),
            value: time.clone(),
            subtitle: date.clone(),
            percent: None,
            colour: CLOCK_COLOUR,
        },
    }
}

fn weather_tip(city: &str, units: streamdock_store::profile::Units, cache: &HashMap<String, Weather>) -> Tip {
    let entry = cache.get(city).cloned().unwrap_or_else(Weather::pending);
    let name: String = city.chars().take(12).collect::<String>().to_uppercase();
    let _ = units;
    Tip {
        label: name,
        value: entry.value,
        subtitle: entry.text,
        percent: None,
        colour: WEATHER_COLOUR,
    }
}

/// Local time and date, such as `14:05` and `Sat 01 Nov`.
fn local_time() -> (String, String) {
    let mut now: SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe {
        GetLocalTime(&mut now);
    }

    let time = format!("{:02}:{:02}", now.wHour, now.wMinute);
    let weekday = WEEKDAYS
        .get(now.wDayOfWeek as usize % WEEKDAYS.len())
        .unwrap_or(&"---");
    let month = MONTHS
        .get((now.wMonth.saturating_sub(1)) as usize % MONTHS.len())
        .unwrap_or(&"---");
    let date = format!("{weekday} {:02} {month}", now.wDay);
    (time, date)
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

/// Used share of the drive that holds Windows.
fn disk_percent() -> Option<f32> {
    let root = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_string());
    let path = wide(&format!("{root}\\"));
    let mut free_to_caller: u64 = 0;
    let mut total: u64 = 0;
    let mut free: u64 = 0;

    let ok = unsafe {
        GetDiskFreeSpaceExW(
            path.as_ptr(),
            &mut free_to_caller as *mut u64 as *mut _,
            &mut total as *mut u64 as *mut _,
            &mut free as *mut u64 as *mut _,
        )
    };
    if ok == 0 || total == 0 {
        return None;
    }
    Some((total - free) as f32 * 100.0 / total as f32)
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
            .fold(None, |best: Option<f32>, value| {
                Some(best.map_or(value, |b| b.max(value)))
            })
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
    use streamdock_store::profile::{Screen, Units};

    fn profile_with(slots: Vec<Slot>) -> Profile {
        let mut profile = Profile::new("Test");
        profile.screen = Screen {
            slots,
            text_scale: 1.0,
        };
        profile
    }

    fn reading() -> Reading {
        Reading {
            cpu: 12.0,
            gpu: Some(34.0),
            memory: 56.0,
            disk: Some(70.0),
            used_bytes: 8_000_000_000,
            total_bytes: 16_000_000_000,
        }
    }

    fn tips(profile: &Profile) -> Vec<Tip> {
        describe_slots(profile, &reading(), &HashMap::new())
    }

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
        let before = reading();
        let same = Reading {
            cpu: 12.4,
            gpu: Some(34.4),
            memory: 56.4,
            disk: Some(70.4),
            ..before
        };
        let moved = Reading {
            cpu: 13.2,
            ..same
        };
        let lost = Reading {
            gpu: None,
            ..same
        };

        assert!(!changed(&before, &same));
        assert!(changed(&before, &moved));
        assert!(changed(&before, &lost));
        assert!(!changed(&before, &before));
    }

    #[test]
    fn the_default_profile_shows_the_three_meters() {
        let profile = Profile::new("Test");
        let tips = tips(&profile);
        assert_eq!(tips.len(), model::STRIP_SLOTS);
        assert_eq!(tips[0].label, "CPU");
        assert_eq!(tips[0].value, "12%");
        assert_eq!(tips[1].label, "GPU");
        assert_eq!(tips[1].value, "34%");
        assert_eq!(tips[2].label, "MEM");
        assert_eq!(tips[2].value, "56%");
        assert!(tips[0].percent.is_some());
    }

    #[test]
    fn a_clock_band_has_no_bar() {
        let profile = profile_with(vec![
            Slot::Clock {
                style: ClockStyle::Both,
            },
            Slot::Empty,
            Slot::Empty,
        ]);
        let tips = tips(&profile);
        assert!(tips[0].percent.is_none());
        assert!(tips[0].value.contains(':'), "time like 14:05");
        assert!(!tips[0].subtitle.is_empty(), "a date under the time");
        assert_eq!(tips[1].value, "");
    }

    #[test]
    fn a_weather_band_waits_for_the_first_answer() {
        let profile = profile_with(vec![
            Slot::Weather {
                city: "Berlin".into(),
                latitude: 52.52,
                longitude: 13.405,
                units: Units::Metric,
            },
            Slot::Empty,
            Slot::Empty,
        ]);
        let tips = tips(&profile);
        assert_eq!(tips[0].label, "BERLIN");
        assert_eq!(tips[0].value, "--");
        assert!(tips[0].percent.is_none());
    }

    #[test]
    fn a_filled_weather_band_shows_the_temperature() {
        let profile = profile_with(vec![
            Slot::Weather {
                city: "Berlin".into(),
                latitude: 52.52,
                longitude: 13.405,
                units: Units::Metric,
            },
            Slot::Empty,
            Slot::Empty,
        ]);
        let mut cache = HashMap::new();
        cache.insert(
            "Berlin".to_string(),
            Weather {
                value: "18°".into(),
                text: "Cloudy".into(),
            },
        );
        let tips = describe_slots(&profile, &reading(), &cache);
        assert_eq!(tips[0].value, "18°");
        assert_eq!(tips[0].subtitle, "Cloudy");
    }

    #[test]
    fn the_strip_is_one_image_for_key_16() {
        let profile = Profile::new("Test");
        let (device, preview) = strip_images(
            &profile,
            &reading(),
            &HashMap::new(),
            (96, 460),
            [20, 20, 24],
            [240, 240, 240],
        )
        .unwrap();

        assert!(device.len() > 500);
        assert!(device.len() <= proto::MAX_IMAGE_BYTES);
        assert_eq!(&device[..2], &[0xFF, 0xD8]);

        // The preview is the upright picture: 96 wide and 460 tall.
        let upright = image::decode(&preview).expect("the preview is a JPEG");
        assert_eq!((upright.width, upright.height), (96, 460));

        // The panel copy is rotated for the device: 460 wide and 96 tall.
        let rotated = image::decode(&device).expect("the device copy is a JPEG");
        assert_eq!((rotated.width, rotated.height), (460, 96));

        assert_eq!(STRIP_KEY, 16);
    }
}
