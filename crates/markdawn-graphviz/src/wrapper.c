#include "wrapper.h"

#include <stdlib.h>
#include <string.h>

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

int markdawn_gv_render_svg(const char *dot,
                           char **out_svg,
                           size_t *out_len,
                           char **out_err) {
    *out_svg = NULL;
    *out_len = 0;
    *out_err = NULL;

    err_reset();
    agseterrf(err_sink);

    if (!dot) {
        *out_err = err_take();
        return 1;
    }

    GVC_t *gvc = gvContext();
    if (!gvc) {
        *out_err = err_take();
        return 1;
    }

    Agraph_t *g = agmemread(dot);
    if (!g) {
        gvFreeContext(gvc);
        *out_err = err_take();
        return 1;
    }

    if (gvLayout(gvc, g, "dot") != 0) {
        agclose(g);
        gvFreeContext(gvc);
        *out_err = err_take();
        return 1;
    }

    char *rendered = NULL;
    size_t length = 0;
    int rc = gvRenderData(gvc, g, "svg", &rendered, &length);

    gvFreeLayout(gvc, g);
    agclose(g);
    gvFreeContext(gvc);

    if (rc != 0 || !rendered) {
        if (rendered) {
            gvFreeRenderData(rendered);
        }
        *out_err = err_take();
        return 1;
    }

    *out_svg = rendered;
    *out_len = length;
    return 0;
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
