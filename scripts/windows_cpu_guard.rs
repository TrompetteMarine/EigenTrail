//! Baseline x86-64 Windows CPU gate, compiled separately from the AVX/FMA extension.
//! ABI references: Windows GROUP_AFFINITY, SetThreadGroupAffinity and processor groups.
const ECX: u32 = (1 << 0)
    | (1 << 1)
    | (1 << 9)
    | (1 << 12)
    | (1 << 13)
    | (1 << 19)
    | (1 << 20)
    | (1 << 22)
    | (1 << 23)
    | (1 << 26)
    | (1 << 27)
    | (1 << 28)
    | (1 << 29)
    | (1 << 30);
const EDX: u32 = (1 << 24) | (1 << 25) | (1 << 26);
const EBX7: u32 = (1 << 3) | (1 << 5) | (1 << 8);

#[derive(Clone, Copy, Default)]
struct Facts {
    max: u32,
    extended: u32,
    ecx: u32,
    edx: u32,
    ebx7: u32,
    ext_ecx: u32,
    save: u32,
    xcr0: u64,
}
fn haswell(f: Facts) -> bool {
    f.max >= 0xd
        && f.extended >= 0x8000_0001
        && f.ecx & ECX == ECX
        && f.edx & EDX == EDX
        && f.ebx7 & EBX7 == EBX7
        && f.ext_ecx & (1 << 5) != 0
        && f.save & 1 != 0
        && f.xcr0 & 6 == 6
}

#[cfg(all(windows, not(target_arch = "x86_64")))]
compile_error!("EigenTrail's Windows CPU gate supports only x86-64");

