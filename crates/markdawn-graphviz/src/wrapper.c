#include "wrapper.h"

#include <stdlib.h>
#include <string.h>
#include <time.h>

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
