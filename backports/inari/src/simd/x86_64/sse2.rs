// Copyright (c) 2026 Gabriel Bontemps. MIT; see ../../../LICENSE.
//! Baseline x86-64 directed operations. Interval algorithms are unchanged.
//! Each arithmetic operation executes inside one assembly block with a fixed
//! MXCSR (gradual underflow, masked exceptions), then restores the caller's
//! complete MXCSR. No compiler-generated floating operation observes the change.
use std::arch::asm;

macro_rules! impl_op_round {
    ($t:ty, $f:ident ($x:ident $(,$y:ident)*), $inst:literal, $rounding:expr) => {
        pub(crate) fn $f(mut $x: $t, $($y: $t,)*) -> $t {
            let mut saved = 0u32;
            let rounding: u32 = $rounding;
            unsafe {
                asm!(
                    "stmxcsr [{saved}]",
                    "ldmxcsr [{rounding}]",
                    $inst,
                    "ldmxcsr [{saved}]",
                    saved = in(reg) &mut saved,
                    rounding = in(reg) &rounding,
                    $x = inout(xmm_reg) $x,
                    $($y = in(xmm_reg) $y,)*
                    options(nostack, preserves_flags)
                );
            }
            $x
        }
    };
}

impl_op_round!(f64, sqrt1_rd(x), "sqrtsd {x}, {x}", 0x3f80);
impl_op_round!(f64, sqrt1_ru(x), "sqrtsd {x}, {x}", 0x5f80);
impl_op_round!(f64, sub1_ru(x, y), "subsd {x}, {y}", 0x5f80);
impl_op_round!(super::F64X2, add_ru(x, y), "addpd {x}, {y}", 0x5f80);
impl_op_round!(super::F64X2, sub_ru(x, y), "subpd {x}, {y}", 0x5f80);
impl_op_round!(super::F64X2, mul_ru(x, y), "mulpd {x}, {y}", 0x5f80);
impl_op_round!(super::F64X2, div_ru(x, y), "divpd {x}, {y}", 0x5f80);

#[cfg(unix)]
#[link(name = "m")]
extern "C" {
    #[link_name = "fma"]
    fn c99_fma(x: f64, y: f64, z: f64) -> f64;
}

#[cfg(not(unix))]
compile_error!("The baseline SSE2 backend currently requires a Unix C99 libm fma implementation.");

#[repr(C, align(16))]
struct FloatingState([u8; 512]);

/// Keep the mode change and C99 libm call opaque to the Rust optimizer.
/// FXSAVE64/FXRSTOR64 preserve defined x87 state, including 64-bit pointers
/// for pending exceptions, and MXCSR. r12/r13 survive the C call; result bits survive
/// restoration of XMM0. clobber_abi describes the call's register effects.
#[cfg(unix)]
fn fma_ru(x: f64, y: f64, z: f64) -> f64 {
    let mut saved = FloatingState([0; 512]);
    let upward_x87 = 0x0b7fu16;
    let upward_mxcsr = 0x5f80u32;
    let result_bits: u64;
    unsafe {
        asm!(
            "fxsave64 [r12]",
            "fnclex",
            "fldcw [{upward_x87}]",
            "ldmxcsr [{upward_mxcsr}]",
            "call r13",
            "movq rax, xmm0",
            "fxrstor64 [r12]",
            in("r12") saved.0.as_mut_ptr(),
            in("r13") c99_fma as unsafe extern "C" fn(f64, f64, f64) -> f64,
            upward_x87 = in(reg) &upward_x87,
            upward_mxcsr = in(reg) &upward_mxcsr,
            in("xmm0") x,
            in("xmm1") y,
            in("xmm2") z,
            lateout("rax") result_bits,
            clobber_abi("C"),
        );
    }
    f64::from_bits(result_bits)
}

