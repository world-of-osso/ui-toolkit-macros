use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{
    Expr, Ident, LitBool, LitFloat, LitInt, LitStr, Token, braced,
    ext::IdentExt,
    parse::{Parse, ParseStream},
    token::Brace,
};

// ──────────────────────────────────────────────────────────────────
// Tag name mapping
// ──────────────────────────────────────────────────────────────────

fn tag_to_type(raw: &str) -> &'static str {
    match raw {
        "frame" => "Frame",
        "button" => "Button",
        "texture" => "Texture",
        "fontstring" => "FontString",
        "editbox" => "EditBox",
        "checkbutton" => "CheckButton",
        "scrollframe" => "ScrollFrame",
        "statusbar" => "StatusBar",
        "gametooltip" => "GameTooltip",
        "playermodel" => "PlayerModel",
        "modelscene" => "ModelScene",
        "colorselect" => "ColorSelect",
        "messageframe" => "MessageFrame",
        "simplehtml" => "SimpleHTML",
        _ => "",
    }
}

fn capitalize_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

fn resolve_tag(raw: &str) -> String {
    let known = tag_to_type(raw);
    if known.is_empty() {
        capitalize_first(raw)
    } else {
        known.to_string()
    }
}

// ──────────────────────────────────────────────────────────────────
// AST types
// ──────────────────────────────────────────────────────────────────

enum AttrValueAst {
    StringLit(String),
    NumLit(String),
    BoolLit(bool),
    Expr(Expr),
}

struct AttrAst {
    name: String,
    value: AttrValueAst,
}

struct NineSliceAst {
    attrs: Vec<AttrAst>,
}

enum ChildAst {
    Element(ElementAst),
    Block(Expr),
    If(IfAst),
}

struct IfAst {
    cond: Expr,
    then_children: Vec<ChildAst>,
    else_children: Option<Vec<ChildAst>>,
}

struct ElementAst {
    tag: String,
    name_attr: Option<NameAttr>,
    attrs: Vec<AttrAst>,
    nine_slice: Option<NineSliceAst>,
    children: Vec<ChildAst>,
}

/// The `name` attribute gets special handling.
enum NameAttr {
    Lit(String),
    /// FrameName const: generate `Some(expr.0.to_string())`
    Expr(Expr),
}

// ──────────────────────────────────────────────────────────────────
// Parsing
// ──────────────────────────────────────────────────────────────────

struct RsxInput {
    roots: Vec<ChildAst>,
}

impl Parse for RsxInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut roots = Vec::new();
        while !input.is_empty() {
            roots.push(parse_child(input)?);
        }
        Ok(RsxInput { roots })
    }
}

fn parse_child(input: ParseStream) -> syn::Result<ChildAst> {
    if input.peek(Token![if]) {
        return Ok(ChildAst::If(parse_if(input)?));
    }
    if input.peek(Brace) {
        let content;
        braced!(content in input);
        return Ok(ChildAst::Block(content.parse()?));
    }
    Ok(ChildAst::Element(parse_element(input)?))
}

fn parse_if(input: ParseStream) -> syn::Result<IfAst> {
    input.parse::<Token![if]>()?;
    let negated = input.peek(Token![!]);
    if negated {
        input.parse::<Token![!]>()?;
    }
    let mut cond: Expr = input.call(Expr::parse_without_eager_brace)?;
    if negated {
        cond = Expr::Unary(syn::ExprUnary {
            attrs: vec![],
            op: syn::UnOp::Not(syn::token::Not::default()),
            expr: Box::new(cond),
        });
    }
    let then_content;
    braced!(then_content in input);
    let then_children = parse_children(&then_content)?;
    let else_children = if input.peek(Token![else]) {
        input.parse::<Token![else]>()?;
        let else_content;
        braced!(else_content in input);
        Some(parse_children(&else_content)?)
    } else {
        None
    };
    Ok(IfAst {
        cond,
        then_children,
        else_children,
    })
}

fn parse_children(input: ParseStream) -> syn::Result<Vec<ChildAst>> {
    let mut children = Vec::new();
    while !input.is_empty() {
        children.push(parse_child(input)?);
    }
    Ok(children)
}

