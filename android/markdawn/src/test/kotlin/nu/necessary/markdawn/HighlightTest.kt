package nu.necessary.markdawn

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

/**
 * Verifies the syntax-highlight span parser against the *real* arborium
 * CustomElements HTML — captured from markdawn-android's
 * `highlighted_code_emits_custom_element_html` test (rust: `fn main() {}`).
 */
class HighlightTest {
    private val highlighted = "<a-k>fn</a-k> <a-f>main</a-f><a-p>()</a-p> <a-p>{}</a-p>"

    @Test
    fun coloursHighlightSpans() {
        val a = inlineHtmlToAnnotatedString(highlighted)
        assertEquals("fn main() {}", a.text)

        val keyword = a.spanStyles.firstOrNull { it.start == 0 && it.end == 2 }
        assertNotNull(keyword, "expected a coloured span over 'fn'")
        assertTrue(keyword.item.color != Color.Unspecified, "keyword should be coloured")

        val function = a.spanStyles.firstOrNull { it.start == 3 && it.end == 7 }
        assertNotNull(function, "expected a coloured span over 'main'")
        assertTrue(function.item.color != Color.Unspecified)
        assertTrue(
            keyword.item.color != function.item.color,
            "keyword and function should get distinct colours",
        )
    }

    @Test
    fun unescapesEntitiesAndKeepsUnknownTagText() {
        val a = inlineHtmlToAnnotatedString("""<a-s>"a &lt; b"</a-s><unknown>z</unknown>""")
        assertEquals("\"a < b\"z", a.text)
    }

    @Test
    fun appliesInlineEmphasis() {
        val a = inlineHtmlToAnnotatedString("<strong>x</strong><em>y</em>")
        assertEquals("xy", a.text)
        assertEquals(FontWeight.Bold, a.spanStyles.first { it.start == 0 && it.end == 1 }.item.fontWeight)
        assertEquals(FontStyle.Italic, a.spanStyles.first { it.start == 1 && it.end == 2 }.item.fontStyle)
    }
}
