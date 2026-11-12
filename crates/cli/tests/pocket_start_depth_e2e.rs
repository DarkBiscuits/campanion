//! Pocket continuation below an explicitly declared already-cleared depth.
#![allow(clippy::expect_used)]

use std::path::PathBuf;
use std::process::Command;

use campanion_ir::{ClBlock, ClMotion, Document, MoveRole};
use campanion_post::{audit, builtin, compare, read_back};
use serde_json::{Value, json};

const EPS: f64 = 1e-6;

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/pocket_start.job.json")).expect("synthetic job")
}
fn folder(name: &str) -> PathBuf {
    std::env::var_os("CARGO_TARGET_TMPDIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join(format!("campanion-pocket-start-{name}"))
}
fn run(name: &str, job: &Value) -> (String, Document) {
    let dir = folder(name);
    std::fs::create_dir_all(&dir).expect("test dir");
    std::fs::write(dir.join("pocket_start.svg"), include_str!("fixtures/pocket_start.svg")).expect("drawing");
    std::fs::write(dir.join("pocket_start.job.json"), job.to_string()).expect("job");
    let output = Command::new(env!("CARGO_BIN_EXE_campanion"))
        .current_dir(&dir)
        .args(["run", "pocket_start.job.json", "--post", "linuxcnc", "-o", "out.nc", "--ir", "out.ir.json"])
        .output()
        .expect("run CLI");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("changed"));
    let text = std::fs::read_to_string(dir.join("out.nc")).expect("NC");
    let doc = Document::from_json(&std::fs::read_to_string(dir.join("out.ir.json")).expect("IR")).expect("document");
    let post = builtin("linuxcnc").expect("post");
    compare(doc.program(), &read_back(&text, &post).expect("read-back"), 0.001, &post).expect("motion compare");
    audit(&text, &post).expect("modal audit");
    let verify = Command::new(env!("CARGO_BIN_EXE_campanion")).current_dir(&dir).args(["verify", "out.ir.json"]).output().expect("verify CLI");
    assert!(verify.status.success(), "{}{}", String::from_utf8_lossy(&verify.stdout), String::from_utf8_lossy(&verify.stderr));
    (text, doc)
}
fn cut_levels(doc: &Document) -> Vec<Vec<f64>> {
    doc.program()
        .toolpaths()
        .iter()
        .map(|path| {
            let mut levels: Vec<f64> = Vec::new();
            for block in path.blocks() {
                if let ClBlock::Motion(
                    ClMotion::Linear { to, role: MoveRole::Cut, .. }
                    | ClMotion::Arc { to, role: MoveRole::Cut, .. }
                    | ClMotion::Helix { to, role: MoveRole::Cut, .. },
                ) = block
                {
                    let z = to.tip().z();
                    if levels.last().is_none_or(|last| (*last - z).abs() > EPS) {
                        levels.push(z);
                    }
                }
            }
            levels
        })
        .collect()
}
fn check_rapid_heights(doc: &Document, retract: f64) {
    let mut at = None;
    for path in doc.program().toolpaths() {
        for block in path.blocks() {
            if let ClBlock::Motion(motion) = block {
                let (to, rapid) = match motion {
                    ClMotion::Rapid { to, .. } => (to.tip(), true),
                    ClMotion::Linear { to, .. } | ClMotion::Arc { to, .. } | ClMotion::Helix { to, .. } | ClMotion::Thread { to, .. } => {
                        (to.tip(), false)
                    }
                };
                if rapid {
                    assert!(to.z() >= retract - EPS, "rapid target below retract: {to:?}");
                    if let Some(from) = at {
                        let from: campanion_ir::Vec3 = from;
                        if (to.x() - from.x()).hypot(to.y() - from.y()) > EPS {
                            assert!(from.z() >= retract - EPS);
                        }
                    }
                }
                at = Some(to);
            }
        }
    }
}

#[test]
fn pocket_start_depth_posts_known_levels_and_matches_golden() {
    let (text, doc) = run("golden", &fixture());
    assert_eq!(text, include_str!("golden/pocket_start.linuxcnc.nc"));
    assert_eq!(cut_levels(&doc), vec![vec![-5.0], vec![-7.0], vec![-8.0]]);
    assert!(text.contains("Z-5.0000 F300.0000"));
    check_rapid_heights(&doc, 2.0);
}

#[test]
fn omitted_start_depth_and_explicit_zero_produce_identical_programs() {
    let mut job = fixture();
    job["operations"][0].as_object_mut().expect("op").remove("start_depth");
    let (omitted, _) = run("omitted", &job);
    job["operations"][0]["start_depth"] = json!(0);
    let (zero, doc) = run("zero", &job);
    assert_eq!(omitted, zero);
    assert_eq!(cut_levels(&doc), vec![vec![-2.0], vec![-4.0], vec![-6.0], vec![-8.0]]);
}

#[test]
fn pocket_start_depth_applies_to_roughing_floor_and_wall_finish() {
    let mut job = fixture();
    job["operations"][0]["stock_to_leave"] = json!({"floor":1,"wall":0.5});
    job["operations"][0]["finish"] = json!({"step_down":1});
    job["operations"][0]["onion_skin"] = json!(1);
    let (_, doc) = run("finish", &job);
    assert_eq!(cut_levels(&doc), vec![vec![-5.0], vec![-7.0], vec![-8.0], vec![-4.0, -5.0, -6.0, -7.0, -8.0]]);
    check_rapid_heights(&doc, 2.0);
    job["operations"][0].as_object_mut().expect("op").remove("finish");
    let (_, rough) = run("rough-only", &job);
    assert_eq!(cut_levels(&rough), vec![vec![-5.0], vec![-7.0]]);
}

