//! The computer's sound devices, for `{` (inputs) and `}` (outputs) mode:
//! listing them, making one the system default, and sounding a short test
//! tone through an output so you can tell which one it is before switching.
//!
//! Devices are named by the OS's own id for them — a CoreAudio object id on
//! macOS, an endpoint id string on Windows — carried as text so the rest of
//! the app never has to know which.

use std::f32::consts::TAU;
use std::sync::atomic::{AtomicU64, Ordering};

/// Which side of the sound system a device sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Input,
    Output,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
    /// The one the system uses right now.
    pub is_default: bool,
}

/// Every device that can currently record (`Input`) or play (`Output`), in
/// the order the OS lists them. Empty when the query fails.
pub fn list(direction: Direction) -> Vec<Device> {
    platform::list(direction).unwrap_or_else(|error| {
        log::warn!("listing audio devices failed: {error}");
        Vec::new()
    })
}

/// Make `id` the system's default device for `direction`.
pub fn set_default(direction: Direction, id: &str) -> Result<(), String> {
    platform::set_default(direction, id)
}

/// The test tone: an A440 short enough to sit between two arrow presses.
const TONE_HZ: f32 = 440.0;
const TONE_MS: u32 = 100;
/// Quiet enough not to startle on a pair of headphones turned up.
const TONE_LEVEL: f32 = 0.25;
/// Ramp in and out over this long, or the tone starts and ends on a click.
const FADE_MS: u32 = 5;

/// Bumped by every new tone. A tone still sounding when the next one starts
/// goes quiet, so arrowing quickly through the list never stacks them up.
static TONE_GENERATION: AtomicU64 = AtomicU64::new(0);

fn current_tone(generation: u64) -> bool {
    TONE_GENERATION.load(Ordering::Relaxed) == generation
}

/// Play the test tone through output device `id`, without waiting for it and
/// without touching which device is the default.
pub fn play_tone(id: &str) {
    let generation = TONE_GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    let id = id.to_string();
    std::thread::spawn(move || {
        if let Err(error) = platform::play_tone(&id, generation) {
            log::warn!("test tone on {id} failed: {error}");
        }
    });
}

/// How many frames the tone lasts at `rate` frames per second.
fn tone_frames(rate: f64) -> u64 {
    (rate * TONE_MS as f64 / 1000.0).round() as u64
}

/// Sample `frame` of the tone, or silence once it is over.
fn tone_sample(frame: u64, rate: f64) -> f32 {
    let total = tone_frames(rate);
    if frame >= total {
        return 0.0;
    }
    let fade = (rate * FADE_MS as f64 / 1000.0).max(1.0) as u64;
    let envelope = (frame.min(total - 1 - frame) as f32 / fade as f32).min(1.0);
    let t = frame as f64 / rate;
    (TAU * TONE_HZ * t as f32).sin() * TONE_LEVEL * envelope
}

#[cfg(target_os = "macos")]
mod platform {
    //! CoreAudio. The HAL lists devices, holds the default-device properties,
    //! and runs an IOProc straight on a device for the tone, so nothing above
    //! it (AudioToolbox, AVFoundation) is needed.

    use super::{current_tone, tone_frames, tone_sample, Device, Direction};
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::ptr::{null, NonNull};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use objc2_core_audio::{
        kAudioDevicePropertyNominalSampleRate, kAudioDevicePropertyStreamConfiguration,
        kAudioHardwarePropertyDefaultInputDevice, kAudioHardwarePropertyDefaultOutputDevice,
        kAudioHardwarePropertyDevices, kAudioObjectPropertyElementMain, kAudioObjectPropertyName,
        kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyScopeInput,
        kAudioObjectPropertyScopeOutput, kAudioObjectSystemObject, AudioDeviceCreateIOProcID,
        AudioDeviceDestroyIOProcID, AudioDeviceIOProcID, AudioDeviceStart, AudioDeviceStop,
        AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectID,
        AudioObjectPropertyAddress, AudioObjectPropertyScope, AudioObjectPropertySelector,
        AudioObjectSetPropertyData,
    };
    use objc2_core_audio_types::{AudioBuffer, AudioBufferList, AudioTimeStamp};
    use objc2_core_foundation::{CFRetained, CFString};

    const SYSTEM: AudioObjectID = kAudioObjectSystemObject as AudioObjectID;

