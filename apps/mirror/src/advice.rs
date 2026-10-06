//! Portfolio advice is computed and rendered locally. Profile generation is
//! the only Mirror command that calls an LLM; policy acknowledgement added no
//! user-visible information and is intentionally not part of the score path.
mod cache;
mod policy;
mod profile_context;

pub(crate) use cache::{cache_current_deterministic, invalidate_cached, load_cached_for_score};
pub(crate) use policy::{
    deterministic_advice, portfolio_policy, PortfolioMode, PortfolioPolicy, LONG_TERM_WINDOW_DAYS,
};
pub(crate) use profile_context::save_profile_context;
#[cfg(test)]
pub(crate) use profile_context::{load_profile_context_from_path, save_profile_context_to_path};
