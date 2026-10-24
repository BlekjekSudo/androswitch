//! API26 TextUtils.isEmpty dispatches the actual CharSequence.length method.
//! In particular, it does not stringify a custom sequence or bypass DEX code.
use super::*;
pub(super) fn call_static(
    vm: &mut Engine,
    class: &str,
    name: &str,
    sig: &str,
    args: &[Value],
) -> Result<Option<Value>> {
    if class != "Landroid/text/TextUtils;"
        || name != "isEmpty"
        || sig != "(Ljava/lang/CharSequence;)Z"
    {
        return Ok(None);
    }
    let value = args
        .first()
        .ok_or_else(|| error("INVALID_ARGUMENT", "TextUtils.isEmpty argument"))?;
    let Some(sequence) = value.as_ref()? else {
        return Ok(Some(Value::Int(1)));
    };
    if !vm
        .cp
        .is_subtype(vm.heap.class_of(sequence)?, "Ljava/lang/CharSequence;")
    {
        return Err(error(
            "VM_TYPE_ERROR",
            "TextUtils.isEmpty requires CharSequence",
        ));
    }
    let length = vm
        .invoke_instance(sequence, "length", "()I", &[])?
        .as_int()?;
    Ok(Some(Value::Int(i32::from(length == 0))))
}
