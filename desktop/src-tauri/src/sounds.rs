//! Short audio cues for the dictation cycle.
//!
//! Dictation happens while looking at some other window, so the only
//! feedback the overlay gives is feedback nobody sees. These are the
//! non-visual half: a blip when the recording arms, when it stops, when the
//! text lands, and when something went wrong.
//!
//! The built-in waveforms are synthesised here rather than shipped as assets
//! — a few sine segments with a click-free envelope cost less than a WAV in
//! the bundle. A user's own sound is decoded once on import and kept as a
//! normalised WAV in the data directory, so playback never has to decode.
//!
//! Playback is Windows-only for now (`PlaySoundW` from winmm, already
//! reachable through `windows-sys`). On other platforms [`play`] is a no-op:
//! the app is Windows-first, and a half-working cue is worse than none.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// Which moment in the dictation cycle a cue marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    /// Recording armed and the microphone is live.
    Start,
    /// Recording stopped, transcription is running. Only played in toggle
    /// mode — under push-to-talk the user's own key release already marks
    /// this moment, and a blip there would just precede [`Cue::Done`] by a
    /// second or two of nothing.
    Stop,
    /// Final text pasted into the focused window.
    Done,
    /// The cycle produced no text — a failure, or a recording that turned
    /// out to be silence. Both leave the user waiting for an insert that
    /// never comes, so both get the same cue.
    Error,
}

impl Cue {
    /// The cue's name in config, in IPC and in its custom file's name.
    fn key(self) -> &'static str {
        match self {
            Cue::Start => "start",
            Cue::Stop => "stop",
            Cue::Done => "done",
            Cue::Error => "error",
        }
    }

    fn parse(key: &str) -> Option<Self> {
        [Cue::Start, Cue::Stop, Cue::Done, Cue::Error]
            .into_iter()
            .find(|cue| cue.key() == key)
    }
}

/// A family of synthesised cues. One theme voices all four, so Start and
/// Stop stay a recognisable rising/falling pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Theme {
    Wood,
    Chord,
    Sonar,
    Classic,
}

/// How a note's loudness evolves.
#[derive(Clone, Copy)]
enum Envelope {
    /// Sustained, then faded out over `release_ms` at the end of the note.
    Held { release_ms: f32 },
    /// Struck: decays exponentially to about −80 dB over the note's length.
    Struck,
}

/// How a theme sounds a note.
struct Voice {
    /// `(frequency multiple, relative amplitude)` of sine partials, summed.
    partials: &'static [(f32, f32)],
    attack_ms: f32,
    envelope: Envelope,
    /// `(delay ms, feedback)` of a repeating echo, if any.
    echo: Option<(f32, f32)>,
}

/// One note of a cue: `(frequency Hz, onset ms, length ms)`. Notes may
/// overlap — a struck note keeps ringing under the next one.
type Note = (f32, u32, u32);

impl Theme {
    fn parse(key: &str) -> Option<Self> {
        match key {
            "wood" => Some(Theme::Wood),
            "chord" => Some(Theme::Chord),
            "sonar" => Some(Theme::Sonar),
            "classic" => Some(Theme::Classic),
            _ => None,
        }
    }

    fn voice(self) -> Voice {
        match self {
            // An inharmonic partial and almost no tail: a dry knock that
            // marks the moment and is gone before speech starts.
            Theme::Wood => Voice {
                partials: &[(1.0, 1.0), (2.7, 0.5)],
                attack_ms: 1.0,
                envelope: Envelope::Struck,
                echo: None,
            },
            // A slower attack than the others keeps the arpeggio soft.
            Theme::Chord => Voice {
                partials: &[(1.0, 1.0), (2.0, 0.08)],
                attack_ms: 14.0,
                envelope: Envelope::Struck,
                echo: None,
            },
            Theme::Sonar => Voice {
                partials: &[(1.0, 1.0)],
                attack_ms: 5.0,
                envelope: Envelope::Struck,
                echo: Some((170.0, 0.35)),
            },
            Theme::Classic => Voice {
                partials: &[(1.0, 1.0)],
                attack_ms: FADE_MS,
                envelope: Envelope::Held {
                    release_ms: FADE_MS,
                },
                echo: None,
            },
        }
    }