    fn address(
        selector: AudioObjectPropertySelector,
        scope: AudioObjectPropertyScope,
    ) -> AudioObjectPropertyAddress {
        AudioObjectPropertyAddress {
            mSelector: selector,
            mScope: scope,
            mElement: kAudioObjectPropertyElementMain,
        }
    }

    fn scope(direction: Direction) -> AudioObjectPropertyScope {
        match direction {
            Direction::Input => kAudioObjectPropertyScopeInput,
            Direction::Output => kAudioObjectPropertyScopeOutput,
        }
    }

    fn default_selector(direction: Direction) -> AudioObjectPropertySelector {
        match direction {
            Direction::Input => kAudioHardwarePropertyDefaultInputDevice,
            Direction::Output => kAudioHardwarePropertyDefaultOutputDevice,
        }
    }

    fn check(status: i32, what: &str) -> Result<(), String> {
        if status == 0 {
            Ok(())
        } else {
            Err(format!("{what} failed (OSStatus {status})"))
        }
    }

    /// A property whose value is a variable number of bytes.
    fn get_bytes(object: AudioObjectID, mut address: AudioObjectPropertyAddress) -> Result<Vec<u8>, String> {
        let mut size: u32 = 0;
        // SAFETY: the address and size pointers are valid for the call.
        check(
            unsafe {
                AudioObjectGetPropertyDataSize(
                    object,
                    NonNull::from(&mut address),
                    0,
                    null(),
                    NonNull::from(&mut size),
                )
            },
            "AudioObjectGetPropertyDataSize",
        )?;
        // u64-backed so the structs read out of it are aligned.
        let mut buffer = vec![0u64; (size as usize).div_ceil(8).max(1)];
        // SAFETY: `buffer` holds at least `size` bytes.
        check(
            unsafe {
                AudioObjectGetPropertyData(
                    object,
                    NonNull::from(&mut address),
                    0,
                    null(),
                    NonNull::from(&mut size),
                    NonNull::new_unchecked(buffer.as_mut_ptr().cast()),
                )
            },
            "AudioObjectGetPropertyData",
        )?;
        let bytes = buffer.iter().flat_map(|word| word.to_ne_bytes()).take(size as usize).collect();
        Ok(bytes)
    }

    /// A property whose value is one plain `T`.
    fn get<T: Copy + Default>(object: AudioObjectID, mut address: AudioObjectPropertyAddress) -> Result<T, String> {
        let mut value = T::default();
        let mut size = size_of::<T>() as u32;
        // SAFETY: `value` is a `T` and `size` says so.
        check(
            unsafe {
                AudioObjectGetPropertyData(
                    object,
                    NonNull::from(&mut address),
                    0,
                    null(),
                    NonNull::from(&mut size),
                    NonNull::from(&mut value).cast(),
                )
            },
            "AudioObjectGetPropertyData",
        )?;
        Ok(value)
    }

    fn device_ids() -> Result<Vec<AudioObjectID>, String> {
        let bytes = get_bytes(SYSTEM, address(kAudioHardwarePropertyDevices, kAudioObjectPropertyScopeGlobal))?;
        let (ids, _) = bytes.as_chunks::<{ size_of::<AudioObjectID>() }>();
        Ok(ids.iter().map(|&id| AudioObjectID::from_ne_bytes(id)).collect())
    }

    /// How many channels `device` has on the `direction` side. A device with
    /// none there (a microphone, asked about output) does not belong in that
    /// list.
    fn channels(device: AudioObjectID, direction: Direction) -> usize {
        let Ok(bytes) =
            get_bytes(device, address(kAudioDevicePropertyStreamConfiguration, scope(direction)))
        else {
            return 0;
        };
        if bytes.len() < size_of::<u32>() {
            return 0;
        }
        // An AudioBufferList: a u32 count, then that many AudioBuffers at
        // pointer alignment.
        let count = u32::from_ne_bytes(bytes[..4].try_into().unwrap()) as usize;
        let first = std::mem::offset_of!(AudioBufferList, mBuffers);
        (0..count)
            .filter_map(|index| {
                let start = first + index * size_of::<AudioBuffer>();
                let field = bytes.get(start..start + 4)?;
                Some(u32::from_ne_bytes(field.try_into().unwrap()) as usize)
            })
            .sum()
    }

