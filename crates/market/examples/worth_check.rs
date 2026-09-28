//! Compares Rust predictions with the Python model's on real listings (a local parity check):
//! cargo run -p market --example worth_check -- <worth_model.json> <cases.json>
//! where cases.json holds Python's answers: [{item_id, rolls, quantity, slot, value, floor, typical, confidence}].

use market::{Confidence, WorthModel};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, model_path, cases_path] = args.as_slice() else {
        eprintln!("usage: worth_check <worth_model.json> <cases.json>");
        std::process::exit(2);
    };
    let model = WorthModel::load(std::path::Path::new(model_path)).expect("worth model");
    let cases: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(cases_path).expect("cases file")).expect("cases json");
    let cases = cases.as_array().expect("a list of cases");
    let (mut worst, mut mismatched) = (0.0f64, 0);
    for case in cases {
        let rolls: Vec<(String, f64)> = case["rolls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| (r[0].as_str().unwrap().to_owned(), r[1].as_f64().unwrap()))
            .collect();
        let quantity = case["quantity"].as_u64().unwrap_or(1) as u32;
        let estimate = model.predict(case["item_id"].as_str().unwrap(), &rolls, quantity, case["slot"].as_str(), None);
        for (got, field) in [(estimate.value, "value"), (estimate.floor, "floor"), (estimate.typical, "typical")] {
            let want = case[field].as_f64().unwrap();
            worst = worst.max((got - want).abs() / want);
        }
        let confidence = match estimate.confidence {
            Confidence::High => "high",
            Confidence::Medium => "medium",
            Confidence::Low => "low",
            Confidence::Unknown => "unknown",
        };
        if confidence != case["confidence"].as_str().unwrap() {
            mismatched += 1;
        }
    }
    println!("{} cases, worst relative difference {worst:.2e}, confidence mismatches {mismatched}", cases.len());
}