    /// The notes of each cue.
    ///
    /// Start rises and Stop falls, so the pair is distinguishable without
    /// having to learn it. Done is brighter and shorter — it fires while the
    /// user is already moving on. Error sits well below the others.
    fn notes(self, cue: Cue) -> &'static [Note] {
        match (self, cue) {
            (Theme::Wood, Cue::Start) => &[(880.00, 0, 60), (1174.66, 70, 60)],
            (Theme::Wood, Cue::Stop) => &[(1174.66, 0, 60), (880.00, 70, 60)],
            (Theme::Wood, Cue::Done) => &[(1396.91, 0, 60)],
            (Theme::Wood, Cue::Error) => &[(440.00, 0, 60), (440.00, 90, 60)],
            (Theme::Chord, Cue::Start) => &[(523.25, 0, 520), (659.25, 45, 520), (783.99, 90, 520)],
            (Theme::Chord, Cue::Stop) => &[(783.99, 0, 520), (659.25, 45, 520), (523.25, 90, 520)],
            (Theme::Chord, Cue::Done) => &[(783.99, 0, 520), (1046.50, 50, 520)],
            (Theme::Chord, Cue::Error) => &[(392.00, 0, 520), (369.99, 60, 520)],
            (Theme::Sonar, Cue::Start) => &[(987.77, 0, 600)],
            (Theme::Sonar, Cue::Stop) => &[(739.99, 0, 600)],
            (Theme::Sonar, Cue::Done) => &[(1318.51, 0, 600), (1318.51, 110, 600)],
            (Theme::Sonar, Cue::Error) => &[(440.00, 0, 600)],
            (Theme::Classic, Cue::Start) => &[(587.33, 0, 55), (880.00, 55, 70)],
            (Theme::Classic, Cue::Stop) => &[(880.00, 0, 55), (587.33, 55, 70)],
            (Theme::Classic, Cue::Done) => &[(1046.50, 0, 45), (1396.91, 45, 75)],
            (Theme::Classic, Cue::Error) => &[(311.13, 0, 90), (233.08, 90, 150)],
        }
    }
}

/// Config key toggling the whole feature.
const CONFIG_ENABLED: &str = "sound_feedback";
/// Config key for the overall cue loudness, a linear amplitude `0.0`–`1.0`.
const CONFIG_VOLUME: &str = "sound_volume";
/// Config key for the [`Theme`].
const CONFIG_THEME: &str = "sound_theme";
/// Config key for the per-cue settings: `{ "<cue>": { "source", "theme",
/// "volume" } }`. `source` is `theme`, `custom` or `mute`; `theme` overrides
/// the overall theme for that cue; `volume` scales the overall one.
const CONFIG_CUES: &str = "sound_cues";
/// Quiet enough to sit under speech without being startling, loud enough to
/// carry over laptop speakers.
const DEFAULT_VOLUME: f32 = 0.35;
const DEFAULT_THEME: Theme = Theme::Wood;

/// Sample rate of the generated PCM. 22.05 kHz is plenty for tones under
/// 1.5 kHz and halves the buffer against 44.1.
const SAMPLE_RATE: u32 = 22_050;
/// Fade that pins the end of every note to zero. Without it the waveform
/// stops at a non-zero sample and the speaker clicks.
const FADE_MS: f32 = 8.0;

