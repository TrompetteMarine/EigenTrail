#include <R_ext/Rdynload.h>
#include <Rinternals.h>
#include <R_ext/Utils.h>
void R_init_eigentrail_extendr(DllInfo *dll);
void R_init_eigentrail(DllInfo *dll) {
#if !(defined(__linux__) && defined(__x86_64__)) && \
    !(defined(__APPLE__) && (defined(__aarch64__) || defined(__x86_64__)))
    Rf_error("EigenTrail supports macOS ARM64/x86-64 or GNU/Linux x86-64.");
#endif
    R_init_eigentrail_extendr(dll);
}
static void check_interrupt(void *unused) { (void)unused; R_CheckUserInterrupt(); }
int eigentrail_check_interrupt(void) { return R_ToplevelExec(check_interrupt, NULL); }
