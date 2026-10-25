#include <R_ext/Rdynload.h>
#include <Rinternals.h>
#include <R_ext/Utils.h>
void R_init_eigentrail_extendr(DllInfo *dll);
void R_init_eigentrail(DllInfo *dll) { R_init_eigentrail_extendr(dll); }
static void check_interrupt(void *unused) { (void)unused; R_CheckUserInterrupt(); }
int eigentrail_check_interrupt(void) { return R_ToplevelExec(check_interrupt, NULL); }
