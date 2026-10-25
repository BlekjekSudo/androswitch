//! `MEDIA-01..03`, `MEDIA-05`: Virtual Audio (`AudioManager`, `AudioFocusRequest`,
//! `AudioRecord`, `AudioTrack`), `MediaPlayer`, `MediaSession`/`MediaController`,
//! and `MediaProjectionManager`/`MediaProjection`/`VirtualDisplay`.

use super::{
    alloc, alloc_with, array_len, bool_arg, call_dex, camera::enqueue_frame_to_surface, check_perm,
    float_arg, get_field, get_float_field, get_int_field, get_long_field, illegal, illegal_state,
    int_arg, long_arg, obj_arg, opt_str_arg, put_field, read_byte_array, str_val,
};
use crate::{Engine, ObjRef, Value, VmExit};
use std::collections::BTreeMap;

const AUDIO_MANAGER: &str = "Landroid/media/AudioManager;";
const AUDIO_FOCUS_REQ: &str = "Landroid/media/AudioFocusRequest;";
const AUDIO_FOCUS_REQ_BUILDER: &str = "Landroid/media/AudioFocusRequest$Builder;";
const AUDIO_ATTRIBUTES: &str = "Landroid/media/AudioAttributes;";
const AUDIO_ATTRIBUTES_BUILDER: &str = "Landroid/media/AudioAttributes$Builder;";
const AUDIO_FORMAT: &str = "Landroid/media/AudioFormat;";
const AUDIO_FORMAT_BUILDER: &str = "Landroid/media/AudioFormat$Builder;";
const AUDIO_RECORD: &str = "Landroid/media/AudioRecord;";
const AUDIO_RECORD_BUILDER: &str = "Landroid/media/AudioRecord$Builder;";
const AUDIO_TRACK: &str = "Landroid/media/AudioTrack;";
const AUDIO_TRACK_BUILDER: &str = "Landroid/media/AudioTrack$Builder;";

const MEDIA_PLAYER: &str = "Landroid/media/MediaPlayer;";

const MEDIA_SESSION_MANAGER: &str = "Landroid/media/session/MediaSessionManager;";
const MEDIA_SESSION: &str = "Landroid/media/session/MediaSession;";
const MEDIA_SESSION_CALLBACK: &str = "Landroid/media/session/MediaSession$Callback;";
const MEDIA_SESSION_TOKEN: &str = "Landroid/media/session/MediaSession$Token;";
const MEDIA_CONTROLLER: &str = "Landroid/media/session/MediaController;";
const MEDIA_TRANSPORT: &str = "Landroid/media/session/MediaController$TransportControls;";
const PLAYBACK_STATE: &str = "Landroid/media/session/PlaybackState;";
const PLAYBACK_STATE_BUILDER: &str = "Landroid/media/session/PlaybackState$Builder;";
const MEDIA_METADATA: &str = "Landroid/media/MediaMetadata;";
const MEDIA_METADATA_BUILDER: &str = "Landroid/media/MediaMetadata$Builder;";

const PROJECTION_MANAGER: &str = "Landroid/media/projection/MediaProjectionManager;";
const PROJECTION: &str = "Landroid/media/projection/MediaProjection;";
const PROJECTION_CALLBACK: &str = "Landroid/media/projection/MediaProjection$Callback;";
const VIRTUAL_DISPLAY: &str = "Landroid/hardware/display/VirtualDisplay;";

#[derive(Debug, Clone)]
pub struct AudioFocusEntry {
    pub listener: Option<ObjRef>,
    pub stream_type: i32,
    pub duration_hint: i32,
}

#[derive(Debug, Clone)]
pub struct AudioState {
    pub stream_volumes: BTreeMap<i32, (i32, i32)>, // stream -> (current, max)
    pub ringer_mode: i32,                          // 0=SILENT, 1=VIBRATE, 2=NORMAL
    pub mode: i32,                                 // 0=NORMAL
    pub speakerphone_on: bool,
    pub microphone_mute: bool,
    pub focus_stack: Vec<AudioFocusEntry>,
    pub pcm_input_queue: Vec<u8>,
}

