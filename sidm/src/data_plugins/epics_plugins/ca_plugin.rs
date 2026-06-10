//! `ca://` — EPICS Channel Access backend (feature `ca`).
//!
//! Ports `pydm/data_plugins/epics_plugin.py` (the pyepics CA connection) onto
//! [`epics_ca_rs`]. One async task per pooled connection drives a single
//! [`CaChannel`] with a [`tokio::select!`] loop over four sources:
//!
//! - **connection events** ([`CaChannel::connection_events`]) — connect /
//!   disconnect / access-rights / native-type-changed,
//! - **the monitor** ([`CaChannel::subscribe`]) — value + alarm + timestamp,
//! - **the GUI write queue** — [`crate::Channel::put`] values, and
//! - **cancellation** — fired when the last [`crate::Channel`] drops.
//!
//! On connect (and reconnect / native-type change) the task issues one
//! `get_with_metadata(DbrClass::Ctrl)` to publish units / precision / limits /
//! enum strings together with the initial value, then the monitor streams
//! value+alarm updates (metadata is connect-time, refetched on
//! [`ConnectionEvent::NativeTypeChanged`], matching PyDM). On disconnect the
//! stale value is kept and only `connected` flips, which drives
//! [`crate::AlarmSeverity::Disconnected`] styling.
//!
//! The [`CaClient`] is created lazily on first connect and shared across every
//! `ca://` connection (one client per engine), mirroring PyDM's process-wide
//! pyepics context.
//!
//! **Write path:** [`pv_to_epics_basic`] is the minimal scalar/array coercion
//! used for the round-trip test; native-type-aware coercion (incl. string→enum)
//! and write-while-disconnected handling are hardened in a follow-up commit.

use std::sync::Arc;
use std::time::Duration;

use epics_base_rs::server::snapshot::{DbrClass, Snapshot};
use epics_base_rs::types::EpicsValue;
use epics_ca_rs::client::{CaChannel, CaClient, ConnectionEvent};
use tokio::sync::{OnceCell, broadcast, mpsc};
use tokio_util::sync::CancellationToken;

use crate::channel::{AlarmSeverity, ChannelState, PvValue, StateWriter};
use crate::data_plugins::{ConnectionCtx, DataPlugin};
use crate::engine::EngineError;

/// The `ca://` data plugin. Holds the lazily-initialized, engine-shared
/// [`CaClient`] (PyDM's process-wide pyepics context).
pub struct CaPlugin {
    client: Arc<OnceCell<Arc<CaClient>>>,
}

impl CaPlugin {
    /// Create the plugin. The CA client is not built until the first
    /// `ca://` connection (so a plugin-less headless build pays nothing).
    pub fn new() -> Self {
        Self {
            client: Arc::new(OnceCell::new()),
        }
    }
}

impl Default for CaPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl DataPlugin for CaPlugin {
    fn protocol(&self) -> &'static str {
        "ca"
    }

    fn connect(&self, ctx: ConnectionCtx) -> Result<(), EngineError> {
        let ConnectionCtx {
            writer,
            writes,
            cancel,
            runtime,
            address,
        } = ctx;
        let pv = address.full_address();
        let client = self.client.clone();
        runtime.spawn(run_channel(client, pv, writer, writes, cancel));
        Ok(())
    }
}

