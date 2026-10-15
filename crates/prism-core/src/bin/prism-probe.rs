//! `prism-probe`: prints the Corsair devices Prism can see.
//! Read-only; it does not open or change any device.

use hidapi::HidApi;
use prism_core::find_devices;

fn main() {
    let api = match HidApi::new() {
        Ok(api) => api,
        Err(e) => {
            eprintln!("Could not start the USB device library: {e}");
            std::process::exit(1);
        }
    };

    let devices = find_devices(&api);
    if devices.is_empty() {
        println!("No Corsair devices found.");
        return;
    }

    println!("Found {} Corsair device(s):\n", devices.len());
    for d in &devices {
        let status = if d.is_supported() { "recognized" } else { "not recognized yet" };
        println!("  {}  [{:04x}:{:04x}]  ({status})", d.display_name(), prism_core::CORSAIR_VID, d.pid);
        if !d.product.is_empty() {
            println!("    reports itself as: {}", d.product);
        }
        for i in &d.interfaces {
            println!(
                "    interface {:>2}  usage page 0x{:04x}  usage 0x{:04x}  {}",
                i.number, i.usage_page, i.usage, i.path
            );
        }
        println!();
    }
}