/// Imported sounds keep this rate: what people pick as a cue often has
/// content above the 11 kHz the synthesis rate can carry.
const CUSTOM_SAMPLE_RATE: u32 = 44_100;
/// Longest custom cue kept. A cue marks a moment; past this it talks over
/// the dictation it is supposed to frame.
const CUSTOM_MAX_SECONDS: f64 = 3.0;
/// How much leading silence an imported file may have before its sound.
/// Exported clips often start with some, and a cue that lags its moment
/// reads as a fault.
const CUSTOM_LEADING_SECONDS: f64 = 2.0;
/// Below about −50 dBFS counts as the silence before the sound.
const CUSTOM_SILENCE: f32 = 0.003;
/// Fade over the end of a sound cut at [`CUSTOM_MAX_SECONDS`].
const CUSTOM_CUT_FADE_MS: f32 = 20.0;
/// Directory under the data dir holding one `<cue>.wav` per custom cue.
const CUSTOM_DIR: &str = "sounds";

/// What playing one cue amounts to under the current settings.
#[derive(Debug, PartialEq)]
struct Plan {
    theme: Theme,
    custom: bool,
    volume: f32,
}

/// Work out how `cue` should sound from an already-loaded config value.
///
/// Returns `None` when it should stay silent. Volumes are clamped rather than
/// rejected: a hand-edited `config.json` should not be able to produce a
/// waveform that clips. Unknown values fall back to the defaults.
fn plan(config: &Value, cue: Cue) -> Option<Plan> {
    let enabled = config
        .get(CONFIG_ENABLED)
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if !enabled {
        return None;
    }
    let overall = config
        .get(CONFIG_VOLUME)
        .and_then(Value::as_f64)
        .map(|v| v as f32)
        .unwrap_or(DEFAULT_VOLUME)
        .clamp(0.0, 1.0);
    let setting = config.get(CONFIG_CUES).and_then(|cues| cues.get(cue.key()));
    let overall_theme = config
        .get(CONFIG_THEME)
        .and_then(Value::as_str)
        .and_then(Theme::parse)
        .unwrap_or(DEFAULT_THEME);
    // A cue may borrow its sound from another theme than the one chosen for
    // the rest; without a valid override it follows that one.
    let theme = setting
        .and_then(|s| s.get("theme"))
        .and_then(Value::as_str)
        .and_then(Theme::parse)
        .unwrap_or(overall_theme);
    let custom = match setting
        .and_then(|s| s.get("source"))
        .and_then(Value::as_str)
    {
        Some("mute") => return None,
        Some("custom") => true,
        _ => false,
    };
    let relative = setting
        .and_then(|s| s.get("volume"))
        .and_then(Value::as_f64)
        .map(|v| v as f32)
        .unwrap_or(1.0)
        .clamp(0.0, 1.0);
    let volume = overall * relative;
    (volume > 0.0).then_some(Plan {
        theme,
        custom,
        volume,
    })
}

/// One cue to render and play.
struct Job {
    cue: Cue,
    theme: Theme,
    /// The imported sound to play instead of the theme's.
    custom: Option<PathBuf>,
    volume: f32,
}

/// Play `cue`, unless the user has turned it off.
///
/// Never blocks the caller and never fails loudly: a cue that does not come
/// out is not worth interrupting a dictation over. Safe to call from any
/// thread.
pub fn play(app: &tauri::AppHandle, cue: Cue) {
    let Ok(config) = crate::config::Config::load(app) else {
        return;
    };
    let Some(plan) = plan(config.as_value(), cue) else {
        return;
    };
    submit(Job {
        cue,
        theme: plan.theme,
        custom: plan.custom.then(|| custom_path(cue)),
        volume: plan.volume,
    });
}

fn custom_path(cue: Cue) -> PathBuf {
    crate::user_data::data_dir()
        .join(CUSTOM_DIR)
        .join(format!("{}.wav", cue.key()))
}

/// Render `job` into a complete WAV image.
///
/// A custom sound that has gone missing or unreadable falls back to the
/// theme's: the user asked for a cue, and some cue beats none.
fn render(job: &Job) -> Vec<u8> {
    if let Some(path) = &job.custom {
        match load_custom(path, job.volume) {
            Some(wav) => return wav,
            None => log::warn!(
                "custom sound for the {} cue is unreadable; playing the theme's",
                job.cue.key()
            ),
        }
    }
    crate::wav::encode_pcm16_mono(&render_pcm(job.theme, job.cue, job.volume), SAMPLE_RATE)
}