/// Service one CA connection until cancelled or the channel shuts down.
async fn run_channel(
    client_cell: Arc<OnceCell<Arc<CaClient>>>,
    pv: String,
    writer: StateWriter,
    mut writes: mpsc::UnboundedReceiver<PvValue>,
    cancel: CancellationToken,
) {
    // One CA client per engine, created on first use.
    let client = match client_cell
        .get_or_try_init(|| async { CaClient::new().await.map(Arc::new) })
        .await
    {
        Ok(c) => c.clone(),
        Err(_) => {
            // Client construction failed — leave the channel disconnected.
            writer.update(|s| s.connected = false);
            return;
        }
    };

    let ch = client.create_channel(&pv);
    let mut events = ch.connection_events();
    let mut monitor = match ch.subscribe().await {
        Ok(m) => m,
        Err(_) => {
            writer.update(|s| s.connected = false);
            return;
        }
    };

    let mut enum_cache: Option<Arc<[String]>> = None;
    let mut connected_now = false;

    // Deterministic first-connect trigger. `connection_events` is a broadcast
    // subscribed just above, so a `Connected` posted before that subscribe
    // would be missed. `wait_connected` independently detects an established
    // channel, closing that race; `connected_now` dedups the metadata fetch
    // when both the probe and the `Connected` event fire for one connection.
    let initial = ch.wait_connected(Duration::from_secs(86_400));
    tokio::pin!(initial);
    let mut initial_done = false;

    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,

            res = &mut initial, if !initial_done => {
                initial_done = true;
                if res.is_ok() && !connected_now {
                    connected_now = true;
                    on_connect(&ch, &writer, &mut enum_cache).await;
                }
            }

            ev = events.recv() => match ev {
                Ok(ConnectionEvent::Connected) => {
                    if !connected_now {
                        connected_now = true;
                        on_connect(&ch, &writer, &mut enum_cache).await;
                    }
                }
                Ok(ConnectionEvent::Disconnected | ConnectionEvent::Unresponsive) => {
                    connected_now = false;
                    // Keep the stale value (PyDM behaviour); only `connected`
                    // flips, which drives Disconnected styling.
                    writer.update(|s| s.connected = false);
                }
                Ok(ConnectionEvent::AccessRightsChanged { write, .. }) => {
                    writer.update(move |s| s.write_access = write);
                }
                Ok(ConnectionEvent::NativeTypeChanged { .. }) => {
                    // Record type changed under us — refetch metadata (units,
                    // enum strings, limits) against the new native type.
                    on_connect(&ch, &writer, &mut enum_cache).await;
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            },

            snap = monitor.recv() => match snap {
                Some(Ok(snap)) => {
                    connected_now = true;
                    let strings = enum_cache.clone();
                    writer.update(move |s| apply_value(s, &snap, strings.as_deref()));
                }
                Some(Err(_)) => {}  // transient monitor error; keep the connection
                None => break,      // subscription ended (channel shutdown)
            },

            maybe = writes.recv() => match maybe {
                Some(value) => {
                    if let Some(ev) = pv_to_epics_basic(&value) {
                        let _ = ch.put(&ev).await;
                    }
                }
                None => break,  // all Channels dropped
            },
        }
    }
}

/// Fetch full control metadata and publish it (plus the value/alarm) as one
/// update. Caches enum strings for subsequent monitor label resolution.
async fn on_connect(ch: &CaChannel, writer: &StateWriter, enum_cache: &mut Option<Arc<[String]>>) {
    match ch.get_with_metadata(DbrClass::Ctrl).await {
        Ok(snap) => {
            let strings: Option<Arc<[String]>> = snap
                .enums
                .as_ref()
                .filter(|e| !e.strings.is_empty())
                .map(|e| Arc::from(e.strings.clone()));
            *enum_cache = strings.clone();
            writer.update(move |s| apply_metadata(s, &snap, strings));
        }
        Err(_) => {
            // Connected, but the metadata read failed; reflect the connection so
            // widgets un-gate and let the monitor stream supply the value.
            writer.update(|s| s.connected = true);
        }
    }
}

