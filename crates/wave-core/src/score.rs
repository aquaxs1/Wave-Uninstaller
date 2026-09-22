//! How sure Wave is that a finding belongs to the program being removed.
//!
//! Every signal is listed here with its weight, in one table, because a
//! scoring rule scattered through the code that gathers findings is a rule
//! nobody can review. The score is the sum of whatever applied, capped at 100.
//!
//! The reasons travel with the score. A user cannot judge a selection they
//! cannot see the argument for, and "path sits under the install location"
//! is an argument; "94 %" is not.

use serde::{Deserialize, Serialize};

/// The weight of each signal, in one place.
pub mod weight {
    /// The program's own uninstall key. Not a heuristic: the key *is* the
    /// program's registry entry, and leaving it behind is what produces the
    /// ghost entries in Apps & Features.
    pub const OWN_UNINSTALL_KEY: i32 = 100;
    /// The path the program itself recorded as its install location.
    pub const INSTALL_LOCATION: i32 = 75;
    /// Below that path.
    pub const UNDER_INSTALL_LOCATION: i32 = 65;
    /// Named after the MSI product code — no other product uses it.
    pub const PRODUCT_CODE: i32 = 65;
    /// Named after the Store package family.
    pub const PACKAGE_FAMILY: i32 = 65;
    /// A registry value whose data points into the install location.
    pub const POINTS_AT_INSTALL_LOCATION: i32 = 55;
    /// The folder or key is named exactly like the product.
    pub const NAME_EXACT: i32 = 70;
    /// The name contains the product's tokens — a folder called `Designer`
    /// for a product called `Acme Designer`.
    pub const NAME_TOKENS: i32 = 45;
    /// Close, but not the same string. On its own this never leaves the
    /// hidden bucket: a near miss is exactly how one product's folder gets
    /// mistaken for another's.
    pub const NAME_FUZZY: i32 = 25;
    /// Named after the publisher.
    pub const VENDOR: i32 = 35;
    /// Sits inside a folder or key named after the publisher.
    pub const UNDER_VENDOR: i32 = 25;
    /// Sits where programs keep their state — corroboration, never enough on
    /// its own.
    pub const PROGRAM_STATE_LOCATION: i32 = 15;
}

/// Everything below 50 stays hidden until a user asks for it.
const REVIEW_THRESHOLD: u8 = 50;
/// Everything from here up is preselected.
const SAFE_THRESHOLD: u8 = 85;

/// Which bucket a finding lands in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// Preselected.
    Safe,
    /// Shown, but a user has to tick it.
    Review,
    /// Hidden behind "show everything".
    Unsure,
}

/// One reason a finding scored what it did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    pub weight: i32,
    pub reason: String,
}

impl Evidence {
    pub fn new(weight: i32, reason: impl Into<String>) -> Self {
        Self { weight, reason: reason.into() }
    }
}

/// The result of weighing one finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Score {
    pub value: u8,
    pub confidence: Confidence,
    pub evidence: Vec<Evidence>,
}

/// Add up the evidence, strongest reason first.
pub fn weigh(mut evidence: Vec<Evidence>) -> Score {
    evidence.sort_by(|a, b| b.weight.cmp(&a.weight));

    let total: i32 = evidence.iter().map(|item| item.weight).sum();
    let value = total.clamp(0, 100) as u8;

    Score { value, confidence: confidence_for(value), evidence }
}

fn confidence_for(value: u8) -> Confidence {
    match value {
        value if value >= SAFE_THRESHOLD => Confidence::Safe,
        value if value >= REVIEW_THRESHOLD => Confidence::Review,
        _ => Confidence::Unsure,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_install_location_alone_is_not_enough_to_preselect() {
        // A recorded install location can be stale or plain wrong, so it needs
        // corroboration before Wave ticks the box for the user.
        let score = weigh(vec![Evidence::new(weight::INSTALL_LOCATION, "recorded location")]);
        assert_eq!(score.confidence, Confidence::Review);
    }

    #[test]
    fn a_name_match_on_top_of_the_location_preselects() {
        let score = weigh(vec![
            Evidence::new(weight::INSTALL_LOCATION, "recorded location"),
            Evidence::new(weight::NAME_EXACT, "named after the product"),
        ]);
        assert_eq!(score.value, 100, "the score is capped");
        assert_eq!(score.confidence, Confidence::Safe);
    }

    #[test]
    fn a_folder_that_only_sits_in_the_right_place_stays_hidden() {
        let score = weigh(vec![Evidence::new(weight::PROGRAM_STATE_LOCATION, "in AppData")]);
        assert_eq!(score.confidence, Confidence::Unsure);
    }

    #[test]
    fn a_name_that_only_resembles_the_product_stays_hidden() {
        // The near miss is how one product's folder becomes another's
        // leftover, so it takes more than a resemblance and a location.
        let score = weigh(vec![
            Evidence::new(weight::NAME_FUZZY, "a similar name"),
            Evidence::new(weight::PROGRAM_STATE_LOCATION, "in AppData"),
        ]);
        assert_eq!(score.confidence, Confidence::Unsure);
    }

    #[test]
    fn the_product_folder_inside_the_publisher_folder_is_preselected() {
        // `%ProgramData%\Acme\Designer` for Acme's Designer: three weak
        // signals that only line up for the right program.
        let score = weigh(vec![
            Evidence::new(weight::NAME_TOKENS, "named after the product"),
            Evidence::new(weight::UNDER_VENDOR, "under the publisher's folder"),
            Evidence::new(weight::PROGRAM_STATE_LOCATION, "in ProgramData"),
        ]);
        assert_eq!(score.confidence, Confidence::Safe);
    }

    #[test]
    fn reasons_are_ordered_by_how_much_they_carried() {
        let score = weigh(vec![
            Evidence::new(weight::PROGRAM_STATE_LOCATION, "weak"),
            Evidence::new(weight::OWN_UNINSTALL_KEY, "strong"),
        ]);
        assert_eq!(score.evidence.first().map(|e| e.reason.as_str()), Some("strong"));
    }
}
