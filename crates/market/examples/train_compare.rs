//! Trains the Rust worth model on a real `market_history.sqlite` (release mode; reports timing)
//! and compares it with a Python-trained model on the same listings.
//!
//! Two steps: first have Python train and save its model —
//!   python train_python_model.py <db_path> <items.json> <python_worth_model.json>
//! (see the script alongside this crate's task notes) — then run:
//!   cargo run -p market --release --example train_compare -- <db_path> <items.json> <python_worth_model.json>

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use game_data::ItemCatalog;
use market::{evaluate, load_listings, train, TrainListing, TrainOptions, WorthModel};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, db_path, items_path, python_model_path] = args.as_slice() else {
        eprintln!("usage: train_compare <market_history.sqlite> <items.json> <python_worth_model.json>");
        std::process::exit(2);
    };

    let catalog = ItemCatalog::load(Path::new(items_path)).expect("items.json should load");
    let item_slots: HashMap<String, String> = catalog
        .iter()
        .filter(|item| !item.slot_type.is_empty())
        .map(|item| (item.id.clone(), item.slot_type.clone()))
        .collect();
    println!("loaded {} item slots from {items_path}", item_slots.len());

    let listings = load_listings(Path::new(db_path)).expect("market_history.sqlite should load");
    println!("loaded {} listings from {db_path}", listings.len());

    let start = Instant::now();
    let trained = train(&listings, &item_slots, TrainOptions::default()).expect("training listings");
    let train_time = start.elapsed();
    let json = trained.to_json();
    let coef_count = json["coef"].as_object().map_or(0, serde_json::Map::len);
    println!(
        "rust train: {:.2}s, {coef_count} coefficients, {} listings kept (of {})",
        train_time.as_secs_f64(),
        json["listings"],
        listings.len()
    );

    let rust_model = WorthModel::from_json(&serde_json::to_string(&json).expect("model json")).expect("rust model parses");
    let python_text = std::fs::read_to_string(python_model_path).expect("python model json should exist (run train_python_model.py first)");
    let python_model = WorthModel::from_json(&python_text).expect("python model parses");

    let sample = sample_listings(&listings, 1000);
    let (correlation, median_pct_diff) = compare_predictions(&rust_model, &python_model, &sample, &item_slots);
    println!("compared {} predictions", sample.len());
    println!("correlation between rust and python predictions: {correlation:.4}");
    println!("median absolute percentage difference: {median_pct_diff:.2}%");

    let rust_eval = evaluate(&listings, &item_slots, 0.2, 7, TrainOptions::default()).expect("rust evaluate");
    println!(
        "rust evaluate: mdape={:.1}% within_25={:.1}% (item-median baseline: mdape={:.1}% within_25={:.1}%) tested={} trained={}",
        rust_eval.model_mdape, rust_eval.model_within_25, rust_eval.baseline_mdape, rust_eval.baseline_within_25, rust_eval.tested, rust_eval.trained
    );
    println!("(the Python model's own evaluate() MdAPE is printed by train_python_model.py when it runs)");
}

/// Up to `n` listings with a usable price, evenly spaced through a deterministic order (by item
/// then price) so the sample spans many items rather than clustering on one.
fn sample_listings(listings: &[TrainListing], n: usize) -> Vec<&TrainListing> {
    let mut candidates: Vec<&TrainListing> = listings.iter().filter(|l| l.price > 0).collect();
    candidates.sort_by(|a, b| a.item_id.cmp(&b.item_id).then(a.price.cmp(&b.price)));
    let step = (candidates.len() / n.max(1)).max(1);
    candidates.into_iter().step_by(step).take(n).collect()
}

/// `(Pearson correlation, median absolute percentage difference)` between the two models'
/// predicted values (per unit, fresh) over `sample`.
fn compare_predictions(rust_model: &WorthModel, python_model: &WorthModel, sample: &[&TrainListing], item_slots: &HashMap<String, String>) -> (f64, f64) {
    let mut rust_values = Vec::with_capacity(sample.len());
    let mut python_values = Vec::with_capacity(sample.len());
    let mut pct_diffs = Vec::with_capacity(sample.len());
    for listing in sample {
        let slot = item_slots.get(&listing.item_id).map(String::as_str);
        let r = rust_model.predict(&listing.item_id, &listing.rolls, 1, slot, None).value;
        let p = python_model.predict(&listing.item_id, &listing.rolls, 1, slot, None).value;
        rust_values.push(r);
        python_values.push(p);
        if p > 0.0 {
            pct_diffs.push((r - p).abs() / p * 100.0);
        }
    }
    pct_diffs.sort_by(f64::total_cmp);
    let median = pct_diffs.get(pct_diffs.len() / 2).copied().unwrap_or(0.0);
    (pearson(&rust_values, &python_values), median)
}

fn pearson(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len() as f64;
    let mean_a = a.iter().sum::<f64>() / n;
    let mean_b = b.iter().sum::<f64>() / n;
    let cov: f64 = a.iter().zip(b).map(|(x, y)| (x - mean_a) * (y - mean_b)).sum();
    let var_a: f64 = a.iter().map(|x| (x - mean_a).powi(2)).sum();
    let var_b: f64 = b.iter().map(|y| (y - mean_b).powi(2)).sum();
    cov / (var_a.sqrt() * var_b.sqrt())
}
