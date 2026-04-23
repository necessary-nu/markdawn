#include "wrapper.h"

#include <errno.h>
#include <poll.h>
#include <signal.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#include <graphviz/gvc.h>
#include <graphviz/cgraph.h>

/*
 * libgraphviz's agseterrf installs a callback that receives a
 * pre-formatted diagnostic string. We accumulate those strings into a
 * static buffer and expose them to Rust via out_err. The static state
 * is safe because markdawn-graphviz serialises calls under a Rust
 * Mutex.
 */
static char *g_err_buf = NULL;
static size_t g_err_len = 0;
static size_t g_err_cap = 0;

static void err_reset(void) {
    g_err_len = 0;
    if (g_err_buf && g_err_cap > 0) {
        g_err_buf[0] = '\0';
    }
}

static int err_sink(char *message) {
    if (!message) {
        return 0;
    }
    size_t n = strlen(message);
    size_t needed = g_err_len + n + 1;
    if (needed > g_err_cap) {
        size_t new_cap = g_err_cap ? g_err_cap * 2 : 256;
        while (new_cap < needed) {
            new_cap *= 2;
        }
        char *tmp = realloc(g_err_buf, new_cap);
        if (!tmp) {
            return 0;
        }
        g_err_buf = tmp;
        g_err_cap = new_cap;
    }
    memcpy(g_err_buf + g_err_len, message, n);
    g_err_len += n;
    g_err_buf[g_err_len] = '\0';
    return 0;
}

static char *err_take(void) {
    if (g_err_len == 0) {
        return NULL;
    }
    char *copy = malloc(g_err_len + 1);
    if (!copy) {
        return NULL;
    }
    memcpy(copy, g_err_buf, g_err_len + 1);
    return copy;
}

/* Monotonic clock snapshot in milliseconds. */
static uint64_t mono_ms(void) {
    struct timespec ts;
    if (clock_gettime(CLOCK_MONOTONIC, &ts) != 0) {
        return 0;
    }
    return (uint64_t)ts.tv_sec * 1000u + (uint64_t)(ts.tv_nsec / 1000000);
}

int markdawn_gv_render_svg(const char *dot,
                           char **out_svg,
                           size_t *out_len,
                           char **out_err,
                           uint64_t parse_budget_ms,
                           uint64_t layout_budget_ms,
                           uint64_t render_budget_ms,
                           uint64_t *out_parse_ms,
                           uint64_t *out_layout_ms,
                           uint64_t *out_render_ms) {
    *out_svg = NULL;
    *out_len = 0;
    *out_err = NULL;
    *out_parse_ms = 0;
    *out_layout_ms = 0;
    *out_render_ms = 0;

    err_reset();
    agusererrf prev_errf = agseterrf(err_sink);

    int rc = MARKDAWN_GV_FAILED;
    GVC_t *gvc = NULL;
    Agraph_t *g = NULL;
    int laid_out = 0;
    char *rendered = NULL;
    size_t length = 0;

    if (!dot) {
        goto done;
    }

    gvc = gvContext();
    if (!gvc) {
        goto done;
    }

    /* Stage 1: parse (agmemread). */
    uint64_t parse_start = mono_ms();
    g = agmemread(dot);
    *out_parse_ms = mono_ms() - parse_start;
    if (!g) {
        goto done;
    }
    if (parse_budget_ms > 0 && *out_parse_ms > parse_budget_ms) {
        rc = MARKDAWN_GV_TIMEOUT_PARSE;
        goto done;
    }

    /* Stage 2: layout (gvLayout). */
    uint64_t layout_start = mono_ms();
    int layout_rc = gvLayout(gvc, g, "dot");
    *out_layout_ms = mono_ms() - layout_start;
    if (layout_rc != 0) {
        goto done;
    }
    laid_out = 1;
    if (layout_budget_ms > 0 && *out_layout_ms > layout_budget_ms) {
        rc = MARKDAWN_GV_TIMEOUT_LAYOUT;
        goto done;
    }

    /* Stage 3: render (gvRenderData). */
    uint64_t render_start = mono_ms();
    int render_rc = gvRenderData(gvc, g, "svg", &rendered, &length);
    *out_render_ms = mono_ms() - render_start;
    if (render_rc != 0 || !rendered) {
        if (rendered) {
            gvFreeRenderData(rendered);
            rendered = NULL;
        }
        goto done;
    }
    if (render_budget_ms > 0 && *out_render_ms > render_budget_ms) {
        gvFreeRenderData(rendered);
        rendered = NULL;
        rc = MARKDAWN_GV_TIMEOUT_RENDER;
        goto done;
    }

    *out_svg = rendered;
    *out_len = length;
    rendered = NULL;
    rc = MARKDAWN_GV_OK;

done:
    if (laid_out && gvc && g) {
        gvFreeLayout(gvc, g);
    }
    if (g) {
        agclose(g);
    }
    if (gvc) {
        gvFreeContext(gvc);
    }
    if (rc != MARKDAWN_GV_OK) {
        *out_err = err_take();
    }
    agseterrf(prev_errf);
    return rc;
}

