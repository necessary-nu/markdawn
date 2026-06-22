package nu.necessary.markdawn

/**
 * Raw JNI surface — one external fun per `Java_nu_necessary_markdawn_Native_*`
 * export in `crates/markdawn-android/src/lib.rs`. Every string crossing the
 * boundary is JSON; callers parse it via the kotlinx.serialization model in
 * [VNode]. Prefer the typed wrappers in [Markdawn] over calling these directly.
 */
internal object Native {
    init {
        System.loadLibrary("markdawn_android")
    }

    external fun parseToVdom(source: String, options: String): String

    /** Resolve a plugin's `mw_lang_<name>_init` (already dlopen'd) and register it. */
    external fun loadLanguage(name: String): Boolean

    // Streaming parser (opaque native pointer).
    external fun parserNew(options: String): Long
    external fun parserPush(ptr: Long, chunk: String)
    external fun parserFinish(ptr: Long)
    external fun parserTakeEvents(ptr: Long): String
    external fun parserFree(ptr: Long)

    // Edit-aware document (opaque native pointer).
    external fun documentNew(options: String): Long
    external fun documentSetSource(ptr: Long, source: String)
    external fun documentEdit(ptr: Long, start: Long, end: Long, replacement: String)
    external fun documentSetCursor(ptr: Long, byteOffset: Long)
    external fun documentTakeEvents(ptr: Long): String
    external fun documentFree(ptr: Long)
}
