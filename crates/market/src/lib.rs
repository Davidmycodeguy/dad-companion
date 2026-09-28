//! The Marketplace: every listing seen (SQLite history) and the price facts drawn from it.

pub mod card;
pub mod history;
pub mod patterns;
pub mod pricing;
pub mod rules;
pub mod worth;
pub mod worth_train;

pub use history::{rarity_of, DayMedian, Listing, MarketHistory, Stat, DAY_S, LISTING_DAYS};
pub use pricing::{
    price_from_market, stat_name, MarketRow, RollPrice, BASE_TOLERANCE, CLOSE_ROLL_RATIO, EXTRA_ROLL_SHARE,
    FAR_APART_FLAG, MAX_SYNERGY_PCT, NO_SELLERS_REASON, PROPERTY_PREFIX,
};
pub use rules::{
    compute_price, is_merchant_reason, listing_fee, rarity_id, select_candidates, Candidate, ListerRules,
    PriceDecision, Skip, CURRENCY_ITEM_PREFIXES, INVENTORY_STASH_ID, LISTING_FEE_MIN, LISTING_FEE_RATE,
    LOWEST_ASK_MIN_RATIO, MAX_UNDERCUT_PCT, MERCHANT_REASONS, MERCHANT_REASON_PREFIX, PRICE_SOURCES,
};
pub use worth::{roll_quality, Confidence, Estimate, PairWorth, RollWorth, WorthError, WorthModel};
// `worth_train::Candidate` is not re-exported here: `rules::Candidate` already claims that name at
// the crate root. Reach it as `market::worth_train::Candidate`.
pub use worth_train::{
    evaluate, evaluate_default, load_listings, similar, train, DbError, EvalReport, TrainError, TrainListing, TrainOptions,
    TrainedModel,
};
// `patterns::Listing` is not re-exported here: `history::Listing` already claims that name at the
// crate root. Reach it as `market::patterns::Listing`.
pub use patterns::{
    analyze, extra_good_roll_factor, extra_roll_share, good_roll_counts, load_model, lowball_share,
    model_from_report, pair_bonuses, pair_synergies, percentile, rarity_steps, roll_ranges, save_model,
    seller_concentration, stat_premiums, stat_premiums_by_type, GoodRollCount, PairSynergy, RarityStep, Report,
    SellerShare, StatPremium,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("market database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("the market database is unavailable after an earlier failure")]
    Poisoned,
}
