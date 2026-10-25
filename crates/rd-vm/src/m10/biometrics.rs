//! `MEDIA-08`: Virtual Biometrics (`android.hardware.biometrics`, `androidx.biometric`,
//! and `android.hardware.fingerprint.FingerprintManager`).

use super::{alloc, alloc_with, call_dex, get_field, illegal, obj_arg, put_field, str_val};
use crate::{Engine, ObjRef, Value, VmExit};

const FW_BM_MANAGER: &str = "Landroid/hardware/biometrics/BiometricManager;";
const FW_BM_PROMPT: &str = "Landroid/hardware/biometrics/BiometricPrompt;";
const FW_BM_BUILDER: &str = "Landroid/hardware/biometrics/BiometricPrompt$Builder;";
const FW_BM_CALLBACK: &str = "Landroid/hardware/biometrics/BiometricPrompt$AuthenticationCallback;";
const FW_BM_RESULT: &str = "Landroid/hardware/biometrics/BiometricPrompt$AuthenticationResult;";
const FW_BM_CRYPTO: &str = "Landroid/hardware/biometrics/BiometricPrompt$CryptoObject;";

const AX_BM_MANAGER: &str = "Landroidx/biometric/BiometricManager;";
const AX_BM_PROMPT: &str = "Landroidx/biometric/BiometricPrompt;";
const AX_BM_INFO: &str = "Landroidx/biometric/BiometricPrompt$PromptInfo;";
const AX_BM_INFO_BUILDER: &str = "Landroidx/biometric/BiometricPrompt$PromptInfo$Builder;";
const AX_BM_CALLBACK: &str = "Landroidx/biometric/BiometricPrompt$AuthenticationCallback;";
const AX_BM_RESULT: &str = "Landroidx/biometric/BiometricPrompt$AuthenticationResult;";
const AX_BM_CRYPTO: &str = "Landroidx/biometric/BiometricPrompt$CryptoObject;";

const FP_MANAGER: &str = "Landroid/hardware/fingerprint/FingerprintManager;";
const FP_CALLBACK: &str =
    "Landroid/hardware/fingerprint/FingerprintManager$AuthenticationCallback;";
const FP_RESULT: &str = "Landroid/hardware/fingerprint/FingerprintManager$AuthenticationResult;";
const FP_CRYPTO: &str = "Landroid/hardware/fingerprint/FingerprintManager$CryptoObject;";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BiometricKind {
    Framework,
    AndroidX,
    Fingerprint,
}

#[derive(Debug, Clone)]
pub struct ActiveBiometricPrompt {
    pub kind: BiometricKind,
    pub callback: ObjRef,
    pub crypto: Option<ObjRef>,
}

#[derive(Debug, Clone)]
pub enum BiometricOutcome {
    Pending,
    Success,
    Failed,
    Error { code: i32, message: String },
}

#[derive(Debug, Clone)]
pub struct BiometricState {
    pub hardware_present: bool,
    pub enrolled: bool,
    pub auto_outcome: BiometricOutcome,
    pub active_prompt: Option<ActiveBiometricPrompt>,
    pub auth_attempts: u32,
}

impl Default for BiometricState {
    fn default() -> Self {
        Self {
            hardware_present: true,
            enrolled: true,
            auto_outcome: BiometricOutcome::Pending,
            active_prompt: None,
            auth_attempts: 0,
        }
    }
}

impl BiometricState {
    pub fn can_authenticate_code(&self) -> i32 {
        if !self.hardware_present {
            12 // BIOMETRIC_ERROR_NO_HARDWARE
        } else if !self.enrolled {
            11 // BIOMETRIC_ERROR_NONE_ENROLLED
        } else {
            0 // BIOMETRIC_SUCCESS
        }
    }
}

