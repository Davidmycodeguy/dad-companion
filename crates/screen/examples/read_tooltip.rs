//! Runs the tooltip reader step by step on a saved full-screen screenshot, with Windows OCR:
//! cargo run -p screen --release --example read_tooltip -- <screenshot.png> <cursor x> <cursor y>
use game_data::ItemCatalog;
use screen::Ocr;
use tooltip::finder;
use tooltip::parser::{looks_like_tooltip, parse_tooltip, ItemIndex};
use tooltip::{Frame, Region};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let image = image::open(&args[1]).expect("screenshot").to_rgb8();
    let frame = Frame::new(image.width() as usize, image.height() as usize, image.into_raw());
    let cursor = (args[2].parse::<i32>().unwrap(), args[3].parse::<i32>().unwrap());
    let screen = (frame.width() as i32, frame.height() as i32);
    let scale = f64::from(screen.1) / 2160.0;
    let mut grab = |(l, t, r, b): Region| {
        let c = |v: i32, max: usize| (v.max(0) as usize).min(max);
        frame.crop(c(l, frame.width()), c(t, frame.height()), c(r, frame.width()), c(b, frame.height()))
    };
    println!("screen {screen:?} scale {scale} cursor {cursor:?}");
    println!("probe strips: {:?}", finder::probe_strips(cursor, scale, screen));
    let rows = finder::probe(&mut grab, cursor, scale, screen);
    println!("probe rows: {:?}", rows.iter().map(|(row, _)| *row).collect::<Vec<_>>());
    let whole = finder::find_tooltips(&frame, cursor, scale, 3);
    println!("whole-frame boxes: {whole:?}");
    let catalog = ItemCatalog::load(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/items.json"))).unwrap();
    let index = ItemIndex::from_catalog(&catalog);
    let ocr = Ocr::new().expect("ocr");
    for (row, _) in rows.iter().take(finder::MAX_PROBE_ROWS) {
        let boxes = finder::measure(&mut grab, *row, cursor, scale, screen, 3);
        println!("row {row}: boxes {boxes:?}");
        for b in boxes {
            let region = b.ocr_region(screen.0, screen.1, scale);
            let crop = grab(region);
            let small = if scale >= 1.0 { half(&crop) } else { crop.clone() };
            let lines = ocr.read(&small).expect("ocr read");
            println!("  region {region:?} crop {}x{} -> {} lines", crop.width(), crop.height(), lines.len());
            for line in &lines {
                println!("    {:?} at ({}, {}) {}x{}", line.text, line.x, line.y, line.w, line.h);
            }
            let factor = if scale >= 1.0 { 0.5 } else { 1.0 };
            println!("  looks like tooltip: {}", looks_like_tooltip(&lines));
            println!("  parsed: {:?}", parse_tooltip(&lines, &index, f64::from(finder::TITLE_HEIGHT_PX) * scale * factor));
        }
    }
}

fn half(frame: &Frame) -> Frame {
    let (w, h) = (frame.width() / 2, frame.height() / 2);
    let mut data = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            let p = [frame.pixel(2 * x, 2 * y), frame.pixel(2 * x + 1, 2 * y), frame.pixel(2 * x, 2 * y + 1), frame.pixel(2 * x + 1, 2 * y + 1)];
            for c in 0..3 {
                data.push(((p.iter().map(|q| u32::from(q[c])).sum::<u32>() + 2) / 4) as u8);
            }
        }
    }
    Frame::new(w, h, data)
}