void markdawn_gv_free_render_data(char *ptr) {
    if (ptr) {
        gvFreeRenderData(ptr);
    }
}

void markdawn_gv_free_error(char *ptr) {
    if (ptr) {
        free(ptr);
    }
}

/*
 * Write `n` bytes from `buf` to `fd` ignoring short writes. Returns
 * 0 on success, -1 on error.
 */
static int write_all(int fd, const void *buf, size_t n) {
    const unsigned char *p = buf;
    while (n > 0) {
        ssize_t w = write(fd, p, n);
        if (w < 0) {
            if (errno == EINTR) continue;
            return -1;
        }
        p += (size_t)w;
        n -= (size_t)w;
    }
    return 0;
}

/*
 * IPC frame layout written from child to parent over the pipe:
 *   rc         int32_t
 *   parse_ms   uint64_t
 *   layout_ms  uint64_t
 *   render_ms  uint64_t
 *   svg_len    uint64_t
 *   svg_bytes  (svg_len bytes)
 *   err_len    uint64_t
 *   err_bytes  (err_len bytes)
 * All in host byte order — parent and child are the same process
 * pre-fork, so no serialisation mismatch.
 */
#define MARKDAWN_GV_MAX_RESPONSE_BYTES (10 * 1024 * 1024)

static uint64_t mono_ms_local(void) {
    struct timespec ts;
    if (clock_gettime(CLOCK_MONOTONIC, &ts) != 0) return 0;
    return (uint64_t)ts.tv_sec * 1000u + (uint64_t)(ts.tv_nsec / 1000000);
}

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
                                  uint64_t *out_render_ms) {
    *out_svg = NULL;
    *out_len = 0;
    *out_err = NULL;
    *out_parse_ms = 0;
    *out_layout_ms = 0;
    *out_render_ms = 0;

    int pipefd[2];
    if (pipe(pipefd) != 0) {
        return MARKDAWN_GV_FORK_SYSCALL_ERROR;
    }

    pid_t pid = fork();
    if (pid < 0) {
        close(pipefd[0]);
        close(pipefd[1]);
        return MARKDAWN_GV_FORK_SYSCALL_ERROR;
    }

    if (pid == 0) {
        /* Child: do the render, stream results to the pipe, _exit. */
        close(pipefd[0]);

        char *svg = NULL;
        size_t svg_len = 0;
        char *err = NULL;
        uint64_t pms = 0, lms = 0, rms = 0;
        int rc = markdawn_gv_render_svg(
            dot, &svg, &svg_len, &err,
            parse_budget_ms, layout_budget_ms, render_budget_ms,
            &pms, &lms, &rms);

        int32_t rc32 = (int32_t)rc;
        uint64_t svg_len64 = (uint64_t)svg_len;
        uint64_t err_len64 = err ? (uint64_t)strlen(err) : 0u;

        int wrote_ok = 1;
        if (write_all(pipefd[1], &rc32, sizeof(rc32)) < 0) wrote_ok = 0;
        if (write_all(pipefd[1], &pms,  sizeof(pms)) < 0)  wrote_ok = 0;
        if (write_all(pipefd[1], &lms,  sizeof(lms)) < 0)  wrote_ok = 0;
        if (write_all(pipefd[1], &rms,  sizeof(rms)) < 0)  wrote_ok = 0;
        if (write_all(pipefd[1], &svg_len64, sizeof(svg_len64)) < 0) wrote_ok = 0;
        if (wrote_ok && svg && svg_len > 0) {
            if (write_all(pipefd[1], svg, svg_len) < 0) wrote_ok = 0;
        }
        if (write_all(pipefd[1], &err_len64, sizeof(err_len64)) < 0) wrote_ok = 0;
        if (wrote_ok && err && err_len64 > 0) {
            if (write_all(pipefd[1], err, (size_t)err_len64) < 0) wrote_ok = 0;
        }
        close(pipefd[1]);
        if (svg) gvFreeRenderData(svg);
        if (err) free(err);
        _exit(wrote_ok ? 0 : 1);
    }

    /* Parent: read from pipe with timeout. */
    close(pipefd[1]);

    size_t buf_cap = 16 * 1024;
    size_t buf_len = 0;
    unsigned char *buf = malloc(buf_cap);
    if (!buf) {
        kill(pid, SIGKILL);
        waitpid(pid, NULL, 0);
        close(pipefd[0]);
        return MARKDAWN_GV_FORK_SYSCALL_ERROR;
    }

    uint64_t deadline = mono_ms_local() + total_timeout_ms;
    int timed_out = 0;
    int oversize = 0;
    int io_err = 0;

    for (;;) {
        uint64_t now = mono_ms_local();
        int wait_ms = (now >= deadline) ? 0 : (int)(deadline - now);
        struct pollfd pfd = {.fd = pipefd[0], .events = POLLIN};
        int pr = poll(&pfd, 1, wait_ms);
        if (pr < 0) {
            if (errno == EINTR) continue;
            io_err = 1;
            break;
        }
        if (pr == 0) {
            timed_out = 1;
            break;
        }
        if (buf_cap - buf_len < 4096) {
            size_t new_cap = buf_cap * 2;
            if (new_cap > MARKDAWN_GV_MAX_RESPONSE_BYTES) {
                oversize = 1;
                break;
            }
            unsigned char *nb = realloc(buf, new_cap);
            if (!nb) { io_err = 1; break; }
            buf = nb;
            buf_cap = new_cap;
        }
        ssize_t n = read(pipefd[0], buf + buf_len, buf_cap - buf_len);
        if (n < 0) {
            if (errno == EINTR) continue;
            io_err = 1;
            break;
        }
        if (n == 0) break;  /* EOF: child closed the pipe */
        buf_len += (size_t)n;
        if (buf_len > MARKDAWN_GV_MAX_RESPONSE_BYTES) {
            oversize = 1;
            break;
        }
    }
    close(pipefd[0]);

    int status = 0;
    if (timed_out || oversize || io_err) {
        kill(pid, SIGKILL);
    }
    waitpid(pid, &status, 0);

    if (timed_out) {
        free(buf);
        return MARKDAWN_GV_FORK_TIMEOUT;
    }
    if (oversize || io_err) {
        free(buf);
        return MARKDAWN_GV_FORK_SYSCALL_ERROR;
    }
    /* Child died abnormally before writing the frame. */
    if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) {
        free(buf);
        if (WIFSIGNALED(status)) {
            return MARKDAWN_GV_FORK_CRASHED;
        }
        return MARKDAWN_GV_FORK_CRASHED;
    }

    /* Parse the frame out of buf. */
    size_t off = 0;
    int32_t rc32;
    uint64_t pms, lms, rms, svg_len64, err_len64;
    if (buf_len < sizeof(rc32) + 4 * sizeof(uint64_t)) {
        free(buf);
        return MARKDAWN_GV_FORK_CRASHED;
    }
    memcpy(&rc32, buf + off, sizeof(rc32)); off += sizeof(rc32);
    memcpy(&pms,  buf + off, sizeof(pms));  off += sizeof(pms);
    memcpy(&lms,  buf + off, sizeof(lms));  off += sizeof(lms);
    memcpy(&rms,  buf + off, sizeof(rms));  off += sizeof(rms);
    memcpy(&svg_len64, buf + off, sizeof(svg_len64)); off += sizeof(svg_len64);
    *out_parse_ms  = pms;
    *out_layout_ms = lms;
    *out_render_ms = rms;

    if (buf_len < off + (size_t)svg_len64 + sizeof(err_len64)) {
        free(buf);
        return MARKDAWN_GV_FORK_CRASHED;
    }
    if (svg_len64 > 0) {
        char *svg_copy = malloc((size_t)svg_len64);
        if (!svg_copy) {
            free(buf);
            return MARKDAWN_GV_FORK_SYSCALL_ERROR;
        }
        memcpy(svg_copy, buf + off, (size_t)svg_len64);
        *out_svg = svg_copy;
        *out_len = (size_t)svg_len64;
    }
    off += (size_t)svg_len64;
    memcpy(&err_len64, buf + off, sizeof(err_len64)); off += sizeof(err_len64);
    if (err_len64 > 0 && buf_len >= off + (size_t)err_len64) {
        char *err_copy = malloc((size_t)err_len64 + 1);
        if (err_copy) {
            memcpy(err_copy, buf + off, (size_t)err_len64);
            err_copy[err_len64] = '\0';
            *out_err = err_copy;
        }
    }
    free(buf);
    return (int)rc32;
}