impl Default for AudioState {
    fn default() -> Self {
        let mut v = BTreeMap::new();
        v.insert(0, (4, 6)); // STREAM_VOICE_CALL
        v.insert(1, (5, 7)); // STREAM_SYSTEM
        v.insert(2, (5, 7)); // STREAM_RING
        v.insert(3, (10, 15)); // STREAM_MUSIC
        v.insert(4, (6, 7)); // STREAM_ALARM
        v.insert(5, (5, 7)); // STREAM_NOTIFICATION
        Self {
            stream_volumes: v,
            ringer_mode: 2,
            mode: 0,
            speakerphone_on: false,
            microphone_mute: false,
            focus_stack: Vec::new(),
            pcm_input_queue: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AudioRecordState {
    pub sample_rate: i32,
    pub channels: i32,
    pub recording: bool,
    pub bytes_read: u64,
}

#[derive(Debug, Clone)]
pub struct AudioTrackState {
    pub sample_rate: i32,
    pub play_state: i32, // 1=STOPPED, 2=PAUSED, 3=PLAYING
    pub bytes_written: u64,
    pub last_samples_preview: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaPlayerState {
    Idle,
    Initialized,
    Prepared,
    Started,
    Paused,
    Stopped,
    PlaybackCompleted,
    End,
}

#[derive(Debug, Clone)]
pub struct MediaPlayerEntry {
    pub state: MediaPlayerState,
    pub data_source: String,
    pub duration_ms: i32,
    pub position_ms: i32,
    pub looping: bool,
    pub left_vol: f32,
    pub right_vol: f32,
    pub on_prepared: Option<ObjRef>,
    pub on_completion: Option<ObjRef>,
    pub on_seek_complete: Option<ObjRef>,
    pub on_error: Option<ObjRef>,
}

impl Default for MediaPlayerEntry {
    fn default() -> Self {
        Self {
            state: MediaPlayerState::Idle,
            data_source: String::new(),
            duration_ms: 30_000,
            position_ms: 0,
            looping: false,
            left_vol: 1.0,
            right_vol: 1.0,
            on_prepared: None,
            on_completion: None,
            on_seek_complete: None,
            on_error: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MediaSessionEntry {
    pub tag: String,
    pub active: bool,
    pub callback: Option<ObjRef>,
    pub playback_state: Option<ObjRef>,
    pub metadata: Option<ObjRef>,
    pub state_code: i32,
    pub position_ms: i64,
    pub title: Option<String>,
    pub artist: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MediaProjectionEntry {
    pub callbacks: Vec<ObjRef>,
    pub displays: Vec<ObjRef>,
    pub stopped: bool,
}

pub(super) fn host_static(
    vm: &mut Engine,
    owner: &str,
    name: &str,
) -> Result<Option<Value>, VmExit> {
    match (owner, name) {
        (AUDIO_MANAGER, "STREAM_VOICE_CALL") => Ok(Some(Value::Int(0))),
        (AUDIO_MANAGER, "STREAM_SYSTEM") => Ok(Some(Value::Int(1))),
        (AUDIO_MANAGER, "STREAM_RING") => Ok(Some(Value::Int(2))),
        (AUDIO_MANAGER, "STREAM_MUSIC") => Ok(Some(Value::Int(3))),
        (AUDIO_MANAGER, "STREAM_ALARM") => Ok(Some(Value::Int(4))),
        (AUDIO_MANAGER, "STREAM_NOTIFICATION") => Ok(Some(Value::Int(5))),
        (AUDIO_MANAGER, "RINGER_MODE_SILENT") => Ok(Some(Value::Int(0))),
        (AUDIO_MANAGER, "RINGER_MODE_VIBRATE") => Ok(Some(Value::Int(1))),
        (AUDIO_MANAGER, "RINGER_MODE_NORMAL") => Ok(Some(Value::Int(2))),
        (AUDIO_MANAGER, "AUDIOFOCUS_REQUEST_FAILED") => Ok(Some(Value::Int(0))),
        (AUDIO_MANAGER, "AUDIOFOCUS_REQUEST_GRANTED") => Ok(Some(Value::Int(1))),
        (AUDIO_MANAGER, "AUDIOFOCUS_REQUEST_DELAYED") => Ok(Some(Value::Int(2))),
        (AUDIO_MANAGER, "AUDIOFOCUS_GAIN") => Ok(Some(Value::Int(1))),
        (AUDIO_MANAGER, "AUDIOFOCUS_GAIN_TRANSIENT") => Ok(Some(Value::Int(2))),
        (AUDIO_MANAGER, "AUDIOFOCUS_GAIN_TRANSIENT_MAY_DUCK") => Ok(Some(Value::Int(3))),
        (AUDIO_MANAGER, "AUDIOFOCUS_LOSS") => Ok(Some(Value::Int(-1))),
        (AUDIO_MANAGER, "AUDIOFOCUS_LOSS_TRANSIENT") => Ok(Some(Value::Int(-2))),
        (AUDIO_MANAGER, "AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK") => Ok(Some(Value::Int(-3))),
        (AUDIO_FORMAT, "ENCODING_PCM_16BIT") => Ok(Some(Value::Int(2))),
        (AUDIO_FORMAT, "ENCODING_PCM_8BIT") => Ok(Some(Value::Int(3))),
        (AUDIO_FORMAT, "ENCODING_PCM_FLOAT") => Ok(Some(Value::Int(4))),
        (AUDIO_FORMAT, "CHANNEL_IN_MONO") => Ok(Some(Value::Int(16))),
        (AUDIO_FORMAT, "CHANNEL_IN_STEREO") => Ok(Some(Value::Int(12))),
        (AUDIO_FORMAT, "CHANNEL_OUT_MONO") => Ok(Some(Value::Int(4))),
        (AUDIO_FORMAT, "CHANNEL_OUT_STEREO") => Ok(Some(Value::Int(12))),
        (AUDIO_RECORD, "STATE_INITIALIZED") | (AUDIO_TRACK, "STATE_INITIALIZED") => {
            Ok(Some(Value::Int(1)))
        }
        (AUDIO_RECORD, "RECORDSTATE_STOPPED") | (AUDIO_TRACK, "PLAYSTATE_STOPPED") => {
            Ok(Some(Value::Int(1)))
        }
        (AUDIO_TRACK, "PLAYSTATE_PAUSED") => Ok(Some(Value::Int(2))),
        (AUDIO_RECORD, "RECORDSTATE_RECORDING") | (AUDIO_TRACK, "PLAYSTATE_PLAYING") => {
            Ok(Some(Value::Int(3)))
        }
        (AUDIO_TRACK, "MODE_STATIC") => Ok(Some(Value::Int(0))),
        (AUDIO_TRACK, "MODE_STREAM") => Ok(Some(Value::Int(1))),
        (AUDIO_TRACK, "WRITE_BLOCKING") => Ok(Some(Value::Int(0))),
        (AUDIO_TRACK, "WRITE_NON_BLOCKING") => Ok(Some(Value::Int(1))),
        (PLAYBACK_STATE, "STATE_NONE") => Ok(Some(Value::Int(0))),
        (PLAYBACK_STATE, "STATE_STOPPED") => Ok(Some(Value::Int(1))),
        (PLAYBACK_STATE, "STATE_PAUSED") => Ok(Some(Value::Int(2))),
        (PLAYBACK_STATE, "STATE_PLAYING") => Ok(Some(Value::Int(3))),
        (PLAYBACK_STATE, "ACTION_STOP") => Ok(Some(Value::Long(1))),
        (PLAYBACK_STATE, "ACTION_PAUSE") => Ok(Some(Value::Long(2))),
        (PLAYBACK_STATE, "ACTION_PLAY") => Ok(Some(Value::Long(4))),
        (PLAYBACK_STATE, "ACTION_SKIP_TO_PREVIOUS") => Ok(Some(Value::Long(16))),
        (PLAYBACK_STATE, "ACTION_SKIP_TO_NEXT") => Ok(Some(Value::Long(32))),
        (PLAYBACK_STATE, "ACTION_SEEK_TO") => Ok(Some(Value::Long(256))),
        (MEDIA_METADATA, "METADATA_KEY_TITLE")
        | (MEDIA_METADATA, "METADATA_KEY_ARTIST")
        | (MEDIA_METADATA, "METADATA_KEY_ALBUM")
        | (MEDIA_METADATA, "METADATA_KEY_DURATION") => str_val(vm, name).map(Some),
        _ => Ok(None),
    }
}

pub(super) fn static_call(
    vm: &mut Engine,
    class: &str,
    name: &str,
    _sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match (class, name) {
        (AUDIO_RECORD | AUDIO_TRACK, "getMinBufferSize") => {
            let sr = int_arg(args, 0);
            if sr <= 0 {
                return Ok(Some(Value::Int(-2))); // ERROR_BAD_VALUE
            }
            let min = ((sr / 10).max(1024)) * 2;
            Ok(Some(Value::Int(min)))
        }
        (AUDIO_TRACK, "getNativeOutputSampleRate") => Ok(Some(Value::Int(48_000))),
        (MEDIA_PLAYER, "create") => {
            let mp = alloc(vm, MEDIA_PLAYER)?;
            vm.fw.vdev.media_players.insert(
                mp,
                MediaPlayerEntry {
                    state: MediaPlayerState::Prepared,
                    data_source: "res://created".into(),
                    ..MediaPlayerEntry::default()
                },
            );
            Ok(Some(Value::Obj(mp)))
        }
        _ => Ok(None),
    }
}

pub(super) fn instance_call(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match class {
        MEDIA_SESSION_CALLBACK | PROJECTION_CALLBACK => {
            if name == "<init>" {
                return Ok(Some(Value::Null));
            }
            Ok(None)
        }
        AUDIO_MANAGER => audio_manager_call(vm, name, sig, args),
        AUDIO_FOCUS_REQ_BUILDER => match name {
            "<init>" => {
                put_field(vm, recv, "rd_gain", Value::Int(int_arg(args, 0)))?;
                Ok(Some(Value::Null))
            }
            "setOnAudioFocusChangeListener" => {
                let l = args.first().cloned().unwrap_or(Value::Null);
                put_field(vm, recv, "rd_listener", l)?;
                Ok(Some(Value::Obj(recv)))
            }
            "setAudioAttributes"
            | "setAcceptsDelayedFocusGain"
            | "setWillPauseWhenDucked"
            | "setForceDucking" => Ok(Some(Value::Obj(recv))),
            "build" => {
                let gain = get_int_field(vm, recv, "rd_gain", 1);
                let l = get_field(vm, recv, "rd_listener").unwrap_or(Value::Null);
                let r = alloc_with(
                    vm,
                    AUDIO_FOCUS_REQ,
                    vec![
                        ("rd_gain".into(), Value::Int(gain)),
                        ("rd_listener".into(), l),
                    ],
                )?;
                Ok(Some(Value::Obj(r)))
            }
            _ => Ok(None),
        },
        AUDIO_ATTRIBUTES_BUILDER => match name {
            "<init>" => Ok(Some(Value::Null)),
            "setUsage" | "setContentType" | "setFlags" | "setLegacyStreamType" => {
                Ok(Some(Value::Obj(recv)))
            }
            "build" => Ok(Some(Value::Obj(alloc(vm, AUDIO_ATTRIBUTES)?))),
            _ => Ok(None),
        },
        AUDIO_FORMAT_BUILDER => match name {
            "<init>" => Ok(Some(Value::Null)),
            "setSampleRate" => {
                put_field(vm, recv, "rd_sr", Value::Int(int_arg(args, 0)))?;
                Ok(Some(Value::Obj(recv)))
            }
            "setEncoding" | "setChannelMask" | "setChannelIndexMask" => Ok(Some(Value::Obj(recv))),
            "build" => {
                let sr = get_int_field(vm, recv, "rd_sr", 44_100);
                let r = alloc_with(vm, AUDIO_FORMAT, vec![("rd_sr".into(), Value::Int(sr))])?;
                Ok(Some(Value::Obj(r)))
            }
            _ => Ok(None),
        },
        AUDIO_RECORD_BUILDER => match name {
            "<init>" => Ok(Some(Value::Null)),
            "setAudioSource" | "setAudioFormat" | "setBufferSizeInBytes" => {
                Ok(Some(Value::Obj(recv)))
            }
            "build" => {
                check_perm(vm, "android.permission.RECORD_AUDIO")?;
                let r = alloc(vm, AUDIO_RECORD)?;
                vm.fw.vdev.audio_records.insert(
                    r,
                    AudioRecordState {
                        sample_rate: 44_100,
                        channels: 1,
                        recording: false,
                        bytes_read: 0,
                    },
                );
                Ok(Some(Value::Obj(r)))
            }
            _ => Ok(None),
        },
        AUDIO_RECORD => audio_record_call(vm, name, recv, args),
        AUDIO_TRACK_BUILDER => match name {
            "<init>" => Ok(Some(Value::Null)),
            "setAudioAttributes"
            | "setAudioFormat"
            | "setBufferSizeInBytes"
            | "setTransferMode"
            | "setPerformanceMode" => Ok(Some(Value::Obj(recv))),
            "build" => {
                let r = alloc(vm, AUDIO_TRACK)?;
                vm.fw.vdev.audio_tracks.insert(
                    r,
                    AudioTrackState {
                        sample_rate: 48_000,
                        play_state: 1,
                        bytes_written: 0,
                        last_samples_preview: Vec::new(),
                    },
                );
                Ok(Some(Value::Obj(r)))
            }
            _ => Ok(None),
        },
        AUDIO_TRACK => audio_track_call(vm, name, recv, args),
        MEDIA_PLAYER => media_player_call(vm, name, recv, args),
        MEDIA_SESSION_MANAGER => match name {
            "getActiveSessions" => {
                let ctrls: Vec<Value> = vm
                    .fw
                    .vdev
                    .media_sessions
                    .iter()
                    .filter(|(_, s)| s.active)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .map(|(k, _)| *k)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .map(|sess| {
                        alloc_with(
                            vm,
                            MEDIA_CONTROLLER,
                            vec![("rd_session".into(), Value::Obj(sess))],
                        )
                        .map(Value::Obj)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Some(super::array_list(vm, ctrls)?))
            }
            _ => Ok(None),
        },
        MEDIA_SESSION => media_session_call(vm, name, recv, args),
        MEDIA_CONTROLLER => match name {
            "<init>" => {
                let sess = obj_arg(args, 1)
                    .and_then(|tok| get_field(vm, tok, "rd_session"))
                    .unwrap_or(Value::Null);
                put_field(vm, recv, "rd_session", sess)?;
                Ok(Some(Value::Null))
            }
            "getTransportControls" => {
                let sess = get_field(vm, recv, "rd_session").unwrap_or(Value::Null);
                let tc = alloc_with(vm, MEDIA_TRANSPORT, vec![("rd_session".into(), sess)])?;
                Ok(Some(Value::Obj(tc)))
            }
            "getPlaybackState" => {
                let st = match get_field(vm, recv, "rd_session") {
                    Some(Value::Obj(s)) => vm
                        .fw
                        .vdev
                        .media_sessions
                        .get(&s)
                        .and_then(|e| e.playback_state)
                        .map(Value::Obj)
                        .unwrap_or(Value::Null),
                    _ => Value::Null,
                };
                Ok(Some(st))
            }
            "getMetadata" => {
                let md = match get_field(vm, recv, "rd_session") {
                    Some(Value::Obj(s)) => vm
                        .fw
                        .vdev
                        .media_sessions
                        .get(&s)
                        .and_then(|e| e.metadata)
                        .map(Value::Obj)
                        .unwrap_or(Value::Null),
                    _ => Value::Null,
                };
                Ok(Some(md))
            }
            _ => Ok(None),
        },
        MEDIA_TRANSPORT => {
            let Some(Value::Obj(sess)) = get_field(vm, recv, "rd_session") else {
                return Ok(Some(Value::Null));
            };
            let cb = vm
                .fw
                .vdev
                .media_sessions
                .get(&sess)
                .and_then(|s| s.callback);
            match name {
                "play" | "pause" | "stop" | "skipToNext" | "skipToPrevious" => {
                    let cb_name = match name {
                        "play" => "onPlay",
                        "pause" => "onPause",
                        "stop" => "onStop",
                        "skipToNext" => "onSkipToNext",
                        _ => "onSkipToPrevious",
                    };
                    if let Some(c) = cb {
                        let _ = call_dex(vm, c, cb_name, "()V", Vec::new())?;
                    }
                    Ok(Some(Value::Null))
                }
                "seekTo" => {
                    let pos = long_arg(args, 0);
                    if let Some(c) = cb {
                        let _ = call_dex(vm, c, "onSeekTo", "(J)V", vec![Value::Long(pos)])?;
                    }
                    Ok(Some(Value::Null))
                }
                _ => Ok(None),
            }
        }
        PLAYBACK_STATE_BUILDER => match name {
            "<init>" => Ok(Some(Value::Null)),
            "setState" => {
                put_field(vm, recv, "rd_state", Value::Int(int_arg(args, 0)))?;
                put_field(vm, recv, "rd_pos", Value::Long(long_arg(args, 1)))?;
                put_field(vm, recv, "rd_speed", Value::Float(float_arg(args, 2)))?;
                Ok(Some(Value::Obj(recv)))
            }
            "setActions" => {
                put_field(vm, recv, "rd_actions", Value::Long(long_arg(args, 0)))?;
                Ok(Some(Value::Obj(recv)))
            }
            "build" => {
                let st = alloc_with(
                    vm,
                    PLAYBACK_STATE,
                    vec![
                        (
                            "rd_state".into(),
                            Value::Int(get_int_field(vm, recv, "rd_state", 0)),
                        ),
                        (
                            "rd_pos".into(),
                            Value::Long(get_long_field(vm, recv, "rd_pos", 0)),
                        ),
                        (
                            "rd_speed".into(),
                            Value::Float(get_float_field(vm, recv, "rd_speed", 1.0)),
                        ),
                        (
                            "rd_actions".into(),
                            Value::Long(get_long_field(vm, recv, "rd_actions", 0)),
                        ),
                    ],
                )?;
                Ok(Some(Value::Obj(st)))
            }
            _ => Ok(None),
        },
        PLAYBACK_STATE => match name {
            "getState" => Ok(Some(Value::Int(get_int_field(vm, recv, "rd_state", 0)))),
            "getPosition" => Ok(Some(Value::Long(get_long_field(vm, recv, "rd_pos", 0)))),
            "getPlaybackSpeed" => Ok(Some(Value::Float(get_float_field(
                vm, recv, "rd_speed", 1.0,
            )))),
            "getActions" => Ok(Some(Value::Long(get_long_field(vm, recv, "rd_actions", 0)))),
            _ => Ok(None),
        },
        MEDIA_METADATA_BUILDER => match name {
            "<init>" => Ok(Some(Value::Null)),
            "putString" | "putText" => {
                if let Some(k) = opt_str_arg(vm, args, 0)? {
                    let v = args.get(1).cloned().unwrap_or(Value::Null);
                    put_field(vm, recv, &format!("rd_md_{k}"), v)?;
                }
                Ok(Some(Value::Obj(recv)))
            }
            "putLong" => {
                if let Some(k) = opt_str_arg(vm, args, 0)? {
                    put_field(
                        vm,
                        recv,
                        &format!("rd_md_{k}"),
                        Value::Long(long_arg(args, 1)),
                    )?;
                }
                Ok(Some(Value::Obj(recv)))
            }
            "putBitmap" => Ok(Some(Value::Obj(recv))),
            "build" => {
                let mut fields = Vec::new();
                if let Ok(crate::heap::HeapObj::Instance { fields: src, .. }) = vm.heap.get(recv) {
                    fields = src.clone();
                }
                Ok(Some(Value::Obj(alloc_with(vm, MEDIA_METADATA, fields)?)))
            }
            _ => Ok(None),
        },
        MEDIA_METADATA => match name {
            "getString" | "getText" => {
                let k = opt_str_arg(vm, args, 0)?.unwrap_or_default();
                Ok(Some(
                    get_field(vm, recv, &format!("rd_md_{k}")).unwrap_or(Value::Null),
                ))
            }
            "getLong" => {
                let k = opt_str_arg(vm, args, 0)?.unwrap_or_default();
                Ok(Some(Value::Long(get_long_field(
                    vm,
                    recv,
                    &format!("rd_md_{k}"),
                    0,
                ))))
            }
            _ => Ok(None),
        },
        PROJECTION_MANAGER => match name {
            "createScreenCaptureIntent" => {
                let intent = alloc(vm, "Landroid/content/Intent;")?;
                vm.attach_host_state("Landroid/content/Intent;", intent)?;
                Ok(Some(Value::Obj(intent)))
            }
            "getMediaProjection" => {
                let result_code = int_arg(args, 0);
                // Android Activity.RESULT_OK == -1
                if result_code != -1 || obj_arg(args, 1).is_none() {
                    return Ok(Some(Value::Null));
                }
                let proj = alloc(vm, PROJECTION)?;
                vm.fw.vdev.media_projections.insert(
                    proj,
                    MediaProjectionEntry {
                        callbacks: Vec::new(),
                        displays: Vec::new(),
                        stopped: false,
                    },
                );
                Ok(Some(Value::Obj(proj)))
            }
            _ => Ok(None),
        },
        PROJECTION => match name {
            "registerCallback" => {
                if let (Some(cb), Some(entry)) = (
                    obj_arg(args, 0),
                    vm.fw.vdev.media_projections.get_mut(&recv),
                ) {
                    if !entry.callbacks.contains(&cb) {
                        entry.callbacks.push(cb);
                    }
                }
                Ok(Some(Value::Null))
            }
            "unregisterCallback" => {
                if let (Some(cb), Some(entry)) = (
                    obj_arg(args, 0),
                    vm.fw.vdev.media_projections.get_mut(&recv),
                ) {
                    entry.callbacks.retain(|x| *x != cb);
                }
                Ok(Some(Value::Null))
            }
            "createVirtualDisplay" => {
                let stopped = vm
                    .fw
                    .vdev
                    .media_projections
                    .get(&recv)
                    .map(|p| p.stopped)
                    .unwrap_or(true);
                if stopped {
                    return Err(illegal_state("MediaProjection has been stopped"));
                }
                let w = int_arg(args, 1);
                let h = int_arg(args, 2);
                let dpi = int_arg(args, 3);
                if w <= 0 || h <= 0 || dpi <= 0 {
                    return Err(illegal("VirtualDisplay width, height, and dpi must be > 0"));
                }
                let surface = obj_arg(args, 5);
                let vd = alloc_with(
                    vm,
                    VIRTUAL_DISPLAY,
                    vec![
                        ("rd_w".into(), Value::Int(w)),
                        ("rd_h".into(), Value::Int(h)),
                        ("rd_dpi".into(), Value::Int(dpi)),
                    ],
                )?;
                if let Some(entry) = vm.fw.vdev.media_projections.get_mut(&recv) {
                    entry.displays.push(vd);
                }
                if let Some(surf) = surface {
                    // Render deterministic RGBA_8888 screen capture frame into target surface
                    let px = ((w as usize) * (h as usize)).min(16_384);
                    let mut rgba = Vec::with_capacity(px * 4);
                    for i in 0..px {
                        rgba.push(((i * 3) & 0xFF) as u8);
                        rgba.push(((i * 7 + 40) & 0xFF) as u8);
                        rgba.push(((i * 11 + 80) & 0xFF) as u8);
                        rgba.push(0xFF);
                    }
                    let _ = enqueue_frame_to_surface(vm, surf, w, h, 1, &rgba)?;
                }
                Ok(Some(Value::Obj(vd)))
            }
            "stop" => {
                let cbs = if let Some(entry) = vm.fw.vdev.media_projections.get_mut(&recv) {
                    if entry.stopped {
                        Vec::new()
                    } else {
                        entry.stopped = true;
                        entry.callbacks.clone()
                    }
                } else {
                    Vec::new()
                };
                for cb in cbs {
                    let _ = call_dex(vm, cb, "onStop", "()V", Vec::new())?;
                }
                Ok(Some(Value::Null))
            }
            _ => Ok(None),
        },
        VIRTUAL_DISPLAY => match name {
            "release" => Ok(Some(Value::Null)),
            _ => Ok(None),
        },
        _ => Ok(None),
    }
}

fn audio_manager_call(
    vm: &mut Engine,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match name {
        "getStreamVolume" => {
            let s = int_arg(args, 0);
            let cur = vm
                .fw
                .vdev
                .audio
                .stream_volumes
                .get(&s)
                .map(|(c, _)| *c)
                .unwrap_or(5);
            Ok(Some(Value::Int(cur)))
        }
        "getStreamMaxVolume" => {
            let s = int_arg(args, 0);
            let max = vm
                .fw
                .vdev
                .audio
                .stream_volumes
                .get(&s)
                .map(|(_, m)| *m)
                .unwrap_or(7);
            Ok(Some(Value::Int(max)))
        }
        "getStreamMinVolume" => Ok(Some(Value::Int(0))),
        "setStreamVolume" => {
            let s = int_arg(args, 0);
            let idx = int_arg(args, 1);
            let max = vm
                .fw
                .vdev
                .audio
                .stream_volumes
                .get(&s)
                .map(|(_, m)| *m)
                .unwrap_or(15);
            vm.fw
                .vdev
                .audio
                .stream_volumes
                .insert(s, (idx.clamp(0, max), max));
            Ok(Some(Value::Null))
        }
        "isVolumeFixed" => Ok(Some(Value::Int(0))),
        "getRingerMode" => Ok(Some(Value::Int(vm.fw.vdev.audio.ringer_mode))),
        "setRingerMode" => {
            vm.fw.vdev.audio.ringer_mode = int_arg(args, 0).clamp(0, 2);
            Ok(Some(Value::Null))
        }
        "getMode" => Ok(Some(Value::Int(vm.fw.vdev.audio.mode))),
        "setMode" => {
            vm.fw.vdev.audio.mode = int_arg(args, 0);
            Ok(Some(Value::Null))
        }
        "isSpeakerphoneOn" => Ok(Some(Value::Int(i32::from(
            vm.fw.vdev.audio.speakerphone_on,
        )))),
        "setSpeakerphoneOn" => {
            vm.fw.vdev.audio.speakerphone_on = bool_arg(args, 0);
            Ok(Some(Value::Null))
        }
        "isMicrophoneMute" => Ok(Some(Value::Int(i32::from(
            vm.fw.vdev.audio.microphone_mute,
        )))),
        "setMicrophoneMute" => {
            vm.fw.vdev.audio.microphone_mute = bool_arg(args, 0);
            Ok(Some(Value::Null))
        }
        "isMusicActive" => {
            let playing = vm
                .fw
                .vdev
                .media_players
                .values()
                .any(|p| p.state == MediaPlayerState::Started)
                || vm.fw.vdev.audio_tracks.values().any(|t| t.play_state == 3);
            Ok(Some(Value::Int(i32::from(playing))))
        }
        "requestAudioFocus" => {
            let (listener, stream, gain) = if sig.contains("AudioFocusRequest;") {
                let req = obj_arg(args, 0);
                let l = req.and_then(|r| match get_field(vm, r, "rd_listener") {
                    Some(Value::Obj(o)) => Some(o),
                    _ => None,
                });
                let g = req.map(|r| get_int_field(vm, r, "rd_gain", 1)).unwrap_or(1);
                (l, 3, g)
            } else {
                (obj_arg(args, 0), int_arg(args, 1), int_arg(args, 2))
            };
            let loss_code = match gain {
                2 => -2, // AUDIOFOCUS_LOSS_TRANSIENT
                3 => -3, // AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK
                _ => -1, // AUDIOFOCUS_LOSS
            };
            if let Some(prev) = vm.fw.vdev.audio.focus_stack.last().cloned() {
                if prev.listener != listener {
                    if let Some(prev_l) = prev.listener {
                        let _ = call_dex(
                            vm,
                            prev_l,
                            "onAudioFocusChange",
                            "(I)V",
                            vec![Value::Int(loss_code)],
                        )?;
                    }
                }
            }
            vm.fw
                .vdev
                .audio
                .focus_stack
                .retain(|e| e.listener != listener);
            vm.fw.vdev.audio.focus_stack.push(AudioFocusEntry {
                listener,
                stream_type: stream,
                duration_hint: gain,
            });
            Ok(Some(Value::Int(1))) // AUDIOFOCUS_REQUEST_GRANTED
        }
        "abandonAudioFocus" | "abandonAudioFocusRequest" => {
            let listener = if name == "abandonAudioFocusRequest" {
                obj_arg(args, 0).and_then(|r| match get_field(vm, r, "rd_listener") {
                    Some(Value::Obj(o)) => Some(o),
                    _ => None,
                })
            } else {
                obj_arg(args, 0)
            };
            vm.fw
                .vdev
                .audio
                .focus_stack
                .retain(|e| e.listener != listener);
            Ok(Some(Value::Int(1)))
        }
        _ => Ok(None),
    }
}

fn audio_record_call(
    vm: &mut Engine,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match name {
        "<init>" => {
            check_perm(vm, "android.permission.RECORD_AUDIO")?;
            let sr = int_arg(args, 1).max(8_000);
            vm.fw.vdev.audio_records.insert(
                recv,
                AudioRecordState {
                    sample_rate: sr,
                    channels: 1,
                    recording: false,
                    bytes_read: 0,
                },
            );
            Ok(Some(Value::Null))
        }
        "getState" => Ok(Some(Value::Int(i32::from(
            vm.fw.vdev.audio_records.contains_key(&recv),
        )))),
        "getRecordingState" => {
            let st = vm
                .fw
                .vdev
                .audio_records
                .get(&recv)
                .map(|r| if r.recording { 3 } else { 1 })
                .unwrap_or(1);
            Ok(Some(Value::Int(st)))
        }
        "getSampleRate" => Ok(Some(Value::Int(
            vm.fw
                .vdev
                .audio_records
                .get(&recv)
                .map(|r| r.sample_rate)
                .unwrap_or(44_100),
        ))),
        "startRecording" => {
            let Some(st) = vm.fw.vdev.audio_records.get_mut(&recv) else {
                return Err(illegal_state("AudioRecord not initialized"));
            };
            st.recording = true;
            Ok(Some(Value::Null))
        }
        "stop" => {
            if let Some(st) = vm.fw.vdev.audio_records.get_mut(&recv) {
                st.recording = false;
            }
            Ok(Some(Value::Null))
        }
        "release" => {
            vm.fw.vdev.audio_records.remove(&recv);
            Ok(Some(Value::Null))
        }
        "read" => {
            let recording = vm
                .fw
                .vdev
                .audio_records
                .get(&recv)
                .map(|r| r.recording)
                .unwrap_or(false);
            if !recording {
                return Ok(Some(Value::Int(-3))); // ERROR_INVALID_OPERATION
            }
            let Some(dst) = obj_arg(args, 0) else {
                return Ok(Some(Value::Int(-2)));
            };
            let cls = vm.heap.class_of(dst).ok().unwrap_or("").to_string();
            if cls == "Ljava/nio/ByteBuffer;" {
                let req_bytes = int_arg(args, 1).max(0) as usize;
                let mut chunk = vec![0u8; req_bytes];
                let avail = vm.fw.vdev.audio.pcm_input_queue.len().min(req_bytes);
                if avail > 0 {
                    chunk[..avail].copy_from_slice(&vm.fw.vdev.audio.pcm_input_queue[..avail]);
                    vm.fw.vdev.audio.pcm_input_queue.drain(..avail);
                }
                if let Some(Value::Obj(arr)) = get_field(vm, dst, "rd_buf") {
                    let cap = array_len(vm, arr).unwrap_or(0);
                    let n = req_bytes.min(cap);
                    let elems = vm.heap.array_elems_mut(arr)?;
                    for i in 0..n {
                        elems[i] = Value::Int(i32::from(chunk[i] as i8));
                    }
                    put_field(vm, dst, "rd_pos", Value::Int(0))?;
                    put_field(vm, dst, "rd_limit", Value::Int(n as i32))?;
                    if let Some(st) = vm.fw.vdev.audio_records.get_mut(&recv) {
                        st.bytes_read += n as u64;
                    }
                    return Ok(Some(Value::Int(n as i32)));
                }
                return Ok(Some(Value::Int(0)));
            }
            let off = int_arg(args, 1).max(0) as usize;
            let count = int_arg(args, 2).max(0) as usize;
            let arr_len = array_len(vm, dst)?;
            if off + count > arr_len {
                return Ok(Some(Value::Int(-2)));
            }
            if cls == "[S" {
                let bytes_needed = count * 2;
                let mut raw = vec![0u8; bytes_needed];
                let avail = vm.fw.vdev.audio.pcm_input_queue.len().min(bytes_needed);
                if avail > 0 {
                    raw[..avail].copy_from_slice(&vm.fw.vdev.audio.pcm_input_queue[..avail]);
                    vm.fw.vdev.audio.pcm_input_queue.drain(..avail);
                }
                let elems = vm.heap.array_elems_mut(dst)?;
                for i in 0..count {
                    let sample = i16::from_le_bytes([raw[i * 2], raw[i * 2 + 1]]);
                    elems[off + i] = Value::Int(i32::from(sample));
                }
                if let Some(st) = vm.fw.vdev.audio_records.get_mut(&recv) {
                    st.bytes_read += bytes_needed as u64;
                }
                Ok(Some(Value::Int(count as i32)))
            } else {
                let mut raw = vec![0u8; count];
                let avail = vm.fw.vdev.audio.pcm_input_queue.len().min(count);
                if avail > 0 {
                    raw[..avail].copy_from_slice(&vm.fw.vdev.audio.pcm_input_queue[..avail]);
                    vm.fw.vdev.audio.pcm_input_queue.drain(..avail);
                }
                let elems = vm.heap.array_elems_mut(dst)?;
                for i in 0..count {
                    elems[off + i] = Value::Int(i32::from(raw[i] as i8));
                }
                if let Some(st) = vm.fw.vdev.audio_records.get_mut(&recv) {
                    st.bytes_read += count as u64;
                }
                Ok(Some(Value::Int(count as i32)))
            }
        }
        _ => Ok(None),
    }
}

fn audio_track_call(
    vm: &mut Engine,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match name {
        "<init>" => {
            let sr = int_arg(args, 1).max(8_000);
            vm.fw.vdev.audio_tracks.insert(
                recv,
                AudioTrackState {
                    sample_rate: sr,
                    play_state: 1,
                    bytes_written: 0,
                    last_samples_preview: Vec::new(),
                },
            );
            Ok(Some(Value::Null))
        }
        "getState" => Ok(Some(Value::Int(i32::from(
            vm.fw.vdev.audio_tracks.contains_key(&recv),
        )))),
        "getPlayState" => Ok(Some(Value::Int(
            vm.fw
                .vdev
                .audio_tracks
                .get(&recv)
                .map(|t| t.play_state)
                .unwrap_or(1),
        ))),
        "play" => {
            if let Some(t) = vm.fw.vdev.audio_tracks.get_mut(&recv) {
                t.play_state = 3;
            }
            Ok(Some(Value::Null))
        }
        "pause" => {
            if let Some(t) = vm.fw.vdev.audio_tracks.get_mut(&recv) {
                t.play_state = 2;
            }
            Ok(Some(Value::Null))
        }
        "stop" | "flush" => {
            if let Some(t) = vm.fw.vdev.audio_tracks.get_mut(&recv) {
                if name == "stop" {
                    t.play_state = 1;
                }
            }
            Ok(Some(Value::Null))
        }
        "release" => {
            vm.fw.vdev.audio_tracks.remove(&recv);
            Ok(Some(Value::Null))
        }
        "write" => {
            let Some(src) = obj_arg(args, 0) else {
                return Ok(Some(Value::Int(-2)));
            };
            let cls = vm.heap.class_of(src).ok().unwrap_or("").to_string();
            if cls == "Ljava/nio/ByteBuffer;" {
                let n = int_arg(args, 1).max(0) as usize;
                let pos = get_int_field(vm, src, "rd_pos", 0) as usize;
                let mut preview = Vec::new();
                if let Some(Value::Obj(arr)) = get_field(vm, src, "rd_buf") {
                    let bytes = read_byte_array(vm, arr)?;
                    let end = (pos + n).min(bytes.len());
                    preview.extend_from_slice(&bytes[pos..end.min(pos + 64)]);
                    put_field(vm, src, "rd_pos", Value::Int(end as i32))?;
                }
                if let Some(t) = vm.fw.vdev.audio_tracks.get_mut(&recv) {
                    t.bytes_written += n as u64;
                    t.last_samples_preview = preview;
                }
                return Ok(Some(Value::Int(n as i32)));
            }
            let off = int_arg(args, 1).max(0) as usize;
            let count = int_arg(args, 2).max(0) as usize;
            let bytes_written = if cls == "[S" { count * 2 } else { count };
            if cls == "[B" {
                let bytes = read_byte_array(vm, src)?;
                let end = (off + count).min(bytes.len());
                if let Some(t) = vm.fw.vdev.audio_tracks.get_mut(&recv) {
                    t.bytes_written += (end.saturating_sub(off)) as u64;
                    t.last_samples_preview = bytes[off..end.min(off + 64)].to_vec();
                }
            } else if let Some(t) = vm.fw.vdev.audio_tracks.get_mut(&recv) {
                t.bytes_written += bytes_written as u64;
            }
            Ok(Some(Value::Int(count as i32)))
        }
        _ => Ok(None),
    }
}

fn media_player_call(
    vm: &mut Engine,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match name {
        "<init>" => {
            vm.fw
                .vdev
                .media_players
                .insert(recv, MediaPlayerEntry::default());
            Ok(Some(Value::Null))
        }
        "setDataSource" => {
            let ds = opt_str_arg(vm, args, 0)?.unwrap_or_else(|| "uri://datasource".into());
            let entry = vm.fw.vdev.media_players.entry(recv).or_default();
            if entry.state != MediaPlayerState::Idle {
                return Err(illegal_state(
                    "MediaPlayer.setDataSource requires Idle state",
                ));
            }
            entry.data_source = ds;
            entry.state = MediaPlayerState::Initialized;
            Ok(Some(Value::Null))
        }
        "prepare" | "prepareAsync" => {
            let cb = {
                let entry = vm.fw.vdev.media_players.entry(recv).or_default();
                if !matches!(
                    entry.state,
                    MediaPlayerState::Initialized | MediaPlayerState::Stopped
                ) {
                    return Err(illegal_state(
                        "MediaPlayer.prepare requires Initialized or Stopped state",
                    ));
                }
                entry.state = MediaPlayerState::Prepared;
                entry.on_prepared
            };
            if let Some(l) = cb {
                let _ = call_dex(
                    vm,
                    l,
                    "onPrepared",
                    "(Landroid/media/MediaPlayer;)V",
                    vec![Value::Obj(recv)],
                )?;
            }
            Ok(Some(Value::Null))
        }
        "start" => {
            let entry = vm.fw.vdev.media_players.entry(recv).or_default();
            if !matches!(
                entry.state,
                MediaPlayerState::Prepared
                    | MediaPlayerState::Started
                    | MediaPlayerState::Paused
                    | MediaPlayerState::PlaybackCompleted
            ) {
                return Err(illegal_state("MediaPlayer.start in invalid state"));
            }
            entry.state = MediaPlayerState::Started;
            Ok(Some(Value::Null))
        }
        "pause" => {
            let entry = vm.fw.vdev.media_players.entry(recv).or_default();
            if !matches!(
                entry.state,
                MediaPlayerState::Started
                    | MediaPlayerState::Paused
                    | MediaPlayerState::PlaybackCompleted
            ) {
                return Err(illegal_state("MediaPlayer.pause in invalid state"));
            }
            entry.state = MediaPlayerState::Paused;
            Ok(Some(Value::Null))
        }
        "stop" => {
            let entry = vm.fw.vdev.media_players.entry(recv).or_default();
            if matches!(entry.state, MediaPlayerState::Idle | MediaPlayerState::End) {
                return Err(illegal_state("MediaPlayer.stop in invalid state"));
            }
            entry.state = MediaPlayerState::Stopped;
            Ok(Some(Value::Null))
        }
        "seekTo" => {
            let pos = int_arg(args, 0).max(0);
            let cb = {
                let entry = vm.fw.vdev.media_players.entry(recv).or_default();
                if matches!(
                    entry.state,
                    MediaPlayerState::Idle
                        | MediaPlayerState::Initialized
                        | MediaPlayerState::Stopped
                        | MediaPlayerState::End
                ) {
                    return Err(illegal_state("MediaPlayer.seekTo in invalid state"));
                }
                entry.position_ms = pos.min(entry.duration_ms);
                entry.on_seek_complete
            };
            if let Some(l) = cb {
                let _ = call_dex(
                    vm,
                    l,
                    "onSeekComplete",
                    "(Landroid/media/MediaPlayer;)V",
                    vec![Value::Obj(recv)],
                )?;
            }
            Ok(Some(Value::Null))
        }
        "reset" => {
            if let Some(entry) = vm.fw.vdev.media_players.get_mut(&recv) {
                entry.state = MediaPlayerState::Idle;
                entry.position_ms = 0;
                entry.data_source.clear();
            }
            Ok(Some(Value::Null))
        }
        "release" => {
            if let Some(entry) = vm.fw.vdev.media_players.get_mut(&recv) {
                entry.state = MediaPlayerState::End;
            }
            vm.fw.vdev.media_players.remove(&recv);
            Ok(Some(Value::Null))
        }
        "isPlaying" => {
            let on = vm
                .fw
                .vdev
                .media_players
                .get(&recv)
                .map(|e| e.state == MediaPlayerState::Started)
                .unwrap_or(false);
            Ok(Some(Value::Int(i32::from(on))))
        }
        "getCurrentPosition" => Ok(Some(Value::Int(
            vm.fw
                .vdev
                .media_players
                .get(&recv)
                .map(|e| e.position_ms)
                .unwrap_or(0),
        ))),
        "getDuration" => Ok(Some(Value::Int(
            vm.fw
                .vdev
                .media_players
                .get(&recv)
                .map(|e| e.duration_ms)
                .unwrap_or(0),
        ))),
        "setLooping" => {
            let entry = vm.fw.vdev.media_players.entry(recv).or_default();
            entry.looping = bool_arg(args, 0);
            Ok(Some(Value::Null))
        }
        "isLooping" => Ok(Some(Value::Int(i32::from(
            vm.fw
                .vdev
                .media_players
                .get(&recv)
                .map(|e| e.looping)
                .unwrap_or(false),
        )))),
        "setVolume" => {
            let entry = vm.fw.vdev.media_players.entry(recv).or_default();
            entry.left_vol = float_arg(args, 0);
            entry.right_vol = float_arg(args, 1);
            Ok(Some(Value::Null))
        }
        "setAudioAttributes" | "setAudioStreamType" | "setWakeMode" | "setScreenOnWhilePlaying" => {
            Ok(Some(Value::Null))
        }
        "setOnPreparedListener" => {
            vm.fw
                .vdev
                .media_players
                .entry(recv)
                .or_default()
                .on_prepared = obj_arg(args, 0);
            Ok(Some(Value::Null))
        }
        "setOnCompletionListener" => {
            vm.fw
                .vdev
                .media_players
                .entry(recv)
                .or_default()
                .on_completion = obj_arg(args, 0);
            Ok(Some(Value::Null))
        }
        "setOnSeekCompleteListener" => {
            vm.fw
                .vdev
                .media_players
                .entry(recv)
                .or_default()
                .on_seek_complete = obj_arg(args, 0);
            Ok(Some(Value::Null))
        }
        "setOnErrorListener" => {
            vm.fw.vdev.media_players.entry(recv).or_default().on_error = obj_arg(args, 0);
            Ok(Some(Value::Null))
        }
        _ => Ok(None),
    }
}

fn media_session_call(
    vm: &mut Engine,
    name: &str,
    recv: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match name {
        "<init>" => {
            let tag = opt_str_arg(vm, args, 1)?.unwrap_or_else(|| "default".into());
            vm.fw.vdev.media_sessions.insert(
                recv,
                MediaSessionEntry {
                    tag,
                    active: false,
                    callback: None,
                    playback_state: None,
                    metadata: None,
                    state_code: 0,
                    position_ms: 0,
                    title: None,
                    artist: None,
                },
            );
            Ok(Some(Value::Null))
        }
        "setActive" => {
            if let Some(s) = vm.fw.vdev.media_sessions.get_mut(&recv) {
                s.active = bool_arg(args, 0);
            }
            Ok(Some(Value::Null))
        }
        "isActive" => Ok(Some(Value::Int(i32::from(
            vm.fw
                .vdev
                .media_sessions
                .get(&recv)
                .map(|s| s.active)
                .unwrap_or(false),
        )))),
        "setCallback" => {
            if let Some(s) = vm.fw.vdev.media_sessions.get_mut(&recv) {
                s.callback = obj_arg(args, 0);
            }
            Ok(Some(Value::Null))
        }
        "setPlaybackState" => {
            let st_obj = obj_arg(args, 0);
            let (code, pos) = st_obj
                .map(|r| {
                    (
                        get_int_field(vm, r, "rd_state", 0),
                        get_long_field(vm, r, "rd_pos", 0),
                    )
                })
                .unwrap_or((0, 0));
            if let Some(s) = vm.fw.vdev.media_sessions.get_mut(&recv) {
                s.playback_state = st_obj;
                s.state_code = code;
                s.position_ms = pos;
            }
            Ok(Some(Value::Null))
        }
        "setMetadata" => {
            let md_obj = obj_arg(args, 0);
            let title = md_obj.and_then(|r| {
                super::get_str_field(vm, r, "rd_md_METADATA_KEY_TITLE")
                    .or_else(|| super::get_str_field(vm, r, "rd_md_android.media.metadata.TITLE"))
            });
            let artist = md_obj.and_then(|r| {
                super::get_str_field(vm, r, "rd_md_METADATA_KEY_ARTIST")
                    .or_else(|| super::get_str_field(vm, r, "rd_md_android.media.metadata.ARTIST"))
            });
            if let Some(s) = vm.fw.vdev.media_sessions.get_mut(&recv) {
                s.metadata = md_obj;
                s.title = title;
                s.artist = artist;
            }
            Ok(Some(Value::Null))
        }
        "getController" => {
            let c = alloc_with(
                vm,
                MEDIA_CONTROLLER,
                vec![("rd_session".into(), Value::Obj(recv))],
            )?;
            Ok(Some(Value::Obj(c)))
        }
        "getSessionToken" => {
            let t = alloc_with(
                vm,
                MEDIA_SESSION_TOKEN,
                vec![("rd_session".into(), Value::Obj(recv))],
            )?;
            Ok(Some(Value::Obj(t)))
        }
        "setFlags" | "setSessionActivity" | "setMediaButtonReceiver" => Ok(Some(Value::Null)),
        "release" => {
            vm.fw.vdev.media_sessions.remove(&recv);
            Ok(Some(Value::Null))
        }
        _ => Ok(None),
    }
}

impl Engine {
    /// Enqueue virtual PCM input bytes for `AudioRecord.read(...)`.
    pub fn m10_push_audio_input(&mut self, pcm_bytes: &[u8]) {
        self.fw
            .vdev
            .audio
            .pcm_input_queue
            .extend_from_slice(pcm_bytes);
    }

    /// Advance/complete any started `MediaPlayer`s and fire `OnCompletionListener`.
    pub fn m10_complete_media_players(&mut self) -> Result<usize, VmExit> {
        let mut callbacks = Vec::new();
        for (mp, entry) in &mut self.fw.vdev.media_players {
            if entry.state == MediaPlayerState::Started {
                if entry.looping {
                    entry.position_ms = 0;
                } else {
                    entry.position_ms = entry.duration_ms;
                    entry.state = MediaPlayerState::PlaybackCompleted;
                    if let Some(cb) = entry.on_completion {
                        callbacks.push((*mp, cb));
                    }
                }
            }
        }
        if callbacks.is_empty() {
            return Ok(0);
        }
        self.with_host_roots(|vm| {
            let mut fired = 0usize;
            for (mp, cb) in callbacks {
                if call_dex(
                    vm,
                    cb,
                    "onCompletion",
                    "(Landroid/media/MediaPlayer;)V",
                    vec![Value::Obj(mp)],
                )?
                .is_some()
                {
                    fired += 1;
                }
            }
            Ok(fired)
        })
    }

    /// Dispatch a media session transport action (`play`, `pause`, `stop`, `next`,
    /// `previous`, `seek`, or `complete_player`) to active `MediaSession` callbacks.
    pub fn m10_media_action(
        &mut self,
        action: &str,
        seek_ms: Option<i64>,
    ) -> Result<usize, VmExit> {
        if action == "complete_player" {
            return self.m10_complete_media_players();
        }
        let cbs: Vec<ObjRef> = self
            .fw
            .vdev
            .media_sessions
            .values()
            .filter(|s| s.active)
            .filter_map(|s| s.callback)
            .collect();
        if cbs.is_empty() {
            return Ok(0);
        }
        let method = match action {
            "play" => "onPlay",
            "pause" => "onPause",
            "stop" => "onStop",
            "next" => "onSkipToNext",
            "previous" => "onSkipToPrevious",
            "seek" => "onSeekTo",
            _ => return Err(illegal(format!("unknown media_action: {action}"))),
        };
        self.with_host_roots(|vm| {
            let mut fired = 0usize;
            for cb in cbs {
                let res = if method == "onSeekTo" {
                    call_dex(
                        vm,
                        cb,
                        method,
                        "(J)V",
                        vec![Value::Long(seek_ms.unwrap_or(0))],
                    )?
                } else {
                    call_dex(vm, cb, method, "()V", Vec::new())?
                };
                if res.is_some() {
                    fired += 1;
                }
            }
            Ok(fired)
        })
    }
}
