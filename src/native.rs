//! Direct Bevy entity construction; the legacy widget-definition backend is untouched.

#[cfg(test)]
#[path = "native_tests.rs"]
mod tests;

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::{
    Block, Expr, Ident, Token, braced, parenthesized,
    parse::{Parse, ParseStream},
};

pub(super) struct NativeInput {
    commands: Expr,
    parent: Expr,
    children: Vec<Child>,
    result: Expr,
}

enum Child {
    Node(Node),
    Statements(Block),
}

struct Node {
    id: Option<Ident>,
    name: Option<Expr>,
    layout: Option<Expr>,
    components: Option<Expr>,
    fields: Vec<(Ident, Expr)>,
    children: Vec<Child>,
}

impl Parse for NativeInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        input.parse::<Token![@]>()?;
        let mode: Ident = input.parse()?;
        if mode != "native" {
            return Err(syn::Error::new(mode.span(), "expected @native"));
        }
        let args;
        parenthesized!(args in input);
        let commands = args.parse()?;
        args.parse::<Token![,]>()?;
        let parent = args.parse()?;
        if !args.is_empty() {
            return Err(args.error("expected commands and optional parent"));
        }
        let body;
        braced!(body in input);
        let children = parse_children(&body)?;
        input.parse::<Token![=>]>()?;
        let result = input.parse()?;
        Ok(Self {
            commands,
            parent,
            children,
            result,
        })
    }
}

fn parse_children(input: ParseStream) -> syn::Result<Vec<Child>> {
    let mut children = Vec::new();
    while !input.is_empty() {
        if input.peek(syn::token::Brace) {
            children.push(Child::Statements(input.parse()?));
        } else {
            children.push(Child::Node(parse_node(input)?));
        }
    }
    Ok(children)
}

fn parse_node(input: ParseStream) -> syn::Result<Node> {
    let tag: Ident = input.parse()?;
    if tag != "node" {
        return Err(syn::Error::new(
            tag.span(),
            "native RSX expects a node or Rust block",
        ));
    }
    let body;
    braced!(body in input);
    let mut node = Node {
        id: None,
        name: None,
        layout: None,
        components: None,
        fields: Vec::new(),
        children: Vec::new(),
    };
    while !body.is_empty() {
        if body.peek(syn::token::Brace) {
            node.children.push(Child::Statements(body.parse()?));
            continue;
        }
        let fork = body.fork();
        let _: Ident = fork.parse()?;
        if fork.peek(syn::token::Brace) {
            node.children.push(Child::Node(parse_node(&body)?));
            continue;
        }
        let key: Ident = body.parse()?;
        body.parse::<Token![:]>()?;
        let value: Expr = body.parse()?;
        store_attribute(&mut node, key, value)?;
        if !body.is_empty() {
            body.parse::<Token![,]>()?;
        }
    }
    Ok(node)
}

fn store_attribute(node: &mut Node, key: Ident, value: Expr) -> syn::Result<()> {
    match key.to_string().as_str() {
        "id" => {
            if node.id.is_some() {
                return Err(syn::Error::new(key.span(), "duplicate id"));
            }
            node.id = Some(binding_identifier(value, key.span())?);
        }
        "name" => set_once(&mut node.name, key, value)?,
        "layout" => set_once(&mut node.layout, key, value)?,
        "components" => set_once(&mut node.components, key, value)?,
        _ => {
            if node.fields.iter().any(|(field, _)| *field == key) {
                return Err(syn::Error::new(key.span(), "duplicate node field"));
            }
            node.fields.push((key, value));
        }
    }
    Ok(())
}

fn binding_identifier(value: Expr, span: Span) -> syn::Result<Ident> {
    let Expr::Path(path) = value else {
        return Err(syn::Error::new(span, "id must be an identifier"));
    };
    path.path
        .get_ident()
        .cloned()
        .ok_or_else(|| syn::Error::new(span, "id must be an identifier"))
}

fn set_once(slot: &mut Option<Expr>, key: Ident, value: Expr) -> syn::Result<()> {
    if slot.is_some() {
        return Err(syn::Error::new(key.span(), "duplicate native attribute"));
    }
    *slot = Some(value);
    Ok(())
}

impl NativeInput {
    pub(super) fn expand(self) -> TokenStream {
        let Self {
            commands,
            parent,
            children,
            result,
        } = self;
        let parent_id = format_ident!("__rsx_native_parent", span = Span::mixed_site());
        let mut counter = 0;
        let body = emit_children(&children, &commands, &quote!(#parent_id), &mut counter);
        quote! {{
            let #parent_id: ::core::option::Option<::bevy::prelude::Entity> = #parent;
            #body
            #result
        }}
    }
}

fn emit_children(
    children: &[Child],
    commands: &Expr,
    parent: &TokenStream,
    counter: &mut usize,
) -> TokenStream {
    let mut output = TokenStream::new();
    for child in children {
        match child {
            Child::Statements(block) => {
                let statements = &block.stmts;
                output.extend(quote!(#(#statements)*));
            }
            Child::Node(node) => output.extend(emit_node(node, commands, parent, counter)),
        }
    }
    output
}

fn emit_node(
    node: &Node,
    commands: &Expr,
    parent: &TokenStream,
    counter: &mut usize,
) -> TokenStream {
    let id = node.id.clone().unwrap_or_else(|| {
        let id = format_ident!("__rsx_native_node_{}", *counter, span = Span::mixed_site());
        *counter += 1;
        id
    });
    let base = node
        .layout
        .as_ref()
        .map(|expr| quote!(#expr))
        .unwrap_or_else(|| quote!(::core::default::Default::default()));
    let fields = node.fields.iter().map(|(key, value)| quote!(#key: #value));
    let components = node
        .components
        .as_ref()
        .map(|expr| quote!(#expr))
        .unwrap_or_else(|| quote!(()));
    let name = node.name.as_ref().map(|expr| {
        quote! {
            (#commands).entity(#id).insert(::bevy::prelude::Name::new(#expr));
        }
    });
    let children = emit_children(&node.children, commands, &quote!(Some(#id)), counter);
    quote! {
        let #id = (#commands).spawn((::bevy::prelude::Node { #(#fields,)* ..#base }, #components)).id();
        #name
        if let Some(parent) = #parent {
            (#commands).entity(#id).insert(::bevy::prelude::ChildOf(parent));
        }
        #children
    }
}