/// Read an imported cue back and scale it to `volume`. Imported cues are
/// stored peak-normalised, so the same volume means the same peak as a
/// synthesised one.
fn load_custom(path: &Path, volume: f32) -> Option<Vec<u8>> {
    let (samples, rate) = crate::wav::decode_pcm16_mono(&std::fs::read(path).ok()?)?;
    let scaled: Vec<i16> = samples
        .iter()
        .map(|&sample| (f32::from(sample) * volume) as i16)
        .collect();
    Some(crate::wav::encode_pcm16_mono(&scaled, rate))
}

/// Hand a cue to the playback backend.
#[cfg(windows)]
fn submit(job: Job) {
    use std::sync::mpsc::{sync_channel, SyncSender};
    use std::sync::OnceLock;
    use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_MEMORY, SND_NODEFAULT};

    static WORKER: OnceLock<SyncSender<Job>> = OnceLock::new();

    // One long-lived worker rather than a thread per cue. It also renders,
    // so neither synthesis nor reading a custom file lands on the dictation
    // path. `PlaySoundW` is called *synchronously* here on purpose: with
    // `SND_ASYNC` the API keeps reading from the buffer after returning, so
    // the buffer would have to outlive the call — and winmm has one playback
    // slot per process anyway, so an async cue would be cut off by the next
    // one regardless.
    let tx = WORKER.get_or_init(|| {
        // Bounded and small: cues are only meaningful near the moment they
        // describe. A backlog means playback is wedged, and the right
        // response is to drop cues, not to queue them up.
        let (tx, rx) = sync_channel::<Job>(2);
        std::thread::Builder::new()
            .name("sound-cues".to_string())
            .spawn(move || {
                while let Ok(job) = rx.recv() {
                    let wav = render(&job);
                    // SAFETY: `wav` is a complete in-memory RIFF/WAVE image
                    // that outlives the synchronous call. `SND_NODEFAULT`
                    // stops winmm from substituting the system default sound
                    // if it cannot parse it.
                    let played = unsafe {
                        PlaySoundW(wav.as_ptr() as *const u16, 0, SND_MEMORY | SND_NODEFAULT)
                    };
                    if played == 0 {
                        // No error code to report — winmm only says yes or
                        // no. Worth a line anyway: silent cues otherwise look
                        // like a config problem to whoever reports them.
                        log::warn!("PlaySoundW rejected a cue");
                    }
                }
            })
            .ok();
        tx
    });
    let _ = tx.try_send(job);
}

#[cfg(not(windows))]
fn submit(_job: Job) {}

fn ms_to_samples(ms: f32, rate: u32) -> usize {
    (ms / 1000.0 * rate as f32) as usize
}

/// Echo repeats rendered. The fourth comes back at 0.35⁴ ≈ −36 dB of the
/// dry sound; past that the tail is inaudible under any speech.
const ECHO_REPEATS: usize = 4;

/// Synthesise `cue` in `theme`, peak-normalised to `volume`.
///
/// Notes are mixed at their onsets, echoes added after, and the result
/// normalised as a whole, since overlapping notes and echoes sum.
fn render_pcm(theme: Theme, cue: Cue, volume: f32) -> Vec<i16> {
    let voice = theme.voice();
    let notes = theme.notes(cue);
    let samples = |ms: u32| ms_to_samples(ms as f32, SAMPLE_RATE);
    let dry_len = notes
        .iter()
        .map(|&(_, at, ms)| samples(at) + samples(ms))
        .max()
        .unwrap_or(0);
    let mut dry = vec![0.0f32; dry_len];
    for &(freq, at, ms) in notes {
        let (onset, len) = (samples(at), samples(ms));
        for i in 0..len {
            let phase = 2.0 * std::f32::consts::PI * freq * (i as f32 / SAMPLE_RATE as f32);
            let tone: f32 = voice
                .partials
                .iter()
                .map(|&(multiple, amplitude)| (phase * multiple).sin() * amplitude)
                .sum();
            dry[onset + i] += tone * envelope(&voice, i, len);
        }
    }
    let mix = match voice.echo {
        Some((delay_ms, feedback)) => {
            with_echo(&dry, ms_to_samples(delay_ms, SAMPLE_RATE), feedback)
        }
        None => dry,
    };
    let peak = mix.iter().fold(0.0f32, |peak, &s| peak.max(s.abs()));
    let gain = if peak > 0.0 { volume / peak } else { 0.0 };
    mix.iter()
        .map(|&s| (s * gain * i16::MAX as f32) as i16)
        .collect()
}

