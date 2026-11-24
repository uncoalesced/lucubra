// SPDX-License-Identifier: Apache-2.0
// Engineered by uncoalesced

//! Spaced repetition through the `fsrs` crate (SPEC section 14). Cards are
//! stored as JSON in the CARDS collection; this module turns a review into the
//! next memory state and due day, capped at the exam (exam crunch).
//!
//! shortcut: default FSRS parameters for every user, optimise per user once
//! someone has 400+ reviews.

use fsrs::{FSRS, MemoryState};
use serde_json::{Value, json};

/// Desired retention (SPEC section 14).
pub const DESIRED_RETENTION: f32 = 0.9;

/// Self-rating of a review, also derived from engine grades.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rating {
    Again,
    Hard,
    Good,
    Easy,
}

impl Rating {
    /// Parses `again`, `hard`, `good` or `easy`.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "again" => Self::Again,
            "hard" => Self::Hard,
            "good" => Self::Good,
            "easy" => Self::Easy,
            _ => return None,
        })
    }

    /// Default rating for a graded answer with `score` 0 to 1.
    pub fn from_score(correct: bool, score: f64) -> Self {
        match (correct, score) {
            (false, _) => Self::Again,
            (true, s) if s < 1.0 => Self::Hard,
            _ => Self::Good,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Again => "again",
            Self::Hard => "hard",
            Self::Good => "good",
            Self::Easy => "easy",
        }
    }
}

fn fsrs() -> FSRS {
    FSRS::default()
}

/// The memory state stored on a card, if it has been reviewed.
pub fn memory_of(card: &Value) -> Option<MemoryState> {
    let f = |k: &str| card.get(k).and_then(Value::as_f64).map(|v| v as f32);
    let stability = f("stability")?;
    Some(MemoryState {
        stability,
        difficulty: f("difficulty")?,
        stability_fast: f("stability_fast").unwrap_or(stability),
    })
}

/// Next state after a review on day `today` (days since the epoch). The
/// card's `last_day` gives the elapsed time; `exam_day` caps the interval.
/// Returns the fields to merge into the card.
pub fn review(
    card: &Value,
    today: i64,
    rating: Rating,
    exam_day: Option<i64>,
) -> Result<Value, String> {
    let prev = memory_of(card);
    let elapsed = card
        .get("last_day")
        .and_then(Value::as_i64)
        .map_or(0, |d| (today - d).max(0)) as u32;
    let next = fsrs()
        .next_states(prev, DESIRED_RETENTION, elapsed)
        .map_err(|e| format!("FSRS could not schedule the card: {e}"))?;
    let state = match rating {
        Rating::Again => next.again,
        Rating::Hard => next.hard,
        Rating::Good => next.good,
        Rating::Easy => next.easy,
    };
    let mut interval = if rating == Rating::Again {
        0
    } else {
        (state.interval.round() as i64).max(1)
    };
    let mut capped = false;
    if let Some(exam) = exam_day
        && exam > today
        && today + interval > exam
    {
        interval = exam - today;
        capped = true;
    }
    let reps = card.get("reps").and_then(Value::as_u64).unwrap_or(0) + 1;
    let lapses = card.get("lapses").and_then(Value::as_u64).unwrap_or(0)
        + u64::from(rating == Rating::Again && prev.is_some());
    Ok(json!({
        "stability": state.memory.stability,
        "difficulty": state.memory.difficulty,
        "stability_fast": state.memory.stability_fast,
        "last_day": today,
        "due_day": today + interval,
        "interval_days": interval,
        "capped_by_exam": capped,
        "reps": reps,
        "lapses": lapses,
        "last_rating": rating.as_str(),
    }))
}

/// Probability of recall on `today`; 0 for a card never reviewed.
pub fn retrievability(card: &Value, today: i64) -> f64 {
    let (Some(state), Some(last)) = (
        memory_of(card),
        card.get("last_day").and_then(Value::as_i64),
    ) else {
        return 0.0;
    };
    f64::from(fsrs().current_retrievability(state, (today - last).max(0) as f32))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn merge(card: &mut Value, fields: &Value) {
        for (k, v) in fields.as_object().unwrap() {
            card[k] = v.clone();
        }
    }

    #[test]
    fn srs_matches_fsrs_next_states() {
        let mut card = json!({});
        let out = review(&card, 100, Rating::Good, None).unwrap();
        let expect = fsrs().next_states(None, DESIRED_RETENTION, 0).unwrap().good;
        assert_eq!(
            out["stability"].as_f64().unwrap() as f32,
            expect.memory.stability
        );
        let interval = out["interval_days"].as_i64().unwrap();
        assert_eq!(interval, (expect.interval.round() as i64).max(1));
        assert_eq!(out["due_day"], json!(100 + interval));

        merge(&mut card, &out);
        let day = out["due_day"].as_i64().unwrap();
        let r = retrievability(&card, day);
        assert!(r > 0.8 && r < 0.97, "retrievability near the target: {r}");
        assert_eq!(retrievability(&json!({}), day), 0.0);
        let again = review(&card, day, Rating::Again, None).unwrap();
        assert_eq!(again["interval_days"], json!(0));
        assert_eq!(again["lapses"], json!(1));
        assert_eq!(again["reps"], json!(2));
    }

    #[test]
    fn srs_exam_crunch_caps_interval() {
        let mut card = json!({});
        let mut today = 0;
        for _ in 0..10 {
            let out = review(&card, today, Rating::Easy, Some(20)).unwrap();
            assert!(
                out["due_day"].as_i64().unwrap() <= 20,
                "past the exam: {out}"
            );
            merge(&mut card, &out);
            today = card["due_day"].as_i64().unwrap();
            if today == 20 {
                break;
            }
        }
        assert_eq!(today, 20);
        assert_eq!(card["capped_by_exam"], json!(true));
        let after = review(&card, 21, Rating::Easy, Some(20)).unwrap();
        assert!(after["interval_days"].as_i64().unwrap() > 1);
    }
}
