//! Formatação numérica no estilo Java (`Float.toString` / `Double.toString`).
//!
//! O DoD do M2 compara resultados com a execução real (JVM), então a
//! representação de float/double precisa bater EXATAMENTE com a da plataforma:
//! decimal curto quando `1e-3 ≤ |v| < 1e7`, senão notação `1.0E23`/`1.0E-5`
//! (mantissa com pelo menos um dígito após o ponto, `E` maiúsculo, sem `+`).

use alloc::{
    format,
    string::{String, ToString},
};
/// Formata um `f64` como `Double.toString` do Java.
pub fn java_double(v: f64) -> String {
    if v.is_nan() {
        return "NaN".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if v == 0.0 {
        return if v.is_sign_negative() { "-0.0" } else { "0.0" }.into();
    }
    let neg = v < 0.0;
    format_mantissa_exp(format!("{:e}", v.abs()), neg)
}

/// Formata um `f32` como `Float.toString` do Java.
pub fn java_float(v: f32) -> String {
    if v.is_nan() {
        return "NaN".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if v == 0.0 {
        return if v.is_sign_negative() { "-0.0" } else { "0.0" }.into();
    }
    let neg = v < 0.0;
    format_mantissa_exp(format!("{:e}", v.abs()), neg)
}

/// Entrada: `{:e}` do Rust no valor absoluto, ex. "1.5e23" / "9.5e-7".
/// Java imprime decimal entre 1e-3 (inclusive) e 1e7 (exclusivo).
fn format_mantissa_exp(sci: String, neg: bool) -> String {
    let (mant, exp) = sci.split_once('e').expect("LowerExp sempre tem 'e'");
    let exp: i32 = exp.parse().expect("expoente decimal");
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let sign = if neg { "-" } else { "" };

    // faixa decimal: |v| ∈ [1e-3, 1e7)  ⇔  exp ∈ [-3, 6]
    if (-3..=6).contains(&exp) {
        let out = if exp >= 0 {
            let ip = exp as usize + 1;
            let (int_part, frac) = if digits.len() > ip {
                (digits[..ip].to_string(), digits[ip..].to_string())
            } else {
                (format!("{digits:0<ip$}"), String::new())
            };
            if frac.is_empty() {
                format!("{int_part}.0")
            } else {
                format!("{int_part}.{frac}")
            }
        } else {
            let zeros = "0".repeat((-exp - 1) as usize);
            format!("0.{zeros}{digits}")
        };
        return format!("{sign}{out}");
    }

    // científica: d[.ddd]E±dd — sem '+', sem zeros à esquerda no expoente
    let mut out = String::new();
    out.push_str(&digits[..1]);
    if digits.len() > 1 {
        out.push('.');
        out.push_str(&digits[1..]);
    } else {
        out.push_str(".0");
    }
    out.push('E');
    out.push_str(&exp.to_string());
    format!("{sign}{out}")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(all(not(feature = "std"), feature = "upstream-runtime"))]
    #[allow(unused_imports)]
    use crate::port_prelude::*;

    #[test]
    fn double_matches_java_tostring() {
        assert_eq!(java_double(0.0), "0.0");
        assert_eq!(java_double(-0.0), "-0.0");
        assert_eq!(java_double(1.0), "1.0");
        assert_eq!(java_double(3.5), "3.5");
        assert_eq!(java_double(-3.5), "-3.5");
        assert_eq!(java_double(0.001), "0.001");
        assert_eq!(java_double(1e-4), "1.0E-4");
        assert_eq!(java_double(9999999.0), "9999999.0");
        assert_eq!(java_double(1e7), "1.0E7");
        assert_eq!(java_double(12345678.0), "1.2345678E7");
        assert_eq!(java_double(1e23), "1.0E23");
        assert_eq!(java_double(f64::NAN), "NaN");
        assert_eq!(java_double(f64::INFINITY), "Infinity");
        assert_eq!(java_double(f64::NEG_INFINITY), "-Infinity");
    }

    #[test]
    fn float_matches_java_tostring() {
        assert_eq!(java_float(1.5), "1.5");
        assert_eq!(java_float(0.25), "0.25");
        assert_eq!(java_float(-2.0), "-2.0");
        assert_eq!(java_float(1e-4), "1.0E-4");
        assert_eq!(java_float(1e7), "1.0E7");
        assert_eq!(java_float(f32::MAX), "3.4028235E38");
        assert_eq!(java_float(f32::INFINITY), "Infinity");
        assert_eq!(java_float(f32::NAN), "NaN");
    }
}