/// Returns true if the stream is at `ident :` (but not `ident ::`).
fn peek_is_attr(input: ParseStream) -> bool {
    let fork = input.fork();
    let Ok(_) = fork.call(Ident::parse_any) else {
        return false;
    };
    fork.peek(Token![:]) && !fork.peek(Token![::])
}

/// Returns true if the stream is at `ident ,` or `ident` at end-of-stream
/// (shorthand field syntax: `name,` means `name: name,`).
fn peek_is_shorthand_attr(input: ParseStream) -> bool {
    let fork = input.fork();
    let Ok(_) = fork.call(Ident::parse_any) else {
        return false;
    };
    fork.peek(Token![,]) || fork.is_empty()
}

fn parse_element(input: ParseStream) -> syn::Result<ElementAst> {
    let tag_ident: Ident = input.parse()?;
    let raw = tag_ident.to_string();
    reject_legacy_anchor(&tag_ident)?;
    let tag = resolve_tag(raw.trim_start_matches("r#"));
    let content;
    braced!(content in input);
    parse_element_contents(&content, tag)
}

fn parse_element_contents(content: ParseStream, tag: String) -> syn::Result<ElementAst> {
    let mut parts = ElementParts::default();
    while parse_element_content_item(content, &mut parts)? {}
    Ok(ElementAst {
        tag,
        name_attr: parts.name_attr,
        attrs: parts.attrs,
        nine_slice: parts.nine_slice,
        children: parts.children,
    })
}

#[derive(Default)]
struct ElementParts {
    name_attr: Option<NameAttr>,
    attrs: Vec<AttrAst>,
    nine_slice: Option<NineSliceAst>,
    children: Vec<ChildAst>,
}

fn parse_element_content_item(content: ParseStream, parts: &mut ElementParts) -> syn::Result<bool> {
    if content.is_empty() {
        return Ok(false);
    }
    if content.peek(Token![if]) || content.peek(Brace) {
        parts.children.push(parse_child(content)?);
        return Ok(true);
    }
    if peek_is_attr(content) {
        parse_attr_item(content, &mut parts.name_attr, &mut parts.attrs)?;
        parse_optional_comma(content)?;
        return Ok(true);
    }
    if peek_is_shorthand_attr(content) {
        parse_shorthand_attr_item(content, &mut parts.name_attr, &mut parts.attrs)?;
        return Ok(true);
    }
    if content.peek(Ident::peek_any) {
        parse_child_item(content, &mut parts.nine_slice, &mut parts.children)?;
        return Ok(true);
    }
    Ok(false)
}

