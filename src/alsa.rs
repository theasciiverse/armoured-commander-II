//! Sound output on Linux: ALSA, loaded with `dlopen` when the first
//! sound plays rather than linked, so the game starts on a system without
//! libasound and plays on silently there. Through ALSA's default device
//! it reaches PipeWire or PulseAudio where those run. rodio still decodes
//! and mixes; this only writes the mix out. Taken over from Armoured
//! Commander I, with the theme and the volume (`sound::source_for`).

use std::ffi::{CStr, c_char, c_int, c_long, c_uint, c_ulong, c_void};
use std::num::NonZero;
use std::sync::mpsc::Receiver;

/// The mix is 16-bit stereo at 44,100 Hz (rodio converts the sounds).
const RATE: u32 = 44_100;
const CHANNELS: u16 = 2;
/// Frames written at a time, about 23 ms.
const CHUNK: usize = 1024;
/// What ALSA is asked to buffer, in microseconds.
const LATENCY_US: c_uint = 100_000;

const SND_PCM_STREAM_PLAYBACK: c_int = 0;
const SND_PCM_FORMAT_S16_LE: c_int = 2;
const SND_PCM_ACCESS_RW_INTERLEAVED: c_int = 3;

type Pcm = *mut c_void;
type ErrorHandler =
    unsafe extern "C" fn(*const c_char, c_int, *const c_char, c_int, *const c_char, *mut c_void);

/// The few libasound functions the output needs.
struct Lib {
    open: unsafe extern "C" fn(*mut Pcm, *const c_char, c_int, c_int) -> c_int,
    set_params: unsafe extern "C" fn(Pcm, c_int, c_int, c_uint, c_uint, c_int, c_uint) -> c_int,
    writei: unsafe extern "C" fn(Pcm, *const c_void, c_ulong) -> c_long,
    recover: unsafe extern "C" fn(Pcm, c_int, c_int) -> c_int,
    drain: unsafe extern "C" fn(Pcm) -> c_int,
    prepare: unsafe extern "C" fn(Pcm) -> c_int,
    strerror: unsafe extern "C" fn(c_int) -> *const c_char,
}

/// alsa-lib writes its complaints to stderr, which is the game's screen;
/// they go to the log instead (only the format string: the arguments are
/// a `va_list`).
unsafe extern "C" fn quiet(
    _file: *const c_char,
    _line: c_int,
    _func: *const c_char,
    _err: c_int,
    fmt: *const c_char,
    _args: *mut c_void,
) {
    if !fmt.is_null() {
        let fmt = unsafe { CStr::from_ptr(fmt) };
        log::debug!("alsa-lib: {}", fmt.to_string_lossy());
    }
}

impl Lib {
    fn load() -> Option<Lib> {
        unsafe {
            let handle = libc::dlopen(c"libasound.so.2".as_ptr(), libc::RTLD_NOW);
            if handle.is_null() {
                return None;
            }
            let sym = |name: &CStr| {
                let p = libc::dlsym(handle, name.as_ptr());
                (!p.is_null()).then_some(p)
            };
            // since alsa-lib 1.1.6 (2018); per thread, and only this one
            // calls ALSA
            if let Some(p) = sym(c"snd_lib_error_set_local") {
                let set: unsafe extern "C" fn(Option<ErrorHandler>) -> Option<ErrorHandler> =
                    std::mem::transmute(p);
                set(Some(quiet));
            }
            Some(Lib {
                open: std::mem::transmute(sym(c"snd_pcm_open")?),
                set_params: std::mem::transmute(sym(c"snd_pcm_set_params")?),
                writei: std::mem::transmute(sym(c"snd_pcm_writei")?),
                recover: std::mem::transmute(sym(c"snd_pcm_recover")?),
                drain: std::mem::transmute(sym(c"snd_pcm_drain")?),
                prepare: std::mem::transmute(sym(c"snd_pcm_prepare")?),
                strerror: std::mem::transmute(sym(c"snd_strerror")?),
            })
        }
    }

    fn error(&self, err: c_int) -> String {
        unsafe { CStr::from_ptr((self.strerror)(err)).to_string_lossy().into_owned() }
    }

    fn open_default(&self) -> Result<Pcm, String> {
        let mut pcm: Pcm = std::ptr::null_mut();
        let err = unsafe { (self.open)(&mut pcm, c"default".as_ptr(), SND_PCM_STREAM_PLAYBACK, 0) };
        if err < 0 {
            return Err(self.error(err));
        }
        let err = unsafe {
            (self.set_params)(
                pcm,
                SND_PCM_FORMAT_S16_LE,
                SND_PCM_ACCESS_RW_INTERLEAVED,
                CHANNELS as c_uint,
                RATE,
                1,
                LATENCY_US,
            )
        };
        if err < 0 {
            return Err(self.error(err));
        }
        Ok(pcm)
    }

    /// Writes every frame of `buf`, recovering from an underrun.
    fn write(&self, pcm: Pcm, mut buf: &[i16]) -> Result<(), String> {
        let channels = CHANNELS as usize;
        while !buf.is_empty() {
            let frames = (buf.len() / channels) as c_ulong;
            let n = unsafe { (self.writei)(pcm, buf.as_ptr().cast(), frames) };
            if n < 0 {
                let err = unsafe { (self.recover)(pcm, n as c_int, 1) };
                if err < 0 {
                    return Err(self.error(err));
                }
                continue;
            }
            buf = &buf[n as usize * channels..];
        }
        Ok(())
    }
}

/// The audio thread: waits for a sound, then writes the mix out until
/// everything playing has finished, then waits again.
pub fn play(rx: Receiver<crate::sound::Command>) {
    let Some(lib) = Lib::load() else {
        log::warn!("no libasound.so.2, playing without sound");
        return;
    };
    let pcm = match lib.open_default() {
        Ok(p) => p,
        Err(e) => {
            log::warn!("no audio device, playing without sound: {e}");
            return;
        }
    };
    log::info!("sound: audio device open");
    let (mixer, mut mix) =
        rodio::mixer::mixer(NonZero::new(CHANNELS).unwrap(), NonZero::new(RATE).unwrap());
    let add = |cmd: crate::sound::Command| {
        if let Some(source) = crate::sound::source_for(cmd) {
            mixer.add(source);
        }
    };
    let mut buf = vec![0i16; CHUNK * CHANNELS as usize];
    while let Ok(cmd) = rx.recv() {
        add(cmd);
        loop {
            rx.try_iter().for_each(&add);
            let mut playing = true;
            for s in buf.iter_mut() {
                let v = if playing { mix.next() } else { None };
                playing = v.is_some();
                *s = (v.unwrap_or(0.0).clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            }
            if let Err(e) = lib.write(pcm, &buf) {
                log::error!("sound: {e}, playing without sound from now on");
                return;
            }
            if !playing {
                break;
            }
        }
        // let the last of it play out, then be ready for the next
        unsafe {
            (lib.drain)(pcm);
            (lib.prepare)(pcm);
        }
    }
}
