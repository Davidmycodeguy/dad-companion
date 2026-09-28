//! Prints the Marketplace hover-test points for a resolution: `layout_points 3840 2160`.
fn main() {
    let args: Vec<u32> = std::env::args().skip(1).filter_map(|a| a.parse().ok()).collect();
    let (w, h) = (args.first().copied().unwrap_or(1920), args.get(1).copied().unwrap_or(1080));
    let layout = input::marketplace::build_layout((w, h), (0, 0), &serde_json::Value::Null);
    for (name, (x, y)) in layout.hover_targets() {
        println!("{name} {x} {y}");
    }
}
