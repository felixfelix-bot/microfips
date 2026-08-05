pub fn init() {
    // ESP32-C3 dram2_seg is ~64.7KB (66320 bytes). Use 64KB to fit with alignment.
    // ESP32/S3 have larger DRAM and can use 72KB.
    #[cfg(feature = "esp32c3")]
    const HEAP_SIZE: usize = 64 * 1024;
    #[cfg(not(feature = "esp32c3"))]
    const HEAP_SIZE: usize = 72 * 1024;

    #[link_section = ".dram2_uninit"]
    static mut HEAP: [u8; HEAP_SIZE] = [0; HEAP_SIZE];
    // SAFETY: HEAP is a static mut accessed once during initialization before any allocation.
    // The pointer (&raw mut HEAP) has 'static lifetime. esp_alloc requires the region to
    // remain valid for the program duration — satisfied because HEAP is static.
    unsafe {
        esp_alloc::HEAP.add_region(esp_alloc::HeapRegion::new(
            &raw mut HEAP as *mut u8,
            HEAP_SIZE,
            esp_alloc::MemoryCapability::Internal.into(),
        ));
    }
}
