#ifndef MARKDAWN_GRAPHVIZ_WRAPPER_H
#define MARKDAWN_GRAPHVIZ_WRAPPER_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/*
 * Return codes from markdawn_gv_render_svg*:
 *   0 = success
 *   1 = generic failure (parse / layout / render error from libgraphviz)
 *   2 = parse stage exceeded parse_budget_ms
 *   3 = layout stage exceeded layout_budget_ms
 *   4 = render stage exceeded render_budget_ms
 *   5 = fork() isolation: child process was killed after exceeding
 *       total_timeout_ms
 *   6 = fork() isolation: child process died unexpectedly (crash,
 *       OOM-killed, signal) — this is how we catch libgraphviz
 *       abort() / SIGSEGV / stack overflow without taking down the
 *       host process
 *   7 = fork() / pipe / I/O syscall error; can't retry safely
 */
#define MARKDAWN_GV_OK                    0
#define MARKDAWN_GV_FAILED                1
#define MARKDAWN_GV_TIMEOUT_PARSE         2
#define MARKDAWN_GV_TIMEOUT_LAYOUT        3
#define MARKDAWN_GV_TIMEOUT_RENDER        4
#define MARKDAWN_GV_FORK_TIMEOUT          5
#define MARKDAWN_GV_FORK_CRASHED          6
#define MARKDAWN_GV_FORK_SYSCALL_ERROR    7

/*
 * In-process render. Does agmemread → gvLayout → gvRenderData in the
 * calling thread. Used by the fork()-isolated wrapper below as well
 * as any caller that's happy to share its process with libgraphviz.
 *
 * libgraphviz holds process-global context — callers must serialise
 * invocations of this function across threads themselves.
 */
int markdawn_gv_render_svg(const char *dot,
                           char **out_svg,
                           size_t *out_len,
                           char **out_err,
                           uint64_t parse_budget_ms,
                           uint64_t layout_budget_ms,
                           uint64_t render_budget_ms,
                           uint64_t *out_parse_ms,
                           uint64_t *out_layout_ms,
                           uint64_t *out_render_ms);

/*
 * fork()-isolated render. Parent forks, child runs the in-process
 * render and writes the result to a pipe, child _exit()s. Parent
 * reads with poll()-based timeout. On timeout or oversize response,
 * parent SIGKILLs the child; kernel reclaims everything. If the
 * child crashes (libgraphviz abort / SIGSEGV / stack overflow), the
 * parent observes the death and returns MARKDAWN_GV_FORK_CRASHED
 * without going down with it.
 *
 * Caveat — fork() in a threaded program: if any thread in the
 * calling process holds a libc allocator lock at fork time, the
 * child inherits that lock held forever. Modern glibc / musl /
 * macOS have `pthread_atfork` handlers that reset allocator state in
 * the child, so this is fine in practice for the common case. Still,
 * callers who run this inside heavily-threaded programs with
 * unusual allocators should prefer the non-forking entry point.
 */
int markdawn_gv_render_svg_forked(const char *dot,
                                  char **out_svg,
                                  size_t *out_len,
                                  char **out_err,
                                  uint64_t total_timeout_ms,
                                  uint64_t parse_budget_ms,
                                  uint64_t layout_budget_ms,
                                  uint64_t render_budget_ms,
                                  uint64_t *out_parse_ms,
                                  uint64_t *out_layout_ms,
                                  uint64_t *out_render_ms);

/*
 * Force libgraphviz plugin load + Objective-C `+initialize` in the
 * calling process. Safe to call any number of times. On macOS this
 * is essential before the first fork()-isolated render: if the
 * parent hasn't yet touched libgraphviz, the child will be the one
 * to trigger +initialize, and the objc runtime's fork-safety check
 * will deadlock or crash the child.
 */
void markdawn_gv_prewarm(void);

/* Free a buffer returned via *out_svg. */
void markdawn_gv_free_render_data(char *ptr);

/* Free a diagnostic string returned via *out_err. */
void markdawn_gv_free_error(char *ptr);

#ifdef __cplusplus
}
#endif

#endif
