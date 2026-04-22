#ifndef MARKDAWN_GRAPHVIZ_WRAPPER_H
#define MARKDAWN_GRAPHVIZ_WRAPPER_H

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/*
 * Render DOT source as SVG.
 *
 * Returns 0 on success. On success:
 *   *out_svg    points at a malloc'ed SVG buffer; caller frees via
 *               markdawn_gv_free_render_data.
 *   *out_len    is the byte length of the SVG.
 *   *out_err    is NULL.
 *
 * Returns non-zero on failure. On failure:
 *   *out_svg    is NULL.
 *   *out_len    is 0.
 *   *out_err    is either NULL (no diagnostic available) or a malloc'ed
 *               NUL-terminated string; caller frees via
 *               markdawn_gv_free_error.
 *
 * libgraphviz holds process-global context — callers must serialise
 * invocations of this function across threads themselves.
 */
int markdawn_gv_render_svg(const char *dot,
                           char **out_svg,
                           size_t *out_len,
                           char **out_err);

/* Free a buffer returned via *out_svg. */
void markdawn_gv_free_render_data(char *ptr);

/* Free a diagnostic string returned via *out_err. */
void markdawn_gv_free_error(char *ptr);

#ifdef __cplusplus
}
#endif

#endif
