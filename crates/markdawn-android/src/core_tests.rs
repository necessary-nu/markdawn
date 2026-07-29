//! Unit tests for the JNI-free core: option decoding, the plugin loader's
//! failure path, and the JSON wire format Kotlin deserializes. None of
//! these need a `JNIEnv`, so they run on the host.

#[cfg(test)]
mod tests {
    use crate::{
        VdomDocument, VdomParser, filter_from_options, parse_to_vdom_json, register_language,
    };
    use serde_json::Value;

    /// Only the exact string `"unrestricted"` opts out of sanitization;
    /// everything else — including malformed JSON, a wrong-typed value and
    /// a near-miss spelling — has to land on the safe filter.
    // [spec:mw:req:core.html-filter.default-safe/test]
    #[test]
    fn only_the_exact_unrestricted_mode_opts_out() {
        assert!(filter_from_options(r#"{"rawHtml":"unrestricted"}"#).is_unrestricted());
        for options in [
            "{}",
            "",
            "not json at all",
            "[]",
            r#"{"rawHtml":null}"#,
            r#"{"rawHtml":""}"#,
            r#"{"rawHtml":"Unrestricted"}"#,
            r#"{"rawHtml":"safe"}"#,
            r#"{"rawHtml":42}"#,
            r#"{"rawhtml":"unrestricted"}"#,
        ] {
            assert!(
                !filter_from_options(options).is_unrestricted(),
                "{options} must decode to the safe filter"
            );
        }
    }

    /// The options blob is decoded per entry point, so a malformed one has
    /// to sanitize rather than propagate an error to the host.
    // [spec:mw:def:core.html-filter.modes/test]
    #[test]
    fn malformed_options_still_sanitize() {
        let json = parse_to_vdom_json("<script>alert(1)</script>\n", "{ not json").unwrap();
        assert!(!json.contains("<script"), "got: {json}");
    }

    /// `loadLanguage` for a grammar whose plugin `.so` was never loaded
    /// resolves no symbol and reports failure instead of aborting.
    // [spec:mw:def:ffi.plugin.grammar-registration/test]
    #[test]
    fn unknown_grammar_plugin_reports_failure() {
        assert!(!register_language("no-such-language-plugin"));
        assert!(!register_language(""));
        // Hyphens are sanitized into the symbol name, so this must miss the
        // same way rather than build a malformed symbol.
        assert!(!register_language("c-sharp"));
    }

    /// Nothing parsed means an empty block list, not `null` or an error.
    // [spec:mw:def:html.vdom.model/test]
    #[test]
    fn empty_source_yields_an_empty_block_array() {
        let json = parse_to_vdom_json("", "{}").unwrap();
        let blocks: Vec<Value> = serde_json::from_str(&json).unwrap();
        assert!(blocks.is_empty(), "got: {json}");
    }

    /// Every streamed event is tagged and keyed the way Kotlin's
    /// `kotlinx.serialization` discriminator expects.
    // [spec:mw:req:core.stream.upsert-replaces/test]
    #[test]
    fn upsert_events_carry_a_type_tag_id_and_vtree() {
        let mut parser = VdomParser::new("{}");
        parser.push_str("# Title\n\nbody\n").unwrap();
        parser.finish().unwrap();
        let events: Vec<Value> = serde_json::from_str(&parser.take_events().unwrap()).unwrap();
        assert!(!events.is_empty(), "expected at least one event");
        for event in &events {
            assert_eq!(event.get("type").and_then(Value::as_str), Some("upsert"));
            assert!(event.get("id").and_then(Value::as_u64).is_some(), "{event}");
            assert!(event.get("vtree").is_some(), "{event}");
        }
    }

    /// `take_events` hands ownership of the buffer to the host; a second
    /// drain must not replay what Kotlin already applied.
    // [spec:mw:req:core.stream.upsert-replaces/test]
    #[test]
    fn taking_events_drains_the_buffer() {
        let mut parser = VdomParser::new("{}");
        parser.push_str("one\n\ntwo\n").unwrap();
        parser.finish().unwrap();
        assert_ne!(parser.take_events().unwrap(), "[]");
        assert_eq!(parser.take_events().unwrap(), "[]");
    }

    /// Shortening a document has to emit a `delete` for the block that went
    /// away, otherwise Kotlin keeps rendering a stale slot.
    // [spec:mw:req:core.stream.slot-identity/test]
    #[test]
    fn removing_a_block_emits_a_delete_event() {
        let mut doc = VdomDocument::new("{}");
        doc.set_source("alpha\n\nbeta\n").unwrap();
        let _ = doc.take_events().unwrap();
        doc.set_source("alpha\n").unwrap();
        let events: Vec<Value> = serde_json::from_str(&doc.take_events().unwrap()).unwrap();
        let deletes: Vec<&Value> = events
            .iter()
            .filter(|e| e.get("type").and_then(Value::as_str) == Some("delete"))
            .collect();
        assert_eq!(deletes.len(), 1, "events: {events:?}");
        assert!(deletes[0].get("vtree").is_none(), "{:?}", deletes[0]);
    }

    /// An edit re-renders the touched block: its upsert must carry the new
    /// text and reuse the slot id the block already had.
    // [spec:mw:req:core.stream.upsert-replaces/test]
    #[test]
    fn editing_a_block_upserts_the_same_slot_with_new_content() {
        let mut doc = VdomDocument::new("{}");
        doc.set_source("hello\n\nworld\n").unwrap();
        let before: Vec<Value> = serde_json::from_str(&doc.take_events().unwrap()).unwrap();
        let original_ids: Vec<u64> = before
            .iter()
            .filter_map(|e| e.get("id").and_then(Value::as_u64))
            .collect();

        doc.edit(7, 12, "there").unwrap();
        let after: Vec<Value> = serde_json::from_str(&doc.take_events().unwrap()).unwrap();
        let touched: Vec<&Value> = after
            .iter()
            .filter(|e| e.get("type").and_then(Value::as_str) == Some("upsert"))
            .collect();
        assert!(!touched.is_empty(), "an edit must upsert something");
        for event in &touched {
            let id = event.get("id").and_then(Value::as_u64).unwrap();
            assert!(original_ids.contains(&id), "new slot id {id} for an edit");
        }
        let json = serde_json::to_string(&after).unwrap();
        assert!(json.contains("there"), "edited text missing: {json}");
    }

    /// Clearing the cursor is a legal no-content transition — it must not
    /// error and must leave the block list intact.
    // [spec:mw:def:ffi.ownership.handle/test]
    #[test]
    fn cursor_updates_do_not_disturb_the_block_list() {
        let mut doc = VdomDocument::new("{}");
        doc.set_source("para\n").unwrap();
        let baseline = doc.take_events().unwrap();
        assert!(baseline.contains("upsert"), "{baseline}");

        doc.set_cursor(Some(0)).unwrap();
        doc.set_cursor(None).unwrap();
        let events: Vec<Value> = serde_json::from_str(&doc.take_events().unwrap()).unwrap();
        for event in &events {
            assert_ne!(
                event.get("type").and_then(Value::as_str),
                Some("delete"),
                "a cursor move must not retire a block: {event}"
            );
        }
    }

    /// The unrestricted filter reaches the streaming path too, not just the
    /// one-shot entry point.
    // [spec:mw:def:core.html-filter.modes/test]
    #[test]
    fn streaming_honours_the_raw_html_option() {
        let mut parser = VdomParser::new(r#"{"rawHtml":"unrestricted"}"#);
        parser.push_str("<script>alert(1)</script>\n").unwrap();
        parser.finish().unwrap();
        let json = parser.take_events().unwrap();
        assert!(json.contains("<script"), "got: {json}");
    }
}
