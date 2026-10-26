//! Shared RustyDroid object/value helpers; dispatch remains target-adapted.
use crate::{heap::HeapObj, Engine, Value, VmExit};
use alloc::{
    format,
    string::{String, ToString},
    vec,
};

/// Igualdade JAVA para chaves/elementos de coleções (M8): Null==Null;
/// String por conteúdo; boxes (Integer/Long/Boolean/Double/Float) por valor;
/// senão identidade de referência (default de Object.equals). Classe de
/// usuário que sobrescreve equals NÃO é consultada — limitação documentada
/// em compat_gaps (`java-util-collections`).
pub fn values_equal(heap: &crate::heap::Heap, a: &Value, b: &Value) -> Result<bool, VmExit> {
    match (a, b) {
        (Value::Null, Value::Null) => Ok(true),
        (Value::Null, _) | (_, Value::Null) => Ok(false),
        (Value::Int(x), Value::Int(y)) => Ok(x == y),
        (Value::Long(x), Value::Long(y)) => Ok(x == y),
        (Value::Obj(x), Value::Obj(y)) => {
            if x == y {
                return Ok(true);
            }
            let cx = heap.class_of(*x).unwrap_or("");
            let cy = heap.class_of(*y).unwrap_or("");
            if cx != cy {
                return Ok(false);
            }
            match cx {
                "Ljava/lang/String;" => Ok(heap.as_str(*x)? == heap.as_str(*y)?),
                // Long payloads are 64-bit (an Int payload widens)
                "Ljava/lang/Long;" => {
                    let wide = |v: Value| -> Result<i64, String> {
                        match v {
                            Value::Int(i) => Ok(i as i64),
                            other => other.as_long(),
                        }
                    };
                    Ok(wide(heap.get_field(*x, "value")?)? == wide(heap.get_field(*y, "value")?)?)
                }
                "Ljava/lang/Integer;"
                | "Ljava/lang/Boolean;"
                | "Ljava/lang/Short;"
                | "Ljava/lang/Byte;"
                | "Ljava/lang/Character;" => Ok(heap.get_field(*x, "value")?.as_int()?
                    == heap.get_field(*y, "value")?.as_int()?),
                "Ljava/lang/Double;" => {
                    let a = heap.get_field(*x, "value")?.as_double()?;
                    let b = heap.get_field(*y, "value")?.as_double()?;
                    Ok((a.is_nan() && b.is_nan()) || a.to_bits() == b.to_bits())
                }
                "Ljava/lang/Float;" => {
                    let a = heap.get_field(*x, "value")?.as_float()?;
                    let b = heap.get_field(*y, "value")?.as_float()?;
                    Ok((a.is_nan() && b.is_nan()) || a.to_bits() == b.to_bits())
                }
                _ => Ok(false), // identidade (já comparada acima)
            }
        }
        _ => Ok(false),
    }
}

/// toString de um objeto para print/println/append(Object)/String.valueOf
/// (issues #43/#48): Str → conteúdo; boxes de plataforma → valor; classe de
/// usuário que DECLARA toString → executa no DEX (pode lançar exceção Java);
/// caso contrário → `Classe@id` (contrato Object).
pub(crate) fn object_to_string(vm: &mut Engine, r: crate::heap::ObjRef) -> Result<String, VmExit> {
    if let Ok(HeapObj::Str(s)) = vm.heap.get(r) {
        return Ok(s.clone());
    }
    crate::arrays::with_host_reentrant(vm, |vm| {
        let cls = vm.heap.class_of(r)?.to_string();
        match crate::intrinsics::call_instance_intrinsic_mref(
            vm,
            &cls,
            &cls,
            "toString",
            "()Ljava/lang/String;",
            r,
            &[],
        )? {
            Some(Value::Obj(sref)) => Ok(vm.heap.as_str(sref)?.to_string()),
            Some(Value::Null) => Ok("null".to_string()),
            Some(_) => Err(crate::err::vm_error(
                "INVALID_FORMAT",
                format!("{cls}->toString() retornou não-string"),
            )
            .into()),
            None => {
                // classe declara toString no DEX: executa de verdade
                if let Some((dex_idx, def, m)) =
                    vm.cp
                        .resolve_method(&cls, "toString", "()Ljava/lang/String;")
                {
                    match vm.call(dex_idx, def, &m, vec![Value::Obj(r)])? {
                        Value::Obj(sref) => Ok(vm.heap.as_str(sref)?.to_string()),
                        Value::Null => Ok("null".to_string()),
                        _ => Err(crate::err::vm_error(
                            "INVALID_FORMAT",
                            format!("{cls}->toString() retornou não-string"),
                        )
                        .into()),
                    }
                } else {
                    Ok(default_to_string(&cls, r))
                }
            }
        }
    })
}

/// toString default da JVM: `Classe@hash` (hex da identidade).
pub(crate) fn default_to_string(class_desc: &str, r: crate::heap::ObjRef) -> String {
    let name = class_desc
        .trim_start_matches('L')
        .trim_end_matches(';')
        .replace('/', ".");
    format!("{name}@{r:x}")
}
