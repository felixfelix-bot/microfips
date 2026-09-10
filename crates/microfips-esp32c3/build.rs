fn main() {
    microfips_build::emit_all_keys();
    // Track WiFi credential env vars so cargo rebuilds when they change
    println!("cargo:rerun-if-env-changed=WIFI_SSID");
    println!("cargo:rerun-if-env-changed=WIFI_PASSWORD");

    // When espnow feature is enabled, esp-radio's build.rs handles linking
    // the ESP-IDF WiFi/ESP-NOW static libs via esp-wifi-sys. No manual
    // link directives needed — esp-radio propagates them when using its
    // safe EspNow API (Path B hybrid approach).
}