package nu.necessary.markdawn

import kotlinx.serialization.DeserializationStrategy
import kotlinx.serialization.ExperimentalSerializationApi
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonClassDiscriminator
import kotlinx.serialization.json.JsonContentPolymorphicSerializer
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonObject

/**
 * The framework-neutral VNode the Rust core hands us, as field-discriminated
 * JSON (mirrors the shapes consumed by `web/preact.js`):
 *
 *   { "text": "hi" }                              -> [Text]
 *   { "tag", "props", "children": [...] }         -> [Element]
 *   { "tag", "props", "html": "<svg>..." }        -> [Raw] (innerHTML is raw)
 *   { "children": [...] }   (no tag)              -> [Fragment]
 *
 * `props` values are JSON primitives (string | bool | number). The HTML-only
 * `nl_open`/`nl_close`/`void` flags never appear in this serialization.
 */
@Serializable(with = VNodeSerializer::class)
sealed interface VNode

@Serializable
data class Text(val text: String) : VNode

@Serializable
data class Element(
    val tag: String,
    val props: Map<String, JsonPrimitive> = emptyMap(),
    val children: List<VNode> = emptyList(),
) : VNode

@Serializable
data class Raw(
    val tag: String? = null,
    val props: Map<String, JsonPrimitive> = emptyMap(),
    val html: String,
) : VNode

@Serializable
data class Fragment(val children: List<VNode> = emptyList()) : VNode

/** Field-discriminated dispatch — no `type` tag, so select on key presence. */
object VNodeSerializer : JsonContentPolymorphicSerializer<VNode>(VNode::class) {
    override fun selectDeserializer(element: JsonElement): DeserializationStrategy<VNode> {
        val obj = element.jsonObject
        return when {
            "text" in obj -> Text.serializer()
            "html" in obj -> Raw.serializer()
            "tag" in obj -> Element.serializer()
            else -> Fragment.serializer()
        }
    }
}

/** One keyed reconciliation event from `takeEvents()`. */
@OptIn(ExperimentalSerializationApi::class)
@Serializable
@JsonClassDiscriminator("type")
sealed interface VdomEvent {
    val id: Long
}

@Serializable
@SerialName("upsert")
data class Upsert(override val id: Long, val vtree: VNode) : VdomEvent

@Serializable
@SerialName("delete")
data class Delete(override val id: Long) : VdomEvent

internal val MarkdawnJson = Json {
    ignoreUnknownKeys = true
    isLenient = true
}

internal fun parseVNodeList(json: String): List<VNode> =
    MarkdawnJson.decodeFromString(json)

internal fun parseEvents(json: String): List<VdomEvent> =
    MarkdawnJson.decodeFromString(json)