    fn name(device: AudioObjectID) -> Option<String> {
        let mut raw: *const CFString = null();
        let mut size = size_of::<*const CFString>() as u32;
        let mut address = address(kAudioObjectPropertyName, kAudioObjectPropertyScopeGlobal);
        // SAFETY: the property is a CFStringRef, handed over retained.
        let status = unsafe {
            AudioObjectGetPropertyData(
                device,
                NonNull::from(&mut address),
                0,
                null(),
                NonNull::from(&mut size),
                NonNull::from(&mut raw).cast(),
            )
        };
        if status != 0 {
            return None;
        }
        // SAFETY: a Get of a CFType property follows the Create rule.
        let name = unsafe { CFRetained::from_raw(NonNull::new(raw as *mut CFString)?) };
        Some(name.to_string())
    }

    pub fn list(direction: Direction) -> Result<Vec<Device>, String> {
        let default: AudioObjectID =
            get(SYSTEM, address(default_selector(direction), kAudioObjectPropertyScopeGlobal))
                .unwrap_or(0);
        Ok(device_ids()?
            .into_iter()
            .filter(|&device| channels(device, direction) > 0)
            .filter_map(|device| {
                Some(Device { id: device.to_string(), name: name(device)?, is_default: device == default })
            })
            .collect())
    }

    fn parse(id: &str) -> Result<AudioObjectID, String> {
        id.parse().map_err(|_| format!("not a CoreAudio device id: {id}"))
    }

    pub fn set_default(direction: Direction, id: &str) -> Result<(), String> {
        let mut device = parse(id)?;
        let mut address = address(default_selector(direction), kAudioObjectPropertyScopeGlobal);
        // SAFETY: the property is an AudioObjectID, and `device` is one.
        check(
            unsafe {
                AudioObjectSetPropertyData(
                    SYSTEM,
                    NonNull::from(&mut address),
                    0,
                    null(),
                    size_of::<AudioObjectID>() as u32,
                    NonNull::from(&mut device).cast(),
                )
            },
            "setting the default device",
        )
    }

    /// What the IOProc needs, owned by `play_tone` for as long as it runs.
    struct Tone {
        rate: f64,
        generation: u64,
        frame: AtomicU64,
    }

    /// Fill every output buffer with the next stretch of the tone. The HAL
    /// hands an IOProc native-endian Float32 regardless of the hardware, one
    /// interleaved buffer per stream.
    unsafe extern "C-unwind" fn render(
        _device: AudioObjectID,
        _now: NonNull<AudioTimeStamp>,
        _input: NonNull<AudioBufferList>,
        _input_time: NonNull<AudioTimeStamp>,
        output: NonNull<AudioBufferList>,
        _output_time: NonNull<AudioTimeStamp>,
        client: *mut c_void,
    ) -> i32 {
        // SAFETY: `client` is the `Tone` play_tone keeps alive until the
        // IOProc is stopped and destroyed.
        let tone = unsafe { &*(client as *const Tone) };
        let list = output.as_ptr();
        // SAFETY: the HAL hands a valid list of `mNumberBuffers` buffers.
        let count = unsafe { (*list).mNumberBuffers } as usize;
        let buffers = unsafe { std::slice::from_raw_parts((*list).mBuffers.as_ptr(), count) };
        let start = tone.frame.load(Ordering::Relaxed);
        let live = current_tone(tone.generation);
        let mut frames_written = 0;
        for buffer in buffers {
            let channels = buffer.mNumberChannels.max(1) as usize;
            let len = buffer.mDataByteSize as usize / size_of::<f32>();
            if buffer.mData.is_null() {
                continue;
            }
            // SAFETY: mData holds mDataByteSize bytes of Float32.
            let samples = unsafe { std::slice::from_raw_parts_mut(buffer.mData.cast::<f32>(), len) };
            for (index, frame) in samples.chunks_mut(channels).enumerate() {
                let value = if live { tone_sample(start + index as u64, tone.rate) } else { 0.0 };
                frame.fill(value);
            }
            frames_written = frames_written.max(len / channels);
        }
        tone.frame.store(start + frames_written as u64, Ordering::Relaxed);
        0
    }