pub(crate) fn mul_add_ru(x: super::F64X2, y: super::F64X2, z: super::F64X2) -> super::F64X2 {
    let [x0, x1] = super::extract(x);
    let [y0, y1] = super::extract(y);
    let [z0, z1] = super::extract(z);
    super::constant(fma_ru(x0, y0, z0), fma_ru(x1, y1, z1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::simd::{constant, extract};

    fn mxcsr() -> u32 {
        let mut value = 0;
        unsafe {
            asm!("stmxcsr [{value}]", value = in(reg) &mut value, options(nostack, preserves_flags));
        }
        value
    }

    fn floating_state() -> FloatingState {
        let mut state = FloatingState([0; 512]);
        unsafe {
            asm!("fxsave64 [{state}]", state = in(reg) state.0.as_mut_ptr(), options(nostack, preserves_flags));
        }
        state
    }

    unsafe fn restore_state(state: &FloatingState) {
        asm!("fxrstor64 [{state}]", state = in(reg) state.0.as_ptr(), clobber_abi("C"));
    }

    #[test]
    fn directed_rounding_and_gradual_underflow() {
        let tiny = f64::from_bits(1);
        assert_eq!(
            extract(add_ru(constant(1., -1.), constant(tiny, -tiny))),
            [f64::from_bits(1f64.to_bits() + 1), -1.]
        );
        assert_eq!(
            extract(mul_ru(constant(tiny, -tiny), constant(0.5, 0.5))),
            [tiny, -0.]
        );
        assert_eq!(
            extract(div_ru(constant(tiny, -tiny), constant(2., 2.))),
            [tiny, -0.]
        );
        assert_eq!(sqrt1_rd(2.).to_bits(), 0x3ff6a09e667f3bcc);
        assert_eq!(sqrt1_ru(2.).to_bits(), 0x3ff6a09e667f3bcd);
    }

    #[test]
    fn overflow_and_special_values() {
        assert_eq!(
            extract(mul_ru(constant(f64::MAX, -f64::MAX), constant(2., 2.))),
            [f64::INFINITY, -f64::MAX]
        );
        assert_eq!(fma_ru(f64::MAX, 2., 0.), f64::INFINITY);
        assert_eq!(fma_ru(-f64::MAX, 2., 0.), -f64::MAX);
        assert!(fma_ru(f64::INFINITY, 0., 1.).is_nan());
        assert!(fma_ru(f64::NAN, 1., 0.).is_nan());
        assert_eq!(fma_ru(-0., 1., -0.).to_bits(), (-0f64).to_bits());
    }

    #[test]
    fn fma_is_fused_and_rounds_upward() {
        let eps = f64::EPSILON;
        assert_eq!(fma_ru(1. + eps, 1. - eps, -1.), -eps * eps);
        assert_eq!(
            fma_ru(1., 1., f64::from_bits(1)),
            f64::from_bits(1f64.to_bits() + 1)
        );
        assert_eq!(fma_ru(f64::from_bits(1), 0.5, 0.).to_bits(), 1);
        assert_eq!(
            fma_ru(-f64::from_bits(1), 0.5, 0.).to_bits(),
            (-0f64).to_bits()
        );
    }

    #[test]
    fn integer_rounding_and_horizontal_sum_at_baseline() {
        use crate::simd::{ceil, floor, hadd_rn, round_ties_even, trunc};
        assert_eq!(extract(ceil(constant(-0.25, 1.25))), [-0., 2.]);
        assert_eq!(extract(floor(constant(-0.25, 1.25))), [-1., 1.]);
        assert_eq!(extract(trunc(constant(-1.75, 1.75))), [-1., 1.]);
        assert_eq!(extract(round_ties_even(constant(0.5, 1.5))), [0., 2.]);
        assert_eq!(extract(round_ties_even(constant(2.5, -1.5))), [2., -2.]);
        assert_eq!(
            extract(hadd_rn(constant(1., 2.), constant(4., 8.))),
            [3., 12.]
        );
        for operation in [ceil, floor, trunc, round_ties_even] {
            let [zero, infinity] = extract(operation(constant(-0., f64::INFINITY)));
            assert_eq!(zero.to_bits(), (-0f64).to_bits());
            assert_eq!(infinity, f64::INFINITY);
            assert!(extract(operation(constant(f64::NAN, 0.)))[0].is_nan());
        }
    }

    #[test]
    fn restores_rounding_status_and_flush_modes() {
        let original = floating_state();
        let mut unusual = FloatingState(original.0);
        unusual.0[0..2].copy_from_slice(&0x0f7fu16.to_le_bytes());
        unusual.0[2..4].copy_from_slice(&0x0024u16.to_le_bytes());
        let unusual_mxcsr = mxcsr() | 0xe040 | 0x20;
        unusual.0[24..28].copy_from_slice(&unusual_mxcsr.to_le_bytes());
        unsafe {
            restore_state(&unusual);
        }
        let before = floating_state();
        let _ = add_ru(constant(1., 1.), constant(f64::from_bits(1), 1.));
        let arithmetic_restored = mxcsr() == unusual_mxcsr;
        let result = fma_ru(f64::from_bits(1), 0.5, 0.);
        let after = floating_state();
        unsafe {
            restore_state(&original);
        }
        assert!(arithmetic_restored);
        // Defined FENV fields only; reserved/SIMD scratch bytes are not compared.
        assert_eq!(&before.0[..28], &after.0[..28]);
        assert_eq!(result.to_bits(), 1);
    }

    // Rosetta omits these diagnostic fields; Linux qualification must exercise
    // their architectural 64-bit representation, including on AMD (ES = 1).
    #[cfg(target_os = "linux")]
    #[test]
    fn restores_high_x87_error_pointers() {
        let original = floating_state();
        let mut unusual = FloatingState(original.0);
        let fip = fma_ru as *const () as usize as u64;
        let fdp = original.0.as_ptr() as usize as u64;
        unusual.0[0..2].copy_from_slice(&0x0f7bu16.to_le_bytes()); // unmask ZE
        unusual.0[2..4].copy_from_slice(&0x8084u16.to_le_bytes()); // pending ZE/ES
        unusual.0[4] = 0; // empty x87 register stack at a C ABI boundary
        unusual.0[8..16].copy_from_slice(&fip.to_le_bytes());
        unusual.0[16..24].copy_from_slice(&fdp.to_le_bytes());
        unsafe {
            restore_state(&unusual);
        }
        let before = floating_state();
        let result = fma_ru(f64::from_bits(1), 0.5, 0.);
        let after = floating_state();
        unsafe {
            restore_state(&original);
        }
        assert!(fip > u32::MAX as u64 && fdp > u32::MAX as u64);
        assert_eq!(
            &before.0[8..16],
            &fip.to_le_bytes(),
            "host/emulator omits 64-bit FIP"
        );
        assert_eq!(
            &before.0[16..24],
            &fdp.to_le_bytes(),
            "host/emulator omits 64-bit FDP"
        );
        assert_eq!(
            u16::from_le_bytes(before.0[2..4].try_into().unwrap()) & 0x84,
            0x84
        );
        assert_eq!(&before.0[..28], &after.0[..28]);
        assert_eq!(result.to_bits(), 1);
    }
}