#[test]
fn pocket_start_depth_with_onion_skin_keeps_a_separate_final_pass() {
    let mut job = fixture();
    job["operations"][0]["onion_skin"] = json!(0.5);
    let (_, doc) = run("onion", &job);
    assert_eq!(cut_levels(&doc), vec![vec![-5.0], vec![-7.0], vec![-7.5], vec![-8.0]]);
}

#[test]
fn inch_start_depth_and_step_down_are_converted_once() {
    let mut job = fixture();
    job["units"] = json!("inch");
    job["heights"] = json!({"clearance":0.2,"retract":0.1});
    job["tools"][0]["diameter"] = json!(0.25);
    job["operations"][0]["start_depth"] = json!(0.125);
    job["operations"][0]["depth"] = json!(0.375);
    job["operations"][0]["step_down"] = json!(0.125);
    job["operations"][0]["stepover"] = json!(0.125);
    let (_, doc) = run("inch", &job);
    let levels = cut_levels(&doc);
    assert_eq!(levels.len(), 2);
    assert!((levels[0][0] + 6.35).abs() < EPS);
    assert!((levels[1][0] + 9.525).abs() < EPS);
    check_rapid_heights(&doc, 2.54);
}

fn check_entry_slopes(doc: &Document, angle_deg: f64) {
    let post = builtin("linuxcnc").expect("post");
    // Read-back expands helices to arcs, with their analytic sweep/radius available.
    let nc = campanion_post::post_program(doc.program(), &post, &campanion_post::PostOptions { arc_tolerance: 0.01 }).expect("post");
    let mut descents = 0;
    let mut from = [0.0; 3];
    for motion in audit(&nc, &post).expect("audit").motions {
        if motion.mode != campanion_post::MotionMode::Rapid && motion.end[2] < from[2] - 0.0001 {
            let horizontal = if let Some(arc) = motion.arc {
                let radius = (from[0] - arc.center[0]).hypot(from[1] - arc.center[1]);
                let first = (from[1] - arc.center[1]).atan2(from[0] - arc.center[0]);
                let last = (motion.end[1] - arc.center[1]).atan2(motion.end[0] - arc.center[0]);
                let sweep = if arc.sense == campanion_ir::ArcSense::Ccw { last - first } else { first - last }.rem_euclid(std::f64::consts::TAU);
                radius * if sweep < 1e-8 { std::f64::consts::TAU } else { sweep }
            } else {
                (motion.end[0] - from[0]).hypot(motion.end[1] - from[1])
            };
            assert!(horizontal > 0.0, "unexpected plunge");
            let angle = (from[2] - motion.end[2]).atan2(horizontal).to_degrees();
            assert!(angle <= angle_deg + 0.005, "descent angle {angle} exceeds {angle_deg}");
            descents += 1;
        }
        from = motion.end;
    }
    assert!(descents > 0);
}

#[test]
fn ramp_and_helix_cover_the_full_drop_from_retract_for_area_and_wall_finish() {
    for (name, entry) in [("ramp", json!({"type":"ramp","angle_deg":10})), ("helix", json!({"type":"helix","radius":3,"angle_deg":10}))] {
        for retract in [0.0, 2.0] {
            let mut job = fixture();
            job["heights"]["retract"] = json!(retract);
            job["operations"][0]["entry"] = entry.clone();
            job["operations"][0]["stock_to_leave"] = json!({"wall":0.5});
            job["operations"][0]["finish"] = json!({"step_down":2});
            let (_, doc) = run(&format!("{name}-{retract}"), &job);
            let actual = cut_levels(&doc);
            let expected = [vec![-5.0], vec![-7.0], vec![-8.0], vec![-5.0, -7.0, -8.0]];
            assert_eq!(actual.len(), expected.len());
            for (got, want) in actual.iter().zip(expected) {
                assert_eq!(got.len(), want.len());
                assert!(got.iter().zip(want).all(|(a, b)| (a - b).abs() < EPS), "{got:?}");
            }
            check_rapid_heights(&doc, retract);
            check_entry_slopes(&doc, 10.0);
        }
    }
}

#[test]
fn invalid_start_depth_fails_before_writing_outputs() {
    let dir = folder("invalid-no-output");
    std::fs::create_dir_all(&dir).expect("dir");
    let mut job = fixture();
    job["operations"][0]["start_depth"] = json!(8);
    std::fs::write(dir.join("pocket_start.job.json"), job.to_string()).expect("job");
    let nc = dir.join("invalid.nc");
    let ir = dir.join("invalid.ir.json");
    for file in [&nc, &ir] {
        if file.exists() {
            std::fs::remove_file(file).expect("old test output");
        }
    }
    let output = Command::new(env!("CARGO_BIN_EXE_campanion"))
        .current_dir(&dir)
        .args(["run", "pocket_start.job.json", "-o", "invalid.nc", "--ir", "invalid.ir.json"])
        .output()
        .expect("CLI");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("start_depth must be less than depth"));
    assert!(!nc.exists() && !ir.exists());
}
