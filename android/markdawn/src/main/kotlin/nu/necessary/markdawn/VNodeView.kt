package nu.necessary.markdawn

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Checkbox
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import coil3.compose.AsyncImage
import coil3.request.ImageRequest

/**
 * Render one markdawn [VNode] to Compose — the native analog of `toVNode` in
 * `web/preact.js`. Block-level nodes become composables; inline runs are folded
 * into an [AnnotatedString].
 *
 * Starter-level (parity with the Preact mapper's coverage, room to grow):
 *  - syntax-highlighted code arrives as a `Raw` HTML span tree; here its tags
 *    are stripped to plain monospace text (span coloring is a follow-up).
 *  - inline images mid-paragraph aren't embedded into the text run yet.
 *  - raw SVG (math, future diagrams) needs an [coil3.ImageLoader] with the SVG
 *    decoder registered (`io.coil-kt.coil3:coil-svg`).
 */
@Composable
fun VNodeView(node: VNode, modifier: Modifier = Modifier) {
    when (node) {
        is Text -> Text(node.text, modifier)
        is Fragment -> Column(modifier) { node.children.forEach { VNodeView(it) } }
        is Raw -> RawView(node, modifier)
        is Element -> ElementView(node, modifier)
    }
}

private val HEADING = mapOf(
    "h1" to (30.sp to FontWeight.Bold),
    "h2" to (26.sp to FontWeight.Bold),
    "h3" to (22.sp to FontWeight.SemiBold),
    "h4" to (19.sp to FontWeight.SemiBold),
    "h5" to (17.sp to FontWeight.Medium),
    "h6" to (15.sp to FontWeight.Medium),
)

@Composable
private fun ElementView(node: Element, modifier: Modifier) {
    when (node.tag) {
        "p" -> Text(buildInline(node.children), modifier.padding(vertical = 4.dp))

        in HEADING.keys -> {
            val (size, weight) = HEADING.getValue(node.tag)
            Text(
                buildInline(node.children),
                modifier.padding(top = 12.dp, bottom = 4.dp),
                fontSize = size,
                fontWeight = weight,
            )
        }

        "ul", "ol" -> Column(modifier.padding(start = 8.dp, top = 2.dp, bottom = 2.dp)) {
            val ordered = node.tag == "ol"
            node.children.filterIsInstance<Element>()
                .filter { it.tag == "li" }
                .forEachIndexed { i, li -> ListItemRow(ordered, i, li) }
        }

        "blockquote" -> Row(modifier.padding(vertical = 4.dp)) {
            Box(
                Modifier
                    .width(4.dp)
                    .background(MaterialTheme.colorScheme.outlineVariant),
            )
            Column(Modifier.padding(start = 8.dp)) {
                node.children.forEach { VNodeView(it) }
            }
        }

        "pre" -> CodeBlock(node, modifier)

        "hr" -> HorizontalDivider(modifier.padding(vertical = 8.dp))

        "table" -> TableView(node, modifier)

        "img" -> ImageView(node, modifier)

        "input" -> Checkbox(
            checked = node.props["checked"]?.let { it.content == "true" } ?: false,
            onCheckedChange = null,
            enabled = false,
            modifier = modifier,
        )

        // Inline-only tags reaching here standalone — render as a text run.
        "em", "strong", "code", "del", "a", "br" ->
            Text(buildInline(listOf(node)), modifier)

        else -> Column(modifier) { node.children.forEach { VNodeView(it) } }
    }
}

@Composable
private fun ListItemRow(ordered: Boolean, index: Int, li: Element) {
    Row(Modifier.padding(vertical = 1.dp)) {
        Text(if (ordered) "${index + 1}. " else "•  ")
        // A list item's content is usually a tight paragraph's worth of inline
        // nodes; nested blocks (sub-lists) recurse.
        Column {
            val (inline, block) = li.children.partition { isInline(it) }
            if (inline.isNotEmpty()) Text(buildInline(inline))
            block.forEach { VNodeView(it) }
        }
    }
}

