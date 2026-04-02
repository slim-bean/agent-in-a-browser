#![allow(dead_code, unused_variables)]
//! WASM shim for cpal — audio capture and playback via WIT `host:browser/audio` interface.
//!
//! Provides the subset of the `cpal` API used by `voice.rs` and `audio_device.rs`
//! in the upstream Codex TUI. Each method delegates to registered handler functions
//! that call the `host:browser/audio@0.1.0` WIT imports.
//!
//! The handler pattern matches wasi-arboard and wasi-webbrowser: the codex-wasm-tui
//! wrapper registers handlers in `ensure_initialized()` before running the TUI.

use std::fmt;
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// Handler function types
// ---------------------------------------------------------------------------

type ListDevicesFn = fn(bool) -> Result<Vec<String>, String>;
type DefaultConfigFn = fn(bool) -> Result<(u32, u16, u8), String>;
type StartCaptureFn = fn(Option<&str>, u32, u16) -> Result<u32, String>;
type ReadCaptureDataFn = fn(u32) -> Result<Vec<u8>, String>;
type GetCapturePeakFn = fn(u32) -> u16;
type StopCaptureFn = fn(u32) -> Result<(), String>;
type StartPlaybackFn = fn(Option<&str>, u32, u16) -> Result<u32, String>;
type EnqueuePlaybackFn = fn(u32, &[u8]) -> Result<(), String>;
type ClearPlaybackFn = fn(u32) -> Result<(), String>;
type StopPlaybackFn = fn(u32) -> Result<(), String>;

static LIST_DEVICES_HANDLER: Mutex<Option<ListDevicesFn>> = Mutex::new(None);
static DEFAULT_CONFIG_HANDLER: Mutex<Option<DefaultConfigFn>> = Mutex::new(None);
static START_CAPTURE_HANDLER: Mutex<Option<StartCaptureFn>> = Mutex::new(None);
static READ_CAPTURE_DATA_HANDLER: Mutex<Option<ReadCaptureDataFn>> = Mutex::new(None);
static GET_CAPTURE_PEAK_HANDLER: Mutex<Option<GetCapturePeakFn>> = Mutex::new(None);
static STOP_CAPTURE_HANDLER: Mutex<Option<StopCaptureFn>> = Mutex::new(None);
static START_PLAYBACK_HANDLER: Mutex<Option<StartPlaybackFn>> = Mutex::new(None);
static ENQUEUE_PLAYBACK_HANDLER: Mutex<Option<EnqueuePlaybackFn>> = Mutex::new(None);
static CLEAR_PLAYBACK_HANDLER: Mutex<Option<ClearPlaybackFn>> = Mutex::new(None);
static STOP_PLAYBACK_HANDLER: Mutex<Option<StopPlaybackFn>> = Mutex::new(None);

/// Register the handler for listing devices (is_input: true=input, false=output).
pub fn set_list_devices_handler(handler: ListDevicesFn) {
    *LIST_DEVICES_HANDLER.lock().unwrap_or_else(|e| e.into_inner()) = Some(handler);
}

/// Register the handler for getting default config (is_input: true=input, false=output).
pub fn set_default_config_handler(handler: DefaultConfigFn) {
    *DEFAULT_CONFIG_HANDLER.lock().unwrap_or_else(|e| e.into_inner()) = Some(handler);
}

/// Register the handler for starting audio capture.
pub fn set_start_capture_handler(handler: StartCaptureFn) {
    *START_CAPTURE_HANDLER.lock().unwrap_or_else(|e| e.into_inner()) = Some(handler);
}

/// Register the handler for reading captured audio data.
pub fn set_read_capture_data_handler(handler: ReadCaptureDataFn) {
    *READ_CAPTURE_DATA_HANDLER.lock().unwrap_or_else(|e| e.into_inner()) = Some(handler);
}

/// Register the handler for getting capture peak level.
pub fn set_get_capture_peak_handler(handler: GetCapturePeakFn) {
    *GET_CAPTURE_PEAK_HANDLER.lock().unwrap_or_else(|e| e.into_inner()) = Some(handler);
}