#[cfg(all(windows, target_arch = "x86_64"))]
mod windows {
    use super::{Facts, haswell};
    use std::{
        arch::x86_64::{__cpuid, __cpuid_count, _xgetbv},
        ffi::c_void,
    };
    type Handle = *mut c_void;
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct Affinity {
        mask: u64,
        group: u16,
        reserved: [u16; 3],
    }
    #[repr(C)]
    #[derive(Default)]
    struct Processor {
        group: u16,
        number: u8,
        reserved: u8,
    }
    const _: () = assert!(std::mem::size_of::<Affinity>() == 16);
    const _: () = assert!(std::mem::align_of::<Affinity>() == 8);
    const _: () = assert!(std::mem::size_of::<Processor>() == 4);
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetActiveProcessorGroupCount() -> u16;
        fn GetActiveProcessorCount(group: u16) -> u32;
        fn GetCurrentThread() -> Handle;
        fn GetThreadGroupAffinity(thread: Handle, affinity: *mut Affinity) -> i32;
        fn SetThreadGroupAffinity(
            thread: Handle,
            affinity: *const Affinity,
            previous: *mut Affinity,
        ) -> i32;
        fn GetCurrentProcessorNumberEx(processor: *mut Processor);
    }

    struct Restore {
        thread: Handle,
        original: Affinity,
        armed: bool,
    }
    impl Restore {
        fn restore(&mut self) -> bool {
            if !self.armed {
                return true;
            }
            let restored = unsafe {
                SetThreadGroupAffinity(self.thread, &self.original, std::ptr::null_mut())
            } != 0;
            if restored {
                self.armed = false;
            }
            restored
        }
    }
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = self.restore();
        }
    }

    fn facts() -> Facts {
        // XGETBV is executed only after both architectural and OS XSAVE support.
        unsafe {
            let max = __cpuid(0).eax;
            let extended = __cpuid(0x8000_0000).eax;
            if max < 0xd || extended < 0x8000_0001 {
                return Facts::default();
            }
            let first = __cpuid(1);
            if first.ecx & ((1 << 26) | (1 << 27)) != ((1 << 26) | (1 << 27)) {
                return Facts::default();
            }
            Facts {
                max,
                extended,
                ecx: first.ecx,
                edx: first.edx,
                ebx7: __cpuid_count(7, 0).ebx,
                ext_ecx: __cpuid(0x8000_0001).ecx,
                save: __cpuid_count(0xd, 1).eax,
                xcr0: _xgetbv(0),
            }
        }
    }

    fn inspect(restore: &Restore) -> u32 {
        let groups = unsafe { GetActiveProcessorGroupCount() };
        // Bound unsupported future topology; refusal cannot admit an unchecked CPU.
        if groups == 0 || groups > 64 {
            return 1;
        }
        let mut counts = [0u32; 64];
        for group in 0..groups {
            let count = unsafe { GetActiveProcessorCount(group) };
            if count == 0 || count > 64 {
                return 1;
            }
            counts[usize::from(group)] = count;
        }
        for group in 0..groups {
            for number in 0..counts[usize::from(group)] {
                let requested = Affinity {
                    mask: 1u64 << number,
                    group,
                    reserved: [0; 3],
                };
                if unsafe {
                    SetThreadGroupAffinity(restore.thread, &requested, std::ptr::null_mut())
                } == 0
                {
                    return 2;
                }
                let mut current = Processor::default();
                unsafe {
                    GetCurrentProcessorNumberEx(&mut current);
                }
                if current.group != group || u32::from(current.number) != number {
                    return 2;
                }
                if !haswell(facts()) {
                    return 3;
                }
            }
        }
        if unsafe { GetActiveProcessorGroupCount() } != groups {
            return 5;
        }
        for group in 0..groups {
            if unsafe { GetActiveProcessorCount(group) } != counts[usize::from(group)] {
                return 5;
            }
        }
        0
    }

    fn worker() -> u32 {
        let thread = unsafe { GetCurrentThread() };
        let mut original = Affinity::default();
        if unsafe { GetThreadGroupAffinity(thread, &mut original) } == 0 {
            return 1;
        }
        let mut restore = Restore {
            thread,
            original,
            armed: true,
        };
        let status = inspect(&restore);
        if restore.restore() { status } else { 4 }
    }

    /// C ABI status0 means every active logical CPU passed; every other value refuses.
    #[unsafe(no_mangle)]
    pub extern "C" fn eigentrail_windows_haswell_guard() -> u32 {
        // A worker avoids narrowing the Python caller's Win11 multi-group affinity.
        std::panic::catch_unwind(|| match std::thread::Builder::new().spawn(worker) {
            Ok(thread) => thread.join().unwrap_or(5),
            Err(_) => 1,
        })
        .unwrap_or(5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn supported() -> Facts {
        Facts {
            max: 0xd,
            extended: 0x8000_0001,
            ecx: ECX,
            edx: EDX,
            ebx7: EBX7,
            ext_ecx: 1 << 5,
            save: 1,
            xcr0: 7,
        }
    }
    #[test]
    fn every_required_feature_and_os_avx_state_is_mandatory() {
        assert!(haswell(supported()));
        for slot in 0..5 {
            let mask = [ECX, EDX, EBX7, 1 << 5, 1][slot];
            for bit in 0..32 {
                if mask & (1 << bit) == 0 {
                    continue;
                }
                let mut f = supported();
                let field = match slot {
                    0 => &mut f.ecx,
                    1 => &mut f.edx,
                    2 => &mut f.ebx7,
                    3 => &mut f.ext_ecx,
                    _ => &mut f.save,
                };
                *field &= !(1 << bit);
                assert!(!haswell(f));
            }
        }
        for xcr0 in [0, 1, 2, 3, 4, 5] {
            let mut f = supported();
            f.xcr0 = xcr0;
            assert!(!haswell(f));
        }
        let mut f = supported();
        f.max = 0xc;
        assert!(!haswell(f));
        f = supported();
        f.extended = 0x8000_0000;
        assert!(!haswell(f));
        assert!(!haswell(Facts::default()));
    }
}
