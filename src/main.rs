//! Chromium font gamma / contrast binary patcher.
//! Watch out for sudo without env, sudo is needed for actual patching
//! The engine lives in the library crate so it can be tested without a display

use chromium_gamma_patcher::{apply, ui};

fn usage() {
    println!("chromium font gamma patcher");
    println!();
    println!("usage:");
    println!("  chromium-gamma-patcher                          open the GUI");
    println!("  chromium-gamma-patcher scan                     list patchable / patched binaries");
    println!("  chromium-gamma-patcher patch <path> [gamma] [contrast]");
    println!("  chromium-gamma-patcher restore <path>");
    println!();
    println!("env:");
    println!("  CGP_SCREENSHOT=/path.png   render one GUI frame to a PNG and exit");
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(|s| s.as_str()) {
        Some("scan") | Some("--scan") => std::process::exit(ui::headless_scan()),
        Some("patch") | Some("--patch") => std::process::exit(apply::cli_patch(&args[1..])),
        Some("restore") | Some("--restore") => std::process::exit(apply::cli_restore(&args[1..])),
        Some("-h") | Some("--help") | Some("help") => {
            usage();
            std::process::exit(0);
        }
        _ => {}
    }

    // Fixed size like a dialog should be: min == max so no compositor can offer
    // resize handles, `resizable(false)` on top, and ui.rs puts it back if
    // anything still moves it.
    let size = [ui::WINDOW_W, ui::WINDOW_H];
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(size)
            .with_min_inner_size(size)
            .with_max_inner_size(size)
            .with_resizable(false)
            .with_title("Chromium font gamma patcher"),
        ..Default::default()
    };

    if let Err(e) = eframe::run_native(
        "Chromium font gamma patcher",
        options,
        Box::new(|cc| Ok(Box::new(ui::App::new(cc)))),
    ) {
        eprintln!("failed to start the GUI: {e}");
        eprintln!("(headless? use: chromium-gamma-patcher scan | patch <path> [gamma] [contrast])");
        std::process::exit(1);
    }
}