/// `dry` plus [`ECHO_REPEATS`] copies, each `delay` samples later and
/// `feedback` times quieter than the one before. Every copy ends where the
/// dry sound does — on silence — so the tail needs no fade of its own.
fn with_echo(dry: &[f32], delay: usize, feedback: f32) -> Vec<f32> {
    let mut out = vec![0.0f32; dry.len() + ECHO_REPEATS * delay];
    let mut gain = 1.0;
    for repeat in 0..=ECHO_REPEATS {
        let offset = repeat * delay;
        for (i, &sample) in dry.iter().enumerate() {
            out[offset + i] += sample * gain;
        }
        gain *= feedback;
    }
    out
}

/// Gain of sample `i` of a note `len` samples long.
fn envelope(voice: &Voice, i: usize, len: usize) -> f32 {
    // A ramp longer than half the note would never reach full amplitude and
    // the two ramps would fight each other.
    let attack = ms_to_samples(voice.attack_ms, SAMPLE_RATE).min(len / 2);
    if i < attack {
        return i as f32 / attack as f32;
    }
    match voice.envelope {
        Envelope::Held { release_ms } => {
            let release = ms_to_samples(release_ms, SAMPLE_RATE).min(len / 2);
            // `len - 1 - i`, not `len - i`: the ramp has to reach exactly
            // zero on the last sample, otherwise the note ends on a step and
            // clicks — which is the whole point of the envelope.
            if release > 0 && i >= len - release {
                (len - 1 - i) as f32 / release as f32
            } else {
                1.0
            }
        }
        Envelope::Struck => {
            // ln(1e-4): about −80 dB by the end of the note. The short fade
            // then pins it to zero instead of leaving a quiet step.
            let progress = (i - attack) as f32 / (len - attack).max(1) as f32;
            let decay = (-9.21 * progress).exp();
            let fade = ms_to_samples(FADE_MS, SAMPLE_RATE).min(len / 2);
            let tail = if i >= len - fade {
                (len - 1 - i) as f32 / fade as f32
            } else {
                1.0
            };
            decay * tail
        }
    }
}

/// Turn decoded audio into a stored cue: leading silence trimmed, cut to
/// [`CUSTOM_MAX_SECONDS`] with a fade, peak-normalised to full scale.
///
/// `None` when there is nothing audible in it.
fn prepare_custom(samples: &[f32]) -> Option<Vec<i16>> {
    let start = samples.iter().position(|s| s.abs() >= CUSTOM_SILENCE)?;
    let max = (CUSTOM_MAX_SECONDS * f64::from(CUSTOM_SAMPLE_RATE)) as usize;
    let sound = &samples[start..];
    let mut clip: Vec<f32> = sound.iter().take(max).copied().collect();
    if sound.len() > max {
        let fade = ms_to_samples(CUSTOM_CUT_FADE_MS, CUSTOM_SAMPLE_RATE).min(clip.len());
        let from = clip.len() - fade;
        for (k, sample) in clip[from..].iter_mut().enumerate() {
            *sample *= (fade - 1 - k) as f32 / fade as f32;
        }
    }
    let peak = clip.iter().fold(0.0f32, |peak, &s| peak.max(s.abs()));
    Some(crate::wav::f32_to_pcm16(
        &clip.iter().map(|&s| s / peak).collect::<Vec<_>>(),
    ))
}