fn parse_shorthand_attr_item(
    input: ParseStream,
    name_attr: &mut Option<NameAttr>,
    attrs: &mut Vec<AttrAst>,
) -> syn::Result<()> {
    let ident: Ident = input.call(Ident::parse_any)?;
    parse_optional_comma(input)?;
    let attr_name = ident.to_string();
    let expr: Expr = syn::parse2(quote! { #ident })?;
    if attr_name == "name" {
        *name_attr = Some(NameAttr::Expr(expr));
    } else {
        attrs.push(AttrAst {
            name: attr_name,
            value: AttrValueAst::Expr(expr),
        });
    }
    Ok(())
}

fn parse_optional_comma(input: ParseStream) -> syn::Result<()> {
    if input.peek(Token![,]) {
        input.parse::<Token![,]>()?;
    }
    Ok(())
}

fn parse_attr_item(
    input: ParseStream,
    name_attr: &mut Option<NameAttr>,
    attrs: &mut Vec<AttrAst>,
) -> syn::Result<()> {
    let ident: Ident = input.call(Ident::parse_any)?;
    input.parse::<Token![:]>()?;
    let attr_name = ident.to_string();
    if attr_name == "name" {
        *name_attr = Some(parse_name_attr(input)?);
    } else {
        attrs.push(AttrAst {
            name: attr_name,
            value: parse_attr_value(input)?,
        });
    }
    Ok(())
}

fn parse_child_item(
    input: ParseStream,
    nine_slice: &mut Option<NineSliceAst>,
    children: &mut Vec<ChildAst>,
) -> syn::Result<()> {
    let child_ident: Ident = input.call(Ident::parse_any)?;
    let child_raw = child_ident.to_string();
    let child_tag = child_raw.trim_start_matches("r#");
    reject_legacy_anchor(&child_ident)?;
    if child_tag == "nine_slice" {
        let ns_content;
        braced!(ns_content in input);
        *nine_slice = Some(NineSliceAst {
            attrs: parse_pseudo_element_attrs(&ns_content)?,
        });
    } else {
        let resolved = resolve_tag(child_tag);
        let content;
        braced!(content in input);
        children.push(ChildAst::Element(parse_element_contents(
            &content, resolved,
        )?));
    }
    Ok(())
}

fn reject_legacy_anchor(ident: &Ident) -> syn::Result<()> {
    if ident
        .to_string()
        .trim_start_matches("r#")
        .eq_ignore_ascii_case("anchor")
    {
        return Err(syn::Error::new(
            ident.span(),
            "legacy anchor { ... } is unsupported; use pos_x, pos_y, pos_type, anchor, and native edge attributes",
        ));
    }
    Ok(())
}

fn parse_pseudo_element_attrs(input: ParseStream) -> syn::Result<Vec<AttrAst>> {
    let mut attrs = Vec::new();
    while !input.is_empty() {
        if peek_is_attr(input) {
            let name_ident: Ident = input.call(Ident::parse_any)?;
            input.parse::<Token![:]>()?;
            let val = parse_attr_value(input)?;
            attrs.push(AttrAst {
                name: name_ident.to_string(),
                value: val,
            });
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        } else if peek_is_shorthand_attr(input) {
            let ident: Ident = input.call(Ident::parse_any)?;
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
            let expr: Expr = syn::parse2(quote! { #ident })?;
            attrs.push(AttrAst {
                name: ident.to_string(),
                value: AttrValueAst::Expr(expr),
            });
        } else {
            break;
        }
    }
    Ok(attrs)
}

fn parse_attr_value(input: ParseStream) -> syn::Result<AttrValueAst> {
    if input.peek(LitStr) {
        let s: LitStr = input.parse()?;
        return Ok(AttrValueAst::StringLit(s.value()));
    }
    if input.peek(LitFloat) {
        let f: LitFloat = input.parse()?;
        return Ok(AttrValueAst::NumLit(f.base10_digits().to_string()));
    }
    if input.peek(LitInt) {
        let i: LitInt = input.parse()?;
        return Ok(AttrValueAst::NumLit(i.base10_digits().to_string()));
    }
    if input.peek(LitBool) {
        let b: LitBool = input.parse()?;
        return Ok(AttrValueAst::BoolLit(b.value));
    }
    Ok(AttrValueAst::Expr(input.parse()?))
}

fn parse_name_attr(input: ParseStream) -> syn::Result<NameAttr> {
    if input.peek(LitStr) {
        let s: LitStr = input.parse()?;
        return Ok(NameAttr::Lit(s.value()));
    }
    Ok(NameAttr::Expr(input.parse()?))
}

// ──────────────────────────────────────────────────────────────────
// Code generation
// ──────────────────────────────────────────────────────────────────

fn gen_widget_child(child: &ChildAst) -> TokenStream2 {
    match child {
        ChildAst::Element(el) => {
            let def = gen_widget_def(el);
            quote! { ui_toolkit::widget_def::WidgetChild::Widget(#def) }
        }
        ChildAst::Block(expr) => quote! {
            ui_toolkit::widget_def::WidgetChild::Fragment({
                let __fragment: Vec<ui_toolkit::widget_def::WidgetChild> = #expr;
                __fragment
            })
        },
        ChildAst::If(if_ast) => gen_if_child(if_ast),
    }
}

fn gen_if_child(if_ast: &IfAst) -> TokenStream2 {
    let cond = &if_ast.cond;
    let then_items: Vec<_> = if_ast.then_children.iter().map(gen_widget_child).collect();
    let else_branch = match &if_ast.else_children {
        Some(else_children) => {
            let items: Vec<_> = else_children.iter().map(gen_widget_child).collect();
            quote! { vec![#(#items),*] }
        }
        None => quote! { vec![] },
    };
    quote! {
        ui_toolkit::widget_def::WidgetChild::Fragment(
            if #cond { vec![#(#then_items),*] } else { #else_branch }
        )
    }
}

fn gen_name_expr(name_attr: &Option<NameAttr>) -> TokenStream2 {
    match name_attr {
        None => quote! { None },
        Some(NameAttr::Lit(s)) => {
            let lit = syn::LitStr::new(s, Span::call_site());
            quote! { Some(#lit.to_string()) }
        }
        Some(NameAttr::Expr(expr)) => quote! { Some(#expr.0.to_string()) },
    }
}

fn gen_widget_def(el: &ElementAst) -> TokenStream2 {
    let tag_lit = syn::LitStr::new(&el.tag, Span::call_site());
    let name_expr = gen_name_expr(&el.name_attr);
    let attrs_tokens: Vec<_> = el.attrs.iter().map(gen_attr).collect();
    let nine_slice_expr = gen_nine_slice(&el.nine_slice);
    let children_tokens: Vec<_> = el.children.iter().map(gen_widget_child).collect();
    quote! {
        ui_toolkit::widget_def::WidgetDef {
            tag: #tag_lit,
            tag_owned: None,
            name: #name_expr,
            attrs: vec![#(#attrs_tokens),*],
            nine_slice: #nine_slice_expr,
            children: vec![#(#children_tokens),*],
        }
    }
}

fn gen_attr(attr: &AttrAst) -> TokenStream2 {
    let name_lit = syn::LitStr::new(&attr.name, Span::call_site());
    let value_expr = gen_attr_value(&attr.value);
    quote! {
        ui_toolkit::widget_def::Attr {
            name: #name_lit,
            name_owned: None,
            value: #value_expr,
        }
    }
}

fn gen_attr_value(val: &AttrValueAst) -> TokenStream2 {
    match val {
        AttrValueAst::StringLit(s) => {
            let lit = syn::LitStr::new(s, Span::call_site());
            quote! { ui_toolkit::widget_def::AttrValue::Static(#lit.to_string()) }
        }
        AttrValueAst::NumLit(s) => {
            let lit = syn::LitStr::new(s, Span::call_site());
            quote! { ui_toolkit::widget_def::AttrValue::Static(#lit.to_string()) }
        }
        AttrValueAst::BoolLit(b) => {
            let s = if *b { "true" } else { "false" };
            let lit = syn::LitStr::new(s, Span::call_site());
            quote! { ui_toolkit::widget_def::AttrValue::Static(#lit.to_string()) }
        }
        AttrValueAst::Expr(expr) => {
            quote! { ui_toolkit::widget_def::AttrValue::Dynamic(#expr.to_string()) }
        }
    }
}

fn gen_nine_slice(ns: &Option<NineSliceAst>) -> TokenStream2 {
    let Some(ns) = ns else {
        return quote! { None };
    };
    let edge_size = gen_nine_slice_f32("edge_size", &ns.attrs, 8.0);
    let bg_color = gen_nine_slice_color("bg_color", &ns.attrs, [0.0, 0.0, 0.0, 0.5]);
    let border_color = gen_nine_slice_color("border_color", &ns.attrs, [1.0, 1.0, 1.0, 1.0]);
    let textures = gen_nine_slice_textures(&ns.attrs);
    quote! {
        Some(ui_toolkit::widget_def::NineSliceDef {
            edge_size: #edge_size,
            bg_color: #bg_color,
            border_color: #border_color,
            textures: #textures,
        })
    }
}

fn gen_nine_slice_f32(name: &str, attrs: &[AttrAst], default: f32) -> TokenStream2 {
    if let Some(attr) = attrs.iter().find(|a| a.name == name) {
        match &attr.value {
            AttrValueAst::NumLit(s) => {
                let val: f32 = s.parse().unwrap_or(default);
                quote! { #val }
            }
            AttrValueAst::Expr(expr) => quote! { #expr as f32 },
            _ => quote! { #default },
        }
    } else {
        quote! { #default }
    }
}

fn gen_nine_slice_color(name: &str, attrs: &[AttrAst], default: [f32; 4]) -> TokenStream2 {
    let from_attr = attrs
        .iter()
        .find(|a| a.name == name)
        .and_then(|attr| gen_nine_slice_color_from_attr(&attr.value));
    from_attr.unwrap_or_else(|| quote_nine_slice_color(default))
}

fn gen_nine_slice_color_from_attr(value: &AttrValueAst) -> Option<TokenStream2> {
    match value {
        AttrValueAst::StringLit(s) => parse_color_components(s).map(quote_nine_slice_color),
        AttrValueAst::Expr(expr) => Some(quote! { {
            let s: &str = #expr;
            let p: Vec<f32> = s.split(',').filter_map(|p| p.trim().parse().ok()).collect();
            if p.len() == 4 { [p[0], p[1], p[2], p[3]] } else { [0.0, 0.0, 0.0, 1.0] }
        } }),
        _ => None,
    }
}

fn parse_color_components(raw: &str) -> Option<[f32; 4]> {
    let parts: Vec<f32> = raw
        .split(',')
        .filter_map(|piece| piece.trim().parse().ok())
        .collect();
    if parts.len() != 4 {
        return None;
    }
    Some([parts[0], parts[1], parts[2], parts[3]])
}

fn quote_nine_slice_color(color: [f32; 4]) -> TokenStream2 {
    let [r, g, b, a] = color;
    quote! { [#r, #g, #b, #a] }
}

fn gen_nine_slice_textures(attrs: &[AttrAst]) -> TokenStream2 {
    let Some(attr) = attrs.iter().find(|a| a.name == "textures") else {
        return quote! { None };
    };
    if let AttrValueAst::Expr(expr) = &attr.value {
        return quote! { Some(#expr.map(|s| String::from(s))) };
    }
    quote! { None }
}

// ──────────────────────────────────────────────────────────────────
// Entry point
// ──────────────────────────────────────────────────────────────────

/// RSX macro: parses WoW-UI RSX syntax and expands to `Vec<WidgetChild>`.
#[proc_macro]
pub fn rsx(input: TokenStream) -> TokenStream {
    let rsx_input = syn::parse_macro_input!(input as RsxInput);
    let children: Vec<TokenStream2> = rsx_input.roots.iter().map(gen_widget_child).collect();
    quote! { { vec![#(#children),*] } }.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_anchor_blocks_have_explicit_diagnostics() {
        for source in [
            "anchor {}",
            "frame { anchor { x: 4, } }",
            "frame { Anchor {} }",
            "frame { if visible { anchor {} } }",
        ] {
            let error = match syn::parse_str::<RsxInput>(source) {
                Ok(_) => panic!("legacy anchor accepted: {source}"),
                Err(error) => error,
            };
            assert!(
                error
                    .to_string()
                    .contains("legacy anchor { ... } is unsupported")
            );
        }
    }

    #[test]
    fn native_layout_attributes_parse_as_regular_values() {
        let input = syn::parse_str::<RsxInput>(
            r#"frame {
            pos_x: 12,
            pos_y: -8,
            pos_type: "absolute",
            anchor: "screen",
            left: "50%",
            right: "auto",
            top: 1,
            bottom: 2,
            translate_x: "-50%",
            translate_y: {offset},
            margin_left: "auto",
            margin_right: 4,
            margin_top: 5,
            margin_bottom: 6,
            width: "auto",
            height: "auto",
            button { text: "Child", }
        }"#,
        )
        .unwrap();
        let ChildAst::Element(frame) = &input.roots[0] else {
            panic!("expected frame")
        };
        assert_eq!(frame.attrs.len(), 16);
        assert!(matches!(&frame.attrs[0].value, AttrValueAst::NumLit(v) if v == "12"));
        assert!(matches!(&frame.attrs[3].value, AttrValueAst::StringLit(v) if v == "screen"));
        assert!(matches!(&frame.attrs[9].value, AttrValueAst::Expr(_)));
        assert_eq!(frame.children.len(), 1);
    }
}