/// Apply a `DBR_CTRL_*` snapshot: value + alarm + timestamp + units / precision
/// / limits / enum strings. `enum_strings` is moved into the state and reused to
/// resolve the value's enum label.
fn apply_metadata(s: &mut ChannelState, snap: &Snapshot, enum_strings: Option<Arc<[String]>>) {
    s.connected = true;
    s.value = Some(epics_to_pv(&snap.value, enum_strings.as_deref()));
    s.severity = AlarmSeverity::from_epics(snap.alarm.severity);
    s.timestamp = Some(snap.timestamp);
    s.enum_strings = enum_strings;
    if let Some(d) = &snap.display {
        s.units = (!d.units.is_empty()).then(|| Arc::from(d.units.as_str()));
        s.precision = Some(i32::from(d.precision));
        s.display_limits = Some((d.lower_disp_limit, d.upper_disp_limit));
        s.warn_limits = Some((d.lower_warning_limit, d.upper_warning_limit));
        s.alarm_limits = Some((d.lower_alarm_limit, d.upper_alarm_limit));
    }
    if let Some(c) = &snap.control {
        s.ctrl_limits = Some((c.lower_ctrl_limit, c.upper_ctrl_limit));
    }
}

/// Apply a monitor snapshot: value + alarm + timestamp only (metadata is
/// connect-time and is not re-published on every monitor event).
fn apply_value(s: &mut ChannelState, snap: &Snapshot, enum_strings: Option<&[String]>) {
    s.connected = true;
    s.value = Some(epics_to_pv(&snap.value, enum_strings));
    s.severity = AlarmSeverity::from_epics(snap.alarm.severity);
    s.timestamp = Some(snap.timestamp);
}

/// Normalize an [`EpicsValue`] into a [`PvValue`], resolving an enum label from
/// `enum_strings` when available.
fn epics_to_pv(value: &EpicsValue, enum_strings: Option<&[String]>) -> PvValue {
    match value {
        EpicsValue::String(v) => PvValue::Str(Arc::from(v.as_str())),
        EpicsValue::Short(v) => PvValue::Int(i64::from(*v)),
        EpicsValue::Float(v) => PvValue::Float(f64::from(*v)),
        EpicsValue::Enum(i) => PvValue::Enum {
            index: *i,
            label: enum_label(enum_strings, *i),
        },
        EpicsValue::Char(v) => PvValue::Int(i64::from(*v)),
        EpicsValue::Long(v) => PvValue::Int(i64::from(*v)),
        EpicsValue::Double(v) => PvValue::Float(*v),
        EpicsValue::Int64(v) => PvValue::Int(*v),
        EpicsValue::UInt64(v) => PvValue::Int(*v as i64),
        EpicsValue::ShortArray(a) => {
            PvValue::IntArray(a.iter().map(|x| i64::from(*x)).collect::<Vec<_>>().into())
        }
        EpicsValue::FloatArray(a) => {
            PvValue::FloatArray(a.iter().map(|x| f64::from(*x)).collect::<Vec<_>>().into())
        }
        EpicsValue::EnumArray(a) => {
            PvValue::IntArray(a.iter().map(|x| i64::from(*x)).collect::<Vec<_>>().into())
        }
        EpicsValue::DoubleArray(a) => PvValue::FloatArray(Arc::from(a.as_slice())),
        EpicsValue::LongArray(a) => {
            PvValue::IntArray(a.iter().map(|x| i64::from(*x)).collect::<Vec<_>>().into())
        }
        EpicsValue::CharArray(a) => PvValue::Bytes(Arc::from(a.as_slice())),
        EpicsValue::Int64Array(a) => PvValue::IntArray(Arc::from(a.as_slice())),
        EpicsValue::UInt64Array(a) => {
            PvValue::IntArray(a.iter().map(|x| *x as i64).collect::<Vec<_>>().into())
        }
        EpicsValue::StringArray(a) => PvValue::StrArray(Arc::from(a.as_slice())),
    }
}

/// Resolve an enum index to its label string, if `enum_strings` covers it.
fn enum_label(enum_strings: Option<&[String]>, index: u16) -> Option<Arc<str>> {
    enum_strings
        .and_then(|s| s.get(usize::from(index)))
        .map(|label| Arc::from(label.as_str()))
}