@Composable
private fun CodeBlock(pre: Element, modifier: Modifier) {
    val code = pre.children.firstOrNull()
    val text = when (code) {
        is Element -> collectText(code)
        is Raw -> stripTags(code.html)
        else -> collectText(pre)
    }
    Box(
        modifier
            .fillMaxWidth()
            .padding(vertical = 4.dp)
            .background(MaterialTheme.colorScheme.surfaceVariant),
    ) {
        Text(text, Modifier.padding(8.dp), fontFamily = FontFamily.Monospace, fontSize = 13.sp)
    }
}

@Composable
private fun TableView(table: Element, modifier: Modifier) {
    // Flatten thead/tbody → rows of th/td cells; render each row as a Row.
    val rows = mutableListOf<Element>()
    fun collectRows(n: VNode) {
        if (n is Element) {
            if (n.tag == "tr") rows.add(n) else n.children.forEach { collectRows(it) }
        }
    }
    collectRows(table)
    Column(modifier.padding(vertical = 4.dp)) {
        rows.forEach { tr ->
            Row {
                tr.children.filterIsInstance<Element>().forEach { cell ->
                    val header = cell.tag == "th"
                    Text(
                        buildInline(cell.children),
                        Modifier
                            .weight(1f)
                            .padding(horizontal = 6.dp, vertical = 3.dp),
                        fontWeight = if (header) FontWeight.Bold else null,
                    )
                }
            }
        }
    }
}

@Composable
private fun RawView(node: Raw, modifier: Modifier) {
    val html = node.html
    if (html.trimStart().startsWith("<svg")) {
        AsyncImage(
            model = ImageRequest.Builder(LocalContext.current)
                .data(html.toByteArray())
                .build(),
            contentDescription = null,
            modifier = modifier.padding(vertical = 4.dp),
        )
    } else {
        // Non-SVG raw HTML (e.g. highlighted code, filtered passthrough) has no
        // structural form here — show its text, tags stripped.
        Text(stripTags(html), modifier, fontFamily = FontFamily.Monospace, fontSize = 13.sp)
    }
}

@Composable
private fun ImageView(node: Element, modifier: Modifier) {
    val src = node.props["src"]?.content.orEmpty()
    if (src.isEmpty()) return
    AsyncImage(
        model = src,
        contentDescription = node.props["alt"]?.content,
        modifier = modifier.padding(vertical = 4.dp),
    )
}

// ----- inline accumulation -----------------------------------------

private fun buildInline(nodes: List<VNode>): AnnotatedString = buildAnnotatedString {
    nodes.forEach { appendInline(it) }
}

private fun AnnotatedString.Builder.appendInline(node: VNode) {
    when (node) {
        is Text -> append(node.text)
        is Fragment -> node.children.forEach { appendInline(it) }
        is Raw -> append(stripTags(node.html))
        is Element -> when (node.tag) {
            "em" -> withStyle(SpanStyle(fontStyle = FontStyle.Italic)) {
                node.children.forEach { appendInline(it) }
            }
            "strong" -> withStyle(SpanStyle(fontWeight = FontWeight.Bold)) {
                node.children.forEach { appendInline(it) }
            }
            "del" -> withStyle(SpanStyle(textDecoration = TextDecoration.LineThrough)) {
                node.children.forEach { appendInline(it) }
            }
            "code" -> withStyle(SpanStyle(fontFamily = FontFamily.Monospace)) {
                node.children.forEach { appendInline(it) }
            }
            "a" -> {
                val href = node.props["href"]?.content.orEmpty()
                pushStringAnnotation("URL", href)
                withStyle(SpanStyle(textDecoration = TextDecoration.Underline)) {
                    node.children.forEach { appendInline(it) }
                }
                pop()
            }
            "br" -> append("\n")
            else -> node.children.forEach { appendInline(it) }
        }
    }
}

// ----- helpers -----------------------------------------------------

private val INLINE_TAGS = setOf("em", "strong", "code", "del", "a", "br", "img")

private fun isInline(node: VNode): Boolean = when (node) {
    is Text -> true
    is Element -> node.tag in INLINE_TAGS
    else -> false
}

private fun collectText(node: VNode): String = when (node) {
    is Text -> node.text
    is Element -> node.children.joinToString("") { collectText(it) }
    is Fragment -> node.children.joinToString("") { collectText(it) }
    is Raw -> stripTags(node.html)
}

private val TAG_RE = Regex("<[^>]*>")

private fun stripTags(html: String): String =
    TAG_RE.replace(html, "")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