/// Register the handler for stopping capture.
pub fn set_stop_capture_handler(handler: StopCaptureFn) {
    *STOP_CAPTURE_HANDLER.lock().unwrap_or_else(|e| e.into_inner()) = Some(handler);
}

/// Register the handler for starting playback.
pub fn set_start_playback_handler(handler: StartPlaybackFn) {
    *START_PLAYBACK_HANDLER.lock().unwrap_or_else(|e| e.into_inner()) = Some(handler);
}

/// Register the handler for enqueuing playback data.
pub fn set_enqueue_playback_handler(handler: EnqueuePlaybackFn) {
    *ENQUEUE_PLAYBACK_HANDLER.lock().unwrap_or_else(|e| e.into_inner()) = Some(handler);
}

/// Register the handler for clearing playback buffer.
pub fn set_clear_playback_handler(handler: ClearPlaybackFn) {
    *CLEAR_PLAYBACK_HANDLER.lock().unwrap_or_else(|e| e.into_inner()) = Some(handler);
}

/// Register the handler for stopping playback.
pub fn set_stop_playback_handler(handler: StopPlaybackFn) {
    *STOP_PLAYBACK_HANDLER.lock().unwrap_or_else(|e| e.into_inner()) = Some(handler);
}

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct BuildStreamError(String);

impl fmt::Display for BuildStreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for BuildStreamError {}

#[derive(Debug)]
pub struct PlayStreamError(String);

impl fmt::Display for PlayStreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for PlayStreamError {}

#[derive(Debug)]
pub struct PauseStreamError(String);

impl fmt::Display for PauseStreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for PauseStreamError {}

#[derive(Debug)]
pub struct DeviceNameError;

impl fmt::Display for DeviceNameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "failed to get device name")
    }
}

impl std::error::Error for DeviceNameError {}

#[derive(Debug)]
pub struct DevicesError;

impl fmt::Display for DevicesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "failed to enumerate devices")
    }
}

impl std::error::Error for DevicesError {}

#[derive(Debug)]
pub struct DefaultStreamConfigError;

impl fmt::Display for DefaultStreamConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "failed to get default stream config")
    }
}

impl std::error::Error for DefaultStreamConfigError {}

#[derive(Debug)]
pub struct SupportedStreamConfigsError;

impl fmt::Display for SupportedStreamConfigsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "failed to enumerate supported stream configs")
    }
}

impl std::error::Error for SupportedStreamConfigsError {}

// ---------------------------------------------------------------------------
// SampleFormat
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SampleFormat {
    I16,
    U16,
    F32,
}

impl fmt::Display for SampleFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SampleFormat::I16 => write!(f, "i16"),
            SampleFormat::U16 => write!(f, "u16"),
            SampleFormat::F32 => write!(f, "f32"),
        }
    }
}

impl SampleFormat {
    fn from_u8(v: u8) -> Self {
        match v {
            0 => SampleFormat::I16,
            1 => SampleFormat::U16,
            _ => SampleFormat::F32,
        }
    }
}

// ---------------------------------------------------------------------------
// SampleRate
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SampleRate(pub u32);

// ---------------------------------------------------------------------------
// StreamConfig / SupportedStreamConfig / SupportedStreamConfigRange
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct StreamConfig {
    pub sample_rate: SampleRate,
    pub channels: u16,
}

#[derive(Debug, Clone)]
pub struct SupportedStreamConfig {
    sample_rate: SampleRate,
    channels: u16,
    sample_format: SampleFormat,
}

