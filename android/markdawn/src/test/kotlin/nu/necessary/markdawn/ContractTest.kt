package nu.necessary.markdawn

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * Cross-language contract: the Kotlin kotlinx.serialization model must
 * deserialize the *exact* JSON the Rust `serde_json` layer emits. The fixture
 * strings below are real output captured from `markdawn-android`
 * (`parse_to_vdom_json` / `VdomParser::take_events`) — not hand-authored — so a
 * drift between the Rust `VNode`/event serialization and this model fails here.
 *
 * Pure JVM unit test (`:markdawn:testDebugUnitTest`); no JNI / emulator.
 */
class ContractTest {

    // Real output of: "# Title\n\nA para with **bold**, `code`, [x](https://a.com).\n\n- [x] done\n- [ ] todo\n"
    private val blocksJson = """[{"tag":"h1","props":{},"children":[{"text":"Title"}]},{"tag":"p","props":{},"children":[{"text":"A para with "},{"tag":"strong","props":{},"children":[{"text":"bold"}]},{"text":", "},{"tag":"code","props":{},"children":[{"text":"code"}]},{"text":", "},{"tag":"a","props":{"href":"https://a.com"},"children":[{"text":"x"}]},{"text":"."}]},{"tag":"ul","props":{},"children":[{"tag":"li","props":{},"children":[{"tag":"input","props":{"type":"checkbox","disabled":true,"checked":true},"children":[]},{"text":" "},{"text":"done"}]},{"tag":"li","props":{},"children":[{"tag":"input","props":{"type":"checkbox","disabled":true},"children":[]},{"text":" "},{"text":"todo"}]}]}]"""

    // Real streaming output (note the provisional re-emit of id 2).
    private val eventsJson = """[{"type":"upsert","id":1,"vtree":{"tag":"p","props":{},"children":[{"text":"hi there"}]}},{"type":"upsert","id":2,"vtree":{"tag":"p","props":{},"children":[{"text":"next"}]}},{"type":"upsert","id":2,"vtree":{"tag":"p","props":{},"children":[{"text":"next"}]}}]"""

    private fun find(node: VNode, pred: (VNode) -> Boolean): VNode? {
        if (pred(node)) return node
        val kids = when (node) {
            is Element -> node.children
            is Fragment -> node.children
            else -> emptyList()
        }
        for (k in kids) find(k, pred)?.let { return it }
        return null
    }

    private fun firstTag(trees: List<VNode>, tag: String): Element? =
        trees.firstNotNullOfOrNull { t -> find(t) { it is Element && it.tag == tag } as? Element }

    @Test
    fun deserializesBlockStructure() {
        val trees = parseVNodeList(blocksJson)
        assertEquals(3, trees.size)

        val h1 = assertIs<Element>(trees[0])
        assertEquals("h1", h1.tag)
        assertEquals("Title", assertIs<Text>(h1.children.single()).text)

        // Link inside the paragraph, with a url-safe href.
        val a = firstTag(trees, "a")!!
        assertEquals("https://a.com", a.props["href"]?.content)
        // Inline emphasis/code survived as nested elements.
        assertEquals("bold", collect(firstTag(trees, "strong")!!))
        assertEquals("code", collect(firstTag(trees, "code")!!))
    }

    @Test
    fun deserializesBooleanProps() {
        val trees = parseVNodeList(blocksJson)
        // Gather both checkbox inputs in document order.
        val checkboxes = mutableListOf<Element>()
        val ul = firstTag(trees, "ul")!!
        ul.children.filterIsInstance<Element>().forEach { li ->
            li.children.filterIsInstance<Element>()
                .filter { it.tag == "input" }
                .forEach { checkboxes.add(it) }
        }
        assertEquals(2, checkboxes.size)

        // First box: checked + disabled (boolean JSON primitives); empty children.
        assertEquals("checkbox", checkboxes[0].props["type"]?.content)
        assertEquals("true", checkboxes[0].props["disabled"]?.content)
        assertEquals("true", checkboxes[0].props["checked"]?.content)
        assertTrue(checkboxes[0].children.isEmpty())

        // Second box: disabled but NOT checked — key absent, not false.
        assertEquals("true", checkboxes[1].props["disabled"]?.content)
        assertNull(checkboxes[1].props["checked"])
    }

    @Test
    fun deserializesStreamEvents() {
        val events = parseEvents(eventsJson)
        assertEquals(3, events.size)
        events.forEach { assertIs<Upsert>(it) }
        assertEquals(listOf(1L, 2L, 2L), events.map { it.id })

        val first = assertIs<Upsert>(events[0])
        val p = assertIs<Element>(first.vtree)
        assertEquals("p", p.tag)
        assertEquals("hi there", assertIs<Text>(p.children.single()).text)
    }

    @Test
    fun deserializesRawAndFragmentAndDelete() {
        // Shapes not present in the fixtures above — same wire format.
        val raw = parseVNodeList(
            """[{"tag":null,"props":{"aria-label":"Pikchr diagram"},"html":"<svg></svg>"}]""",
        ).single()
        val r = assertIs<Raw>(raw)
        assertNull(r.tag)
        assertEquals("<svg></svg>", r.html)
        assertEquals("Pikchr diagram", r.props["aria-label"]?.content)

        val frag = parseVNodeList("""[{"children":[{"text":"x"}]}]""").single()
        assertIs<Fragment>(frag)

        val del = parseEvents("""[{"type":"delete","id":7}]""").single()
        assertEquals(7L, assertIs<Delete>(del).id)
    }

    private fun collect(node: VNode): String = when (node) {
        is Text -> node.text
        is Element -> node.children.joinToString("") { collect(it) }
        is Fragment -> node.children.joinToString("") { collect(it) }
        is Raw -> ""
    }
}
