package nu.necessary.markdawn

import java.io.Closeable

/** How raw inline/block HTML is treated. [Safe] sanitizes; [Unrestricted] passes through. */
enum class RawHtml { Safe, Unrestricted }

/** Render options, serialized to the JSON the native layer expects. */
data class VdomOptions(val rawHtml: RawHtml = RawHtml.Safe) {
    internal fun toJson(): String = when (rawHtml) {
        RawHtml.Safe -> "{}"
        RawHtml.Unrestricted -> """{"rawHtml":"unrestricted"}"""
    }
}

/** High-level entry points over [Native]. The analog of `Markdawn.swift`. */
object Markdawn {
    /** One-shot static render: the ordered top-level block vnode trees. */
    fun parseToVdom(source: String, options: VdomOptions = VdomOptions()): List<VNode> =
        parseVNodeList(Native.parseToVdom(source, options.toJson()))

    /**
     * Load a tree-sitter grammar plugin so subsequent renders highlight code
     * blocks tagged `name`. Mirrors Swift's `Markdawn.loadLanguage`: pull the
     * plugin `.so` into the process, then register its grammar (the dlsym +
     * register dance runs natively). Idempotent; returns false if the plugin
     * `.so` isn't bundled.
     *
     * The `libmarkdawn_swift_lang_<name>.so` must be in the app's jniLibs —
     * built by `crates/markdawn-android/build-android.sh`.
     */
    fun loadLanguage(name: String): Boolean {
        val lib = "markdawn_swift_lang_" + name.replace('-', '_')
        try {
            System.loadLibrary(lib)
        } catch (_: UnsatisfiedLinkError) {
            return false
        }
        return Native.loadLanguage(name)
    }
}

/**
 * Streaming parser for LLM/network output. Push chunks, [finish], and drain
 * reconciliation events after each push. `Closeable` — frees the native parser.
 */
class VdomParser(options: VdomOptions = VdomOptions()) : Closeable {
    private var ptr: Long = Native.parserNew(options.toJson())

    fun push(chunk: String) = Native.parserPush(ptr, chunk)

    fun finish() = Native.parserFinish(ptr)

    fun takeEvents(): List<VdomEvent> = parseEvents(Native.parserTakeEvents(ptr))

    override fun close() {
        if (ptr != 0L) {
            Native.parserFree(ptr)
            ptr = 0L
        }
    }
}

/**
 * Edit-aware document for a live editor. Each mutation returns the
 * reconciliation events it produced (apply them to a [BlockStore]). `Closeable`.
 */
class VdomDocument(options: VdomOptions = VdomOptions()) : Closeable {
    private var ptr: Long = Native.documentNew(options.toJson())

    /** Replace the whole buffer; re-parses from scratch. */
    fun setSource(source: String): List<VdomEvent> {
        Native.documentSetSource(ptr, source)
        return takeEvents()
    }

    /** Splice `replacement` into byte range `[start, end)`; re-parses affected blocks. */
    fun edit(start: Int, end: Int, replacement: String): List<VdomEvent> {
        Native.documentEdit(ptr, start.toLong(), end.toLong(), replacement)
        return takeEvents()
    }

    /** Move the reveal-source cursor; `null` clears it. */
    fun setCursor(byteOffset: Int?): List<VdomEvent> {
        Native.documentSetCursor(ptr, (byteOffset ?: -1).toLong())
        return takeEvents()
    }

    fun takeEvents(): List<VdomEvent> = parseEvents(Native.documentTakeEvents(ptr))

    override fun close() {
        if (ptr != 0L) {
            Native.documentFree(ptr)
            ptr = 0L
        }
    }
}
