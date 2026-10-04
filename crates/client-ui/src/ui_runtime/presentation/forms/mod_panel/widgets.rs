use serde_json::{Value, json};
use ui::mod_panel::{Control, FONT_NAME};

#[derive(Clone, Copy)]
pub(super) struct Palette {
    pub card: [f64; 4],
    pub border: [f64; 4],
    pub text: [f64; 4],
    pub muted: [f64; 4],
    pub raised: [f64; 4],
    pub accent: [f64; 4],
}

impl Palette {
    pub fn new(dark: bool) -> Self {
        if dark {
            Self {
                card: [0.055, 0.058, 0.065, 0.96],
                border: [0.2, 0.21, 0.23, 0.94],
                text: [0.94, 0.95, 0.97, 1.0],
                muted: [0.61, 0.63, 0.67, 1.0],
                raised: [0.15, 0.16, 0.18, 1.0],
                accent: [0.96, 0.46, 0.40, 1.0],
            }
        } else {
            Self {
                card: [0.96, 0.965, 0.975, 0.97],
                border: [0.75, 0.77, 0.8, 0.95],
                text: [0.11, 0.12, 0.14, 1.0],
                muted: [0.39, 0.41, 0.45, 1.0],
                raised: [0.86, 0.875, 0.9, 1.0],
                accent: [0.78, 0.24, 0.20, 1.0],
            }
        }
    }
}

pub(super) fn named(name: &str, definition: Value) -> Value {
    json!({name:definition})
}

pub(super) fn rounded(size: [f64; 2], offset: [f64; 2], radius: f64, color: [f64; 4]) -> Value {
    json!({"type":"custom","renderer":"cinnabar_rounded_rectangle","size":size,"offset":offset,
        "anchor_from":"top_left","anchor_to":"top_left","radius":radius,"color":color})
}

pub(super) fn panel(size: [f64; 2], offset: [f64; 2], controls: Vec<Value>) -> Value {
    json!({"type":"panel","size":size,"offset":offset,
        "anchor_from":"top_left","anchor_to":"top_left","controls":controls})
}

pub(super) fn button(
    action: &str,
    size: [f64; 2],
    offset: [f64; 2],
    controls: Vec<Value>,
) -> Value {
    let mut node = panel(size, offset, controls);
    node["type"] = json!("button");
    node["button_mappings"] = json!([{"from_button_id":"button.menu_select","to_button_id":action,"mapping_type":"pressed"}]);
    node
}

pub(super) fn label(
    text: &str,
    size: [f64; 2],
    offset: [f64; 2],
    color: [f64; 4],
    binding: bool,
) -> Value {
    let mut label = json!({"type":"label","size":size,"offset":offset,
        "anchor_from":"top_left","anchor_to":"top_left","text":text,"text_alignment":"left",
        "color":color,"shadow":false,"clip_children":true,"hide_hyphen":true,"localize":false,"font_type":FONT_NAME});
    if binding {
        label["bindings"] = json!([{"binding_name":text}]);
    }
    label
}

pub(super) fn chrome(size: [f64; 2], palette: Palette, radius: f64) -> Vec<Value> {
    vec![
        named("border", rounded(size, [0.0; 2], radius, palette.border)),
        named(
            "surface",
            rounded(
                [size[0] - 1.0, size[1] - 1.0],
                [0.5; 2],
                radius - 0.5,
                palette.card,
            ),
        ),
    ]
}

pub(super) fn toggle(index: usize, offset: [f64; 2]) -> Value {
    let mut track = rounded([26.0, 13.0], [0.0; 2], 6.5, [0.0; 4]);
    track["bindings"] = json!([{"binding_name":format!("#row_{index}_toggle_color"),"binding_name_override":"#color"}]);
    let mut knob = rounded([9.0, 9.0], [2.0, 2.0], 4.5, [1.0; 4]);
    knob["bindings"] = json!([{"binding_name":format!("#row_{index}_knob_offset"),"binding_name_override":"#offset"}]);
    button(
        &format!("mod.control:{index}"),
        [26.0, 17.0],
        offset,
        vec![named("track", track), named("knob", knob)],
    )
}

pub(super) fn row(control: &Control, index: usize, width: f64, y: f64, palette: Palette) -> Value {
    let action = format!("mod.control:{index}");
    let label_key = format!("#row_{index}_label");
    let value_key = format!("#row_{index}_value");
    let mut controls = vec![named(
        "label",
        label(
            &label_key,
            [width - 8.0, 14.0],
            [0.0, 2.0],
            palette.muted,
            true,
        ),
    )];
    match control {
        Control::Slider { .. } => {
            controls[0] = named(
                "label",
                label(
                    &label_key,
                    [width * 0.62, 14.0],
                    [0.0, 0.0],
                    palette.muted,
                    true,
                ),
            );
            let mut value = label(
                &value_key,
                [width * 0.36, 14.0],
                [width * 0.64, 0.0],
                palette.text,
                true,
            );
            value["text_alignment"] = json!("right");
            controls.push(named("value", value));
            let mut fill = rounded([width, 3.0], [0.0, 4.5], 1.5, palette.accent);
            fill["bindings"] = json!([{"binding_name":format!("#row_{index}_fill"),"binding_name_override":"#size_binding_x"}]);
            let mut knob = rounded([6.0, 6.0], [0.0, 0.0], 3.0, palette.text);
            knob["anchor_from"] = json!("right_middle");
            knob["anchor_to"] = json!("center");
            fill["controls"] = json!([named("knob", knob)]);
            controls.push(named(
                "slider",
                button(
                    &action,
                    [width, 12.0],
                    [0.0, 15.0],
                    vec![
                        named(
                            "track",
                            rounded([width, 3.0], [0.0, 4.5], 1.5, palette.raised),
                        ),
                        named("fill", fill),
                    ],
                ),
            ));
            panel([width, 32.0], [0.0, y], controls)
        }
        Control::Toggle { .. } => {
            controls[0] = named(
                "label",
                label(
                    &label_key,
                    [width - 34.0, 16.0],
                    [0.0, 2.0],
                    palette.text,
                    true,
                ),
            );
            controls.push(named("toggle", toggle(index, [width - 26.0, 3.0])));
            panel([width, 24.0], [0.0, y], controls)
        }
        Control::Choice { .. } => {
            controls.push(named(
                "value",
                label(
                    &value_key,
                    [width - 10.0, 14.0],
                    [0.0, 15.0],
                    palette.text,
                    true,
                ),
            ));
            controls.push(named(
                "arrow",
                label(">", [8.0, 14.0], [width - 8.0, 15.0], palette.muted, false),
            ));
            button(&action, [width, 34.0], [0.0, y], controls)
        }
        Control::Button { .. } => {
            let mut value = label(
                &label_key,
                [width - 10.0, 18.0],
                [5.0, 6.0],
                palette.text,
                true,
            );
            value["text_alignment"] = json!("center");
            button(
                &action,
                [width, 25.0],
                [0.0, y],
                vec![
                    named(
                        "surface",
                        rounded([width, 21.0], [0.0, 1.0], 4.0, palette.raised),
                    ),
                    named("label", value),
                ],
            )
        }
    }
}

pub(super) fn row_height(control: &Control) -> f64 {
    match control {
        Control::Slider { .. } => 32.0,
        Control::Choice { .. } => 34.0,
        Control::Button { .. } => 25.0,
        _ => 24.0,
    }
}
