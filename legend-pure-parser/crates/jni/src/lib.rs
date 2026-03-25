use jni::objects::{JClass, JString};
use jni::sys::jstring;
use jni::JNIEnv;
use legend_pure_parser_emitter::Emitter;
use legend_pure_parser_parser::{Parser, PluginRegistry};
use serde_json::json;
use std::sync::Arc;

#[no_mangle]
pub extern "system" fn Java_org_finos_legend_engine_language_pure_grammar_from_RustPureParser_parse(
    mut env: JNIEnv,
    _class: JClass,
    source: JString,
) -> jstring {
    let source_str: String = env
        .get_string(&source)
        .expect("Couldn't get java string")
        .into();

    let registry = Arc::new(PluginRegistry::new());
    let mut parser = Parser::new("jni_input.pure", &source_str, registry);

    let result = match parser.parse_document() {
        Ok(elements) => {
            let emitter = Emitter::new();
            match emitter.emit_elements(&elements) {
                Ok(json_elements) => json!({
                    "_type": "data",
                    "elements": json_elements
                }),
                Err(e) => json!({
                    "error": format!("Emission error: {:?}", e)
                }),
            }
        }
        Err(legend_pure_parser_parser::ParseError::EngineError(msg, info)) => {
            let end_col = if info.end_column > 0 {
                info.end_column - 1
            } else {
                info.end_column
            };
            json!({
                "engineError": true,
                "message": msg,
                "startLine": info.start_line,
                "startColumn": info.start_column,
                "endLine": info.end_line,
                "endColumn": end_col.max(info.start_column)
            })
        }
        Err(e) => json!({
            "error": format!("Parse error: {:?}", e)
        }),
    };

    let result_json = serde_json::to_string(&result).unwrap();
    let output = env
        .new_string(result_json)
        .expect("Couldn't create java string");

    output.into_raw()
}
