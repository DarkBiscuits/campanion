use super::*;
use serde_json::{Value, json};

fn job() -> Value {
    json!({"format":"campanion-job/1", "source":{"path":"part.svg", "hash":"cbf29ce484222325"}, "units":"mm",
    "tools":[{"id":1,"diameter":6}],
    "operations":[
        {"name":"edge-a","type":"chamfer","layers":["L"],"side":"outside","tool_id":1,"width":0.5,"v_angle_deg":90,"tip_diameter":0.2,"feeds":{"cut":100}},
        {"name":"edge-b","type":"chamfer","layers":["L"],"side":"outside","tool_id":1,"width":1.0,"v_angle_deg":90,"tip_diameter":0.2,"feeds":{"cut":100}}
    ]})
}

fn parse(value: &Value) -> Result<Body, JobFileError> {
    parse_text(&value.to_string(), Path::new("tool-shape-test.json"))
}

#[test]
fn matching_vbit_geometry_is_resolved_and_scaled() {
    for (units, scale) in [("mm", 1.0), ("inch", 25.4)] {
        let mut value = job();
        value["units"] = json!(units);
        value["operations"][1]["tip_clearance"] = json!(0.1);
        let body = parse(&value).expect("same physical cutter");
        let shape = body.tools.first().expect("tool").ir().shape();
        assert_eq!(shape, Some(campanion_ir::ToolShape::VBit { angle: std::f64::consts::FRAC_PI_2, tip_diameter: 0.2 * scale }));
    }
}

#[test]
fn conflicting_vbit_parameters_name_tool_and_operations() {
    for (key, value) in [("v_angle_deg", 60.0), ("tip_diameter", 0.3)] {
        let mut input = job();
        input["operations"][1][key] = json!(value);
        let error = parse(&input).err().expect("conflict").to_string();
        for text in ["tool 1", "conflicting V-bit geometry", "edge-a", "edge-b"] {
            assert!(error.contains(text), "{error}");
        }
    }
}

#[test]
fn explicit_shapes_cannot_be_overwritten_by_chamfers() {
    for shape in [json!("flat"), json!("ball"), json!({"bull": 1})] {
        let mut input = job();
        input["tools"][0]["shape"] = shape;
        let error = parse(&input).err().expect("conflict").to_string();
        assert!(error.contains("tool 1 has an explicit shape conflicting with V-bit geometry"), "{error}");
    }
}

#[test]
fn ordinary_shapes_and_kerf_are_resolved_without_inventing_a_cutter() {
    for (input, expected) in [
        (json!({"id":1,"diameter":6}), Some(campanion_ir::ToolShape::Flat {})),
        (json!({"id":1,"diameter":6,"shape":"ball"}), Some(campanion_ir::ToolShape::Ball {})),
        (json!({"id":1,"diameter":6,"shape":{"bull":1}}), Some(campanion_ir::ToolShape::Bull { corner_radius: 1.0 })),
        (json!({"id":1,"kerf":0.2}), None),
    ] {
        let mut tools = vec![parse_tool_value(&input, 1.0).expect("tool")];
        resolve_ir_shapes(&mut tools, &[]).expect("resolve");
        assert_eq!(tools.first().expect("tool").ir().shape(), expected);
    }
}
