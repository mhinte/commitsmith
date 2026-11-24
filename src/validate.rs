//! Re-export of the lint rules, which now live in the domain layer.

pub use commitsmith::domain::validate::*;

use crate::config::Config;

/// The one place that knows how a config file maps onto lint rules.
pub fn rules_from_config(config: &Config) -> Rules {
    Rules::new(
        config.type_names(),
        config.max_subject,
        config.recommended_subject,
    )
}
