//! libm-backed float operations missing from core on the freestanding target.
#[allow(dead_code)] // Host builds expose inherent float methods; freestanding builds use these adapters.
pub(crate) trait FloatExt: Sized {
    fn floor(self) -> Self;
    fn ceil(self) -> Self;
    fn round(self) -> Self;
    fn trunc(self) -> Self;
    fn sqrt(self) -> Self;
    fn sin(self) -> Self;
    fn cos(self) -> Self;
    fn tan(self) -> Self;
    fn asin(self) -> Self;
    fn acos(self) -> Self;
    fn atan(self) -> Self;
    fn sinh(self) -> Self;
    fn cosh(self) -> Self;
    fn tanh(self) -> Self;
    fn exp(self) -> Self;
    fn exp_m1(self) -> Self;
    fn ln(self) -> Self;
    fn log10(self) -> Self;
    fn ln_1p(self) -> Self;
    fn cbrt(self) -> Self;
    fn powf(self, other: Self) -> Self;
    fn atan2(self, other: Self) -> Self;
    fn hypot(self, other: Self) -> Self;
    fn powi(self, n: i32) -> Self;
    fn fract(self) -> Self;
    fn sin_cos(self) -> (Self, Self);
}
impl FloatExt for f64 {
    fn floor(self) -> Self {
        libm::floor(self)
    }
    fn ceil(self) -> Self {
        libm::ceil(self)
    }
    fn round(self) -> Self {
        libm::round(self)
    }
    fn trunc(self) -> Self {
        libm::trunc(self)
    }
    fn sqrt(self) -> Self {
        libm::sqrt(self)
    }
    fn sin(self) -> Self {
        libm::sin(self)
    }
    fn cos(self) -> Self {
        libm::cos(self)
    }
    fn tan(self) -> Self {
        libm::tan(self)
    }
    fn asin(self) -> Self {
        libm::asin(self)
    }
    fn acos(self) -> Self {
        libm::acos(self)
    }
    fn atan(self) -> Self {
        libm::atan(self)
    }
    fn sinh(self) -> Self {
        libm::sinh(self)
    }
    fn cosh(self) -> Self {
        libm::cosh(self)
    }
    fn tanh(self) -> Self {
        libm::tanh(self)
    }
    fn exp(self) -> Self {
        libm::exp(self)
    }
    fn exp_m1(self) -> Self {
        libm::expm1(self)
    }
    fn ln(self) -> Self {
        libm::log(self)
    }
    fn log10(self) -> Self {
        libm::log10(self)
    }
    fn ln_1p(self) -> Self {
        libm::log1p(self)
    }
    fn cbrt(self) -> Self {
        libm::cbrt(self)
    }
    fn powf(self, other: Self) -> Self {
        libm::pow(self, other)
    }
    fn atan2(self, other: Self) -> Self {
        libm::atan2(self, other)
    }
    fn hypot(self, other: Self) -> Self {
        libm::hypot(self, other)
    }
    fn powi(self, n: i32) -> Self {
        let mut exponent = n.unsigned_abs();
        let mut base = self;
        let mut result = 1.0;
        while exponent != 0 {
            if exponent & 1 != 0 {
                result *= base;
            }
            exponent >>= 1;
            if exponent != 0 {
                base *= base;
            }
        }
        if n < 0 {
            1.0 / result
        } else {
            result
        }
    }
    fn fract(self) -> Self {
        self - <Self as FloatExt>::trunc(self)
    }
    fn sin_cos(self) -> (Self, Self) {
        (<Self as FloatExt>::sin(self), <Self as FloatExt>::cos(self))
    }
}
impl FloatExt for f32 {
    fn floor(self) -> Self {
        libm::floorf(self)
    }
    fn ceil(self) -> Self {
        libm::ceilf(self)
    }
    fn round(self) -> Self {
        libm::roundf(self)
    }
    fn trunc(self) -> Self {
        libm::truncf(self)
    }
    fn sqrt(self) -> Self {
        libm::sqrtf(self)
    }
    fn sin(self) -> Self {
        libm::sinf(self)
    }
    fn cos(self) -> Self {
        libm::cosf(self)
    }
    fn tan(self) -> Self {
        libm::tanf(self)
    }
    fn asin(self) -> Self {
        libm::asinf(self)
    }
    fn acos(self) -> Self {
        libm::acosf(self)
    }
    fn atan(self) -> Self {
        libm::atanf(self)
    }
    fn sinh(self) -> Self {
        libm::sinhf(self)
    }
    fn cosh(self) -> Self {
        libm::coshf(self)
    }
    fn tanh(self) -> Self {
        libm::tanhf(self)
    }
    fn exp(self) -> Self {
        libm::expf(self)
    }
    fn exp_m1(self) -> Self {
        libm::expm1f(self)
    }
    fn ln(self) -> Self {
        libm::logf(self)
    }
    fn log10(self) -> Self {
        libm::log10f(self)
    }
    fn ln_1p(self) -> Self {
        libm::log1pf(self)
    }
    fn cbrt(self) -> Self {
        libm::cbrtf(self)
    }
    fn powf(self, other: Self) -> Self {
        libm::powf(self, other)
    }
    fn atan2(self, other: Self) -> Self {
        libm::atan2f(self, other)
    }
    fn hypot(self, other: Self) -> Self {
        libm::hypotf(self, other)
    }
    fn powi(self, n: i32) -> Self {
        let mut exponent = n.unsigned_abs();
        let mut base = self;
        let mut result = 1.0;
        while exponent != 0 {
            if exponent & 1 != 0 {
                result *= base;
            }
            exponent >>= 1;
            if exponent != 0 {
                base *= base;
            }
        }
        if n < 0 {
            1.0 / result
        } else {
            result
        }
    }
    fn fract(self) -> Self {
        self - <Self as FloatExt>::trunc(self)
    }
    fn sin_cos(self) -> (Self, Self) {
        (<Self as FloatExt>::sin(self), <Self as FloatExt>::cos(self))
    }
}

