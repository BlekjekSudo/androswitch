//! `NET-05` (`TelephonyManager`, `SmsManager`, `ConnectivityManager`) and
//! `NET-06` (`BluetoothManager`/`BluetoothAdapter`/`BluetoothLeScanner`,
//! `NfcManager`/`NfcAdapter`/`Tag`/`NdefMessage`/`NdefRecord`).

use super::{
    alloc, alloc_with, byte_array, call_dex, check_any_perm, check_perm, get_field, get_int_field,
    illegal, int_arg, obj_arg, obj_array, opt_str_arg, put_field, read_byte_array, req_str_arg,
    str_val, system_service_singleton,
};
use crate::framework::HostObj;
use crate::{Engine, ObjRef, Value, VmExit};

const TELEPHONY_MANAGER: &str = "Landroid/telephony/TelephonyManager;";
const SMS_MANAGER: &str = "Landroid/telephony/SmsManager;";

const CONNECTIVITY_MANAGER: &str = "Landroid/net/ConnectivityManager;";
const NETWORK: &str = "Landroid/net/Network;";
const NETWORK_CAPS: &str = "Landroid/net/NetworkCapabilities;";
const NETWORK_INFO: &str = "Landroid/net/NetworkInfo;";
const WIFI_MANAGER: &str = "Landroid/net/wifi/WifiManager;";

const BT_MANAGER: &str = "Landroid/bluetooth/BluetoothManager;";
const BT_ADAPTER: &str = "Landroid/bluetooth/BluetoothAdapter;";
const BT_DEVICE: &str = "Landroid/bluetooth/BluetoothDevice;";
const BLE_SCANNER: &str = "Landroid/bluetooth/le/BluetoothLeScanner;";
const BLE_SCAN_CALLBACK: &str = "Landroid/bluetooth/le/ScanCallback;";
const BLE_SCAN_RESULT: &str = "Landroid/bluetooth/le/ScanResult;";
const BLE_SCAN_RECORD: &str = "Landroid/bluetooth/le/ScanRecord;";
const BLE_SCAN_SETTINGS_BUILDER: &str = "Landroid/bluetooth/le/ScanSettings$Builder;";
const BLE_SCAN_SETTINGS: &str = "Landroid/bluetooth/le/ScanSettings;";
const BLE_SCAN_FILTER_BUILDER: &str = "Landroid/bluetooth/le/ScanFilter$Builder;";
const BLE_SCAN_FILTER: &str = "Landroid/bluetooth/le/ScanFilter;";

const NFC_MANAGER: &str = "Landroid/nfc/NfcManager;";
const NFC_ADAPTER: &str = "Landroid/nfc/NfcAdapter;";
const NFC_TAG: &str = "Landroid/nfc/Tag;";
const NDEF_MESSAGE: &str = "Landroid/nfc/NdefMessage;";
const NDEF_RECORD: &str = "Landroid/nfc/NdefRecord;";

