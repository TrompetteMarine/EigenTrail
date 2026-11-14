EigenTrail R-only baseline x86-64 backport to Inari 2.0.0

Upstream: https://github.com/unageek/inari (registry version 2.0.0).
The original registry cache was copied; it is unchanged.

Unchanged: interval algorithms, constants, AArch64 source, AVX/FMA and AVX-512
directed kernels, all original assertions. Changed: four x86 integer-rounding
operations use Rust's exact scalar operations; horizontal addition uses SSE2;
a baseline SSE2 directed backend saves/restores MXCSR; the fused kernel calls
system C99 libm fma under upward rounding. The fused wrapper saves and restores
defined x87 state and MXCSR with a 16-byte-aligned, 512-byte FXSAVE64 image.
Save, rounding setup, the indirect C99 call, result-bit capture and FXRSTOR64
execute inside one opaque assembly block. Explicit argument/result registers
and C ABI clobbers prevent compiler-generated arithmetic inside the changed
rounding environment. The result bits survive restoration of XMM0.

The Linux regression seeds high 64-bit instruction/data pointers and a pending
unmasked x87 exception (ES=1), then checks the defined FXSAVE64 fields after the
call. This also covers AMD's exception-dependent pointer save/restore rule.
Rosetta omits this diagnostic metadata; it cannot qualify that Linux test.
The radius scalar result normalizes zero to positive zero to satisfy its
existing upstream test: Rust f64::max permits either sign on a signed-zero tie.
Three documentation examples construct the original decimal intervals using
their exact outward binary endpoints, independently checked with MPFR. Their
assertions, including the negative should-panic example, remain unchanged.
The crate floor is Rust 1.77 for round_ties_even; the R workspace requires
Rust/Cargo 1.85.1. No new crate or MPFR production dependency is introduced.

Primary contracts:
- https://sourceware.org/glibc/manual/latest/html_node/Errors-in-Math-Functions.html
- https://sourceware.org/glibc/manual/latest/html_node/Rounding.html
- https://sourceware.org/glibc/manual/latest/html_node/Control-Functions.html
- https://cdrdv2-public.intel.com/789574/253665-sdm-vol-1.pdf
- https://www.amd.com/content/dam/amd/en/documents/processor-tech-docs/programmer-references/26569.pdf
- https://doc.rust-lang.org/reference/inline-assembly.html#rules-for-inline-assembly

Before promotion: compile/debug/release tests on baseline x86-64 Linux, inspect
disassembly for unsupported instructions, compare directed results with MPFR
and exact rational controls (including exceptional operands), rerun package
scientific qualification for each materially different native build, and verify
no change to AArch64 source. Unix C99 libm behavior is a runtime premise; other
libc/toolchain targets require their own qualification. Windows remains held.