#[cfg(test)]
mod tests {
    use super::FloatExt;

    // Explicit UFCS calls exercise libm even when this host harness exposes
    // inherent std float methods. The latter are the independent oracle.
    macro_rules! compare_unary {
        ($ty:ty, $value:ident, $tolerance:expr; $($method:ident),+ $(,)?) => {$(
            let actual = <$ty as FloatExt>::$method($value);
            let expected = $value.$method();
            assert!((actual - expected).abs() <= $tolerance,
                "{}({}): {} != {}", stringify!($method), $value, actual, expected);
        )+};
    }

    macro_rules! compare_methods {
        ($ty:ty, $tolerance:expr) => {{
            for value in [-0.875 as $ty, -0.0, 0.0, 0.375, 0.875] {
                compare_unary!($ty, value, $tolerance;floor, ceil, round, trunc, sin, cos, tan, asin, acos, atan,
                    sinh, cosh, tanh, exp, exp_m1, ln_1p, cbrt, fract);
                let positive = value.abs() + 0.125;
                for (actual, expected) in [
                    (<$ty as FloatExt>::sqrt(positive), positive.sqrt()),
                    (<$ty as FloatExt>::ln(positive), positive.ln()),
                    (<$ty as FloatExt>::log10(positive), positive.log10()),
                    (<$ty as FloatExt>::powf(positive, 1.25), positive.powf(1.25)),
                    (<$ty as FloatExt>::atan2(value, 0.5), value.atan2(0.5)),
                    (<$ty as FloatExt>::hypot(value, 0.5), value.hypot(0.5)),
                ] {
                    assert!((actual - expected).abs() <= $tolerance);
                }
                let pair = <$ty as FloatExt>::sin_cos(value);
                assert!((pair.0 - value.sin()).abs() <= $tolerance);
                assert!((pair.1 - value.cos()).abs() <= $tolerance);
            }
        }};
    }

    #[test]
    fn libm_f64_operations_match_host_float_oracle() {
        compare_methods!(f64, 1e-12);
    }

    #[test]
    fn libm_f32_operations_match_host_float_oracle() {
        compare_methods!(f32, 1e-6);
    }

    #[test]
    fn integer_powers_and_special_values_keep_ieee_behavior() {
        for exponent in [i32::MIN, -127, -3, -1, 0, 1, 2, 3, 127, i32::MAX] {
            for base in [-1.0f64, -0.0, 0.0, 1.0, 2.0, f64::INFINITY] {
                assert_eq!(
                    <f64 as FloatExt>::powi(base, exponent).to_bits(),
                    base.powi(exponent).to_bits()
                );
                let base = base as f32;
                // On this Windows host f32::powi(-1, i32::MAX) loses
                // exponent parity and returns +1. JVM Math.pow and the
                // double oracle return -1. These selected bases have exact
                // powers (including IEEE zero/infinity), so narrowing the
                // double result is an independent f32 expectation.
                assert_eq!(
                    <f32 as FloatExt>::powi(base, exponent).to_bits(),
                    ((base as f64).powi(exponent) as f32).to_bits(),
                    "base={base}, exponent={exponent}"
                );
            }
        }
        assert!(<f64 as FloatExt>::sqrt(-1.0).is_nan());
        assert!(<f32 as FloatExt>::ln(-1.0).is_nan());
        assert_eq!(<f64 as FloatExt>::sin(-0.0).to_bits(), (-0.0f64).to_bits());
        assert_eq!(<f32 as FloatExt>::sin(-0.0).to_bits(), (-0.0f32).to_bits());
    }
}