/// Minimal `PvValue` → `EpicsValue` coercion for the write path.
///
/// This is the basic round-trip mapping: scalars/arrays map to the closest
/// `EpicsValue` and the IOC coerces to the record's native type on write.
/// Native-type-aware coercion (incl. string→enum lookup and
/// write-while-disconnected handling) is hardened in a follow-up commit.
fn pv_to_epics_basic(value: &PvValue) -> Option<EpicsValue> {
    Some(match value {
        PvValue::Int(v) => EpicsValue::Long(*v as i32),
        PvValue::Float(v) => EpicsValue::Double(*v),
        PvValue::Bool(v) => EpicsValue::Long(i32::from(*v)),
        PvValue::Str(s) => EpicsValue::String(s.to_string()),
        PvValue::Enum { index, .. } => EpicsValue::Enum(*index),
        PvValue::FloatArray(a) => EpicsValue::DoubleArray(a.to_vec()),
        PvValue::IntArray(a) => EpicsValue::LongArray(a.iter().map(|x| *x as i32).collect()),
        PvValue::StrArray(a) => EpicsValue::StringArray(a.to_vec()),
        PvValue::Bytes(a) => EpicsValue::CharArray(a.to_vec()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use epics_base_rs::server::snapshot::{ControlInfo, DisplayInfo, EnumInfo};
    use std::time::{Duration, UNIX_EPOCH};

    fn ts() -> std::time::SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_700_000_000)
    }

    #[test]
    fn scalars_map_to_normalized_values() {
        assert_eq!(
            epics_to_pv(&EpicsValue::Double(1.5), None),
            PvValue::Float(1.5)
        );
        assert_eq!(epics_to_pv(&EpicsValue::Long(7), None), PvValue::Int(7));
        assert_eq!(epics_to_pv(&EpicsValue::Short(-3), None), PvValue::Int(-3));
        assert_eq!(epics_to_pv(&EpicsValue::Char(65), None), PvValue::Int(65));
        assert_eq!(
            epics_to_pv(&EpicsValue::Int64(1 << 40), None),
            PvValue::Int(1 << 40)
        );
        assert_eq!(
            epics_to_pv(&EpicsValue::Float(0.5), None),
            PvValue::Float(0.5)
        );
        assert_eq!(
            epics_to_pv(&EpicsValue::String("hi".into()), None),
            PvValue::Str(Arc::from("hi"))
        );
    }

    #[test]
    fn enum_resolves_label_from_strings() {
        let strings = vec!["OFF".to_owned(), "ON".to_owned()];
        assert_eq!(
            epics_to_pv(&EpicsValue::Enum(1), Some(&strings)),
            PvValue::Enum {
                index: 1,
                label: Some(Arc::from("ON")),
            }
        );
        // No cache, or index out of range → no label.
        assert_eq!(
            epics_to_pv(&EpicsValue::Enum(1), None),
            PvValue::Enum {
                index: 1,
                label: None,
            }
        );
        assert_eq!(enum_label(Some(&strings), 9), None);
    }

    #[test]
    fn arrays_map_to_typed_waveforms() {
        assert_eq!(
            epics_to_pv(&EpicsValue::DoubleArray(vec![1.0, 2.0]), None),
            PvValue::FloatArray(Arc::from([1.0_f64, 2.0].as_slice()))
        );
        assert_eq!(
            epics_to_pv(&EpicsValue::LongArray(vec![3, 4]), None),
            PvValue::IntArray(Arc::from([3_i64, 4].as_slice()))
        );
        assert_eq!(
            epics_to_pv(&EpicsValue::FloatArray(vec![1.5_f32]), None),
            PvValue::FloatArray(Arc::from([1.5_f64].as_slice()))
        );
        // CHAR waveform stays raw bytes (the formatter decides string vs array).
        assert_eq!(
            epics_to_pv(&EpicsValue::CharArray(vec![104, 105, 0]), None),
            PvValue::Bytes(Arc::from([104_u8, 105, 0].as_slice()))
        );
        assert_eq!(
            epics_to_pv(&EpicsValue::StringArray(vec!["a".into(), "b".into()]), None),
            PvValue::StrArray(Arc::from(["a".to_owned(), "b".to_owned()].as_slice()))
        );
    }

    #[test]
    fn metadata_snapshot_populates_units_precision_and_limits() {
        let mut snap = Snapshot::new(EpicsValue::Double(2.5), 0, 1, ts());
        snap.display = Some(DisplayInfo {
            units: "mm".to_owned(),
            precision: 3,
            lower_disp_limit: -10.0,
            upper_disp_limit: 10.0,
            lower_warning_limit: -5.0,
            upper_warning_limit: 5.0,
            lower_alarm_limit: -8.0,
            upper_alarm_limit: 8.0,
            ..Default::default()
        });
        snap.control = Some(ControlInfo {
            lower_ctrl_limit: -9.0,
            upper_ctrl_limit: 9.0,
        });

        let mut state = ChannelState::default();
        apply_metadata(&mut state, &snap, None);

        assert!(state.connected);
        assert_eq!(state.value, Some(PvValue::Float(2.5)));
        assert_eq!(state.severity, AlarmSeverity::Minor);
        assert_eq!(state.units.as_deref(), Some("mm"));
        assert_eq!(state.precision, Some(3));
        assert_eq!(state.display_limits, Some((-10.0, 10.0)));
        assert_eq!(state.warn_limits, Some((-5.0, 5.0)));
        assert_eq!(state.alarm_limits, Some((-8.0, 8.0)));
        assert_eq!(state.ctrl_limits, Some((-9.0, 9.0)));
        assert_eq!(state.timestamp, Some(ts()));
    }

    #[test]
    fn metadata_snapshot_caches_enum_strings_and_resolves_label() {
        let mut snap = Snapshot::new(EpicsValue::Enum(1), 0, 0, ts());
        snap.enums = Some(EnumInfo {
            strings: vec!["OFF".to_owned(), "ON".to_owned()],
        });

        let strings: Option<Arc<[String]>> =
            snap.enums.as_ref().map(|e| Arc::from(e.strings.clone()));
        let mut state = ChannelState::default();
        apply_metadata(&mut state, &snap, strings);

        assert_eq!(
            state.value,
            Some(PvValue::Enum {
                index: 1,
                label: Some(Arc::from("ON")),
            })
        );
        assert_eq!(state.enum_strings.as_deref().map(|s| s.len()), Some(2));
    }

    #[test]
    fn monitor_value_keeps_metadata_and_updates_alarm() {
        let mut state = ChannelState {
            units: Some(Arc::from("mm")),
            precision: Some(3),
            ..Default::default()
        };
        let snap = Snapshot::new(EpicsValue::Double(4.0), 0, 2, ts());
        apply_value(&mut state, &snap, None);

        assert!(state.connected);
        assert_eq!(state.value, Some(PvValue::Float(4.0)));
        assert_eq!(state.severity, AlarmSeverity::Major);
        // Connect-time metadata is untouched by a value update.
        assert_eq!(state.units.as_deref(), Some("mm"));
        assert_eq!(state.precision, Some(3));
    }

    #[test]
    fn basic_write_coercions() {
        assert_eq!(
            pv_to_epics_basic(&PvValue::Float(2.5)),
            Some(EpicsValue::Double(2.5))
        );
        assert_eq!(
            pv_to_epics_basic(&PvValue::Int(9)),
            Some(EpicsValue::Long(9))
        );
        assert_eq!(
            pv_to_epics_basic(&PvValue::Bool(true)),
            Some(EpicsValue::Long(1))
        );
        assert_eq!(
            pv_to_epics_basic(&PvValue::Str(Arc::from("x"))),
            Some(EpicsValue::String("x".to_owned()))
        );
        assert_eq!(
            pv_to_epics_basic(&PvValue::Enum {
                index: 2,
                label: None
            }),
            Some(EpicsValue::Enum(2))
        );
    }
}