fn deliver_prompt_outcome(
    vm: &mut Engine,
    prompt: &ActiveBiometricPrompt,
    outcome: &BiometricOutcome,
) -> Result<bool, VmExit> {
    match outcome {
        BiometricOutcome::Pending => Ok(false),
        BiometricOutcome::Success => {
            let (res_cls, sig) = match prompt.kind {
                BiometricKind::Framework => (
                    FW_BM_RESULT,
                    "(Landroid/hardware/biometrics/BiometricPrompt$AuthenticationResult;)V",
                ),
                BiometricKind::AndroidX => (
                    AX_BM_RESULT,
                    "(Landroidx/biometric/BiometricPrompt$AuthenticationResult;)V",
                ),
                BiometricKind::Fingerprint => (
                    FP_RESULT,
                    "(Landroid/hardware/fingerprint/FingerprintManager$AuthenticationResult;)V",
                ),
            };
            let crypto_val = prompt.crypto.map(Value::Obj).unwrap_or(Value::Null);
            let res_obj = alloc_with(
                vm,
                res_cls,
                vec![
                    ("rd_crypto".into(), crypto_val),
                    ("rd_auth_type".into(), Value::Int(2)), // AUTHENTICATION_RESULT_TYPE_BIOMETRIC
                ],
            )?;
            Ok(call_dex(
                vm,
                prompt.callback,
                "onAuthenticationSucceeded",
                sig,
                vec![Value::Obj(res_obj)],
            )?
            .is_some())
        }
        BiometricOutcome::Failed => Ok(call_dex(
            vm,
            prompt.callback,
            "onAuthenticationFailed",
            "()V",
            Vec::new(),
        )?
        .is_some()),
        BiometricOutcome::Error { code, message } => {
            let msg = str_val(vm, message.clone())?;
            Ok(call_dex(
                vm,
                prompt.callback,
                "onAuthenticationError",
                "(ILjava/lang/CharSequence;)V",
                vec![Value::Int(*code), msg],
            )?
            .is_some())
        }
    }
}

pub(super) fn host_static(
    _vm: &mut Engine,
    owner: &str,
    name: &str,
) -> Result<Option<Value>, VmExit> {
    match (owner, name) {
        (FW_BM_MANAGER | AX_BM_MANAGER, "BIOMETRIC_SUCCESS") => Ok(Some(Value::Int(0))),
        (FW_BM_MANAGER | AX_BM_MANAGER, "BIOMETRIC_ERROR_HW_UNAVAILABLE") => {
            Ok(Some(Value::Int(1)))
        }
        (FW_BM_MANAGER | AX_BM_MANAGER, "BIOMETRIC_ERROR_NONE_ENROLLED") => {
            Ok(Some(Value::Int(11)))
        }
        (FW_BM_MANAGER | AX_BM_MANAGER, "BIOMETRIC_ERROR_NO_HARDWARE") => Ok(Some(Value::Int(12))),
        (FW_BM_PROMPT | AX_BM_PROMPT, "ERROR_HW_UNAVAILABLE") => Ok(Some(Value::Int(1))),
        (FW_BM_PROMPT | AX_BM_PROMPT, "ERROR_TIMEOUT") => Ok(Some(Value::Int(3))),
        (FW_BM_PROMPT | AX_BM_PROMPT, "ERROR_CANCELED") => Ok(Some(Value::Int(5))),
        (FW_BM_PROMPT | AX_BM_PROMPT, "ERROR_LOCKOUT") => Ok(Some(Value::Int(7))),
        (FW_BM_PROMPT | AX_BM_PROMPT, "ERROR_LOCKOUT_PERMANENT") => Ok(Some(Value::Int(9))),
        (FW_BM_PROMPT | AX_BM_PROMPT, "ERROR_USER_CANCELED") => Ok(Some(Value::Int(10))),
        (FW_BM_PROMPT | AX_BM_PROMPT, "ERROR_NO_BIOMETRICS") => Ok(Some(Value::Int(11))),
        (FW_BM_PROMPT | AX_BM_PROMPT, "ERROR_HW_NOT_PRESENT") => Ok(Some(Value::Int(12))),
        (FW_BM_PROMPT | AX_BM_PROMPT, "ERROR_NEGATIVE_BUTTON") => Ok(Some(Value::Int(13))),
        (FW_BM_PROMPT | AX_BM_PROMPT, "AUTHENTICATION_RESULT_TYPE_BIOMETRIC") => {
            Ok(Some(Value::Int(2)))
        }
        _ => Ok(None),
    }
}

