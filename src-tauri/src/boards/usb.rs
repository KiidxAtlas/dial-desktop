//! Read-only USB discovery. These rows describe devices, not ports: they have no
//! path that can be passed to connect, reset, or flash. Only use cached descriptors.
use nusb::MaybeFuture;
use serde::Serialize;

#[derive(Serialize)]
pub struct UsbItem {
    pub(super) product: Option<String>,
    pub(super) kind: &'static str,
    pub(super) usb_id: String,
    pub(super) location: String,
    pub(super) functions: Vec<&'static str>,
    pub(super) speed: Option<&'static str>,
    pub(super) interfaces: Vec<String>,
}

type Class = (u8, u8, u8);

// Nominal link tier, not measured application throughput.
fn link_speed(speed: nusb::Speed) -> Option<&'static str> {
    use nusb::Speed::*;
    Some(match speed {
        Low => "1.5 Mb/s", Full => "12 Mb/s", High => "480 Mb/s",
        Super => "5 Gb/s", SuperPlus => "10 Gb/s", _ => return None,
    })
}

// A composite device can expose several functions, independently of its product name.
// Keep labels descriptive: an interface is not permission to record, mount or write.
fn functions(classes: &[Class]) -> Vec<&'static str> {
    let mut items: Vec<_> = classes.iter().filter_map(|&c| Some(match c {
        (0xff, 0x42, 1) => "Android debugging (ADB)",
        (1, 3, _) => "MIDI", (1, _, _) => "USB audio",
        (2, 2, _) => "CDC serial control", (2, 6 | 13 | 14, _) => "USB networking",
        (2, _, _) => "Communications control", (10, _, _) => "Communications data",
        (3, 1, 1) => "Boot keyboard", (3, 1, 2) => "Boot mouse", (3, _, _) => "Human interface (HID)",
        (6, _, _) => "Imaging / media transfer", (7, _, _) => "Printing", (8, _, _) => "Mass storage",
        (9, _, _) => "USB hub", (11, _, _) => "Smart card reader", (14, _, _) => "USB video",
        (0x0d, _, _) => "Content security", (0x0f, _, _) => "Personal healthcare",
        (0x10, _, _) => "Audio/video", (0x11, _, _) => "Billboard (alternate mode status)",
        (0x12, _, _) => "USB Type-C bridge", (0x14, _, _) => "Media transport (MCTP)",
        (0xfe, 2, _) => "Infrared bridge", (0xfe, 3, _) => "Test and measurement (USBTMC)",
        (0xe0, 1, 1) => "Bluetooth controller", (0xfe, 1, _) => "Device firmware upgrade (DFU)",
        (0xff, _, _) => "Vendor-specific interface", _ => return None,
    })).collect();
    items.sort_unstable(); items.dedup(); items
}

fn category(vid: u16, pid: u16, product: &str, classes: &[Class]) -> &'static str {
    let first = product.split_whitespace().next().unwrap_or("");
    if vid == 0x05ac {
        if first.eq_ignore_ascii_case("iphone") { return "iPhone"; }
        if first.eq_ignore_ascii_case("ipad") { return "iPad"; }
        // Shared by multiple generations: never infer a particular iPhone model.
        if pid == 0x12a8 { return "Apple mobile device"; }
    }
    if classes.contains(&(0xff, 0x42, 1))
        || matches!((vid, pid), (0x18d1, 0x4ee1 | 0x4ee2) | (0x04e8, 0x6860))
    {
        return "Android device";
    }
    for (class, label) in [(14, "Video device"), (6, "Imaging / media transfer"),
        (8, "Mass storage"), (1, "Audio device"), (7, "Printer"), (3, "Input device"), (9, "USB hub")] {
        if classes.iter().any(|c| c.0 == class) { return label; }
    }
    "USB device"
}

pub(super) fn inventory() -> Result<Vec<UsbItem>, String> {
    let devices = nusb::list_devices().wait().map_err(|e| e.to_string())?;
    let mut items = Vec::new();
    for d in devices {
        let product = d.product_string().map(str::trim).filter(|s| !s.is_empty())
            .map(|s| s.chars().filter(|c| !c.is_control()).take(128).collect::<String>());
        let classes: Vec<_> = std::iter::once((d.class(), d.subclass(), d.protocol()))
            .chain(d.interfaces().map(|i| (i.class(), i.subclass(), i.protocol()))).collect();
        items.push(UsbItem {
            kind: category(d.vendor_id(), d.product_id(), product.as_deref().unwrap_or(""), &classes),
            usb_id: format!("{:04x}:{:04x}", d.vendor_id(), d.product_id()),
            location: if d.port_chain().is_empty() { format!("{}:addr{}", d.bus_id(), d.device_address()) }
                else { format!("{}:{}", d.bus_id(), d.port_chain().iter().map(u8::to_string).collect::<Vec<_>>().join(".")) },
            functions: functions(&classes),
            speed: d.speed().and_then(link_speed),
            interfaces: d.interfaces().map(|i| format!("#{}: {:02x}/{:02x}/{:02x}", i.interface_number(), i.class(), i.subclass(), i.protocol())).collect(),
            product,
        });
    }
    items.sort_by(|a, b| (a.kind, &a.product, &a.usb_id).cmp(&(b.kind, &b.product, &b.usb_id)));
    Ok(items)
}

#[tauri::command]
pub async fn board_usb_devices(webview: tauri::Webview) -> Result<Vec<UsbItem>, String> {
    crate::app::main_only(&webview, "list USB devices")?;
    tauri::async_runtime::spawn_blocking(inventory).await.map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composite_functions_are_distinct_and_do_not_infer_access() {
        assert_eq!(functions(&[(0xef, 2, 1), (14, 1, 0), (1, 1, 0), (14, 2, 0)]), ["USB audio", "USB video"]);
        assert_eq!(functions(&[(0xff, 0x42, 1), (6, 1, 1)]), ["Android debugging (ADB)", "Imaging / media transfer"]);
        assert!(functions(&[(0, 0, 0)]).is_empty());
        assert_eq!(functions(&[(0xfe, 3, 1), (0x11, 0, 0)]), ["Billboard (alternate mode status)", "Test and measurement (USBTMC)"]);
        assert_eq!(link_speed(nusb::Speed::High), Some("480 Mb/s"));
    }

    #[test]
    fn distinguishes_mobile_evidence_from_shared_makers_and_imaging_interfaces() {
        assert_eq!(category(0x05ac, 0x12a8, "iPhone", &[]), "iPhone");
        assert_eq!(category(0x05ac, 0x12a8, "iPad", &[]), "iPad");
        assert_eq!(category(0x05ac, 0x12a8, "", &[]), "Apple mobile device");
        assert_eq!(category(0x05ac, 0, "Keyboard", &[(3, 1, 1)]), "Input device");
        assert_eq!(category(0x18d1, 0x4ee1, "", &[]), "Android device");
        assert_eq!(category(0, 0, "", &[(6, 1, 1), (0xff, 0x42, 1)]), "Android device");
        assert_eq!(category(0, 0, "", &[(6, 1, 1)]), "Imaging / media transfer");
        assert_eq!(category(0x04e8, 0, "", &[(8, 6, 80)]), "Mass storage");
        assert_eq!(category(0x18d1, 0, "", &[]), "USB device");
    }
}
