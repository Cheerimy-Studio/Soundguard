use std::{
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    time::Duration,
};

use wasapi::{
    get_default_device, get_default_device_for_role, initialize_mta, AudioCaptureClient,
    AudioClient, Direction, Handle, Role, SampleType, StreamMode, WaveFormat,
};
use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};

include!("../../../client/common/audio_core.rs");

const TARGET_CAPTURE_RATE: u32 = 48_000;
const TARGET_CAPTURE_CHANNELS: usize = 2;

pub enum AudioCommand {
    Start,
    Pause,
    Retry,
}

struct WasapiCapture {
    audio_client: AudioClient,
    capture_client: AudioCaptureClient,
    event: Handle,
    queue: VecDeque<u8>,
    frame_bytes: usize,
    channels: usize,
    sample_rate: u32,
    aec_enabled: bool,
    render_meter: Option<IAudioMeterInformation>,
    sample_buffer: Vec<f32>,
    last_meter_poll: Instant,
}

impl WasapiCapture {
    fn new() -> Result<Self, String> {
        let device = get_default_device_for_role(&Direction::Capture, &Role::Communications)
            .or_else(|_| get_default_device(&Direction::Capture))
            .map_err(|error| format!("无法打开默认麦克风: {error}"))?;
        let mut audio_client = device
            .get_iaudioclient()
            .map_err(|error| format!("无法获取麦克风客户端: {error}"))?;
        let desired_format = WaveFormat::new(
            32,
            32,
            &SampleType::Float,
            TARGET_CAPTURE_RATE as usize,
            TARGET_CAPTURE_CHANNELS,
            None,
        );
        let frame_bytes = desired_format.get_blockalign() as usize;
        let (_default_period, min_period) = audio_client
            .get_device_period()
            .map_err(|error| format!("无法读取音频周期: {error}"))?;

        let mode = StreamMode::EventsShared {
            autoconvert: true,
            buffer_duration_hns: min_period,
        };
        audio_client
            .initialize_client(&desired_format, &Direction::Capture, &mode)
            .map_err(|error| format!("无法初始化麦克风: {error}"))?;

        let mut aec_enabled = false;
        let render_device = get_default_device(&Direction::Render).ok();
        let render_id = render_device
            .as_ref()
            .and_then(|device| device.get_id().ok());
        match audio_client.is_aec_supported() {
            Ok(true) => {
                if let Ok(control) = audio_client.get_aec_control() {
                    if control
                        .set_echo_cancellation_render_endpoint(render_id)
                        .is_ok()
                    {
                        aec_enabled = true;
                    }
                }
            }
            Ok(false) => {}
            Err(error) => eprintln!("Could not query AEC support: {error}"),
        }
        // Always keep a render meter so system playback can be detected even when
        // the audio driver does not expose hardware echo cancellation.
        let render_meter = get_render_meter();

        let event = audio_client
            .set_get_eventhandle()
            .map_err(|error| format!("无法创建音频事件: {error}"))?;
        let capture_client = audio_client
            .get_audiocaptureclient()
            .map_err(|error| format!("无法创建采集客户端: {error}"))?;
        audio_client
            .start_stream()
            .map_err(|error| format!("无法启动麦克风: {error}"))?;

        Ok(Self {
            audio_client,
            capture_client,
            event,
            queue: VecDeque::with_capacity(frame_bytes * 2048),
            frame_bytes,
            channels: TARGET_CAPTURE_CHANNELS,
            sample_rate: TARGET_CAPTURE_RATE,
            aec_enabled,
            render_meter,
            sample_buffer: Vec::with_capacity(4096),
            last_meter_poll: Instant::now() - Duration::from_secs(1),
        })
    }

