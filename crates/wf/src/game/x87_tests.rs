//! Checks the f32 model of the original's maths against the x87 itself
//! (x86_64 only): the FPU set up as Direct3D 7 left it, precision control
//! 24 bits (FLDCW 0x007f), round to nearest, then FSIN / FPATAN / FSQRT
//! as the CRT's _CIsin, _CIatan2 and _CIsqrt use them.

use super::math;
use std::arch::asm;

/// x87 control word: all exceptions masked, PC = 24 bits, RC = nearest.
const CW_PC24: u16 = 0x007f;

fn fsin(x: f32) -> f32 {
    let mut out = 0f32;
    // SAFETY: only the x87 stack and the given memory are touched; FNINIT
    // leaves the stack empty and FSTP pops what FLD pushed.
    unsafe {
        asm!(
            "fninit",
            "fldcw word ptr [{cw}]",
            "fld dword ptr [{x}]",
            "fsin",
            "fstp dword ptr [{o}]",
            cw = in(reg) &CW_PC24,
            x = in(reg) &x,
            o = in(reg) &mut out,
            out("st(0)") _, out("st(1)") _,
        );
    }
    out
}

fn fpatan(y: f32, x: f32) -> f32 {
    let mut out = 0f32;
    // SAFETY: as in fsin; FPATAN pops one of the two values pushed.
    unsafe {
        asm!(
            "fninit",
            "fldcw word ptr [{cw}]",
            "fld dword ptr [{y}]",
            "fld dword ptr [{x}]",
            "fpatan",
            "fstp dword ptr [{o}]",
            cw = in(reg) &CW_PC24,
            y = in(reg) &y,
            x = in(reg) &x,
            o = in(reg) &mut out,
            out("st(0)") _, out("st(1)") _,
        );
    }
    out
}

/// `0x100 - (int)(atan2(dy, dx) * -162.97466f)` on the x87: the product
/// rounded to 24 bits, then truncated.
fn x87_angle_of(dx: f32, dy: f32) -> u16 {
    let a = fpatan(dy, dx);
    let p = a * -162.974_66_f32;
    0x100i32.wrapping_sub(math::ftol(p)) as u16
}

fn fsqrt(x: f32) -> f32 {
    let mut out = 0f32;
    // SAFETY: as in fsin.
    unsafe {
        asm!(
            "fninit",
            "fldcw word ptr [{cw}]",
            "fld dword ptr [{x}]",
            "fsqrt",
            "fstp dword ptr [{o}]",
            cw = in(reg) &CW_PC24,
            x = in(reg) &x,
            o = in(reg) &mut out,
            out("st(0)") _,
        );
    }
    out
}

#[test]
fn sin_table_matches_the_x87() {
    #[allow(clippy::approx_constant)]
    let step = 2.0f32 * 3.141_592_65_f32 / 1024.0f32;
    let table = math::sin_table();
    let mut bad = Vec::new();
    for (i, &v) in table.iter().enumerate() {
        let x = fsin(i as f32 * step);
        if x.to_bits() != v.to_bits() {
            bad.push((i, v, x));
        }
    }
    assert!(bad.is_empty(), "{} of 0x500 entries differ: {:?}", bad.len(), &bad[..bad.len().min(8)]);
}

#[test]
fn angle_of_matches_the_x87() {
    let mut bad = Vec::new();
    // Screen-sized differences, whole and quarter pixels.
    for dy in (-480 * 4..=480 * 4).step_by(3) {
        for dx in (-640 * 4..=640 * 4).step_by(7) {
            let (fx, fy) = (dx as f32 * 0.25, dy as f32 * 0.25);
            if math::angle_of(fx, fy) != x87_angle_of(fx, fy) {
                bad.push((fx, fy));
            }
        }
    }
    assert!(bad.is_empty(), "{} inputs differ: {:?}", bad.len(), &bad[..bad.len().min(8)]);
}

#[test]
fn sqrt_matches_the_x87() {
    for i in 0..200_000u32 {
        let x = i as f32 * 0.37 + (i % 7) as f32 * 0.001;
        assert_eq!(((x as f64).sqrt() as f32).to_bits(), fsqrt(x).to_bits(), "sqrt({x})");
    }
}

#[test]
#[ignore]
fn print_sin_table_bits() {
    #[allow(clippy::approx_constant)]
    let step = 2.0f32 * 3.141_592_65_f32 / 1024.0f32;
    let bits: Vec<String> = (0..0x500).map(|i| format!("0x{:08x}", fsin(i as f32 * step).to_bits())).collect();
    println!("BITS {}", bits.join(","));
}
