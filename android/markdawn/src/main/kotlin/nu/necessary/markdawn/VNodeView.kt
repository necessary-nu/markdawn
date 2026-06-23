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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
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
    // pre > code(.language-x). Highlighted code's `code` holds a single Raw of
    // arborium `<a-k>…</a-k>` spans; un-highlighted code holds Text children.
    val code = pre.children.firstOrNull() as? Element
    val inner = code?.children?.firstOrNull()
    val content = when (inner) {
        is Raw -> inlineHtmlToAnnotatedString(inner.html)
        else -> AnnotatedString(collectText(code ?: pre))
    }
    Box(
        modifier
            .fillMaxWidth()
            .padding(vertical = 4.dp)
            .background(MaterialTheme.colorScheme.surfaceVariant),
    ) {
        Text(content, Modifier.padding(8.dp), fontFamily = FontFamily.Monospace, fontSize = 13.sp)
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
                        textAlign = when (cell.props["align"]?.content) {
                            "center" -> TextAlign.Center
                            "right" -> TextAlign.End
                            "left" -> TextAlign.Start
                            else -> null
                        },
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
        // Non-SVG raw HTML (highlighted code, filtered passthrough): render the
        // subset of inline tags we understand; unknown tags drop to their text.
        Text(
            inlineHtmlToAnnotatedString(html),
            modifier,
            fontFamily = FontFamily.Monospace,
            fontSize = 13.sp,
        )
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
        is Raw -> append(inlineHtmlToAnnotatedString(node.html))
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
            // Inline images can't sit in a text run without InlineTextContent;
            // keep the alt text so the content isn't silently dropped.
            "img" -> node.props["alt"]?.content?.let { append(it) }
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
    is Raw -> inlineHtmlToAnnotatedString(node.html).text
}

/**
 * Parse the small subset of inline HTML markdawn emits into a styled
 * [AnnotatedString]:
 *  - arborium syntax-highlight spans `<a-k>…</a-k>` (k=keyword, f=function, …)
 *    map to a colour from [HIGHLIGHT_COLORS];
 *  - `<strong>`/`<b>`, `<em>`/`<i>`, `<code>`, `<br>`;
 *  - HTML entities are unescaped.
 * Unknown tags are dropped but their text is kept. No base font family — the
 * caller decides (code blocks render monospace).
 */
internal fun inlineHtmlToAnnotatedString(html: String): AnnotatedString {
    val sb = StringBuilder()
    val runs = ArrayList<Triple<Int, Int, SpanStyle>>()
    val stack = ArrayDeque<SpanStyle>()
    var i = 0
    val n = html.length
    while (i < n) {
        if (html[i] == '<') {
            val gt = html.indexOf('>', i)
            if (gt < 0) {
                sb.append(unescapeHtml(html.substring(i)))
                break
            }
            val tag = html.substring(i + 1, gt).trim()
            i = gt + 1
            when {
                tag.startsWith("/") -> if (stack.isNotEmpty()) stack.removeLast()
                tag == "br" || tag == "br/" -> sb.append('\n')
                else -> stack.addLast(styleForTag(tag))
            }
        } else {
            val next = html.indexOf('<', i).let { if (it < 0) n else it }
            val piece = unescapeHtml(html.substring(i, next))
            if (piece.isNotEmpty()) {
                val start = sb.length
                sb.append(piece)
                val merged = stack.fold(SpanStyle()) { acc, s -> acc.merge(s) }
                if (merged != SpanStyle()) runs.add(Triple(start, sb.length, merged))
            }
            i = next
        }
    }
    return buildAnnotatedString {
        append(sb.toString())
        runs.forEach { (start, end, style) -> addStyle(style, start, end) }
    }
}

private fun styleForTag(tag: String): SpanStyle = when {
    tag.startsWith("a-") ->
        HIGHLIGHT_COLORS[tag.substring(2)]?.let { SpanStyle(color = it) } ?: SpanStyle()
    tag == "strong" || tag == "b" -> SpanStyle(fontWeight = FontWeight.Bold)
    tag == "em" || tag == "i" -> SpanStyle(fontStyle = FontStyle.Italic)
    tag == "code" -> SpanStyle(fontFamily = FontFamily.Monospace)
    else -> SpanStyle()
}

/** arborium highlight kinds (short tag -> colour), a GitHub-light-ish palette. */
private val HIGHLIGHT_COLORS: Map<String, Color> = mapOf(
    "k" to Color(0xFFD73A49),   // keyword
    "f" to Color(0xFF6F42C1),   // function
    "s" to Color(0xFF032F62),   // string
    "c" to Color(0xFF6A737D),   // comment
    "t" to Color(0xFF005CC5),   // type
    "v" to Color(0xFF24292E),   // variable
    "co" to Color(0xFF005CC5),  // constant
    "p" to Color(0xFF24292E),   // punctuation
    "pr" to Color(0xFF005CC5),  // property
    "at" to Color(0xFF6F42C1),  // attribute
    "tg" to Color(0xFF22863A),  // tag
    "m" to Color(0xFF6F42C1),   // macro
    "l" to Color(0xFFE36209),   // label
    "ns" to Color(0xFF6F42C1),  // namespace
    "cr" to Color(0xFF6F42C1),  // constructor
    "tt" to Color(0xFF005CC5),  // title
    "st" to Color(0xFF24292E),  // strong
    "em" to Color(0xFF24292E),  // emphasis
    "da" to Color(0xFF22863A),  // diff addition
    "dd" to Color(0xFFD73A49),  // diff deletion
    "er" to Color(0xFFD73A49),  // error
)

private fun unescapeHtml(s: String): String =
    if ('&' !in s) {
        s
    } else {
        s.replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#39;", "'")
            .replace("&amp;", "&")
    }
