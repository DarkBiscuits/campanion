//! Fixed output units. Literal post templates are already in their declared units.
use crate::def::PostDef;
use crate::error::{Result, invalid};
use crate::read::{strip_comments, tokenize};

/// Inch programs must select G20 before any dimensional word, and never switch units.
/// Keep the historical metric template contract unchanged.
pub(crate) fn validate(text: &str, post: &PostDef) -> Result<()> {
    check(text, post, false)
}

pub(crate) fn validate_written(text: &str, post: &PostDef) -> Result<()> {
    check(text, post, true)
}

fn check(text: &str, post: &PostDef, written: bool) -> Result<()> {
    if !post.is_inch() {
        return Ok(());
    }
    if text.len() > 64 * 1024 * 1024 {
        return Err(invalid("g-code is too large"));
    }
    let mut selected = false;
    for (index, line) in text.lines().enumerate() {
        if index > 2_000_000 || line.len() > 8192 {
            return Err(invalid("g-code exceeds line limits"));
        }
        let stripped = strip_comments(line);
        if stripped.trim() == "%" {
            continue;
        }
        let words = tokenize(&stripped)?;
        for &(letter, value) in &words {
            if written && letter == 'G' && [91.0, 93.0, 95.0].iter().any(|code| (value - code).abs() < 1e-6) {
                return Err(invalid("inch output requires absolute motion and units-per-minute feed templates"));
            }
            if letter == 'G' && (value - 21.0).abs() < 1e-6 {
                return Err(invalid(format!("line {}: units disagree with inch post (G21)", index + 1)));
            }
            if letter == 'G' && (value - 20.0).abs() < 1e-6 {
                selected = true;
            }
        }
        if !selected && words.iter().any(|(letter, _)| post.axis_letters.contains(*letter) || matches!(letter, 'I' | 'J' | 'K' | 'R' | 'F')) {
            return Err(invalid(format!("line {}: length or feed before G20", index + 1)));
        }
    }
    if !selected {
        return Err(invalid("inch program must declare G20"));
    }
    Ok(())
}