    pub fn play_tone(id: &str, generation: u64) -> Result<(), String> {
        let device = parse(id)?;
        let rate: f64 =
            get(device, address(kAudioDevicePropertyNominalSampleRate, kAudioObjectPropertyScopeGlobal))?;
        if rate <= 0.0 {
            return Err(format!("device reports a sample rate of {rate}"));
        }
        let tone = Box::new(Tone { rate, generation, frame: AtomicU64::new(0) });
        let client = &*tone as *const Tone as *mut c_void;
        let mut proc_id: AudioDeviceIOProcID = None;
        // SAFETY: `render` matches AudioDeviceIOProc, and `tone` outlives it.
        check(
            unsafe { AudioDeviceCreateIOProcID(device, Some(render), client, NonNull::from(&mut proc_id)) },
            "AudioDeviceCreateIOProcID",
        )?;
        // SAFETY: `proc_id` was just registered on `device`.
        let started = check(unsafe { AudioDeviceStart(device, proc_id) }, "AudioDeviceStart");
        if started.is_ok() {
            // Until the tone has played out, or a newer one has taken over.
            // The extra margin lets the last buffer leave the hardware.
            let total = tone_frames(rate);
            for _ in 0..40 {
                if tone.frame.load(Ordering::Relaxed) >= total || !current_tone(generation) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            std::thread::sleep(Duration::from_millis(30));
            // SAFETY: as above; stopping is synchronous, so `render` is not
            // running once this returns.
            unsafe { AudioDeviceStop(device, proc_id) };
        }
        // SAFETY: as above.
        unsafe { AudioDeviceDestroyIOProcID(device, proc_id) };
        drop(tone);
        started
    }
}

#[cfg(windows)]
mod platform {
    //! The Windows Core Audio APIs (MMDevice + WASAPI) list the endpoints and
    //! play the tone. Changing the default endpoint has no documented API:
    //! every switcher, the Sound control panel's own included, goes through
    //! the `IPolicyConfig` COM interface, which has been stable since Vista.

    // IPolicyConfig's methods keep their COM names; `#[interface]` accepts no
    // attribute of its own, so the lint is allowed here.
    #![allow(non_snake_case)]

    use super::{current_tone, tone_frames, tone_sample, Device, Direction};
    use std::time::Duration;

    use windows::core::{interface, GUID, HRESULT, PCWSTR, PWSTR};
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
    use windows::Win32::Media::Audio::{
        eCapture, eCommunications, eConsole, eMultimedia, eRender, EDataFlow, ERole,
        IAudioClient, IAudioRenderClient, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator,
        AUDCLNT_SHAREMODE_SHARED, DEVICE_STATE_ACTIVE, WAVEFORMATEX, WAVEFORMATEXTENSIBLE,
    };
    use windows::Win32::Media::KernelStreaming::WAVE_FORMAT_EXTENSIBLE;
    use windows::Win32::Media::Multimedia::{KSDATAFORMAT_SUBTYPE_IEEE_FLOAT, WAVE_FORMAT_IEEE_FLOAT};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
        COINIT_MULTITHREADED, STGM_READ,
    };

    const CLSID_POLICY_CONFIG_CLIENT: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

    /// Only the slot that matters is typed; the rest keep the vtable layout.
    #[interface("f8679f50-850a-41cf-9c72-430f290290c8")]
    unsafe trait IPolicyConfig: windows::core::IUnknown {
        fn GetMixFormat(&self) -> HRESULT;
        fn GetDeviceFormat(&self) -> HRESULT;
        fn ResetDeviceFormat(&self) -> HRESULT;
        fn SetDeviceFormat(&self) -> HRESULT;
        fn GetProcessingPeriod(&self) -> HRESULT;
        fn SetProcessingPeriod(&self) -> HRESULT;
        fn GetShareMode(&self) -> HRESULT;
        fn SetShareMode(&self) -> HRESULT;
        fn GetPropertyValue(&self) -> HRESULT;
        fn SetPropertyValue(&self) -> HRESULT;
        fn SetDefaultEndpoint(&self, device_id: PCWSTR, role: ERole) -> HRESULT;
        fn SetEndpointVisibility(&self) -> HRESULT;
    }

    /// COM for the life of one call on whatever thread makes it.
    struct Com(bool);

    impl Com {
        fn init() -> Self {
            // SAFETY: balanced in Drop when it succeeded.
            Com(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok())
        }
    }