/// Decode `source` and store it as the custom cue at `target`.
fn import_into(source: &Path, target: &Path) -> Result<(), String> {
    use std::io::Write;

    let samples = crate::audio_file::decode_clip(
        source,
        CUSTOM_SAMPLE_RATE,
        CUSTOM_MAX_SECONDS + CUSTOM_LEADING_SECONDS,
    )?;
    let pcm = prepare_custom(&samples).ok_or_else(|| crate::ui_text::t("В файле нет звука."))?;
    let wav = crate::wav::encode_pcm16_mono(&pcm, CUSTOM_SAMPLE_RATE);
    let failed = |error: std::io::Error| {
        log::error!("import_sound_cue: saving failed: {error}");
        crate::ui_text::t("Не удалось сохранить звук.")
    };
    if let Some(dir) = target.parent() {
        std::fs::create_dir_all(dir).map_err(failed)?;
    }
    // Through a temporary file, so a failed write leaves the previous sound
    // playable rather than a truncated one.
    let tmp = target.with_extension("wav.tmp");
    let write = |tmp: &Path| {
        let mut file = std::fs::File::create(tmp)?;
        file.write_all(&wav)?;
        file.sync_all()
    };
    write(&tmp).map_err(failed)?;
    std::fs::rename(&tmp, target).map_err(failed)?;
    Ok(())
}

/// Play one cue so the user can hear what they are choosing.
///
/// Takes the settings as arguments instead of reading them back from config:
/// the settings UI previews the value under the control *before* it is
/// saved, and a preview that lags the control by one change is useless.
/// `source` is `theme` or `custom`; `volume` is the final amplitude.
#[tauri::command]
pub(crate) fn preview_sound_cue(
    cue: String,
    theme: String,
    source: String,
    volume: f64,
) -> Result<(), String> {
    let cue = Cue::parse(&cue).ok_or_else(|| format!("unknown cue: {cue}"))?;
    let theme = Theme::parse(&theme).ok_or_else(|| format!("unknown theme: {theme}"))?;
    let custom = match source.as_str() {
        "theme" => None,
        "custom" => Some(custom_path(cue)),
        other => return Err(format!("unknown source: {other}")),
    };
    submit(Job {
        cue,
        theme,
        custom,
        volume: (volume as f32).clamp(0.0, 1.0),
    });
    Ok(())
}

