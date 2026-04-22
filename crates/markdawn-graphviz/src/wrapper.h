#ifndef MARKDAWN_GRAPHVIZ_WRAPPER_H
#define MARKDAWN_GRAPHVIZ_WRAPPER_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/*
 * Return codes from markdawn_gv_render_svg:
 *   0 = success
 *   1 = generic failure (parse / layout / render error from libgraphviz)
 *   2 = parse stage exceeded parse_budget_ms
 *   3 = layout stage exceeded layout_budget_ms
 *   4 = render stage exceeded render_budget_ms
 */
#define MARKDAWN_GV_OK                    0
#define MARKDAWN_GV_FAILED                1
#define MARKDAWN_GV_TIMEOUT_PARSE         2
#define MARKDAWN_GV_TIMEOUT_LAYOUT        3
#define MARKDAWN_GV_TIMEOUT_RENDER        4

/*
 * Render DOT source as SVG with per-stage wall-clock budgets.
 *
 * libgraphviz's pipeline has three distinct stages — parse
 * (agmemread), layout (gvLayout), render (gvRenderData) — which are
 * called in sequence. Budgets are enforced BETWEEN stages (we can't
 * interrupt libgraphviz mid-call): after each stage returns, the C
 * shim checks elapsed time against the stage's budget and bails
 * with MARKDAWN_GV_TIMEOUT_* if exceeded. This means a pathological
 * parse stage doesn't consume the layout + render budgets too.
 *
 * Budgets are in milliseconds. Pass 0 for "no stage-specific cap"
 * (total wall-clock is still bounded by the Rust-side recv_timeout
 * on the whole worker thread).
 *
 * Output semantics on MARKDAWN_GV_OK:
 *   *out_svg       points at a malloc'ed SVG buffer; free via
 *                  markdawn_gv_free_render_data.
 *   *out_len       byte length of the SVG.
 *   *out_err       NULL.
 *
 * Output semantics on non-zero return:
 *   *out_svg       NULL.
 *   *out_len       0.
 *   *out_err       NULL or malloc'ed diagnostic; free via
 *                  markdawn_gv_free_error.
 *
 * Per-stage elapsed times (milliseconds) are written to
 * *out_parse_ms / *out_layout_ms / *out_render_ms regardless of
 * outcome, so the caller can see which stage dominated. A stage
 * that wasn't reached (because an earlier stage failed or timed
 * out) reports 0.
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

/* Free a buffer returned via *out_svg. */
void markdawn_gv_free_render_data(char *ptr);

/* Free a diagnostic string returned via *out_err. */
void markdawn_gv_free_error(char *ptr);

#ifdef __cplusplus
}
#endif

#endif