    impl Drop for Com {
        fn drop(&mut self) {
            if self.0 {
                // SAFETY: balances the successful CoInitializeEx.
                unsafe { CoUninitialize() };
            }
        }
    }

    fn flow(direction: Direction) -> EDataFlow {
        match direction {
            Direction::Input => eCapture,
            Direction::Output => eRender,
        }
    }

    fn enumerator() -> windows::core::Result<IMMDeviceEnumerator> {
        // SAFETY: plain COM activation on an initialised thread.
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
    }

    fn take_string(raw: PWSTR) -> String {
        // SAFETY: a CoTaskMem string from the API, freed once copied.
        let text = unsafe { raw.to_string() }.unwrap_or_default();
        unsafe { CoTaskMemFree(Some(raw.0 as *const _)) };
        text
    }

    fn device_id(device: &IMMDevice) -> windows::core::Result<String> {
        // SAFETY: COM call on a live interface.
        Ok(take_string(unsafe { device.GetId() }?))
    }

    fn friendly_name(device: &IMMDevice) -> windows::core::Result<String> {
        // SAFETY: COM calls on live interfaces.
        let store = unsafe { device.OpenPropertyStore(STGM_READ) }?;
        let value = unsafe { store.GetValue(&PKEY_Device_FriendlyName) }?;
        Ok(value.to_string())
    }

    pub fn list(direction: Direction) -> Result<Vec<Device>, String> {
        let _com = Com::init();
        let run = || -> windows::core::Result<Vec<Device>> {
            let enumerator = enumerator()?;
            // No default at all (nothing plugged in) is not an error here.
            // SAFETY: COM calls on live interfaces.
            let default = unsafe { enumerator.GetDefaultAudioEndpoint(flow(direction), eConsole) }
                .ok()
                .and_then(|device| device_id(&device).ok());
            let collection = unsafe { enumerator.EnumAudioEndpoints(flow(direction), DEVICE_STATE_ACTIVE) }?;
            let count = unsafe { collection.GetCount() }?;
            let mut devices = Vec::new();
            for index in 0..count {
                let device = unsafe { collection.Item(index) }?;
                let id = device_id(&device)?;
                let name = friendly_name(&device).unwrap_or_else(|_| id.clone());
                devices.push(Device { is_default: default.as_deref() == Some(id.as_str()), id, name });
            }
            Ok(devices)
        };
        run().map_err(|error| error.to_string())
    }

    pub fn set_default(_direction: Direction, id: &str) -> Result<(), String> {
        let _com = Com::init();
        let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
        let run = || -> windows::core::Result<()> {
            // SAFETY: COM activation and calls on a live interface; `wide` is
            // NUL-terminated and outlives the calls.
            let policy: IPolicyConfig =
                unsafe { CoCreateInstance(&CLSID_POLICY_CONFIG_CLIENT, None, CLSCTX_ALL) }?;
            // All three roles, the way the Sound control panel sets them, so
            // calls and media follow the switch as well.
            for role in [eConsole, eMultimedia, eCommunications] {
                unsafe { policy.SetDefaultEndpoint(PCWSTR(wide.as_ptr()), role) }.ok()?;
            }
            Ok(())
        };
        run().map_err(|error| error.to_string())
    }

    /// The sample formats a shared-mode mix format comes in.
    enum Sample {
        Float,
        Int16,
    }

    fn sample_kind(format: &WAVEFORMATEX) -> Option<Sample> {
        let tag = format.wFormatTag as u32;
        let float = if tag == WAVE_FORMAT_EXTENSIBLE {
            // SAFETY: the tag says the format is the extensible struct. It is
            // packed, so the GUID is read without taking a reference to it.
            let sub_format = unsafe {
                let extensible = format as *const WAVEFORMATEX as *const WAVEFORMATEXTENSIBLE;
                std::ptr::addr_of!((*extensible).SubFormat).read_unaligned()
            };
            sub_format == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT
        } else {
            tag == WAVE_FORMAT_IEEE_FLOAT
        };
        match (float, format.wBitsPerSample) {
            (true, 32) => Some(Sample::Float),
            (false, 16) => Some(Sample::Int16),
            _ => None,
        }
    }