#[derive(Debug, Clone)]
pub struct OutboundSms {
    pub destination: String,
    pub sc_address: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct InboundSms {
    pub sender: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct TelephonyState {
    pub sim_state: i32, // 5 = SIM_STATE_READY, 1 = SIM_STATE_ABSENT
    pub carrier_name: String,
    pub operator_mcc_mnc: String,
    pub country_iso: String,
    pub network_type: i32, // 13 = LTE, 20 = NR
    pub call_state: i32,   // 0 = IDLE, 1 = RINGING, 2 = OFFHOOK
    pub phone_type: i32,   // 1 = PHONE_TYPE_GSM
    pub data_enabled: bool,
    pub roaming: bool,
    pub phone_number: String,
    pub sent_sms: Vec<OutboundSms>,
    pub received_sms: Vec<InboundSms>,
}

impl Default for TelephonyState {
    fn default() -> Self {
        Self {
            sim_state: 5,
            carrier_name: "RustyDroid Virtual".into(),
            operator_mcc_mnc: "72401".into(),
            country_iso: "br".into(),
            network_type: 13,
            call_state: 0,
            phone_type: 1,
            data_enabled: true,
            roaming: false,
            phone_number: "+5511999990000".into(),
            sent_sms: Vec::new(),
            received_sms: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct NetworkState {
    pub online: bool,
    pub transport: String, // "wifi" | "cellular" | "ethernet" | "none"
    pub metered: bool,
    pub airplane_mode: bool,
    pub latency_ms: u32,
}

impl Default for NetworkState {
    fn default() -> Self {
        Self {
            online: true,
            transport: "wifi".into(),
            metered: false,
            airplane_mode: false,
            latency_ms: 10,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BleAdv {
    pub address: String,
    pub name: String,
    pub rssi: i32,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct NfcTagSpec {
    pub id: Vec<u8>,
    pub text: Option<String>,
    pub uri: Option<String>,
}

fn build_network_caps(vm: &mut Engine, net: &NetworkState) -> Result<ObjRef, VmExit> {
    let transport_code = match net.transport.as_str() {
        "cellular" => 0,
        "wifi" => 1,
        "ethernet" => 3,
        _ => -1,
    };
    alloc_with(
        vm,
        NETWORK_CAPS,
        vec![
            ("rd_online".into(), Value::Int(i32::from(net.online))),
            ("rd_transport".into(), Value::Int(transport_code)),
            ("rd_metered".into(), Value::Int(i32::from(net.metered))),
        ],
    )
}

fn build_scan_result(vm: &mut Engine, adv: &BleAdv) -> Result<ObjRef, VmExit> {
    let addr = str_val(vm, adv.address.clone())?;
    let name = str_val(vm, adv.name.clone())?;
    let dev = alloc_with(
        vm,
        BT_DEVICE,
        vec![("rd_addr".into(), addr), ("rd_name".into(), name.clone())],
    )?;
    let raw = byte_array(vm, &adv.payload)?;
    let rec = alloc_with(
        vm,
        BLE_SCAN_RECORD,
        vec![
            ("rd_bytes".into(), Value::Obj(raw)),
            ("rd_name".into(), name),
        ],
    )?;
    let ts = vm.fw.vdev.next_timestamp_ns();
    alloc_with(
        vm,
        BLE_SCAN_RESULT,
        vec![
            ("rd_device".into(), Value::Obj(dev)),
            ("rd_record".into(), Value::Obj(rec)),
            ("rd_rssi".into(), Value::Int(adv.rssi)),
            ("rd_ts".into(), Value::Long(ts)),
        ],
    )
}

fn build_text_ndef_record(vm: &mut Engine, lang: &str, text: &str) -> Result<ObjRef, VmExit> {
    let lb = lang.as_bytes();
    let tb = text.as_bytes();
    let mut payload = Vec::with_capacity(1 + lb.len() + tb.len());
    payload.push((lb.len() & 0x3F) as u8);
    payload.extend_from_slice(lb);
    payload.extend_from_slice(tb);
    let rtype = byte_array(vm, b"T")?;
    let rid = byte_array(vm, &[])?;
    let rpay = byte_array(vm, &payload)?;
    alloc_with(
        vm,
        NDEF_RECORD,
        vec![
            ("rd_tnf".into(), Value::Int(1)), // TNF_WELL_KNOWN
            ("rd_type".into(), Value::Obj(rtype)),
            ("rd_id".into(), Value::Obj(rid)),
            ("rd_payload".into(), Value::Obj(rpay)),
        ],
    )
}

fn build_uri_ndef_record(vm: &mut Engine, uri: &str) -> Result<ObjRef, VmExit> {
    let ub = uri.as_bytes();
    let mut payload = Vec::with_capacity(1 + ub.len());
    payload.push(0x00); // no prefix abbreviation
    payload.extend_from_slice(ub);
    let rtype = byte_array(vm, b"U")?;
    let rid = byte_array(vm, &[])?;
    let rpay = byte_array(vm, &payload)?;
    alloc_with(
        vm,
        NDEF_RECORD,
        vec![
            ("rd_tnf".into(), Value::Int(1)), // TNF_WELL_KNOWN
            ("rd_type".into(), Value::Obj(rtype)),
            ("rd_id".into(), Value::Obj(rid)),
            ("rd_payload".into(), Value::Obj(rpay)),
        ],
    )
}

pub(super) fn host_static(
    vm: &mut Engine,
    owner: &str,
    name: &str,
) -> Result<Option<Value>, VmExit> {
    match (owner, name) {
        (TELEPHONY_MANAGER, "SIM_STATE_UNKNOWN") => Ok(Some(Value::Int(0))),
        (TELEPHONY_MANAGER, "SIM_STATE_ABSENT") => Ok(Some(Value::Int(1))),
        (TELEPHONY_MANAGER, "SIM_STATE_READY") => Ok(Some(Value::Int(5))),
        (TELEPHONY_MANAGER, "CALL_STATE_IDLE") => Ok(Some(Value::Int(0))),
        (TELEPHONY_MANAGER, "CALL_STATE_RINGING") => Ok(Some(Value::Int(1))),
        (TELEPHONY_MANAGER, "CALL_STATE_OFFHOOK") => Ok(Some(Value::Int(2))),
        (TELEPHONY_MANAGER, "PHONE_TYPE_NONE") => Ok(Some(Value::Int(0))),
        (TELEPHONY_MANAGER, "PHONE_TYPE_GSM") => Ok(Some(Value::Int(1))),
        (TELEPHONY_MANAGER, "NETWORK_TYPE_UNKNOWN") => Ok(Some(Value::Int(0))),
        (TELEPHONY_MANAGER, "NETWORK_TYPE_LTE") => Ok(Some(Value::Int(13))),
        (TELEPHONY_MANAGER, "NETWORK_TYPE_NR") => Ok(Some(Value::Int(20))),
        (CONNECTIVITY_MANAGER, "TYPE_MOBILE") => Ok(Some(Value::Int(0))),
        (CONNECTIVITY_MANAGER, "TYPE_WIFI") => Ok(Some(Value::Int(1))),
        (NETWORK_CAPS, "TRANSPORT_CELLULAR") => Ok(Some(Value::Int(0))),
        (NETWORK_CAPS, "TRANSPORT_WIFI") => Ok(Some(Value::Int(1))),
        (NETWORK_CAPS, "TRANSPORT_ETHERNET") => Ok(Some(Value::Int(3))),
        (NETWORK_CAPS, "NET_CAPABILITY_NOT_METERED") => Ok(Some(Value::Int(11))),
        (NETWORK_CAPS, "NET_CAPABILITY_INTERNET") => Ok(Some(Value::Int(12))),
        (NETWORK_CAPS, "NET_CAPABILITY_VALIDATED") => Ok(Some(Value::Int(16))),
        (BT_ADAPTER, "STATE_OFF") => Ok(Some(Value::Int(10))),
        (BT_ADAPTER, "STATE_TURNING_ON") => Ok(Some(Value::Int(11))),
        (BT_ADAPTER, "STATE_ON") => Ok(Some(Value::Int(12))),
        (BT_ADAPTER, "STATE_TURNING_OFF") => Ok(Some(Value::Int(13))),
        (BLE_SCAN_SETTINGS, "CALLBACK_TYPE_ALL_MATCHES") => Ok(Some(Value::Int(1))),
        (NFC_ADAPTER, "ACTION_NDEF_DISCOVERED") => {
            str_val(vm, "android.nfc.action.NDEF_DISCOVERED").map(Some)
        }
        (NFC_ADAPTER, "ACTION_TAG_DISCOVERED") => {
            str_val(vm, "android.nfc.action.TAG_DISCOVERED").map(Some)
        }
        (NFC_ADAPTER, "ACTION_TECH_DISCOVERED") => {
            str_val(vm, "android.nfc.action.TECH_DISCOVERED").map(Some)
        }
        (NFC_ADAPTER, "EXTRA_TAG") => str_val(vm, "android.nfc.extra.TAG").map(Some),
        (NFC_ADAPTER, "EXTRA_NDEF_MESSAGES") => {
            str_val(vm, "android.nfc.extra.NDEF_MESSAGES").map(Some)
        }
        (NFC_ADAPTER, "EXTRA_ID") => str_val(vm, "android.nfc.extra.ID").map(Some),
        (NFC_ADAPTER, "FLAG_READER_NFC_A") => Ok(Some(Value::Int(1))),
        (NFC_ADAPTER, "FLAG_READER_NFC_B") => Ok(Some(Value::Int(2))),
        (NFC_ADAPTER, "FLAG_READER_NFC_F") => Ok(Some(Value::Int(4))),
        (NFC_ADAPTER, "FLAG_READER_NFC_V") => Ok(Some(Value::Int(8))),
        (NDEF_RECORD, "TNF_EMPTY") => Ok(Some(Value::Int(0))),
        (NDEF_RECORD, "TNF_WELL_KNOWN") => Ok(Some(Value::Int(1))),
        (NDEF_RECORD, "TNF_MIME_MEDIA") => Ok(Some(Value::Int(2))),
        (NDEF_RECORD, "TNF_ABSOLUTE_URI") => Ok(Some(Value::Int(3))),
        (NDEF_RECORD, "TNF_EXTERNAL_TYPE") => Ok(Some(Value::Int(4))),
        (NDEF_RECORD, "RTD_TEXT") => Ok(Some(Value::Obj(byte_array(vm, b"T")?))),
        (NDEF_RECORD, "RTD_URI") => Ok(Some(Value::Obj(byte_array(vm, b"U")?))),
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
        (SMS_MANAGER, "getDefault") => {
            Ok(Some(Value::Obj(system_service_singleton(vm, SMS_MANAGER)?)))
        }
        (BT_ADAPTER, "getDefaultAdapter") => {
            Ok(Some(Value::Obj(system_service_singleton(vm, BT_ADAPTER)?)))
        }
        (BT_ADAPTER, "checkBluetoothAddress") => {
            let ok = opt_str_arg(vm, args, 0)?
                .map(|a| a.len() == 17 && a.split(':').count() == 6)
                .unwrap_or(false);
            Ok(Some(Value::Int(i32::from(ok))))
        }
        (NFC_ADAPTER, "getDefaultAdapter") => {
            Ok(Some(Value::Obj(system_service_singleton(vm, NFC_ADAPTER)?)))
        }
        (NDEF_RECORD, "createTextRecord") => {
            let lang = opt_str_arg(vm, args, 0)?.unwrap_or_else(|| "en".into());
            let txt = req_str_arg(vm, args, 1)?;
            Ok(Some(Value::Obj(build_text_ndef_record(vm, &lang, &txt)?)))
        }
        (NDEF_RECORD, "createUri") => {
            let uri_str = match args.first() {
                Some(Value::Obj(r)) => {
                    let cls = vm.heap.class_of(*r).ok().unwrap_or("");
                    if cls == "Ljava/lang/String;" {
                        vm.heap.as_str(*r)?.to_string()
                    } else {
                        vm.fw.m8.uris.get(r).cloned().unwrap_or_default()
                    }
                }
                _ => return Err(illegal("createUri requires non-null Uri or String")),
            };
            Ok(Some(Value::Obj(build_uri_ndef_record(vm, &uri_str)?)))
        }
        (NDEF_RECORD, "createMime") => {
            let mime = req_str_arg(vm, args, 0)?;
            let pay = obj_arg(args, 1)
                .map(|a| read_byte_array(vm, a))
                .transpose()?
                .unwrap_or_default();
            let rtype = byte_array(vm, mime.as_bytes())?;
            let rid = byte_array(vm, &[])?;
            let rpay = byte_array(vm, &pay)?;
            let rec = alloc_with(
                vm,
                NDEF_RECORD,
                vec![
                    ("rd_tnf".into(), Value::Int(2)),
                    ("rd_type".into(), Value::Obj(rtype)),
                    ("rd_id".into(), Value::Obj(rid)),
                    ("rd_payload".into(), Value::Obj(rpay)),
                ],
            )?;
            Ok(Some(Value::Obj(rec)))
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
        BLE_SCAN_CALLBACK => {
            if name == "<init>" {
                return Ok(Some(Value::Null));
            }
            Ok(None)
        }
        TELEPHONY_MANAGER => match name {
            "getSimState" => Ok(Some(Value::Int(vm.fw.vdev.telephony.sim_state))),
            "getNetworkOperatorName" | "getSimOperatorName" => Ok(Some(str_val(
                vm,
                vm.fw.vdev.telephony.carrier_name.clone(),
            )?)),
            "getNetworkOperator" | "getSimOperator" => Ok(Some(str_val(
                vm,
                vm.fw.vdev.telephony.operator_mcc_mnc.clone(),
            )?)),
            "getNetworkCountryIso" | "getSimCountryIso" => {
                Ok(Some(str_val(vm, vm.fw.vdev.telephony.country_iso.clone())?))
            }
            "getPhoneType" => Ok(Some(Value::Int(vm.fw.vdev.telephony.phone_type))),
            "getCallState" => Ok(Some(Value::Int(vm.fw.vdev.telephony.call_state))),
            "getNetworkType" | "getDataNetworkType" => {
                if name == "getDataNetworkType" {
                    check_perm(vm, "android.permission.READ_PHONE_STATE")?;
                }
                Ok(Some(Value::Int(vm.fw.vdev.telephony.network_type)))
            }
            "isNetworkRoaming" => Ok(Some(Value::Int(i32::from(vm.fw.vdev.telephony.roaming)))),
            "isDataEnabled" => Ok(Some(Value::Int(i32::from(
                vm.fw.vdev.telephony.data_enabled,
            )))),
            "getLine1Number" => {
                check_any_perm(
                    vm,
                    &[
                        "android.permission.READ_PHONE_STATE",
                        "android.permission.READ_SMS",
                        "android.permission.READ_PHONE_NUMBERS",
                    ],
                )?;
                Ok(Some(str_val(
                    vm,
                    vm.fw.vdev.telephony.phone_number.clone(),
                )?))
            }
            "hasIccCard" => Ok(Some(Value::Int(i32::from(
                vm.fw.vdev.telephony.sim_state == 5,
            )))),
            _ => Ok(None),
        },
        SMS_MANAGER => match name {
            "sendTextMessage" => {
                check_perm(vm, "android.permission.SEND_SMS")?;
                let dest = req_str_arg(vm, args, 0)?;
                let sc = opt_str_arg(vm, args, 1)?;
                let text = req_str_arg(vm, args, 2)?;
                if dest.is_empty() || text.is_empty() {
                    return Err(illegal("destinationAddress and text must not be empty"));
                }
                vm.fw.vdev.telephony.sent_sms.push(OutboundSms {
                    destination: dest,
                    sc_address: sc,
                    text,
                });
                #[cfg(feature = "advanced-components")]
                if let Some(sent_pi) = obj_arg(args, 3) {
                    if vm.fw.m9.pending.contains_key(&sent_pi) {
                        vm.m9_send_pending(sent_pi)?;
                    }
                }
                Ok(Some(Value::Null))
            }
            "divideMessage" => {
                let text = req_str_arg(vm, args, 0)?;
                let mut parts = Vec::new();
                let chars: Vec<char> = text.chars().collect();
                for chunk in chars.chunks(160) {
                    let s: String = chunk.iter().collect();
                    parts.push(str_val(vm, s)?);
                }
                if parts.is_empty() {
                    parts.push(str_val(vm, "")?);
                }
                Ok(Some(super::array_list(vm, parts)?))
            }
            _ => Ok(None),
        },
        CONNECTIVITY_MANAGER => match name {
            "getActiveNetwork" => {
                if !vm.fw.vdev.network.online || vm.fw.vdev.network.airplane_mode {
                    return Ok(Some(Value::Null));
                }
                let net = alloc_with(vm, NETWORK, vec![("rd_net_id".into(), Value::Int(100))])?;
                Ok(Some(Value::Obj(net)))
            }
            "getNetworkCapabilities" => {
                if obj_arg(args, 0).is_none()
                    || !vm.fw.vdev.network.online
                    || vm.fw.vdev.network.airplane_mode
                {
                    return Ok(Some(Value::Null));
                }
                let net = vm.fw.vdev.network.clone();
                Ok(Some(Value::Obj(build_network_caps(vm, &net)?)))
            }
            "getActiveNetworkInfo" => {
                if !vm.fw.vdev.network.online || vm.fw.vdev.network.airplane_mode {
                    return Ok(Some(Value::Null));
                }
                let t = if vm.fw.vdev.network.transport == "cellular" {
                    0
                } else {
                    1
                };
                let info = alloc_with(
                    vm,
                    NETWORK_INFO,
                    vec![
                        ("rd_connected".into(), Value::Int(1)),
                        ("rd_type".into(), Value::Int(t)),
                    ],
                )?;
                Ok(Some(Value::Obj(info)))
            }
            "isActiveNetworkMetered" => Ok(Some(Value::Int(i32::from(vm.fw.vdev.network.metered)))),
            "registerDefaultNetworkCallback" | "registerNetworkCallback" => {
                let cb = args.last().and_then(|v| match v {
                    Value::Obj(r) => Some(*r),
                    _ => None,
                });
                if let Some(cb_ref) = cb {
                    if !vm.fw.vdev.network_callbacks.contains(&cb_ref) {
                        vm.fw.vdev.network_callbacks.push(cb_ref);
                    }
                    if vm.fw.vdev.network.online && !vm.fw.vdev.network.airplane_mode {
                        let net =
                            alloc_with(vm, NETWORK, vec![("rd_net_id".into(), Value::Int(100))])?;
                        let _ = call_dex(
                            vm,
                            cb_ref,
                            "onAvailable",
                            "(Landroid/net/Network;)V",
                            vec![Value::Obj(net)],
                        )?;
                    }
                }
                Ok(Some(Value::Null))
            }
            "unregisterNetworkCallback" => {
                if let Some(cb_ref) = obj_arg(args, 0) {
                    vm.fw.vdev.network_callbacks.retain(|c| *c != cb_ref);
                }
                Ok(Some(Value::Null))
            }
            _ => Ok(None),
        },
        NETWORK_CAPS => match name {
            "hasCapability" => {
                let cap = int_arg(args, 0);
                let online = get_int_field(vm, recv, "rd_online", 0) != 0;
                let metered = get_int_field(vm, recv, "rd_metered", 0) != 0;
                let has = match cap {
                    12 | 16 => online,        // INTERNET, VALIDATED
                    11 => online && !metered, // NOT_METERED
                    _ => false,
                };
                Ok(Some(Value::Int(i32::from(has))))
            }
            "hasTransport" => {
                let tr = int_arg(args, 0);
                let cur = get_int_field(vm, recv, "rd_transport", -1);
                Ok(Some(Value::Int(i32::from(tr == cur))))
            }
            _ => Ok(None),
        },
        NETWORK_INFO => match name {
            "isConnected" | "isConnectedOrConnecting" => {
                Ok(Some(Value::Int(get_int_field(vm, recv, "rd_connected", 0))))
            }
            "getType" => Ok(Some(Value::Int(get_int_field(vm, recv, "rd_type", 1)))),
            _ => Ok(None),
        },
        WIFI_MANAGER => match name {
            "isWifiEnabled" => Ok(Some(Value::Int(i32::from(
                !vm.fw.vdev.network.airplane_mode && vm.fw.vdev.network.transport == "wifi",
            )))),
            _ => Ok(None),
        },
        BT_MANAGER => match name {
            "getAdapter" => Ok(Some(Value::Obj(system_service_singleton(vm, BT_ADAPTER)?))),
            _ => Ok(None),
        },
        BT_ADAPTER => match name {
            "isEnabled" => Ok(Some(Value::Int(i32::from(
                vm.fw.vdev.bluetooth_enabled && !vm.fw.vdev.network.airplane_mode,
            )))),
            "getState" => Ok(Some(Value::Int(
                if vm.fw.vdev.bluetooth_enabled && !vm.fw.vdev.network.airplane_mode {
                    12
                } else {
                    10
                },
            ))),
            "getName" => Ok(Some(str_val(vm, vm.fw.vdev.bluetooth_name.clone())?)),
            "setName" => {
                if let Some(n) = opt_str_arg(vm, args, 0)? {
                    vm.fw.vdev.bluetooth_name = n;
                }
                Ok(Some(Value::Int(1)))
            }
            "getAddress" => Ok(Some(str_val(vm, vm.fw.vdev.bluetooth_address.clone())?)),
            "getBondedDevices" => {
                let set = alloc(vm, "Ljava/util/HashSet;")?;
                vm.fw
                    .objects
                    .insert(set, HostObj::List { elems: Vec::new() });
                Ok(Some(Value::Obj(set)))
            }
            "getRemoteDevice" => {
                let addr = req_str_arg(vm, args, 0)?;
                let av = str_val(vm, addr)?;
                let nv = str_val(vm, "Virtual BLE Device")?;
                let d = alloc_with(
                    vm,
                    BT_DEVICE,
                    vec![("rd_addr".into(), av), ("rd_name".into(), nv)],
                )?;
                Ok(Some(Value::Obj(d)))
            }
            "getBluetoothLeScanner" => {
                if !vm.fw.vdev.bluetooth_enabled || vm.fw.vdev.network.airplane_mode {
                    return Ok(Some(Value::Null));
                }
                Ok(Some(Value::Obj(system_service_singleton(vm, BLE_SCANNER)?)))
            }
            _ => Ok(None),
        },
        BT_DEVICE => match name {
            "getAddress" => Ok(Some(get_field(vm, recv, "rd_addr").unwrap_or(Value::Null))),
            "getName" => Ok(Some(get_field(vm, recv, "rd_name").unwrap_or(Value::Null))),
            _ => Ok(None),
        },
        BLE_SCANNER => match name {
            "startScan" => {
                check_any_perm(
                    vm,
                    &[
                        "android.permission.BLUETOOTH_SCAN",
                        "android.permission.BLUETOOTH_ADMIN",
                        "android.permission.ACCESS_FINE_LOCATION",
                    ],
                )?;
                let Some(cb) = args.last().and_then(|v| match v {
                    Value::Obj(r) => Some(*r),
                    _ => None,
                }) else {
                    return Err(illegal("ScanCallback is null"));
                };
                if !vm.fw.vdev.ble_scan_callbacks.contains(&cb) {
                    vm.fw.vdev.ble_scan_callbacks.push(cb);
                }
                let advs = vm.fw.vdev.ble_advertisements.clone();
                for adv in &advs {
                    let sr = build_scan_result(vm, adv)?;
                    let _ = call_dex(
                        vm,
                        cb,
                        "onScanResult",
                        "(ILandroid/bluetooth/le/ScanResult;)V",
                        vec![Value::Int(1), Value::Obj(sr)],
                    )?;
                }
                Ok(Some(Value::Null))
            }
            "stopScan" => {
                if let Some(cb) = obj_arg(args, 0) {
                    vm.fw.vdev.ble_scan_callbacks.retain(|x| *x != cb);
                }
                Ok(Some(Value::Null))
            }
            _ => Ok(None),
        },
        BLE_SCAN_RESULT => match name {
            "getDevice" => Ok(Some(
                get_field(vm, recv, "rd_device").unwrap_or(Value::Null),
            )),
            "getScanRecord" => Ok(Some(
                get_field(vm, recv, "rd_record").unwrap_or(Value::Null),
            )),
            "getRssi" => Ok(Some(Value::Int(get_int_field(vm, recv, "rd_rssi", -60)))),
            "getTimestampNanos" => Ok(Some(get_field(vm, recv, "rd_ts").unwrap_or(Value::Long(0)))),
            _ => Ok(None),
        },
        BLE_SCAN_RECORD => match name {
            "getBytes" => Ok(Some(get_field(vm, recv, "rd_bytes").unwrap_or(Value::Null))),
            "getDeviceName" => Ok(Some(get_field(vm, recv, "rd_name").unwrap_or(Value::Null))),
            _ => Ok(None),
        },
        BLE_SCAN_SETTINGS_BUILDER | BLE_SCAN_FILTER_BUILDER => match name {
            "<init>" => Ok(Some(Value::Null)),
            "build" => {
                let target = if class == BLE_SCAN_SETTINGS_BUILDER {
                    BLE_SCAN_SETTINGS
                } else {
                    BLE_SCAN_FILTER
                };
                Ok(Some(Value::Obj(alloc(vm, target)?)))
            }
            _ => Ok(Some(Value::Obj(recv))),
        },
        NFC_MANAGER => match name {
            "getDefaultAdapter" => Ok(Some(Value::Obj(system_service_singleton(vm, NFC_ADAPTER)?))),
            _ => Ok(None),
        },
        NFC_ADAPTER => match name {
            "isEnabled" => Ok(Some(Value::Int(i32::from(vm.fw.vdev.nfc_enabled)))),
            "enableReaderMode" => {
                vm.fw.vdev.nfc_reader_callback = obj_arg(args, 1);
                Ok(Some(Value::Null))
            }
            "disableReaderMode" => {
                vm.fw.vdev.nfc_reader_callback = None;
                Ok(Some(Value::Null))
            }
            "enableForegroundDispatch" | "disableForegroundDispatch" => Ok(Some(Value::Null)),
            _ => Ok(None),
        },
        NFC_TAG => match name {
            "getId" => Ok(Some(get_field(vm, recv, "rd_id").unwrap_or(Value::Null))),
            "getTechList" => Ok(Some(get_field(vm, recv, "rd_techs").unwrap_or(Value::Null))),
            _ => Ok(None),
        },
        NDEF_RECORD => match name {
            "<init>" => {
                // (short tnf, byte[] type, byte[] id, byte[] payload)
                put_field(vm, recv, "rd_tnf", Value::Int(int_arg(args, 0)))?;
                put_field(
                    vm,
                    recv,
                    "rd_type",
                    args.get(1).cloned().unwrap_or(Value::Null),
                )?;
                put_field(
                    vm,
                    recv,
                    "rd_id",
                    args.get(2).cloned().unwrap_or(Value::Null),
                )?;
                put_field(
                    vm,
                    recv,
                    "rd_payload",
                    args.get(3).cloned().unwrap_or(Value::Null),
                )?;
                Ok(Some(Value::Null))
            }
            "getTnf" => Ok(Some(Value::Int(get_int_field(vm, recv, "rd_tnf", 1)))),
            "getType" => Ok(Some(get_field(vm, recv, "rd_type").unwrap_or(Value::Null))),
            "getId" => Ok(Some(get_field(vm, recv, "rd_id").unwrap_or(Value::Null))),
            "getPayload" => Ok(Some(
                get_field(vm, recv, "rd_payload").unwrap_or(Value::Null),
            )),
            _ => Ok(None),
        },
        NDEF_MESSAGE => match name {
            "<init>" => {
                if sig == "([B)V" {
                    let Some(arr) = obj_arg(args, 0) else {
                        return Err(illegal("NdefMessage null bytes"));
                    };
                    let raw = read_byte_array(vm, arr)?;
                    let rec = build_text_ndef_record(vm, "en", &String::from_utf8_lossy(&raw))?;
                    let recs = obj_array(vm, "[Landroid/nfc/NdefRecord;", vec![Value::Obj(rec)])?;
                    put_field(vm, recv, "rd_records", recs)?;
                } else {
                    let recs = args.first().cloned().unwrap_or(Value::Null);
                    put_field(vm, recv, "rd_records", recs)?;
                }
                Ok(Some(Value::Null))
            }
            "getRecords" => Ok(Some(
                get_field(vm, recv, "rd_records").unwrap_or(Value::Null),
            )),
            "toByteArray" => {
                let mut out = Vec::new();
                if let Some(Value::Obj(recs_arr)) = get_field(vm, recv, "rd_records") {
                    if let Ok((elems, _)) = vm.heap.as_array_elems(recs_arr) {
                        let refs: Vec<ObjRef> = elems
                            .iter()
                            .filter_map(|v| match v {
                                Value::Obj(r) => Some(*r),
                                _ => None,
                            })
                            .collect();
                        for r in refs {
                            if let Some(Value::Obj(pay)) = get_field(vm, r, "rd_payload") {
                                if let Ok(bytes) = read_byte_array(vm, pay) {
                                    out.extend_from_slice(&bytes);
                                }
                            }
                        }
                    }
                }
                Ok(Some(Value::Obj(byte_array(vm, &out)?)))
            }
            _ => Ok(None),
        },
        _ => Ok(None),
    }
}

impl Engine {
    /// Update virtual network connectivity and notify registered `NetworkCallback`s.
    pub fn m10_set_network(
        &mut self,
        online: bool,
        transport: &str,
        metered: bool,
        airplane_mode: bool,
    ) -> Result<usize, VmExit> {
        self.fw.vdev.network.online = online && !airplane_mode;
        self.fw.vdev.network.transport = transport.to_string();
        self.fw.vdev.network.metered = metered;
        self.fw.vdev.network.airplane_mode = airplane_mode;
        let cbs = self.fw.vdev.network_callbacks.clone();
        if cbs.is_empty() {
            return Ok(0);
        }
        let is_up = self.fw.vdev.network.online;
        let net_snap = self.fw.vdev.network.clone();
        self.with_host_roots(|vm| {
            let mut fired = 0usize;
            for cb in cbs {
                let net_obj = alloc_with(vm, NETWORK, vec![("rd_net_id".into(), Value::Int(100))])?;
                if is_up {
                    if call_dex(
                        vm,
                        cb,
                        "onAvailable",
                        "(Landroid/net/Network;)V",
                        vec![Value::Obj(net_obj)],
                    )?
                    .is_some()
                    {
                        fired += 1;
                    }
                    let caps = build_network_caps(vm, &net_snap)?;
                    let _ = call_dex(
                        vm,
                        cb,
                        "onCapabilitiesChanged",
                        "(Landroid/net/Network;Landroid/net/NetworkCapabilities;)V",
                        vec![Value::Obj(net_obj), Value::Obj(caps)],
                    )?;
                } else if call_dex(
                    vm,
                    cb,
                    "onLost",
                    "(Landroid/net/Network;)V",
                    vec![Value::Obj(net_obj)],
                )?
                .is_some()
                {
                    fired += 1;
                }
            }
            Ok(fired)
        })
    }

    /// Inject an inbound SMS and broadcast `android.provider.Telephony.SMS_RECEIVED`.
    pub fn m10_receive_sms(&mut self, sender: &str, text: &str) -> Result<usize, VmExit> {
        self.fw.vdev.telephony.received_sms.push(InboundSms {
            sender: sender.to_string(),
            text: text.to_string(),
        });
        let receivers = self.fw.m8.receivers.clone();
        if receivers.is_empty() {
            return Ok(0);
        }
        let sender_s = sender.to_string();
        let text_s = text.to_string();
        self.with_host_roots(|vm| {
            let intent = alloc(vm, "Landroid/content/Intent;")?;
            vm.attach_host_state("Landroid/content/Intent;", intent)?;
            vm.fw.m8.intents.entry(intent).or_default().action =
                Some("android.provider.Telephony.SMS_RECEIVED".into());
            let sv = str_val(vm, sender_s)?;
            let tv = str_val(vm, text_s)?;
            if let Some(HostObj::Intent { extras, .. }) = vm.fw.objects.get_mut(&intent) {
                extras.push(("sender".into(), sv));
                extras.push(("body".into(), tv));
            }
            let ctx = vm
                .fw
                .current_activity
                .map(Value::Obj)
                .unwrap_or(Value::Null);
            let mut fired = 0usize;
            for r in receivers {
                if call_dex(
                    vm,
                    r,
                    "onReceive",
                    "(Landroid/content/Context;Landroid/content/Intent;)V",
                    vec![ctx.clone(), Value::Obj(intent)],
                )?
                .is_some()
                {
                    fired += 1;
                }
            }
            Ok(fired)
        })
    }

    /// Inject a virtual BLE advertisement and deliver to active `ScanCallback`s.
    pub fn m10_inject_ble_advertisement(
        &mut self,
        address: &str,
        name: &str,
        rssi: i32,
        payload: Vec<u8>,
    ) -> Result<usize, VmExit> {
        let adv = BleAdv {
            address: address.to_string(),
            name: name.to_string(),
            rssi,
            payload,
        };
        self.fw.vdev.ble_advertisements.push(adv.clone());
        let cbs = self.fw.vdev.ble_scan_callbacks.clone();
        if cbs.is_empty() {
            return Ok(0);
        }
        self.with_host_roots(|vm| {
            let mut fired = 0usize;
            for cb in cbs {
                let sr = build_scan_result(vm, &adv)?;
                if call_dex(
                    vm,
                    cb,
                    "onScanResult",
                    "(ILandroid/bluetooth/le/ScanResult;)V",
                    vec![Value::Int(1), Value::Obj(sr)],
                )?
                .is_some()
                {
                    fired += 1;
                }
            }
            Ok(fired)
        })
    }

    /// Inject a virtual NFC Tag + NDEF message, invoking `NfcAdapter.ReaderCallback`
    /// if active or delivering `ACTION_NDEF_DISCOVERED` to `Activity.onNewIntent` and receivers.
    pub fn m10_inject_nfc_tag(
        &mut self,
        tag_id: Vec<u8>,
        text: Option<String>,
        uri: Option<String>,
    ) -> Result<usize, VmExit> {
        self.fw.vdev.nfc_last_tag = Some(NfcTagSpec {
            id: tag_id.clone(),
            text: text.clone(),
            uri: uri.clone(),
        });
        let reader_cb = self.fw.vdev.nfc_reader_callback;
        let cur_act = self.fw.current_activity;
        let receivers = self.fw.m8.receivers.clone();
        self.with_host_roots(|vm| {
            let id_arr = byte_array(vm, &tag_id)?;
            let t1 = str_val(vm, "android.nfc.tech.Ndef")?;
            let t2 = str_val(vm, "android.nfc.tech.NfcA")?;
            let techs = obj_array(vm, "[Ljava/lang/String;", vec![t1, t2])?;
            let tag_obj = alloc_with(
                vm,
                NFC_TAG,
                vec![
                    ("rd_id".into(), Value::Obj(id_arr)),
                    ("rd_techs".into(), techs),
                ],
            )?;

            if let Some(cb) = reader_cb {
                let ok = call_dex(
                    vm,
                    cb,
                    "onTagDiscovered",
                    "(Landroid/nfc/Tag;)V",
                    vec![Value::Obj(tag_obj)],
                )?
                .is_some();
                return Ok(usize::from(ok));
            }

            let rec = if let Some(u) = uri {
                build_uri_ndef_record(vm, &u)?
            } else {
                build_text_ndef_record(vm, "en", text.as_deref().unwrap_or("RustyDroid"))?
            };
            let recs_arr = obj_array(vm, "[Landroid/nfc/NdefRecord;", vec![Value::Obj(rec)])?;
            let msg_obj = alloc_with(vm, NDEF_MESSAGE, vec![("rd_records".into(), recs_arr)])?;
            let msgs_arr = obj_array(vm, "[Landroid/os/Parcelable;", vec![Value::Obj(msg_obj)])?;

            let intent = alloc(vm, "Landroid/content/Intent;")?;
            vm.attach_host_state("Landroid/content/Intent;", intent)?;
            vm.fw.m8.intents.entry(intent).or_default().action =
                Some("android.nfc.action.NDEF_DISCOVERED".into());
            if let Some(HostObj::Intent { extras, .. }) = vm.fw.objects.get_mut(&intent) {
                extras.push(("android.nfc.extra.TAG".into(), Value::Obj(tag_obj)));
                extras.push(("android.nfc.extra.ID".into(), Value::Obj(id_arr)));
                extras.push(("android.nfc.extra.NDEF_MESSAGES".into(), msgs_arr));
            }

            let mut delivered = 0usize;
            if let Some(act) = cur_act {
                if call_dex(
                    vm,
                    act,
                    "onNewIntent",
                    "(Landroid/content/Intent;)V",
                    vec![Value::Obj(intent)],
                )?
                .is_some()
                {
                    delivered += 1;
                }
            }
            let ctx = cur_act.map(Value::Obj).unwrap_or(Value::Null);
            for r in receivers {
                if call_dex(
                    vm,
                    r,
                    "onReceive",
                    "(Landroid/content/Context;Landroid/content/Intent;)V",
                    vec![ctx.clone(), Value::Obj(intent)],
                )?
                .is_some()
                {
                    delivered += 1;
                }
            }
            Ok(delivered)
        })
    }
}