    fn wait_and_read(&mut self, shared: &AudioShared) -> Result<(), String> {
        if self.event.wait_for_event(120).is_err() {
            return Ok(());
        }
        self.capture_client
            .read_from_device_to_deque(&mut self.queue)
            .map_err(|error| format!("读取麦克风失败: {error}"))?;

        let complete = self.queue.len() / self.frame_bytes * self.frame_bytes;
        if complete > 0 {
            self.sample_buffer.clear();
            for _ in 0..(complete / 4) {
                let mut bytes = [0u8; 4];
                for byte in &mut bytes {
                    if let Some(value) = self.queue.pop_front() {
                        *byte = value;
                    }
                }
                self.sample_buffer.push(f32::from_le_bytes(bytes));
            }
            shared.push_block(&self.sample_buffer, self.channels, self.sample_rate);
        }

        if self.last_meter_poll.elapsed() >= Duration::from_millis(120) {
            self.last_meter_poll = Instant::now();
            if let Some(meter) = &self.render_meter {
                if let Ok(peak) = unsafe { meter.GetPeakValue() } {
                    shared.set_system_peak(peak);
                }
            }
        }
        Ok(())
    }

    fn pause(&self) {
        let _ = self.audio_client.stop_stream();
    }

    fn resume(&self) {
        let _ = self.audio_client.start_stream();
    }

    fn stop(&self) {
        let _ = self.audio_client.stop_stream();
    }
}

fn get_render_meter() -> Option<IAudioMeterInformation> {
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        device
            .Activate::<IAudioMeterInformation>(CLSCTX_ALL, None)
            .ok()
    }
}

pub fn spawn_audio_engine(shared: Arc<AudioShared>) -> Sender<AudioCommand> {
    let (sender, receiver) = mpsc::channel::<AudioCommand>();
    std::thread::Builder::new()
        .name("soundguard-audio".to_string())
        .spawn(move || audio_loop(shared, receiver))
        .expect("could not start audio thread");
    sender
}

fn audio_loop(shared: Arc<AudioShared>, receiver: Receiver<AudioCommand>) {
    if let Err(error) = initialize_mta().ok() {
        shared.set_error(Some(format!("Windows 音频初始化失败: {error}")));
        return;
    }

    let mut capture: Option<WasapiCapture> = None;
    let mut next_retry = Instant::now();
    let mut consecutive_failures = 0_u32;

    loop {
        loop {
            match receiver.try_recv() {
                Ok(AudioCommand::Start) => {
                    shared.set_capturing(true);
                    shared.reset_window_and_ring();
                    if let Some(current) = &capture {
                        current.resume();
                    } else {
                        next_retry = Instant::now();
                    }
                }
                Ok(AudioCommand::Pause) => {
                    shared.set_capturing(false);
                    shared.set_system_peak(0.0);
                    if let Some(current) = &capture {
                        current.pause();
                    }
                }
                Ok(AudioCommand::Retry) => {
                    if let Some(current) = capture.take() {
                        current.stop();
                    }
                    shared.ready.store(false, Ordering::SeqCst);
                    shared.reset_window_and_ring();
                    shared.set_error(None);
                    shared.set_aec_enabled(false);
                    next_retry = Instant::now();
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }

        if capture.is_none() && Instant::now() >= next_retry {
            match WasapiCapture::new() {
                Ok(current) => {
                    shared.ready.store(true, Ordering::SeqCst);
                    shared.set_error(None);
                    shared.set_aec_enabled(current.aec_enabled);
                    if !shared.capturing() {
                        current.pause();
                    }
                    capture = Some(current);
                    consecutive_failures = 0;
                }
                Err(error) => {
                    shared.ready.store(false, Ordering::SeqCst);
                    shared.reset_window_and_ring();
                    shared.set_error(Some(error));
                    next_retry = Instant::now() + Duration::from_secs(10);
                }
            }
        }

        if let Some(current) = &mut capture {
            if shared.capturing() {
                match current.wait_and_read(&shared) {
                    Ok(()) => {
                        consecutive_failures = 0;
                    }
                    Err(error) => {
                        consecutive_failures += 1;
                        if consecutive_failures >= 10 {
                            shared.set_error(Some(error));
                            shared.ready.store(false, Ordering::SeqCst);
                            shared.reset_window_and_ring();
                            if let Some(current) = capture.take() {
                                current.stop();
                            }
                            next_retry = Instant::now() + Duration::from_secs(5);
                        }
                    }
                }
            } else {
                std::thread::sleep(Duration::from_millis(50));
            }
        } else {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}
