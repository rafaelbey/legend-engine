use crate::{Parser, PluginRegistry};
use legend_pure_parser_ast::Element;
use std::sync::Arc;

fn parse_doc(code: &str) -> Vec<Element> {
    let registry = Arc::new(PluginRegistry::new());
    let mut parser = Parser::new("test.pure", code, registry);
    parser.parse_document().expect("Failed to parse document")
}

#[test]
fn test_parse_class_with_stereotypes_and_tagged_values() {
    let code = "
    ###Pure
    Class <<temporal.businesstemporal>> {doc.doc = 'something'} A extends B
    {
    }
    ";
    
    let elements = parse_doc(code);
    assert_eq!(elements.len(), 1);
    
    if let Element::Class(c) = &elements[0] {
        assert_eq!(c.name.as_str(), "A");
        assert_eq!(c.super_types.len(), 1);
        if let legend_pure_parser_ast::Type::Packageable(p) = &c.super_types[0] {
            assert_eq!(p.full_path, "B");
        } else {
            panic!("Expected Packageable super type");
        }

        assert_eq!(c.stereotypes.len(), 1);
        assert_eq!(c.stereotypes[0].profile, "temporal");
        assert_eq!(c.stereotypes[0].value.as_str(), "businesstemporal");

        assert_eq!(c.tagged_values.len(), 1);
        assert_eq!(c.tagged_values[0].profile, "doc");
        assert_eq!(c.tagged_values[0].tag.as_str(), "doc");
        assert_eq!(c.tagged_values[0].value, "something");
    } else {
        panic!("Expected Class");
    }
}

#[test]
fn test_parse_profile() {
    let code = "
    ###Pure
    Profile test::A
    {
       tags : [tag1, tag2];
       stereotypes : [stereotype1, stereotype2];
    }
    ";
    
    let elements = parse_doc(code);
    assert_eq!(elements.len(), 1);
    
    if let Element::Profile(p) = &elements[0] {
        assert_eq!(p.name.as_str(), "A");
        assert_eq!(p.package.path.join("::"), "test");
        assert_eq!(p.tags.iter().map(|t| t.value.as_str()).collect::<Vec<_>>(), vec!["tag1", "tag2"]);
        assert_eq!(p.stereotypes.iter().map(|t| t.value.as_str()).collect::<Vec<_>>(), vec!["stereotype1", "stereotype2"]);
    } else {
        panic!("Expected Profile");
    }
}

#[test]
fn test_nested_unknown_island_grammar() {
    let code = "
    ###Pure
    function f(): Any[*]
    {
       let x = #Unknown{My random Parser #Test{ OK OK } Yo}#
    }
    ";
    
    let registry = Arc::new(PluginRegistry::new());
    let mut parser = Parser::new("test.pure", code, registry);
    let result = parser.parse_document();
    
    assert!(result.is_err(), "Expected an error for unknown island grammar");
    if let Err(e) = result {
        if let crate::ParseError::EngineError(msg, source_info) = e {
            assert!(msg.contains("Can't find an embedded Pure parser for the type 'Unknown'"));
            assert!(msg.contains("available ones: []"));
            // Verify source info covers the whole island
            assert_eq!(source_info.start_line, 5); // start of #Unknown{
            assert_eq!(source_info.end_line, 6); // end of }#
        } else {
            panic!("Expected EngineError, got {:?}", e);
        }
    }
}
