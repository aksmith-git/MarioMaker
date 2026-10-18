//! Deterministic float math. The sim must compute the same bits on every platform: online peers
//! on macOS, Windows, Linux and the web step the same world, and a Linux server re-simulates
//! replays recorded anywhere. `f32::sin` and friends call the platform's math library, whose last
//! bits differ between systems; these go through `libm` (a pure-Rust port of musl's), which
//! gives the same result everywhere. Basic arithmetic and `sqrt` are exact IEEE operations and
//! need nothing. `clippy.toml` in this crate forbids the platform versions.

pub trait DetMath: Sized {
    fn dsin(self) -> Self;
    fn dcos(self) -> Self;
    fn dsin_cos(self) -> (Self, Self);
    fn dtan(self) -> Self;
    fn dasin(self) -> Self;
    fn datan2(self, x: Self) -> Self;
    fn dhypot(self, other: Self) -> Self;
}

impl DetMath for f32 {
    fn dsin(self) -> f32 {
        libm::sinf(self)
    }
    fn dcos(self) -> f32 {
        libm::cosf(self)
    }
    fn dsin_cos(self) -> (f32, f32) {
        (libm::sinf(self), libm::cosf(self))
    }
    fn dtan(self) -> f32 {
        libm::tanf(self)
    }
    fn dasin(self) -> f32 {
        libm::asinf(self)
    }
    fn datan2(self, x: f32) -> f32 {
        libm::atan2f(self, x)
    }
    fn dhypot(self, other: f32) -> f32 {
        libm::hypotf(self, other)
    }
}

impl DetMath for f64 {
    fn dsin(self) -> f64 {
        libm::sin(self)
    }
    fn dcos(self) -> f64 {
        libm::cos(self)
    }
    fn dsin_cos(self) -> (f64, f64) {
        (libm::sin(self), libm::cos(self))
    }
    fn dtan(self) -> f64 {
        libm::tan(self)
    }
    fn dasin(self) -> f64 {
        libm::asin(self)
    }
    fn datan2(self, x: f64) -> f64 {
        libm::atan2(self, x)
    }
    fn dhypot(self, other: f64) -> f64 {
        libm::hypot(self, other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_to_the_platform_and_fixed_bits() {
        for i in -50..50 {
            let a = i as f32 * 0.137;
            assert!((a.dsin() - libm::sinf(a)).abs() == 0.0);
            #[allow(clippy::disallowed_methods)]
            let platform = a.sin();
            assert!((a.dsin() - platform).abs() < 1e-6);
        }
        // pinned values: these bits must never change between platforms or releases
        assert_eq!(1.0f32.dsin().to_bits(), 0x3f57_6aa4);
        assert_eq!(0.5f32.datan2(-0.25).to_bits(), 0x4002_3454);
    }
}
