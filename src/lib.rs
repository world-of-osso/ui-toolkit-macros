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

struct AnchorAst {
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
    anchors: Vec<AnchorAst>,
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
    let tag = resolve_tag(raw.trim_start_matches("r#"));
    let content;
    braced!(content in input);
    parse_element_contents(&content, tag)
}

fn parse_element_contents(content: ParseStream, tag: String) -> syn::Result<ElementAst> {
    let mut name_attr: Option<NameAttr> = None;
    let mut attrs: Vec<AttrAst> = Vec::new();
    let mut anchors: Vec<AnchorAst> = Vec::new();
    let mut children: Vec<ChildAst> = Vec::new();

    while !content.is_empty() {
        if content.peek(Token![if]) || content.peek(Brace) {
            children.push(parse_child(content)?);
            continue;
        }
        if peek_is_attr(content) {
            parse_attr_item(content, &mut name_attr, &mut attrs)?;
            if content.peek(Token![,]) {
                content.parse::<Token![,]>()?;
            }
            continue;
        }
        if peek_is_shorthand_attr(content) {
            let ident: Ident = content.call(Ident::parse_any)?;
            if content.peek(Token![,]) {
                content.parse::<Token![,]>()?;
            }
            let attr_name = ident.to_string();
            let expr: Expr = syn::parse2(quote! { #ident })?;
            if attr_name == "name" {
                name_attr = Some(NameAttr::Expr(expr));
            } else {
                attrs.push(AttrAst {
                    name: attr_name,
                    value: AttrValueAst::Expr(expr),
                });
            }
            continue;
        }
        if content.peek(Ident::peek_any) {
            parse_child_item(content, &mut anchors, &mut children)?;
            continue;
        }
        break;
    }
    Ok(ElementAst {
        tag,
        name_attr,
        attrs,
        anchors,
        children,
    })
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
    anchors: &mut Vec<AnchorAst>,
    children: &mut Vec<ChildAst>,
) -> syn::Result<()> {
    let child_ident: Ident = input.call(Ident::parse_any)?;
    let child_raw = child_ident.to_string();
    let child_tag = child_raw.trim_start_matches("r#");
    if child_tag == "anchor" {
        let anchor_content;
        braced!(anchor_content in input);
        anchors.push(AnchorAst {
            attrs: parse_anchor_attrs(&anchor_content)?,
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

fn parse_anchor_attrs(input: ParseStream) -> syn::Result<Vec<AttrAst>> {
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
        let s = f
            .base10_digits()
            .split('.')
            .next()
            .unwrap_or("0")
            .to_string();
        return Ok(AttrValueAst::NumLit(s));
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
    let anchors_tokens: Vec<_> = el.anchors.iter().map(gen_anchor).collect();
    let children_tokens: Vec<_> = el.children.iter().map(gen_widget_child).collect();
    quote! {
        ui_toolkit::widget_def::WidgetDef {
            tag: #tag_lit,
            tag_owned: None,
            name: #name_expr,
            attrs: vec![#(#attrs_tokens),*],
            anchors: vec![#(#anchors_tokens),*],
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

fn gen_anchor_field(name: &str, attrs: &[AttrAst]) -> TokenStream2 {
    let default = match name {
        "point" | "relative_point" => quote! { "CENTER".to_string() },
        "relative_to" => quote! { "$parent".to_string() },
        _ => quote! { "0".to_string() },
    };
    attrs
        .iter()
        .find(|a| a.name == name)
        .map(|a| gen_anchor_field_value(&a.value))
        .unwrap_or(default)
}

fn gen_anchor(anchor: &AnchorAst) -> TokenStream2 {
    let point = gen_anchor_field("point", &anchor.attrs);
    let relative_to = gen_anchor_field("relative_to", &anchor.attrs);
    let relative_point = gen_anchor_field("relative_point", &anchor.attrs);
    let x = gen_anchor_field("x", &anchor.attrs);
    let y = gen_anchor_field("y", &anchor.attrs);
    quote! {
        ui_toolkit::widget_def::AnchorDef {
            point: #point,
            relative_to: #relative_to,
            relative_point: #relative_point,
            x: #x,
            y: #y,
        }
    }
}

fn gen_anchor_field_value(val: &AttrValueAst) -> TokenStream2 {
    match val {
        AttrValueAst::StringLit(s) => {
            let lit = syn::LitStr::new(s, Span::call_site());
            quote! { #lit.to_string() }
        }
        AttrValueAst::NumLit(s) => {
            let lit = syn::LitStr::new(s, Span::call_site());
            quote! { #lit.to_string() }
        }
        AttrValueAst::BoolLit(b) => {
            let s = if *b { "true" } else { "false" };
            let lit = syn::LitStr::new(s, Span::call_site());
            quote! { #lit.to_string() }
        }
        AttrValueAst::Expr(expr) => quote! { #expr.to_string() },
    }
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
