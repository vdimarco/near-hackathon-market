/*
 * nofile_clamp: lowers a request for open files to the hard limit of the machine.
 *
 * The NEAR sandbox asks the system for 65,535 open files when it starts, and it
 * stops if the system refuses. Some containers have a lower hard limit and do
 * not let a process raise it. scripts/with_sandbox.sh preloads this library on
 * such a machine, and only there.
 *
 * The library changes one thing. When a program asks for more open files than
 * the hard limit allows, the request goes down to the hard limit. Every other
 * request passes through unchanged.
 *
 * Build: cc -shared -fPIC -O2 -o nofile_clamp.so nofile_clamp.c -ldl
 * For 64-bit Linux with glibc. Test: sh scripts/test_nofile_clamp.sh
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <stddef.h>
#include <sys/resource.h>

/* setrlimit64 below reads its argument as a struct rlimit. */
_Static_assert(sizeof(struct rlimit) == sizeof(struct rlimit64),
               "nofile_clamp is for 64-bit Linux");

typedef int (*setrlimit_fn)(__rlimit_resource_t, const struct rlimit *);

static int clamped(const char *name, __rlimit_resource_t resource, const struct rlimit *want)
{
    setrlimit_fn real = (setrlimit_fn)dlsym(RTLD_NEXT, name);
    struct rlimit now;
    struct rlimit ask;

    if (real == NULL) {
        errno = ENOSYS;
        return -1;
    }
    if (resource != RLIMIT_NOFILE || want == NULL || getrlimit(RLIMIT_NOFILE, &now) != 0) {
        return real(resource, want);
    }
    ask = *want;
    if (ask.rlim_max > now.rlim_max) {
        ask.rlim_max = now.rlim_max;
    }
    if (ask.rlim_cur > ask.rlim_max) {
        ask.rlim_cur = ask.rlim_max;
    }
    return real(resource, &ask);
}

int setrlimit(__rlimit_resource_t resource, const struct rlimit *want)
{
    return clamped("setrlimit", resource, want);
}

int setrlimit64(__rlimit_resource_t resource, const struct rlimit64 *want)
{
    return clamped("setrlimit64", resource, (const struct rlimit *)want);
}
