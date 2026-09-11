use super::NativeInput;

#[test]
fn native_rejects_duplicate_named_attributes_and_node_fields() {
    for attributes in [
        "id: first, id: second,",
        "name: \"a\", name: \"b\",",
        "layout: base, layout: other,",
        "components: (), components: (),",
        "width: px(10), width: px(20),",
    ] {
        let input = format!("@native(commands, None) {{ node {{ {attributes} }} }} => ()");
        assert!(
            syn::parse_str::<NativeInput>(&input).is_err(),
            "accepted {input}"
        );
    }
}

#[test]
fn native_requires_a_simple_binding_and_known_node_tag() {
    for input in [
        "@native(commands, None) { node { id: 7, } } => ()",
        "@native(commands, None) { node { id: other::name, } } => ()",
        "@native(commands, None) { image {} } => ()",
        "@unknown(commands, None) { node {} } => ()",
        "@native(commands, None) { node {} }",
    ] {
        assert!(
            syn::parse_str::<NativeInput>(input).is_err(),
            "accepted {input}"
        );
    }
}

#[test]
fn native_accepts_nested_nodes_typed_expressions_and_inline_statements() {
    let input = r#"
        @native(commands, Some(parent)) {
            node {
                id: root,
                name: format!("{}Root", prefix),
                layout: base,
                width: px(320),
                components: (Button, marker),
                node { id: label, components: Text::new("Login"), }
                { let extra = create_child(commands, root); }
            }
        } => (root, label, extra)
    "#;
    assert!(syn::parse_str::<NativeInput>(input).is_ok());
}
