//! Execute the expansion against its widget-definition output contract.
extern crate self as ui_toolkit;

pub mod widget_def {
    pub enum AttrValue {
        Static(String),
        Dynamic(String),
    }
    pub struct Attr {
        pub name: &'static str,
        pub name_owned: Option<String>,
        pub value: AttrValue,
    }
    pub struct WidgetDef {
        pub tag: &'static str,
        pub tag_owned: Option<String>,
        pub name: Option<String>,
        pub attrs: Vec<Attr>,
        pub nine_slice: Option<NineSliceDef>,
        pub children: Vec<WidgetChild>,
    }
    pub struct NineSliceDef {
        pub edge_size: f32,
        pub bg_color: [f32; 4],
        pub border_color: [f32; 4],
        pub textures: Option<[String; 9]>,
    }
    pub enum WidgetChild {
        Widget(WidgetDef),
        Fragment(Vec<WidgetChild>),
    }
}

#[test]
fn fractional_literals_reach_widget_attributes_and_nine_slice_unchanged() {
    let widgets = ui_toolkit_macros::rsx! {
        frame {
            width: 17.5,
            alpha: 0.4,
            height: 24,
            pos_x: -7.25,
            scale: 1.25f32,
            nine_slice { edge_size: 2.75, }
        }
    };
    let widget_def::WidgetChild::Widget(frame) = &widgets[0] else {
        panic!("expected frame");
    };
    let values: Vec<_> = frame
        .attrs
        .iter()
        .map(|attr| {
            let text = match &attr.value {
                widget_def::AttrValue::Static(value) | widget_def::AttrValue::Dynamic(value) => {
                    value
                }
            };
            (attr.name, text.parse::<f32>().expect("numeric attribute"))
        })
        .collect();
    assert_eq!(
        values,
        vec![
            ("width", 17.5),
            ("alpha", 0.4),
            ("height", 24.0),
            ("pos_x", -7.25),
            ("scale", 1.25),
        ]
    );
    assert_eq!(frame.nine_slice.as_ref().unwrap().edge_size, 2.75);
}
