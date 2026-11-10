//! Optional cutter geometry carried by the IR, in millimetres and radians.

use serde::{Deserialize, Serialize};

use crate::{Result, check};

/// Rotating cutter profile. Absence on a tool means unknown, not flat.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolShape {
    /// Flat end mill.
    Flat {},
    /// Ball nose with radius equal to half the tool diameter.
    Ball {},
    /// Bull nose with a positive corner radius smaller than the tool radius.
    Bull {
        /// Millimetres.
        corner_radius: f64,
    },
    /// V-bit geometry; carrying this metadata does not imply verifier support.
    VBit {
        /// Included angle in radians, strictly between zero and pi.
        angle: f64,
        /// Flat tip diameter in millimetres, from zero up to (but excluding) tool diameter.
        tip_diameter: f64,
    },
}

impl ToolShape {
    pub(crate) fn check(self, diameter: Option<f64>, kerf: Option<f64>) -> Result<()> {
        let Some(diameter) = diameter.filter(|_| kerf.is_none()) else {
            return Err(check::invalid("tool shape requires a diameter tool without kerf"));
        };
        match self {
            Self::Flat {} | Self::Ball {} => {}
            Self::Bull { corner_radius } => {
                check::positive(corner_radius, "bull corner radius")?;
                if corner_radius >= diameter * 0.5 {
                    return Err(check::invalid("bull corner radius must be less than the tool radius"));
                }
            }
            Self::VBit { angle, tip_diameter } => {
                check::positive(angle, "V-bit angle")?;
                if angle >= std::f64::consts::PI {
                    return Err(check::invalid("V-bit angle must be less than pi radians"));
                }
                check::non_negative(tip_diameter, "V-bit tip diameter")?;
                if tip_diameter >= diameter {
                    return Err(check::invalid("V-bit tip diameter must be less than the tool diameter"));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::{Document, Program, Tool};
    use proptest::prelude::*;

    fn document(tool: Tool) -> Document {
        Document::new(Program::try_new(None, vec![tool], None, Vec::new(), Vec::new(), Vec::new(), None).expect("program")).expect("document")
    }

    #[test]
    fn optional_shapes_round_trip_and_old_ir_stays_unchanged() {
        let tool = Tool::try_new(1, None, Some(6.0), None).expect("tool");
        let old = document(tool.clone()).to_json().expect("json");
        assert!(!old.contains("shape"));
        assert_eq!(Document::from_json(&old).expect("old IR").to_json().expect("json"), old);
        for shape in
            [ToolShape::Flat {}, ToolShape::Ball {}, ToolShape::Bull { corner_radius: 1.0 }, ToolShape::VBit { angle: 1.2, tip_diameter: 0.2 }]
        {
            let doc = document(tool.clone().with_shape(shape).expect("shape"));
            assert_eq!(Document::from_json(&doc.to_json().expect("json")).expect("load"), doc);
        }
    }

    #[test]
    fn shape_dimensions_are_checked_by_builders_and_document_loading() {
        let tool = Tool::try_new(1, None, Some(6.0), None).expect("tool");
        for value in [f64::NAN, f64::INFINITY, -1.0, 0.0, 3.0, 4.0] {
            assert!(tool.clone().with_shape(ToolShape::Bull { corner_radius: value }).is_err());
        }
        for angle in [f64::NAN, f64::INFINITY, -1.0, 0.0, std::f64::consts::PI] {
            assert!(tool.clone().with_shape(ToolShape::VBit { angle, tip_diameter: 0.0 }).is_err());
        }
        for tip_diameter in [f64::NAN, f64::INFINITY, -1.0, 6.0] {
            assert!(tool.clone().with_shape(ToolShape::VBit { angle: 1.0, tip_diameter }).is_err());
        }
        assert!(Tool::try_new(2, None, None, Some(0.2)).expect("kerf").with_shape(ToolShape::Flat {}).is_err());
        let doc = document(tool);
        let mut value = serde_json::to_value(&doc).expect("value");
        for shape in [
            serde_json::json!({"type":"bull", "corner_radius":3}),
            serde_json::json!({"type":"v_bit", "angle":1, "tip_diameter":6}),
            serde_json::json!({"type":"flat", "unknown":true}),
        ] {
            value["program"]["tools"][0]["shape"] = shape;
            assert!(Document::from_json(&value.to_string()).is_err(), "accepted {value}");
        }
    }

    proptest! {
        #[test]
        fn valid_vbit_geometry_round_trips(diameter in 0.1f64..1000.0, ratio in 0.0f64..0.99, angle in 0.001f64..3.0) {
            let shape = ToolShape::VBit { angle, tip_diameter: diameter * ratio };
            let tool = Tool::try_new(1, None, Some(diameter), None).expect("tool").with_shape(shape).expect("shape");
            let doc = document(tool);
            let loaded = Document::from_json(&doc.to_json().expect("json")).expect("load");
            let loaded_shape = loaded.program().tools().first().expect("tool").shape().expect("shape");
            let ToolShape::VBit { angle: actual_angle, tip_diameter: actual_tip } = loaded_shape else {
                return Err(TestCaseError::fail("wrong shape"));
            };
            prop_assert!((actual_angle - angle).abs() <= 1e-12);
            prop_assert!((actual_tip - diameter * ratio).abs() <= diameter * 1e-12);
        }
    }
}
