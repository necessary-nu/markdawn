//! Shape of the JNI binding: the exported symbol names (which *are* the
//! Kotlin binding), the parameter/return types each entry point crosses
//! with, and the JSON wire format Kotlin deserializes.
//!
//! Calling an entry point needs a live `JNIEnv`, so the bodies are exercised
//! through the JNI-free core; what is checked here is the boundary itself.

#[cfg(test)]
mod tests {
    use crate::{VdomDocument, VdomParser, filter_from_options, parse_to_vdom_json};
    use jni::JNIEnv;
    use jni::objects::{JClass, JString};
    use jni::sys::{jboolean, jlong, jstring};
    use serde_json::Value;

    // [spec:mw:def:ffi.host.jni/test]
    #[test]
    fn every_entry_point_is_exported_under_its_kotlin_qualified_symbol_name() {
        // `#[unsafe(no_mangle)]` makes the Rust item name the linker symbol,
        // so binding each one to a typed function pointer asserts both the
        // exact `Java_nu_necessary_markdawn_Native_<method>` name and the
        // signature. Renaming the Kotlin package, class or method breaks
        // this instead of failing silently at first call.
        type Env<'a> = JNIEnv<'a>;
        type Class<'a> = JClass<'a>;
        type Str<'a> = JString<'a>;

        // One-shot + grammar loading.
        let _: for<'a> extern "system" fn(Env<'a>, Class<'a>, Str<'a>, Str<'a>) -> jstring =
            crate::Java_nu_necessary_markdawn_Native_parseToVdom;
        let _: for<'a> extern "system" fn(Env<'a>, Class<'a>, Str<'a>) -> jboolean =
            crate::Java_nu_necessary_markdawn_Native_loadLanguage;