    pub fn play_tone(id: &str, generation: u64) -> Result<(), String> {
        let _com = Com::init();
        let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
        let run = || -> windows::core::Result<()> {
            // SAFETY: COM calls on live interfaces; the mix format is freed
            // once copied, and the buffer is written within the frame count
            // GetBuffer granted.
            let device = unsafe { enumerator()?.GetDevice(PCWSTR(wide.as_ptr())) }?;
            let client: IAudioClient = unsafe { device.Activate(CLSCTX_ALL, None) }?;
            let format_ptr = unsafe { client.GetMixFormat() }?;
            let format = unsafe { *format_ptr };
            let kind = sample_kind(&format);
            // Twice the tone's length, in 100 ns units: room for all of it.
            let duration = super::TONE_MS as i64 * 10_000 * 2;
            let initialized =
                unsafe { client.Initialize(AUDCLNT_SHAREMODE_SHARED, 0, duration, 0, format_ptr, None) };
            unsafe { CoTaskMemFree(Some(format_ptr as *const _)) };
            initialized?;
            let Some(kind) = kind else { return Ok(()) };

            let rate = format.nSamplesPerSec as f64;
            let channels = format.nChannels.max(1) as usize;
            let capacity = unsafe { client.GetBufferSize() }?;
            let frames = (tone_frames(rate) as u32).min(capacity);
            let render: IAudioRenderClient = unsafe { client.GetService() }?;
            let data = unsafe { render.GetBuffer(frames) }?;
            let len = frames as usize * channels;
            match kind {
                Sample::Float => {
                    let out = unsafe { std::slice::from_raw_parts_mut(data.cast::<f32>(), len) };
                    for (index, frame) in out.chunks_mut(channels).enumerate() {
                        frame.fill(tone_sample(index as u64, rate));
                    }
                }
                Sample::Int16 => {
                    let out = unsafe { std::slice::from_raw_parts_mut(data.cast::<i16>(), len) };
                    for (index, frame) in out.chunks_mut(channels).enumerate() {
                        frame.fill((tone_sample(index as u64, rate) * i16::MAX as f32) as i16);
                    }
                }
            }
            unsafe { render.ReleaseBuffer(frames, 0) }?;
            unsafe { client.Start() }?;
            // Until the tone has played out, or a newer one has taken over.
            for _ in 0..30 {
                let padding = unsafe { client.GetCurrentPadding() }.unwrap_or(0);
                if padding == 0 || !current_tone(generation) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            unsafe { client.Stop() }?;
            Ok(())
        };
        run().map_err(|error| error.to_string())
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod platform {
    use super::{Device, Direction};

    pub fn list(_direction: Direction) -> Result<Vec<Device>, String> {
        Ok(Vec::new())
    }

    pub fn set_default(_direction: Direction, _id: &str) -> Result<(), String> {
        Err("switching audio devices is not supported on this platform".into())
    }

    pub fn play_tone(_id: &str, _generation: u64) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tone_lasts_a_tenth_of_a_second_at_any_rate() {
        assert_eq!(tone_frames(48_000.0), 4_800);
        assert_eq!(tone_frames(44_100.0), 4_410);
    }

    #[test]
    fn the_tone_fades_in_and_out_and_then_stops() {
        let rate = 48_000.0;
        let total = tone_frames(rate);
        assert_eq!(tone_sample(0, rate), 0.0, "starts from silence, no click");
        assert!(tone_sample(total - 1, rate).abs() < 1e-6, "ends on silence, no click");
        assert_eq!(tone_sample(total, rate), 0.0);
        assert_eq!(tone_sample(total * 10, rate), 0.0);
        let loudest = (0..total).map(|frame| tone_sample(frame, rate).abs()).fold(0.0, f32::max);
        assert!((loudest - TONE_LEVEL).abs() < 0.01, "peaks at {loudest}");
    }

    /// One full period of 440 Hz at 44.1 kHz is a little over 100 frames;
    /// counting rising zero crossings over the steady part gives the pitch.
    #[test]
    fn the_tone_is_an_a440() {
        let rate = 44_100.0;
        let samples: Vec<f32> = (0..tone_frames(rate)).map(|frame| tone_sample(frame, rate)).collect();
        let rising = samples.windows(2).filter(|pair| pair[0] < 0.0 && pair[1] >= 0.0).count();
        // 100 ms of 440 Hz is 44 periods; the fades can shave one off.
        assert!((43..=44).contains(&rising), "{rising} periods");
    }
}