/// Let the user pick an audio file and store it as `cue`'s custom sound.
///
/// Returns the picked file's name for the settings to show, or `None` when
/// the dialog was closed. The config is left to the caller: switching the
/// cue to its custom sound is the same save as any other setting.
#[tauri::command]
pub(crate) async fn import_sound_cue(
    app: tauri::AppHandle,
    cue: String,
) -> Result<Option<String>, String> {
    let cue = Cue::parse(&cue).ok_or_else(|| format!("unknown cue: {cue}"))?;
    let Some(picked) = crate::audio_file::pick_audio_file(app).await? else {
        return Ok(None);
    };
    let source = PathBuf::from(picked);
    let name = source
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let target = custom_path(cue);
    tokio::task::spawn_blocking(move || import_into(&source, &target))
        .await
        .map_err(|error| {
            log::error!("import_sound_cue: import task failed: {error}");
            crate::ui_text::t("Не удалось сохранить звук.")
        })??;
    Ok(Some(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const THEMES: [Theme; 4] = [Theme::Wood, Theme::Chord, Theme::Sonar, Theme::Classic];
    const CUES: [Cue; 4] = [Cue::Start, Cue::Stop, Cue::Done, Cue::Error];

    #[test]
    fn defaults_play_the_wood_theme_at_the_default_volume() {
        assert_eq!(
            plan(&json!({}), Cue::Start),
            Some(Plan {
                theme: Theme::Wood,
                custom: false,
                volume: DEFAULT_VOLUME
            })
        );
    }

    #[test]
    fn disabling_or_muting_plays_nothing() {
        assert_eq!(plan(&json!({ "sound_feedback": false }), Cue::Done), None);
        let muted = json!({ "sound_cues": { "done": { "source": "mute" } } });
        assert_eq!(plan(&muted, Cue::Done), None);
        assert!(plan(&muted, Cue::Start).is_some());
        let silent = json!({ "sound_cues": { "error": { "volume": 0.0 } } });
        assert_eq!(plan(&silent, Cue::Error), None);
    }

    #[test]
    fn cue_volume_scales_the_overall_volume() {
        let config = json!({
            "sound_volume": 0.5,
            "sound_theme": "sonar",
            "sound_cues": { "stop": { "source": "custom", "volume": 0.5 } }
        });
        assert_eq!(
            plan(&config, Cue::Stop),
            Some(Plan {
                theme: Theme::Sonar,
                custom: true,
                volume: 0.25
            })
        );
    }

    #[test]
    fn a_cue_can_take_its_sound_from_another_theme() {
        let config = json!({
            "sound_theme": "wood",
            "sound_cues": { "error": { "source": "theme", "theme": "sonar" } }
        });
        assert_eq!(plan(&config, Cue::Error).unwrap().theme, Theme::Sonar);
        assert_eq!(plan(&config, Cue::Start).unwrap().theme, Theme::Wood);
        // A cleared or unknown override follows the overall theme.
        let cleared =
            json!({ "sound_theme": "chord", "sound_cues": { "done": { "theme": null } } });
        assert_eq!(plan(&cleared, Cue::Done).unwrap().theme, Theme::Chord);
        let unknown =
            json!({ "sound_theme": "chord", "sound_cues": { "done": { "theme": "kazoo" } } });
        assert_eq!(plan(&unknown, Cue::Done).unwrap().theme, Theme::Chord);
    }

    #[test]
    fn volumes_are_clamped_and_unknown_values_fall_back() {
        // A hand-edited config must not be able to produce a clipping
        // waveform (or a negative one, which would invert the phase).
        let loud = json!({
            "sound_volume": 4.0,
            "sound_theme": "kazoo",
            "sound_cues": { "start": { "source": "radio", "volume": 3.0 } }
        });
        assert_eq!(
            plan(&loud, Cue::Start),
            Some(Plan {
                theme: DEFAULT_THEME,
                custom: false,
                volume: 1.0
            })
        );
        assert_eq!(plan(&json!({ "sound_volume": -1.0 }), Cue::Start), None);
    }

    #[test]
    fn cue_names_round_trip() {
        for cue in CUES {
            assert_eq!(Cue::parse(cue.key()), Some(cue));
        }
        assert_eq!(Cue::parse("paste"), None);
    }

    #[test]
    fn every_cue_starts_and_ends_at_silence_and_peaks_at_the_volume() {
        // Non-zero endpoints are what make a short tone click.
        for theme in THEMES {
            for cue in CUES {
                let pcm = render_pcm(theme, cue, 0.5);
                assert_eq!(pcm[0], 0, "{theme:?} {cue:?} starts on a step");
                assert_eq!(*pcm.last().unwrap(), 0, "{theme:?} {cue:?} ends on a step");
                let peak = pcm.iter().map(|s| s.unsigned_abs()).max().unwrap();
                assert!(
                    (i32::from(peak) - i32::from(i16::MAX / 2)).abs() <= 1,
                    "{theme:?} {cue:?} peaks at {peak}"
                );
            }
        }
    }

    #[test]
    fn overlapping_notes_and_echoes_extend_the_cue() {
        let samples = |ms: f32| ms_to_samples(ms, SAMPLE_RATE);
        // The last note's onset plus its length, not the sum of the notes.
        let chord = render_pcm(Theme::Chord, Cue::Start, 1.0).len();
        assert_eq!(chord, samples(90.0) + samples(520.0));
        let sonar = render_pcm(Theme::Sonar, Cue::Start, 1.0).len();
        assert_eq!(sonar, samples(600.0) + ECHO_REPEATS * samples(170.0));
    }

    #[test]
    fn echo_repeats_fade_by_the_feedback() {
        let out = with_echo(&[1.0, 0.0], 3, 0.5);
        assert_eq!(
            out,
            [1.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.25, 0.0, 0.0, 0.125, 0.0, 0.0, 0.0625, 0.0]
        );
    }

    #[test]
    fn zero_volume_renders_silence() {
        let pcm = render_pcm(Theme::Wood, Cue::Start, 0.0);
        assert!(!pcm.is_empty());
        assert!(pcm.iter().all(|&s| s == 0));
    }

    #[test]
    fn custom_sound_loses_leading_silence_and_is_normalised() {
        let mut samples = vec![0.0005; 1000];
        samples.extend([0.0, 0.25, -0.5, 0.25]);
        let pcm = prepare_custom(&samples).unwrap();
        assert_eq!(pcm, [i16::MAX / 2, -i16::MAX, i16::MAX / 2]);
    }

    #[test]
    fn long_custom_sound_is_cut_with_a_fade() {
        let max = (CUSTOM_MAX_SECONDS * f64::from(CUSTOM_SAMPLE_RATE)) as usize;
        let pcm = prepare_custom(&vec![0.5; max + 500]).unwrap();
        assert_eq!(pcm.len(), max);
        assert_eq!(pcm[0], i16::MAX);
        assert_eq!(*pcm.last().unwrap(), 0);
    }

    #[test]
    fn silent_custom_sound_is_refused() {
        assert_eq!(prepare_custom(&[0.0; 4000]), None);
        assert_eq!(prepare_custom(&[]), None);
    }

    #[test]
    fn imported_sound_plays_back_at_the_requested_volume() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("chime.wav");
        let tone: Vec<i16> = (0..1600)
            .map(|i| ((i as f32 * 0.2).sin() * 8000.0) as i16)
            .collect();
        std::fs::write(&source, crate::wav::encode_pcm16_mono(&tone, 16_000)).unwrap();
        let target = dir.path().join("sounds").join("start.wav");

        import_into(&source, &target).unwrap();

        let (stored, rate) =
            crate::wav::decode_pcm16_mono(&std::fs::read(&target).unwrap()).unwrap();
        assert_eq!(rate, CUSTOM_SAMPLE_RATE);
        let peak = stored.iter().map(|s| s.unsigned_abs()).max().unwrap();
        assert!(peak >= i16::MAX as u16 - 1, "stored peak {peak}");
        let (played, _) =
            crate::wav::decode_pcm16_mono(&load_custom(&target, 0.25).unwrap()).unwrap();
        let peak = played.iter().map(|s| s.unsigned_abs()).max().unwrap();
        assert!((i32::from(peak) - i32::from(i16::MAX / 4)).abs() <= 1);
    }

    #[test]
    fn an_unreadable_custom_sound_falls_back_to_the_theme() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("done.wav");
        std::fs::write(&path, b"not a wav").unwrap();
        let job = Job {
            cue: Cue::Done,
            theme: Theme::Classic,
            custom: Some(path),
            volume: 0.5,
        };
        let expected =
            crate::wav::encode_pcm16_mono(&render_pcm(Theme::Classic, Cue::Done, 0.5), SAMPLE_RATE);
        assert_eq!(render(&job), expected);
    }

    /// Actually audible; run with
    /// `cargo test --lib sounds::tests::audition -- --ignored --nocapture`
    /// when tweaking the tones. Nothing about a waveform's correctness on
    /// paper tells you whether it sounds like feedback or like a fault.
    #[test]
    #[ignore = "plays audio through the default output device"]
    fn audition() {
        for theme in THEMES {
            for cue in CUES {
                println!("{theme:?} {cue:?}");
                submit(Job {
                    cue,
                    theme,
                    custom: None,
                    volume: DEFAULT_VOLUME,
                });
                std::thread::sleep(std::time::Duration::from_millis(900));
            }
        }
    }
}