pub(super) fn static_call(
    vm: &mut Engine,
    class: &str,
    name: &str,
    _sig: &str,
    _args: &[Value],
) -> Result<Option<Value>, VmExit> {
    match (class, name) {
        (AX_BM_MANAGER, "from") => Ok(Some(Value::Obj(alloc(vm, AX_BM_MANAGER)?))),
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
        FW_BM_CALLBACK | AX_BM_CALLBACK | FP_CALLBACK => {
            if name == "<init>" {
                return Ok(Some(Value::Null));
            }
            Ok(None)
        }
        FW_BM_CRYPTO | AX_BM_CRYPTO | FP_CRYPTO => match name {
            "<init>" => {
                let inner = args.first().cloned().unwrap_or(Value::Null);
                put_field(vm, recv, "rd_cipher", inner)?;
                Ok(Some(Value::Null))
            }
            "getCipher" | "getSignature" | "getMac" => Ok(Some(
                get_field(vm, recv, "rd_cipher").unwrap_or(Value::Null),
            )),
            _ => Ok(None),
        },
        FW_BM_RESULT | AX_BM_RESULT | FP_RESULT => match name {
            "getCryptoObject" => Ok(Some(
                get_field(vm, recv, "rd_crypto").unwrap_or(Value::Null),
            )),
            "getAuthenticationType" => Ok(Some(
                get_field(vm, recv, "rd_auth_type").unwrap_or(Value::Int(2)),
            )),
            _ => Ok(None),
        },
        FW_BM_MANAGER | AX_BM_MANAGER => match name {
            "canAuthenticate" => Ok(Some(Value::Int(
                vm.fw.vdev.biometrics.can_authenticate_code(),
            ))),
            _ => Ok(None),
        },
        FW_BM_BUILDER => match name {
            "<init>" => Ok(Some(Value::Null)),
            "setTitle"
            | "setSubtitle"
            | "setDescription"
            | "setNegativeButton"
            | "setConfirmationRequired"
            | "setDeviceCredentialAllowed"
            | "setAllowedAuthenticators" => Ok(Some(Value::Obj(recv))),
            "build" => Ok(Some(Value::Obj(alloc(vm, FW_BM_PROMPT)?))),
            _ => Ok(None),
        },
        AX_BM_INFO_BUILDER => match name {
            "<init>" => Ok(Some(Value::Null)),
            "setTitle"
            | "setSubtitle"
            | "setDescription"
            | "setNegativeButtonText"
            | "setConfirmationRequired"
            | "setDeviceCredentialAllowed"
            | "setAllowedAuthenticators" => Ok(Some(Value::Obj(recv))),
            "build" => Ok(Some(Value::Obj(alloc(vm, AX_BM_INFO)?))),
            _ => Ok(None),
        },
        FW_BM_PROMPT => match name {
            "authenticate" => {
                // (CancellationSignal, Executor, AuthenticationCallback) or
                // (CryptoObject, CancellationSignal, Executor, AuthenticationCallback)
                let (crypto, cb) = if sig.contains("CryptoObject;") {
                    (obj_arg(args, 0), obj_arg(args, 3))
                } else {
                    (None, obj_arg(args, 2))
                };
                let Some(cb_ref) = cb else {
                    return Err(illegal("AuthenticationCallback is null"));
                };
                start_biometric_prompt(vm, BiometricKind::Framework, cb_ref, crypto)?;
                Ok(Some(Value::Null))
            }
            _ => Ok(None),
        },
        AX_BM_PROMPT => match name {
            "<init>" => {
                // (FragmentActivity|Fragment, [Executor,] AuthenticationCallback)
                if let Some(cb) = args.last().and_then(|v| match v {
                    Value::Obj(r) => Some(*r),
                    _ => None,
                }) {
                    put_field(vm, recv, "rd_callback", Value::Obj(cb))?;
                }
                Ok(Some(Value::Null))
            }
            "authenticate" => {
                let crypto = if args.len() >= 2 {
                    obj_arg(args, 1)
                } else {
                    None
                };
                let Some(Value::Obj(cb_ref)) = get_field(vm, recv, "rd_callback") else {
                    return Err(illegal("BiometricPrompt missing callback"));
                };
                start_biometric_prompt(vm, BiometricKind::AndroidX, cb_ref, crypto)?;
                Ok(Some(Value::Null))
            }
            "cancelAuthentication" => {
                if let Some(p) = vm.fw.vdev.biometrics.active_prompt.take() {
                    let _ = deliver_prompt_outcome(
                        vm,
                        &p,
                        &BiometricOutcome::Error {
                            code: 5,
                            message: "Authentication canceled".into(),
                        },
                    )?;
                }
                Ok(Some(Value::Null))
            }
            _ => Ok(None),
        },
        FP_MANAGER => match name {
            "isHardwareDetected" => Ok(Some(Value::Int(i32::from(
                vm.fw.vdev.biometrics.hardware_present,
            )))),
            "hasEnrolledFingerprints" => Ok(Some(Value::Int(i32::from(
                vm.fw.vdev.biometrics.hardware_present && vm.fw.vdev.biometrics.enrolled,
            )))),
            "authenticate" => {
                // (CryptoObject, CancellationSignal, int flags, AuthenticationCallback, Handler)
                let crypto = obj_arg(args, 0);
                let Some(cb_ref) = obj_arg(args, 3) else {
                    return Err(illegal("FingerprintManager AuthenticationCallback is null"));
                };
                start_biometric_prompt(vm, BiometricKind::Fingerprint, cb_ref, crypto)?;
                Ok(Some(Value::Null))
            }
            _ => Ok(None),
        },
        _ => Ok(None),
    }
}

