#ifndef MARKDAWN_H
#define MARKDAWN_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ---------- HTML ABI (all platforms) ---------- */

typedef struct MwState MwState;

MwState* mw_state_new(void);
uint32_t mw_state_push_byte(MwState* state, uint8_t byte);
uint32_t mw_state_push_chunk(MwState* state, const uint8_t* ptr, size_t len);
uint32_t mw_state_finish(MwState* state);
char*    mw_state_render(const MwState* state);
void     mw_string_free(char* ptr);
void     mw_state_free(MwState* state);

/* ---------- Grammar registration ABI (all platforms) ---------- */

/* Register a language whose plugin framework is already loaded. Swift
 * dlopen()s MarkdawnLang<Pascal>.framework and then calls this with the
 * language name; the provider symbol lookup and descriptor validation happen
 * inside. No function pointer is handed to the plugin — the descriptor is
 * pulled from it. Return codes: 0 success, 1 invalid name, 2 the plugin's
 * entry symbol is not loaded, 3 the descriptor is invalid or conflicts. */
uint32_t mw_register_language(const char* name, size_t name_len);

/* ---------- UIKit ABI (iOS / Catalyst / Sim only) ---------- */

#if __has_include(<UIKit/UIKit.h>)

typedef struct MwUIKitState MwUIKitState;

MwUIKitState* mw_uikit_state_new(void);
uint32_t mw_uikit_state_push_chunk(MwUIKitState* state, const uint8_t* ptr, size_t len);
uint32_t mw_uikit_state_finish(MwUIKitState* state);
void*    mw_uikit_state_stack_view(const MwUIKitState* state);       /* +1 UIStackView* */
void*    mw_uikit_state_attributed_text(const MwUIKitState* state);  /* +1 NSAttributedString* */
void     mw_uikit_state_free(MwUIKitState* state);

#endif

#ifdef __cplusplus
}
#endif

#endif