        // Streaming parser: constructor mints a handle, destructor takes it.
        let _: for<'a> extern "system" fn(Env<'a>, Class<'a>, Str<'a>) -> jlong =
            crate::Java_nu_necessary_markdawn_Native_parserNew;
        let _: for<'a> extern "system" fn(Env<'a>, Class<'a>, jlong, Str<'a>) =
            crate::Java_nu_necessary_markdawn_Native_parserPush;
        let _: for<'a> extern "system" fn(Env<'a>, Class<'a>, jlong) =
            crate::Java_nu_necessary_markdawn_Native_parserFinish;
        let _: for<'a> extern "system" fn(Env<'a>, Class<'a>, jlong) -> jstring =
            crate::Java_nu_necessary_markdawn_Native_parserTakeEvents;
        let _: for<'a> extern "system" fn(Env<'a>, Class<'a>, jlong) =
            crate::Java_nu_necessary_markdawn_Native_parserFree;

        // Editable document.
        let _: for<'a> extern "system" fn(Env<'a>, Class<'a>, Str<'a>) -> jlong =
            crate::Java_nu_necessary_markdawn_Native_documentNew;
        let _: for<'a> extern "system" fn(Env<'a>, Class<'a>, jlong) -> jstring =
            crate::Java_nu_necessary_markdawn_Native_documentTakeEvents;
        // "No cursor" crosses as a negative `byteOffset`, since `jlong` has
        // no null — hence the plain `jlong` parameter rather than an object.
        let _: for<'a> extern "system" fn(Env<'a>, Class<'a>, jlong, jlong) =
            crate::Java_nu_necessary_markdawn_Native_documentSetCursor;
        let _: for<'a> extern "system" fn(Env<'a>, Class<'a>, jlong) =
            crate::Java_nu_necessary_markdawn_Native_documentFree;
    }

    // [spec:mw:def:ffi.host.jni/test]
    #[test]
    fn a_negative_byte_offset_is_the_encoding_for_no_cursor() {
        // `jlong` is a signed 64-bit integer with no null, so the whole
        // non-negative range is a real offset and the negative range is
        // reserved for absence. Both transitions are legal on the core.
        let mut document = VdomDocument::new("{}");
        document.set_source("para one\n\npara two\n").unwrap();
        let _ = document.take_events().unwrap();

        let decode = |byte_offset: jlong| -> Option<usize> {
            if byte_offset < 0 {
                None
            } else {
                Some(byte_offset as usize)
            }
        };
        assert_eq!(decode(-1), None);
        assert_eq!(decode(-9_999), None);
        assert_eq!(decode(0), Some(0));
        assert_eq!(decode(7), Some(7));

        document.set_cursor(decode(0)).unwrap();
        document.set_cursor(decode(7)).unwrap();
        document.set_cursor(decode(-1)).unwrap();
    }

    // [spec:mw:def:ffi.host.jni/test]
    #[test]
    fn parse_to_vdom_crosses_as_a_json_vnode_array() {
        let json = parse_to_vdom_json("# Title\n\nbody\n", "{}").unwrap();
        let blocks: Vec<Value> = serde_json::from_str(&json).expect("a JSON array of VNodes");
        assert_eq!(blocks.len(), 2, "got: {json}");
        // Each element is a vnode tree, not an event envelope.
        for block in &blocks {
            assert!(block.is_object(), "{block}");
            assert!(block.get("type").is_none(), "{block}");
            assert!(block.get("id").is_none(), "{block}");
        }
        assert!(json.contains("Title"), "got: {json}");
    }

    // [spec:mw:def:ffi.host.jni/test]
    #[test]
    fn take_events_crosses_as_a_json_array_of_tagged_upserts_and_deletes() {
        let mut document = VdomDocument::new("{}");
        document.set_source("alpha\n\nbeta\n").unwrap();
        let _ = document.take_events().unwrap();
        document.set_source("alpha\n").unwrap();

        let events: Vec<Value> = serde_json::from_str(&document.take_events().unwrap()).unwrap();
        let mut saw_delete = false;
        for event in &events {
            match event.get("type").and_then(Value::as_str) {
                // `{"type":"upsert",id,vtree}`
                Some("upsert") => {
                    assert!(event.get("id").and_then(Value::as_u64).is_some(), "{event}");
                    assert!(event.get("vtree").is_some(), "{event}");
                }
                // `{"type":"delete",id}` — no vtree.
                Some("delete") => {
                    saw_delete = true;
                    assert!(event.get("id").and_then(Value::as_u64).is_some(), "{event}");
                    assert!(event.get("vtree").is_none(), "{event}");
                }
                other => panic!("unexpected event discriminator {other:?} in {event}"),
            }
        }
        assert!(saw_delete, "events: {events:?}");

        // The streaming parser uses the same envelope.
        let mut parser = VdomParser::new("{}");
        parser.push_str("one\n").unwrap();
        parser.finish().unwrap();
        let events: Vec<Value> = serde_json::from_str(&parser.take_events().unwrap()).unwrap();
        assert!(
            events
                .iter()
                .all(|e| e.get("type").and_then(Value::as_str) == Some("upsert")),
            "events: {events:?}"
        );
    }

    // [spec:mw:def:ffi.host.jni/test]
    #[test]
    fn inbound_options_are_a_json_object_with_a_sanitizing_fallback() {
        // The documented shape opts out of sanitization...
        assert!(filter_from_options(r#"{"rawHtml":"unrestricted"}"#).is_unrestricted());
        // ...and an absent, unknown or malformed value falls back to safe.
        for options in ["{}", "", "{ not json", r#"{"rawHtml":"nonsense"}"#] {
            assert!(
                !filter_from_options(options).is_unrestricted(),
                "{options} must fall back to the sanitizing default"
            );
        }

        // The fallback reaches both the one-shot and the streaming path,
        // because the blob is decoded per entry point.
        let json = parse_to_vdom_json("<script>alert(1)</script>\n", "{ not json").unwrap();
        assert!(!json.contains("<script"), "got: {json}");
        let mut parser = VdomParser::new("nonsense");
        parser.push_str("<script>alert(1)</script>\n").unwrap();
        parser.finish().unwrap();
        assert!(!parser.take_events().unwrap().contains("<script"));
    }
}