fn start_biometric_prompt(
    vm: &mut Engine,
    kind: BiometricKind,
    callback: ObjRef,
    crypto: Option<ObjRef>,
) -> Result<(), VmExit> {
    vm.fw.vdev.biometrics.auth_attempts += 1;
    if !vm.fw.vdev.biometrics.hardware_present {
        let prompt = ActiveBiometricPrompt {
            kind,
            callback,
            crypto,
        };
        let _ = deliver_prompt_outcome(
            vm,
            &prompt,
            &BiometricOutcome::Error {
                code: 12,
                message: "No biometric hardware present".into(),
            },
        )?;
        return Ok(());
    }
    if !vm.fw.vdev.biometrics.enrolled {
        let prompt = ActiveBiometricPrompt {
            kind,
            callback,
            crypto,
        };
        let _ = deliver_prompt_outcome(
            vm,
            &prompt,
            &BiometricOutcome::Error {
                code: 11,
                message: "No biometrics enrolled".into(),
            },
        )?;
        return Ok(());
    }
    let prompt = ActiveBiometricPrompt {
        kind,
        callback,
        crypto,
    };
    let auto = vm.fw.vdev.biometrics.auto_outcome.clone();
    match auto {
        BiometricOutcome::Pending => {
            vm.fw.vdev.biometrics.active_prompt = Some(prompt);
        }
        other => {
            let _ = deliver_prompt_outcome(vm, &prompt, &other)?;
        }
    }
    Ok(())
}

impl Engine {
    /// Configure biometric hardware/enrollment state and/or trigger an outcome on any
    /// waiting `BiometricPrompt` (plus optionally persist as `auto_outcome` for future prompts).
    pub fn m10_trigger_biometric(
        &mut self,
        outcome: BiometricOutcome,
        persist_auto: bool,
    ) -> Result<bool, VmExit> {
        if persist_auto {
            self.fw.vdev.biometrics.auto_outcome = outcome.clone();
        }
        let active = match &outcome {
            BiometricOutcome::Failed => self.fw.vdev.biometrics.active_prompt.clone(),
            BiometricOutcome::Pending => None,
            _ => self.fw.vdev.biometrics.active_prompt.take(),
        };
        let Some(prompt) = active else {
            return Ok(false);
        };
        self.with_host_roots(|vm| deliver_prompt_outcome(vm, &prompt, &outcome))
    }

    pub fn m10_set_biometric_hardware(&mut self, present: bool, enrolled: bool) {
        self.fw.vdev.biometrics.hardware_present = present;
        self.fw.vdev.biometrics.enrolled = enrolled;
    }
}