impl SupportedStreamConfig {
    pub fn sample_rate(&self) -> SampleRate {
        self.sample_rate
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    pub fn sample_format(&self) -> SampleFormat {
        self.sample_format
    }
}

impl From<SupportedStreamConfig> for StreamConfig {
    fn from(c: SupportedStreamConfig) -> Self {
        StreamConfig {
            sample_rate: c.sample_rate,
            channels: c.channels,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SupportedStreamConfigRange {
    channels: u16,
    min_sample_rate: SampleRate,
    max_sample_rate: SampleRate,
    sample_format: SampleFormat,
}

impl SupportedStreamConfigRange {
    pub fn channels(&self) -> u16 {
        self.channels
    }

    pub fn min_sample_rate(&self) -> SampleRate {
        self.min_sample_rate
    }

    pub fn max_sample_rate(&self) -> SampleRate {
        self.max_sample_rate
    }

    pub fn sample_format(&self) -> SampleFormat {
        self.sample_format
    }

    pub fn with_sample_rate(self, sample_rate: SampleRate) -> SupportedStreamConfig {
        SupportedStreamConfig {
            sample_rate,
            channels: self.channels,
            sample_format: self.sample_format,
        }
    }
}

// ---------------------------------------------------------------------------
// InputCallbackInfo / OutputCallbackInfo
// ---------------------------------------------------------------------------

pub struct InputCallbackInfo;
pub struct OutputCallbackInfo;

// ---------------------------------------------------------------------------
// Data (for callback data parameter)
// ---------------------------------------------------------------------------

pub struct Data;

// ---------------------------------------------------------------------------
// Stream
// ---------------------------------------------------------------------------

/// Active audio stream handle.
///
/// For capture streams, the stream accumulates PCM data on the JS side.
/// A polling mechanism (via `poll_audio`) drains this data and feeds it
/// to the stored data callback.
///
/// For playback streams, the output data callback is stored and polled
/// periodically via `poll_audio()` to drain the upstream queue and push
/// audio to the browser.
pub struct Stream {
    stream_id: u32,
    is_input: bool,
}

impl Drop for Stream {
    fn drop(&mut self) {
        if self.is_input {
            // Clear capture callback/stream ID
            *CAPTURE_CALLBACK
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = None;
            *CAPTURE_STREAM_ID
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = None;
            let handler = STOP_CAPTURE_HANDLER
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Some(f) = *handler {
                drop(handler);
                let _ = f(self.stream_id);
            }
        } else {
            // Clear output callback/stream ID
            *OUTPUT_CALLBACK
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = None;
            *OUTPUT_STREAM_ID
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = None;
            let handler = STOP_PLAYBACK_HANDLER
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Some(f) = *handler {
                drop(handler);
                let _ = f(self.stream_id);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Capture polling
// ---------------------------------------------------------------------------

/// Global storage for the capture data callback.
/// When `poll_audio()` is called, we drain captured data from the JS side
/// and feed it to this callback.
static CAPTURE_CALLBACK: Mutex<Option<Box<dyn FnMut(&[f32], &InputCallbackInfo) + Send>>> =
    Mutex::new(None);
static CAPTURE_STREAM_ID: Mutex<Option<u32>> = Mutex::new(None);

/// Global storage for the output stream callback and player ID.
/// When `poll_audio()` is called, we invoke the output callback to drain
/// the upstream queue, then push the resulting audio to the browser.
static OUTPUT_CALLBACK: Mutex<Option<Box<dyn FnMut(&mut [f32], &OutputCallbackInfo) + Send>>> =
    Mutex::new(None);
static OUTPUT_STREAM_ID: Mutex<Option<u32>> = Mutex::new(None);

/// Number of f32 samples to request per output poll cycle.
/// 1024 samples at 48kHz ≈ 21ms, which is fine for voice playback.
const OUTPUT_POLL_SAMPLES: usize = 1024;

/// Poll for captured audio data and feed it to the stored callback.
/// Also drain the output queue and push audio to the browser for playback.
///
/// This should be called periodically from the WASM event loop (e.g., from
/// the tokio block_on yield point).
pub fn poll_audio() {
    poll_capture();
    poll_playback();
}

fn poll_capture() {
    let stream_id = {
        let guard = CAPTURE_STREAM_ID
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        match *guard {
            Some(id) => id,
            None => return,
        }
    };

    let handler = READ_CAPTURE_DATA_HANDLER
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let read_fn = match *handler {
        Some(f) => f,
        None => return,
    };
    drop(handler);

    let data = match read_fn(stream_id) {
        Ok(d) if !d.is_empty() => d,
        _ => return,
    };

    // Convert 16-bit LE PCM bytes to f32 samples
    let mut samples = Vec::with_capacity(data.len() / 2);
    for pair in data.chunks_exact(2) {
        let sample_i16 = i16::from_le_bytes([pair[0], pair[1]]);
        samples.push(sample_i16 as f32 / i16::MAX as f32);
    }

    let mut cb = CAPTURE_CALLBACK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(ref mut callback) = *cb {
        callback(&samples, &InputCallbackInfo);
    }
}

fn poll_playback() {
    let player_id = {
        let guard = OUTPUT_STREAM_ID
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        match *guard {
            Some(id) => id,
            None => return,
        }
    };

    // Call the output callback to fill a buffer from the upstream queue.
    let mut buf = vec![0.0f32; OUTPUT_POLL_SAMPLES];
    let has_data;
    {
        let mut cb = OUTPUT_CALLBACK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(ref mut callback) = *cb {
            callback(&mut buf, &OutputCallbackInfo);
            // Check if any non-zero samples were produced
            has_data = buf.iter().any(|&s| s != 0.0);
        } else {
            return;
        }
    }

    if !has_data {
        return;
    }

    // Convert f32 samples to i16 LE bytes and push to browser
    let mut bytes = Vec::with_capacity(buf.len() * 2);
    for &sample in &buf {
        let i16_sample = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        bytes.extend_from_slice(&i16_sample.to_le_bytes());
    }

    let handler = ENQUEUE_PLAYBACK_HANDLER
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(f) = *handler {
        drop(handler);
        let _ = f(player_id, &bytes);
    }
}

// ---------------------------------------------------------------------------
// Device
// ---------------------------------------------------------------------------

pub struct Device {
    name: String,
    is_input: bool,
}

// ---------------------------------------------------------------------------
// Host
// ---------------------------------------------------------------------------

pub struct Host;

/// Create the default audio host.
pub fn default_host() -> Host {
    Host
}

// ---------------------------------------------------------------------------
// Traits
// ---------------------------------------------------------------------------

pub mod traits {
    use super::*;

    pub trait HostTrait {
        type Device: DeviceTrait;
        type Devices: Iterator<Item = Self::Device>;

        fn default_input_device(&self) -> Option<Device>;
        fn default_output_device(&self) -> Option<Device>;
        fn input_devices(&self) -> Result<Self::Devices, DevicesError>;
        fn output_devices(&self) -> Result<Self::Devices, DevicesError>;
    }

    pub trait DeviceTrait {
        type SupportedInputConfigs: Iterator<Item = SupportedStreamConfigRange>;
        type SupportedOutputConfigs: Iterator<Item = SupportedStreamConfigRange>;

        fn name(&self) -> Result<String, DeviceNameError>;

        fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError>;
        fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError>;

        fn supported_input_configs(
            &self,
        ) -> Result<Self::SupportedInputConfigs, SupportedStreamConfigsError>;
        fn supported_output_configs(
            &self,
        ) -> Result<Self::SupportedOutputConfigs, SupportedStreamConfigsError>;

        fn build_input_stream<T, D, E>(
            &self,
            config: &StreamConfig,
            data_callback: D,
            error_callback: E,
            timeout: Option<std::time::Duration>,
        ) -> Result<Stream, BuildStreamError>
        where
            T: SizedSample,
            D: FnMut(&[T], &InputCallbackInfo) + Send + 'static,
            E: FnMut(StreamError) + Send + 'static;

        fn build_output_stream<T, D, E>(
            &self,
            config: &StreamConfig,
            data_callback: D,
            error_callback: E,
            timeout: Option<std::time::Duration>,
        ) -> Result<Stream, BuildStreamError>
        where
            T: SizedSample,
            D: FnMut(&mut [T], &OutputCallbackInfo) + Send + 'static,
            E: FnMut(StreamError) + Send + 'static;
    }

    pub trait StreamTrait {
        fn play(&self) -> Result<(), PlayStreamError>;
        fn pause(&self) -> Result<(), PauseStreamError>;
    }
}

// ---------------------------------------------------------------------------
// SizedSample trait
// ---------------------------------------------------------------------------

pub trait SizedSample: Copy + Send + 'static {}
impl SizedSample for f32 {}
impl SizedSample for i16 {}
impl SizedSample for u16 {}

// ---------------------------------------------------------------------------
// StreamError
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct StreamError(String);

impl fmt::Display for StreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for StreamError {}

// ---------------------------------------------------------------------------
// Host implementation
// ---------------------------------------------------------------------------

impl traits::HostTrait for Host {
    type Device = Device;
    type Devices = std::vec::IntoIter<Device>;

    fn default_input_device(&self) -> Option<Device> {
        // Return a device with empty name (signals "default") if listing succeeds
        let handler = LIST_DEVICES_HANDLER
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(f) = *handler {
            drop(handler);
            if let Ok(devices) = f(true) {
                if let Some(name) = devices.into_iter().next() {
                    return Some(Device {
                        name,
                        is_input: true,
                    });
                }
            }
        }
        // Return a default device even if listing fails — the browser will use
        // the default input device when no deviceId is specified.
        Some(Device {
            name: String::new(),
            is_input: true,
        })
    }

    fn default_output_device(&self) -> Option<Device> {
        let handler = LIST_DEVICES_HANDLER
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(f) = *handler {
            drop(handler);
            if let Ok(devices) = f(false) {
                if let Some(name) = devices.into_iter().next() {
                    return Some(Device {
                        name,
                        is_input: false,
                    });
                }
            }
        }
        Some(Device {
            name: String::new(),
            is_input: false,
        })
    }

    fn input_devices(&self) -> Result<std::vec::IntoIter<Device>, DevicesError> {
        let handler = LIST_DEVICES_HANDLER
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        match *handler {
            Some(f) => {
                drop(handler);
                f(true)
                    .map(|names| {
                        names
                            .into_iter()
                            .map(|name| Device {
                                name,
                                is_input: true,
                            })
                            .collect::<Vec<_>>()
                            .into_iter()
                    })
                    .map_err(|_| DevicesError)
            }
            None => Err(DevicesError),
        }
    }

    fn output_devices(&self) -> Result<std::vec::IntoIter<Device>, DevicesError> {
        let handler = LIST_DEVICES_HANDLER
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        match *handler {
            Some(f) => {
                drop(handler);
                f(false)
                    .map(|names| {
                        names
                            .into_iter()
                            .map(|name| Device {
                                name,
                                is_input: false,
                            })
                            .collect::<Vec<_>>()
                            .into_iter()
                    })
                    .map_err(|_| DevicesError)
            }
            None => Err(DevicesError),
        }
    }
}

// ---------------------------------------------------------------------------
// Device implementation
// ---------------------------------------------------------------------------

impl traits::DeviceTrait for Device {
    type SupportedInputConfigs = std::vec::IntoIter<SupportedStreamConfigRange>;
    type SupportedOutputConfigs = std::vec::IntoIter<SupportedStreamConfigRange>;

    fn name(&self) -> Result<String, DeviceNameError> {
        Ok(self.name.clone())
    }

    fn default_input_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        let handler = DEFAULT_CONFIG_HANDLER
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        match *handler {
            Some(f) => {
                drop(handler);
                f(true)
                    .map(|(rate, channels, fmt)| SupportedStreamConfig {
                        sample_rate: SampleRate(rate),
                        channels,
                        sample_format: SampleFormat::from_u8(fmt),
                    })
                    .map_err(|_| DefaultStreamConfigError)
            }
            None => {
                // Sensible default for browser audio: 24kHz mono F32
                Ok(SupportedStreamConfig {
                    sample_rate: SampleRate(24_000),
                    channels: 1,
                    sample_format: SampleFormat::F32,
                })
            }
        }
    }

    fn default_output_config(&self) -> Result<SupportedStreamConfig, DefaultStreamConfigError> {
        let handler = DEFAULT_CONFIG_HANDLER
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        match *handler {
            Some(f) => {
                drop(handler);
                f(false)
                    .map(|(rate, channels, fmt)| SupportedStreamConfig {
                        sample_rate: SampleRate(rate),
                        channels,
                        sample_format: SampleFormat::from_u8(fmt),
                    })
                    .map_err(|_| DefaultStreamConfigError)
            }
            None => {
                // Sensible default for browser audio: 48kHz stereo F32
                Ok(SupportedStreamConfig {
                    sample_rate: SampleRate(48_000),
                    channels: 2,
                    sample_format: SampleFormat::F32,
                })
            }
        }
    }

    fn supported_input_configs(
        &self,
    ) -> Result<std::vec::IntoIter<SupportedStreamConfigRange>, SupportedStreamConfigsError> {
        // Browser audio typically supports a range of sample rates.
        // Return a single range covering common rates.
        Ok(vec![SupportedStreamConfigRange {
            channels: 1,
            min_sample_rate: SampleRate(8_000),
            max_sample_rate: SampleRate(48_000),
            sample_format: SampleFormat::F32,
        }]
        .into_iter())
    }

    fn supported_output_configs(
        &self,
    ) -> Result<std::vec::IntoIter<SupportedStreamConfigRange>, SupportedStreamConfigsError> {
        Ok(vec![SupportedStreamConfigRange {
            channels: 2,
            min_sample_rate: SampleRate(8_000),
            max_sample_rate: SampleRate(48_000),
            sample_format: SampleFormat::F32,
        }]
        .into_iter())
    }

    fn build_input_stream<T, D, E>(
        &self,
        config: &StreamConfig,
        mut data_callback: D,
        error_callback: E,
        timeout: Option<std::time::Duration>,
    ) -> Result<Stream, BuildStreamError>
    where
        T: SizedSample,
        D: FnMut(&[T], &InputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let handler = START_CAPTURE_HANDLER
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let start_fn = match *handler {
            Some(f) => f,
            None => return Err(BuildStreamError("no capture handler registered".into())),
        };
        drop(handler);

        let device_name = if self.name.is_empty() {
            None
        } else {
            Some(self.name.as_str())
        };

        let capture_id = start_fn(device_name, config.sample_rate.0, config.channels)
            .map_err(BuildStreamError)?;

        // Store the data callback for polling.
        // We wrap it to handle f32 data (the JS side always provides f32-convertible data).
        // The callback type is generic over T, but we always receive f32 from the JS side.
        // For f32 callbacks, we pass data directly.
        // For i16/u16 callbacks, we convert.
        let sample_format = std::any::TypeId::of::<T>();
        let f32_id = std::any::TypeId::of::<f32>();
        let i16_id = std::any::TypeId::of::<i16>();
        let u16_id = std::any::TypeId::of::<u16>();

        // Create a wrapper that converts f32 samples to the expected type
        let wrapper: Box<dyn FnMut(&[f32], &InputCallbackInfo) + Send> =
            if sample_format == f32_id {
                // T is f32, cast directly
                // Safety: We verified T == f32 via TypeId
                Box::new(move |data: &[f32], info: &InputCallbackInfo| {
                    let ptr = data.as_ptr() as *const T;
                    let slice = unsafe { std::slice::from_raw_parts(ptr, data.len()) };
                    data_callback(slice, info);
                })
            } else if sample_format == i16_id {
                // T is i16, convert from f32
                Box::new(move |data: &[f32], info: &InputCallbackInfo| {
                    let converted: Vec<i16> = data
                        .iter()
                        .map(|&s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                        .collect();
                    let ptr = converted.as_ptr() as *const T;
                    let slice =
                        unsafe { std::slice::from_raw_parts(ptr, converted.len()) };
                    data_callback(slice, info);
                })
            } else {
                // T is u16, convert from f32
                Box::new(move |data: &[f32], info: &InputCallbackInfo| {
                    let converted: Vec<u16> = data
                        .iter()
                        .map(|&s| {
                            ((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i32 + 32768)
                                .clamp(0, u16::MAX as i32)
                                as u16
                        })
                        .collect();
                    let ptr = converted.as_ptr() as *const T;
                    let slice =
                        unsafe { std::slice::from_raw_parts(ptr, converted.len()) };
                    data_callback(slice, info);
                })
            };

        *CAPTURE_CALLBACK.lock().unwrap_or_else(|e| e.into_inner()) = Some(wrapper);
        *CAPTURE_STREAM_ID.lock().unwrap_or_else(|e| e.into_inner()) = Some(capture_id);

        Ok(Stream {
            stream_id: capture_id,
            is_input: true,
        })
    }

    fn build_output_stream<T, D, E>(
        &self,
        config: &StreamConfig,
        mut data_callback: D,
        error_callback: E,
        timeout: Option<std::time::Duration>,
    ) -> Result<Stream, BuildStreamError>
    where
        T: SizedSample,
        D: FnMut(&mut [T], &OutputCallbackInfo) + Send + 'static,
        E: FnMut(StreamError) + Send + 'static,
    {
        let handler = START_PLAYBACK_HANDLER
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let start_fn = match *handler {
            Some(f) => f,
            None => return Err(BuildStreamError("no playback handler registered".into())),
        };
        drop(handler);

        let device_name = if self.name.is_empty() {
            None
        } else {
            Some(self.name.as_str())
        };

        let player_id = start_fn(device_name, config.sample_rate.0, config.channels)
            .map_err(BuildStreamError)?;

        // Store the output callback for polling.
        // The upstream code fills a VecDeque from enqueue_frame, and the callback
        // pulls from it. We periodically call this callback in poll_playback()
        // to drain the queue and push audio to the browser.
        let sample_format = std::any::TypeId::of::<T>();
        let f32_id = std::any::TypeId::of::<f32>();
        let i16_id = std::any::TypeId::of::<i16>();

        let wrapper: Box<dyn FnMut(&mut [f32], &OutputCallbackInfo) + Send> =
            if sample_format == f32_id {
                // T is f32
                Box::new(move |buf: &mut [f32], info: &OutputCallbackInfo| {
                    let ptr = buf.as_mut_ptr() as *mut T;
                    let slice = unsafe { std::slice::from_raw_parts_mut(ptr, buf.len()) };
                    data_callback(slice, info);
                })
            } else if sample_format == i16_id {
                // T is i16 — callback fills i16 buffer, we convert to f32
                Box::new(move |buf: &mut [f32], info: &OutputCallbackInfo| {
                    let mut i16_buf = vec![0i16; buf.len()];
                    let ptr = i16_buf.as_mut_ptr() as *mut T;
                    let slice = unsafe { std::slice::from_raw_parts_mut(ptr, buf.len()) };
                    data_callback(slice, info);
                    for (i, &sample) in i16_buf.iter().enumerate() {
                        buf[i] = sample as f32 / i16::MAX as f32;
                    }
                })
            } else {
                // T is u16 — callback fills u16 buffer, we convert to f32
                Box::new(move |buf: &mut [f32], info: &OutputCallbackInfo| {
                    let mut u16_buf = vec![0u16; buf.len()];
                    let ptr = u16_buf.as_mut_ptr() as *mut T;
                    let slice = unsafe { std::slice::from_raw_parts_mut(ptr, buf.len()) };
                    data_callback(slice, info);
                    for (i, &sample) in u16_buf.iter().enumerate() {
                        buf[i] = ((sample as i32 - 32768) as f32) / i16::MAX as f32;
                    }
                })
            };

        *OUTPUT_CALLBACK.lock().unwrap_or_else(|e| e.into_inner()) = Some(wrapper);
        *OUTPUT_STREAM_ID.lock().unwrap_or_else(|e| e.into_inner()) = Some(player_id);

        Ok(Stream {
            stream_id: player_id,
            is_input: false,
        })
    }
}

// ---------------------------------------------------------------------------
// Stream implementation
// ---------------------------------------------------------------------------

impl traits::StreamTrait for Stream {
    fn play(&self) -> Result<(), PlayStreamError> {
        // Capture starts immediately on start_capture; playback starts when
        // data is enqueued. play() is a no-op in the browser model.
        Ok(())
    }

    fn pause(&self) -> Result<(), PauseStreamError> {
        // Pause is not commonly used; treat as no-op.
        Ok(())
    }
}
