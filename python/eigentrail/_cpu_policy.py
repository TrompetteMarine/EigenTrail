"""Linux CPU policy: EigenTrail's native code needs only baseline x86-64 SSE2 instructions."""
LINUX_CPU_PROFILE = 'baseline-sse2'
LINUX_CPU_FLAGS = ('sse2',)
LINUX_CPU_DESCRIPTION = 'x86-64 baseline SSE2 instructions'
LINUX_RUSTFLAGS = '-C target-cpu=x86-64'
LINUX_CFLAGS = '-march=x86-64'
LINUX_NUMERICAL_BACKEND = 'inari-r-backport-sse2'
